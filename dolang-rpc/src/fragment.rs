//! Fragmented framing: wire header, receive-side reassembly, and the
//! send-side round-robin scheduler. Shared between [`crate::client`] and
//! [`crate::server`], which differ only in which [`Kind`]s they originate
//! and dispatch.

use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{
    Error, Kind, Limits,
    trailer::{RecvShared, SendAction, SendShared},
    transport::{AnySender, RecvFrame, SendFrame, Sender},
};

/// Fragment header flag bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Flags(u8);

impl Flags {
    pub(crate) const NONE: Flags = Flags(0);
    pub(crate) const FIRST: Flags = Flags(0b0001);
    pub(crate) const LAST: Flags = Flags(0b0010);
    pub(crate) const ABORT: Flags = Flags(0b0100);
    pub(crate) const TRAILER: Flags = Flags(0b1000);
    const VALID: u8 = Self::FIRST.0 | Self::LAST.0 | Self::ABORT.0 | Self::TRAILER.0;

    pub(crate) fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    fn bits(self) -> u8 {
        self.0
    }

    fn from_bits(bits: u8) -> Result<Self, Error> {
        if bits & !Self::VALID != 0 {
            return Err(Error::Protocol(format!("invalid fragment flags {bits:#x}")));
        }
        Ok(Flags(bits))
    }
}

impl std::ops::BitOr for Flags {
    type Output = Flags;

    fn bitor(self, rhs: Flags) -> Flags {
        Flags(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for Flags {
    type Output = Flags;

    fn bitand(self, rhs: Flags) -> Flags {
        Flags(self.0 & rhs.0)
    }
}

#[repr(C, packed)]
struct RawFragmentHeader {
    flags: [u8; 1],
    kind: [u8; 1],
    id: [u8; 8],
    payload_len: [u8; 4],
}

impl RawFragmentHeader {
    const LEN: usize = size_of::<Self>();

    fn new(flags: Flags, kind: Kind, id: u64, payload_len: u32) -> Self {
        Self {
            flags: [flags.bits()],
            kind: [kind as u8],
            id: id.to_le_bytes(),
            payload_len: payload_len.to_le_bytes(),
        }
    }

    fn as_bytes(&self) -> [u8; Self::LEN] {
        // SAFETY: RawFragmentHeader is packed, contains no padding, and
        // consists only of byte arrays, so a bitwise copy of its bytes is
        // always valid.
        unsafe { std::mem::transmute_copy(self) }
    }

    fn decode(bytes: &[u8; Self::LEN]) -> Result<(Flags, Kind, u64, usize), Error> {
        // SAFETY: `bytes` has exactly the layout of `RawFragmentHeader`.
        let header = unsafe { &*bytes.as_ptr().cast::<Self>() };
        Ok((
            Flags::from_bits(header.flags[0])?,
            Kind::try_from(header.kind[0])?,
            u64::from_le_bytes(header.id),
            u32::from_le_bytes(header.payload_len) as usize,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FragmentHeader {
    pub(crate) flags: Flags,
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    pub(crate) payload_len: usize,
}

impl FragmentHeader {
    pub(crate) fn encode_into(&self, buffer: &mut impl BufMut) {
        let payload_len = u32::try_from(self.payload_len).expect("fragment payload is too large");
        let raw = RawFragmentHeader::new(self.flags, self.kind, self.id, payload_len);
        buffer.put_slice(&raw.as_bytes());
    }

    pub(crate) fn encode(&self) -> Bytes {
        let mut buffer = BytesMut::with_capacity(RawFragmentHeader::LEN);
        self.encode_into(&mut buffer);
        buffer.freeze()
    }
}

/// Reads and decodes one fragment header, looping over partial reads.
pub(crate) async fn read_fragment_header<F: RecvFrame>(
    frame: &mut F,
) -> Result<FragmentHeader, Error> {
    let mut buf = [0u8; RawFragmentHeader::LEN];
    let mut filled = 0;
    while filled < buf.len() {
        let mut dest = &mut buf[filled..];
        let n = frame.recv(&mut dest).await?;
        if n == 0 {
            return Err(Error::ConnectionClosed);
        }
        filled += n;
    }
    let (flags, kind, id, payload_len) = RawFragmentHeader::decode(&buf)?;
    Ok(FragmentHeader {
        flags,
        kind,
        id,
        payload_len,
    })
}

/// Reads exactly `len` bytes directly into `dest`, appending to whatever it
/// already contains. Bounded with [`BufMut::limit`] on every call so a
/// single `recv()` can never read past `len` bytes, even if more bytes
/// (belonging to the next fragment) are already available.
async fn read_payload<F: RecvFrame>(
    frame: &mut F,
    dest: &mut BytesMut,
    len: usize,
) -> Result<(), Error> {
    let mut remaining = len;
    while remaining > 0 {
        let mut limited = (&mut *dest).limit(remaining);
        let n = frame.recv(&mut limited).await?;
        if n == 0 {
            return Err(Error::ConnectionClosed);
        }
        remaining -= n;
    }
    Ok(())
}

pub(crate) struct StreamMessage {
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    pub(crate) payload: Bytes,
    pub(crate) trailer: Option<Arc<std::sync::Mutex<RecvShared>>>,
}

pub(crate) enum StreamEvent {
    None,
    Aborted {
        kind: Kind,
        id: u64,
        dispatched: bool,
    },
    Message(StreamMessage),
    Trailer {
        id: u64,
        message: Option<StreamMessage>,
        shared: Arc<std::sync::Mutex<RecvShared>>,
        len: usize,
        /// Set when the local consumer had already discarded this trailer
        /// (via [`crate::TrailerRecv::discard`] or by dropping it) before
        /// this *subsequent* fragment arrived — i.e. the peer is still
        /// sending more than we want. Never set on the fragment that first
        /// hands the trailer to the application. The caller should tell the
        /// peer to stop (`Kind::Discard`) exactly once per message when
        /// this is set.
        notify_discard: bool,
    },
}

struct StreamIncomplete {
    kind: Kind,
    postcard: BytesMut,
    trailer: Option<Arc<std::sync::Mutex<RecvShared>>>,
    trailer_len: usize,
    dispatched: bool,
    discard_notified: bool,
}

/// Reassembles postcard data while handing trailer fragments to a live
/// [`RecvShared`] without buffering their bytes.
pub(crate) struct StreamReassembler {
    limits: Limits,
    incomplete: HashMap<u64, StreamIncomplete>,
    /// Number of `incomplete` entries whose `trailer` is (or was, at some
    /// point while incomplete) `Some`. Enforces `max_incomplete_trailers`
    /// independent of `max_incomplete_messages`.
    incomplete_trailers: usize,
}

impl StreamReassembler {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            incomplete: HashMap::new(),
            incomplete_trailers: 0,
        }
    }

    pub(crate) async fn accept<F: RecvFrame>(
        &mut self,
        header: FragmentHeader,
        frame: &mut F,
    ) -> Result<StreamEvent, Error> {
        let FragmentHeader {
            flags,
            kind,
            id,
            payload_len,
        } = header;
        let first = flags.contains(Flags::FIRST);
        let last = flags.contains(Flags::LAST);
        let abort = flags.contains(Flags::ABORT);
        let trailer = flags.contains(Flags::TRAILER);

        if abort {
            if first || last || trailer || payload_len != 0 {
                return Err(Error::Protocol("invalid ABORT fragment".into()));
            }
            let entry = self.incomplete.remove(&id).ok_or_else(|| {
                Error::Protocol(format!("ABORT for message {id} with no active fragments"))
            })?;
            if let Some(shared) = entry.trailer {
                self.incomplete_trailers -= 1;
                RecvShared::fail(
                    &shared,
                    io::Error::new(io::ErrorKind::Interrupted, "trailer was aborted"),
                );
            }
            return Ok(StreamEvent::Aborted {
                kind: entry.kind,
                id,
                dispatched: entry.dispatched,
            });
        }

        if first && last && trailer {
            return Err(Error::Protocol(
                "a trailer-bearing message cannot complete in its FIRST fragment".into(),
            ));
        }

        if first {
            if self.incomplete.contains_key(&id) {
                return Err(Error::Protocol(format!(
                    "duplicate FIRST fragment for message {id}"
                )));
            }
            if !last && self.incomplete.len() >= self.limits.max_incomplete_messages {
                return Err(Error::Protocol("too many incomplete messages".into()));
            }
        } else {
            let entry = self.incomplete.get(&id).ok_or_else(|| {
                Error::Protocol(format!(
                    "fragment for message {id} without an active message"
                ))
            })?;
            if entry.kind != kind {
                return Err(Error::Protocol(format!(
                    "inconsistent message kind for message {id}"
                )));
            }
            if entry.trailer.is_some() && !trailer {
                return Err(Error::Protocol(format!(
                    "message {id} cannot return to postcard fragments once its trailer has started"
                )));
            }
        }

        if trailer && last {
            if payload_len != 0 {
                return Err(Error::Protocol(
                    "TRAILER|LAST commit fragment must not carry a payload".into(),
                ));
            }
            let mut entry = self.incomplete.remove(&id).ok_or_else(|| {
                Error::Protocol(format!(
                    "fragment for message {id} without an active message"
                ))
            })?;
            if entry.trailer.is_none() {
                // Established and immediately finished by this same
                // fragment (a present-but-empty trailer) — check the limit
                // as if opening a new trailer stream, but there's nothing
                // to decrement afterward since it never actually occupied a
                // slot in `incomplete`.
                if self.incomplete_trailers >= self.limits.max_incomplete_trailers {
                    return Err(Error::Protocol("too many incomplete trailers".into()));
                }
            } else {
                self.incomplete_trailers -= 1;
            }
            let shared = entry
                .trailer
                .get_or_insert_with(|| {
                    RecvShared::new(
                        self.limits.trailer_recv_copy_threshold,
                        self.limits.trailer_recv_demand_copy_threshold,
                    )
                })
                .clone();
            RecvShared::finish(&shared);
            if entry.dispatched {
                return Ok(StreamEvent::None);
            }
            return Ok(StreamEvent::Message(StreamMessage {
                kind,
                id,
                payload: entry.postcard.freeze(),
                trailer: Some(shared),
            }));
        }

        let fragment_limit = if first && last {
            self.limits.max_payload_size
        } else {
            self.limits.max_fragment_size
        };
        if payload_len > fragment_limit {
            return Err(Error::Protocol(format!(
                "fragment of {payload_len} bytes for message {id} exceeds the limit of {fragment_limit}"
            )));
        }

        if first && last {
            let mut payload = BytesMut::with_capacity(payload_len);
            read_payload(frame, &mut payload, payload_len).await?;
            return Ok(StreamEvent::Message(StreamMessage {
                kind,
                id,
                payload: payload.freeze(),
                trailer: None,
            }));
        }

        if first {
            self.incomplete.insert(
                id,
                StreamIncomplete {
                    kind,
                    postcard: BytesMut::new(),
                    trailer: None,
                    trailer_len: 0,
                    dispatched: false,
                    discard_notified: false,
                },
            );
        }
        let entry = self.incomplete.get_mut(&id).unwrap();

        if trailer {
            if entry.trailer_len + payload_len > self.limits.max_trailer_size {
                return Err(Error::Protocol(format!(
                    "message {id} exceeds the maximum trailer size"
                )));
            }
            if entry.trailer.is_none() {
                if self.incomplete_trailers >= self.limits.max_incomplete_trailers {
                    return Err(Error::Protocol("too many incomplete trailers".into()));
                }
                self.incomplete_trailers += 1;
            }
            entry.trailer_len += payload_len;
            let shared = entry
                .trailer
                .get_or_insert_with(|| {
                    RecvShared::new(
                        self.limits.trailer_recv_copy_threshold,
                        self.limits.trailer_recv_demand_copy_threshold,
                    )
                })
                .clone();
            let message = if entry.dispatched {
                None
            } else {
                entry.dispatched = true;
                Some(StreamMessage {
                    kind,
                    id,
                    payload: entry.postcard.clone().freeze(),
                    trailer: Some(shared.clone()),
                })
            };
            // Only a *subsequent* fragment (the message was already
            // dispatched to the app on an earlier one) can trigger a
            // notification — the very first trailer fragment hasn't given
            // the application a chance to discard anything yet.
            let notify_discard =
                message.is_none() && !entry.discard_notified && RecvShared::is_discarded(&shared);
            if notify_discard {
                entry.discard_notified = true;
            }
            return Ok(StreamEvent::Trailer {
                id,
                message,
                shared,
                notify_discard,
                len: payload_len,
            });
        }

        if entry.postcard.len() + payload_len > self.limits.max_payload_size {
            return Err(Error::Protocol(format!(
                "message {id} exceeds the maximum payload size"
            )));
        }
        entry.postcard.reserve(payload_len);
        read_payload(frame, &mut entry.postcard, payload_len).await?;

        if last {
            let entry = self.incomplete.remove(&id).unwrap();
            return Ok(StreamEvent::Message(StreamMessage {
                kind,
                id,
                payload: entry.postcard.freeze(),
                trailer: None,
            }));
        }
        Ok(StreamEvent::None)
    }
}

/// A message's optional trailer, as seen by the send-side scheduler.
#[derive(Clone)]
pub(crate) enum Trailer {
    None,
    Stream(std::sync::Arc<std::sync::Mutex<SendShared>>),
}

impl Trailer {
    fn is_none(&self) -> bool {
        matches!(self, Trailer::None)
    }

    fn total_len(&self) -> usize {
        match self {
            Trailer::None => 0,
            Trailer::Stream(_) => 0,
        }
    }
}

/// One outbound message the scheduler is actively (or about to be) sending.
struct ActiveSend {
    id: u64,
    kind: Kind,
    payload: Bytes,
    offset: usize,
    trailer: Trailer,
    /// Progress through `trailer`'s bytes, once the postcard phase (`offset
    /// == payload.len()`) is done.
    /// Whether any fragment has been sent for this message yet. Distinct
    /// from `offset == 0`, which is ambiguous when `payload` is empty (a
    /// trailer-bearing message with an empty postcard phase still needs an
    /// explicit first fragment before or as part of its trailer data).
    started: bool,
    /// Whether this send occupies a concurrency slot (`payload` did not fit
    /// in one fragment at admission time, or a trailer is present — a
    /// trailer-bearing message is always at least two fragments).
    multi_fragment: bool,
}

/// A control-priority item: a zero-payload `Cancel`/`Error`, or an `ABORT`
/// for a message whose FIRST fragment already went out.
enum ControlSend {
    Empty { kind: Kind, id: u64 },
    Abort { id: u64 },
}

/// Outcome of attempting to cancel an in-flight outbound send.
pub(crate) enum AbortOutcome {
    /// No trace of `id` in the scheduler — its LAST fragment already went
    /// out (or it was never admitted). The caller must fall back to the
    /// ordinary `Cancel` message flow.
    NotActive,
    /// The send was discarded before completion. `started` indicates
    /// whether any bytes (a FIRST fragment) were already sent, in which
    /// case the caller must send `ControlSend::Abort` for `id`.
    Discarded { started: bool },
}

/// Send-side round-robin fragment scheduler, self-throttled so it never
/// admits more concurrently-fragmenting sends than the peer's `Reassembler`
/// is configured to track.
///
/// This throttles against the *local* `Limits`, on the assumption that both
/// ends of a connection are configured identically. There is no handshake
/// to confirm the peer's actual limits (see dolang-org/dolang#385), so an
/// asymmetric configuration can still cause the peer's `Reassembler` to
/// reject a message after bytes have already been sent for it.
pub(crate) struct Scheduler {
    active: VecDeque<ActiveSend>,
    /// Multi-fragment sends admitted but not yet started (no concurrency
    /// slot, or no byte budget, free at admission time).
    waiting: VecDeque<ActiveSend>,
    control: VecDeque<ControlSend>,
    active_fragmented: usize,
    max_active_fragmented: usize,
    max_fragment_size: usize,
    /// `log2` of the current backoff factor applied to `max_fragment_size`
    /// for actual fragment writes: `effective_fragment_size() ==
    /// max_fragment_size >> fragment_shift`. Storing the shift rather than
    /// the resulting size avoids drift when `max_fragment_size` isn't a
    /// power of two — halving/doubling a stored size repeatedly wouldn't
    /// necessarily recover the exact original value.
    fragment_shift: u32,
}

/// Upper bound on `fragment_shift`, chosen to fit a 3-bit "divide by 2^n"
/// wire hint if peer-signaled throttling is added later.
const MAX_FRAGMENT_SHIFT: u32 = 7;

impl Scheduler {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            active: VecDeque::new(),
            waiting: VecDeque::new(),
            control: VecDeque::new(),
            active_fragmented: 0,
            max_active_fragmented: limits.max_incomplete_messages.max(1),
            max_fragment_size: limits.max_fragment_size,
            fragment_shift: 0,
        }
    }

    /// The fragment size to actually target for the next write, after
    /// backoff from recent short writes.
    fn effective_fragment_size(&self) -> usize {
        (self.max_fragment_size >> self.fragment_shift)
            .max(256.min(self.max_fragment_size))
            .max(1)
    }

    /// Adapts `fragment_shift` based on whether the most recent fragment
    /// write completed atomically (in a single underlying write call) or
    /// needed more than one. Backs off by one step on a short write, and
    /// decays back towards `max_fragment_size` by one step per atomic
    /// write — gradual in both directions, so a connection that's
    /// borderline doesn't flap between extremes.
    fn record_write_atomicity(&mut self, atomic: bool) {
        if atomic {
            self.fragment_shift = self.fragment_shift.saturating_sub(1);
        } else {
            self.fragment_shift = (self.fragment_shift + 1).min(MAX_FRAGMENT_SHIFT);
        }
    }

    /// Admits a request/response payload, with an optional trailer, for
    /// sending.
    pub(crate) fn admit_message(&mut self, kind: Kind, id: u64, payload: Bytes, trailer: Trailer) {
        if trailer.is_none() && payload.len() <= self.max_fragment_size {
            self.active.push_back(ActiveSend {
                id,
                kind,
                payload,
                offset: 0,
                trailer,
                started: false,
                multi_fragment: false,
            });
            return;
        }
        let send = ActiveSend {
            id,
            kind,
            payload,
            offset: 0,
            trailer,
            started: false,
            multi_fragment: true,
        };
        if self.active_fragmented < self.max_active_fragmented {
            self.active_fragmented += 1;
            self.active.push_back(send);
        } else {
            self.waiting.push_back(send);
        }
    }

    /// Admits a zero-payload control message (`Cancel`/`Error`), always
    /// sent as a single `FIRST|LAST` fragment ahead of ordinary sends.
    pub(crate) fn admit_empty(&mut self, kind: Kind, id: u64) {
        self.control.push_back(ControlSend::Empty { kind, id });
    }

    /// Admits an `ABORT` fragment for a message whose FIRST fragment was
    /// already sent, ahead of ordinary sends.
    pub(crate) fn admit_abort(&mut self, id: u64) {
        self.control.push_back(ControlSend::Abort { id });
    }

    /// Attempts to cancel an in-flight or not-yet-started outbound send.
    ///
    /// If the send carries a `Trailer::Stream`, its `SendShared` is put into
    /// the error state so the paired `TrailerSend`'s writer observes a clean
    /// failure instead of hanging forever waiting for a lease that will
    /// never come again (the `ActiveSend` itself, and its clone of the
    /// `Arc`, are gone after this call).
    pub(crate) fn try_cancel_active(&mut self, id: u64) -> AbortOutcome {
        if let Some(pos) = self.waiting.iter().position(|s| s.id == id) {
            let send = self.waiting.remove(pos).expect("position was just found");
            if let Trailer::Stream(shared) = &send.trailer {
                SendShared::discard(shared);
            }
            return AbortOutcome::Discarded { started: false };
        }
        if let Some(pos) = self.active.iter().position(|s| s.id == id) {
            let send = self.active.remove(pos).expect("position was just found");
            let started = send.started;
            if let Trailer::Stream(shared) = &send.trailer {
                SendShared::discard(shared);
            }
            if send.multi_fragment {
                self.free_fragmented_slot();
            }
            return AbortOutcome::Discarded { started };
        }
        AbortOutcome::NotActive
    }

    /// Handles a peer's advisory `Discard` notice: an active trailer-bearing
    /// send for `id` has its `SendShared` put into the error state (so the
    /// local producer's writer observes a clean failure), and its trailer is
    /// dropped so the send's next turn falls straight through to an
    /// ordinary zero-length `TRAILER | LAST` terminal commit — exactly as if
    /// the trailer had completed normally — rather than an `ABORT`. Unlike
    /// [`Scheduler::try_cancel_active`], this never affects the message's
    /// own request/response outcome: the postcard payload was already fully
    /// sent (and, on the peer, already dispatched) by the time a trailer can
    /// even begin, so cutting the trailer short doesn't invalidate it.
    ///
    /// A no-op if `id` has no active trailer-bearing send (it may have
    /// already finished, or the notice may have crossed on the wire with
    /// completion).
    pub(crate) fn discard_active_trailer(&mut self, id: u64) {
        if let Some(send) = self.active.iter_mut().find(|s| s.id == id)
            && let Trailer::Stream(shared) = &send.trailer
        {
            SendShared::discard(shared);
            send.trailer = Trailer::None;
        }
    }

    /// Releases the concurrency slot held by a completed or cancelled
    /// multi-fragment send, then promotes the next waiting send if there's
    /// now room for it.
    fn free_fragmented_slot(&mut self) {
        self.active_fragmented -= 1;
        if let Some(next) = self.waiting.pop_front() {
            self.active_fragmented += 1;
            self.active.push_back(next);
        }
    }

    /// Whether `advance` has anything to send right now.
    pub(crate) fn has_work(&self) -> bool {
        !self.control.is_empty() || !self.active.is_empty()
    }

    /// Waits until advancing the scheduler would not block on a trailer
    /// producer. Once this reports ready, `advance` must be driven to
    /// completion without racing ordinary message admission: it may commit
    /// part of a fragment before yielding on transport readiness.
    pub(crate) fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.control.is_empty() {
            return Poll::Ready(());
        }
        for send in &self.active {
            if send.offset != send.payload.len() {
                return Poll::Ready(());
            }
            match &send.trailer {
                Trailer::Stream(shared) if SendShared::poll_action(shared, cx).is_pending() => {}
                _ => return Poll::Ready(()),
            }
        }
        Poll::Pending
    }

    /// Sends one control fragment if any are queued (priority); otherwise
    /// advances the front of `active` by one fragment (postcard bytes,
    /// trailer bytes, or the trailer's terminal commit, whichever phase it's
    /// in), re-queuing it at the back if not yet complete.
    ///
    /// Returns `Ok(Some(id))` when a streaming producer was dropped and this
    /// call aborted its partially sent message. The caller uses the id to
    /// complete any still-pending local call.
    pub(crate) async fn advance(
        &mut self,
        transport: &mut AnySender,
    ) -> Result<Option<u64>, Error> {
        if let Some(control) = self.control.pop_front() {
            self.send_control(transport, control).await?;
            return Ok(None);
        }
        let mut send = std::future::poll_fn(|cx| {
            let count = self.active.len();
            for _ in 0..count {
                let send = self.active.pop_front().unwrap();
                let stream_waiting = send.offset == send.payload.len()
                    && matches!(&send.trailer, Trailer::Stream(shared) if SendShared::poll_action(shared, cx).is_pending());
                if stream_waiting {
                    self.active.push_back(send);
                } else {
                    return Poll::Ready(send);
                }
            }
            Poll::Pending
        })
        .await;

        let first = !send.started;
        // A trailer-bearing message is always at least two fragments (see
        // `Reassembler`), so if its postcard phase is empty and its trailer
        // is *also* empty, an explicit (zero-length) postcard fragment must
        // still open the message before the terminal commit can follow —
        // otherwise it would have to carry FIRST, LAST, and TRAILER all at
        // once, which is rejected on receipt.
        let must_open_with_postcard = first && send.trailer.total_len() == 0;

        if send.offset < send.payload.len() || must_open_with_postcard {
            let start = send.offset;
            let end = (start + self.effective_fragment_size()).min(send.payload.len());
            let postcard_done = end == send.payload.len();
            let mut flags = Flags::NONE;
            if first {
                flags = flags | Flags::FIRST;
            }
            if postcard_done && send.trailer.is_none() {
                flags = flags | Flags::LAST;
            }
            let header = FragmentHeader {
                flags,
                kind: send.kind,
                id: send.id,
                payload_len: end - start,
            };
            let mut buffer = header.encode().chain(send.payload.slice(start..end));
            let atomic = transport.send().finish(&mut buffer).await?;
            self.record_write_atomicity(atomic);
            send.offset = end;
            send.started = true;
            if postcard_done && send.trailer.is_none() {
                if send.multi_fragment {
                    self.free_fragmented_slot();
                }
            } else {
                self.active.push_back(send);
            }
            return Ok(None);
        }

        if let Trailer::Stream(shared) = &send.trailer {
            match std::future::poll_fn(|cx| SendShared::poll_action(shared, cx)).await {
                SendAction::Finish => {}
                SendAction::Abort => {
                    self.control.push_back(ControlSend::Abort { id: send.id });
                    if send.multi_fragment {
                        self.free_fragmented_slot();
                    }
                    return Ok(Some(send.id));
                }
                SendAction::Fragment => {
                    debug_assert!(send.started, "trailer cannot be the first fragment");
                    let token = transport.send();
                    // SAFETY: the lease retains `token`'s mutable borrow and
                    // clears its erased representation before it ends.
                    let lease =
                        unsafe { SendShared::grant(shared, token, self.effective_fragment_size()) };
                    let (action, atomic) = SendShared::wait_fragment(shared).await?;
                    lease.complete();
                    send.started = true;
                    match action {
                        SendAction::Fragment => {
                            self.record_write_atomicity(atomic);
                            self.active.push_back(send);
                            return Ok(None);
                        }
                        SendAction::Finish => {}
                        SendAction::Abort => {
                            self.control.push_back(ControlSend::Abort { id: send.id });
                            if send.multi_fragment {
                                self.free_fragmented_slot();
                            }
                            return Ok(Some(send.id));
                        }
                    }
                }
            }
        }

        // Terminal commit: only reachable once both phases above are
        // exhausted, which (given `must_open_with_postcard`) implies a
        // trailer was present.
        let header = FragmentHeader {
            flags: Flags::TRAILER | Flags::LAST,
            kind: send.kind,
            id: send.id,
            payload_len: 0,
        };
        let mut buffer = header.encode();
        transport.send().finish(&mut buffer).await?;
        if send.multi_fragment {
            self.free_fragmented_slot();
        }
        Ok(None)
    }

    async fn send_control(
        &mut self,
        transport: &mut AnySender,
        control: ControlSend,
    ) -> Result<(), Error> {
        let header = match control {
            ControlSend::Empty { kind, id } => FragmentHeader {
                flags: Flags::FIRST | Flags::LAST,
                kind,
                id,
                payload_len: 0,
            },
            ControlSend::Abort { id } => FragmentHeader {
                flags: Flags::ABORT,
                kind: Kind::Request,
                id,
                payload_len: 0,
            },
        };
        let mut buffer = header.encode();
        transport.send().finish(&mut buffer).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    #[cfg(unix)]
    use std::os::fd::OwnedFd;
    #[cfg(windows)]
    use std::os::windows::io::OwnedHandle;

    use super::*;

    struct FakeRecvFrame {
        chunks: VecDeque<Bytes>,
    }

    impl FakeRecvFrame {
        fn new(data: impl Into<Bytes>) -> Self {
            Self {
                chunks: VecDeque::from([data.into()]),
            }
        }

        fn chunked(pieces: Vec<Vec<u8>>) -> Self {
            Self {
                chunks: pieces.into_iter().map(Bytes::from).collect(),
            }
        }
    }

    impl RecvFrame for FakeRecvFrame {
        #[cfg(unix)]
        fn take_fd(&mut self, _index: u32) -> io::Result<OwnedFd> {
            unimplemented!("fake transport does not carry descriptors")
        }
        #[cfg(windows)]
        fn take_handle(&mut self, _value: usize) -> io::Result<OwnedHandle> {
            unimplemented!("fake transport does not carry handles")
        }

        async fn recv<B: BufMut>(&mut self, buffer: &mut B) -> io::Result<usize> {
            let Some(front) = self.chunks.front_mut() else {
                return Ok(0);
            };
            let n = front.len().min(buffer.remaining_mut());
            if n == 0 {
                return Ok(0);
            }
            buffer.put_slice(&front[..n]);
            front.advance(n);
            if front.is_empty() {
                self.chunks.pop_front();
            }
            Ok(n)
        }
    }

    fn fast_path_bytes(id: u64, kind: Kind, payload: &[u8]) -> Vec<u8> {
        let header = FragmentHeader {
            flags: Flags::FIRST | Flags::LAST,
            kind,
            id,
            payload_len: payload.len(),
        };
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    fn fragment_bytes(flags: Flags, id: u64, kind: Kind, payload: &[u8]) -> Vec<u8> {
        let header = FragmentHeader {
            flags,
            kind,
            id,
            payload_len: payload.len(),
        };
        let mut bytes = header.encode().to_vec();
        bytes.extend_from_slice(payload);
        bytes
    }

    /// Reads exactly `len` trailer-fragment bytes directly off `frame`,
    /// bypassing `RecvShared` (which is exercised separately in
    /// `trailer.rs`'s own unit tests). Needed between fragments so a test's
    /// next `read_fragment_header` call doesn't misread leftover payload
    /// bytes as header bytes.
    async fn drain_trailer_bytes(frame: &mut FakeRecvFrame, len: usize) -> Bytes {
        let mut buf = BytesMut::with_capacity(len);
        read_payload(frame, &mut buf, len).await.unwrap();
        buf.freeze()
    }

    #[tokio::test]
    async fn first_last_fragment_is_the_fast_path_and_bypasses_incomplete_bookkeeping() {
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("fast path completes immediately");
        };
        assert_eq!(msg.kind, Kind::Request);
        assert_eq!(&msg.payload[..], b"hello");
        assert!(msg.trailer.is_none());
        assert_eq!(reassembler.incomplete.len(), 0);
    }

    #[tokio::test]
    async fn continuation_fragments_append_directly_into_the_same_buffer() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"hello, ");
        bytes.extend(fragment_bytes(Flags::NONE, 1, Kind::Request, b"world"));
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Request, b"!"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        for _ in 0..2 {
            let header = read_fragment_header(&mut frame).await.unwrap();
            assert!(matches!(
                reassembler.accept(header, &mut frame).await.unwrap(),
                StreamEvent::None
            ));
        }
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("LAST completes the message");
        };
        assert_eq!(&msg.payload[..], b"hello, world!");
    }

    #[tokio::test]
    async fn payload_read_never_overreads_past_declared_fragment_length() {
        let mut bytes = fragment_bytes(Flags::FIRST | Flags::LAST, 1, Kind::Request, b"one");
        bytes.extend(fragment_bytes(
            Flags::FIRST | Flags::LAST,
            2,
            Kind::Request,
            b"two",
        ));
        // Deliver everything in one chunk so a single `recv()` call could
        // observe bytes belonging to the second fragment while reading the
        // first, if the read weren't correctly bounded.
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"one");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert_eq!(header.id, 2);
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"two");
    }

    #[tokio::test]
    async fn payload_read_handles_partial_chunked_delivery() {
        let bytes = fragment_bytes(Flags::FIRST | Flags::LAST, 1, Kind::Request, b"hello");
        // Split the wire bytes into single-byte chunks to force many partial
        // `recv()` calls across both the header and payload reads.
        let pieces = bytes.into_iter().map(|b| vec![b]).collect();
        let mut frame = FakeRecvFrame::chunked(pieces);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"hello");
    }

    #[tokio::test]
    async fn rejects_duplicate_first_fragment() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a"));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"b"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_continuation_without_active_message() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::NONE, 1, Kind::Request, b"a"));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_fragment_after_terminal_fragment() {
        let mut bytes = fragment_bytes(Flags::FIRST | Flags::LAST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::NONE, 1, Kind::Request, b"b"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_inconsistent_kind_in_continuation() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Response, b"b"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_fragment_exceeding_max_fragment_size() {
        let limits = Limits {
            max_fragment_size: 4,
            ..Limits::default()
        };
        let mut frame =
            FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"hello"));
        let mut reassembler = StreamReassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_single_fragment_exceeding_max_payload_size() {
        let limits = Limits {
            max_payload_size: 4,
            ..Limits::default()
        };
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = StreamReassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_reassembled_message_exceeding_max_payload_size() {
        let limits = Limits {
            max_fragment_size: 4,
            max_payload_size: 6,
            ..Limits::default()
        };
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"abcd");
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Request, b"abcd"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_too_many_incomplete_messages() {
        let limits = Limits {
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut reassembler = StreamReassembler::new(limits);
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 2, Kind::Request, b"a"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_too_many_incomplete_trailers() {
        let limits = Limits {
            max_incomplete_trailers: 1,
            ..Limits::default()
        };
        let mut reassembler = StreamReassembler::new(limits);
        let mut frame = FakeRecvFrame::new(fragment_bytes(
            Flags::FIRST | Flags::TRAILER,
            1,
            Kind::Request,
            b"a",
        ));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::Trailer { .. }
        ));

        let mut frame = FakeRecvFrame::new(fragment_bytes(
            Flags::FIRST | Flags::TRAILER,
            2,
            Kind::Request,
            b"a",
        ));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_nonzero_payload_on_abort() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::ABORT, 1, Kind::Request, b"x"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn abort_discards_accumulated_buffer_without_completing() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"abcd");
        bytes.extend(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        let event = reassembler.accept(header, &mut frame).await.unwrap();
        assert!(matches!(
            event,
            StreamEvent::Aborted {
                dispatched: false,
                ..
            }
        ));
        assert_eq!(reassembler.incomplete.len(), 0);
    }

    #[tokio::test]
    async fn rejects_abort_for_unknown_message() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    // --- Trailer reassembly tests ---

    #[tokio::test]
    async fn message_without_any_trailer_fragment_has_no_trailer() {
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"hello");
        assert!(msg.trailer.is_none());
    }

    #[tokio::test]
    async fn present_but_empty_trailer_is_distinguishable_from_absent() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"hello");
        bytes.extend(fragment_bytes(
            Flags::TRAILER | Flags::LAST,
            1,
            Kind::Request,
            b"",
        ));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("TRAILER|LAST completes the message");
        };
        assert_eq!(&msg.payload[..], b"hello");
        assert!(
            msg.trailer.is_some(),
            "a TRAILER fragment was seen, even though its content is empty"
        );
    }

    #[tokio::test]
    async fn single_fragment_trailer_reassembles_with_postcard_payload() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"hello");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"world"));
        bytes.extend(fragment_bytes(
            Flags::TRAILER | Flags::LAST,
            1,
            Kind::Request,
            b"",
        ));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: Some(msg),
            len,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected the first TRAILER fragment to dispatch the message");
        };
        assert_eq!(&msg.payload[..], b"hello");
        assert_eq!(&drain_trailer_bytes(&mut frame, len).await[..], b"world");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));
    }

    #[tokio::test]
    async fn multi_fragment_trailer_reassembles_with_empty_postcard_payload() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"ab"));
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"cd"));
        bytes.extend(fragment_bytes(
            Flags::TRAILER | Flags::LAST,
            1,
            Kind::Request,
            b"",
        ));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: Some(msg),
            len,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected the first TRAILER fragment to dispatch the message");
        };
        assert_eq!(&msg.payload[..], b"");
        assert_eq!(&drain_trailer_bytes(&mut frame, len).await[..], b"ab");

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: None, len, ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a subsequent TRAILER fragment to not redispatch the message");
        };
        assert_eq!(&drain_trailer_bytes(&mut frame, len).await[..], b"cd");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));
    }

    #[tokio::test]
    async fn rejects_trailer_last_commit_with_nonzero_payload() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"a"));
        bytes.extend(fragment_bytes(
            Flags::TRAILER | Flags::LAST,
            1,
            Kind::Request,
            b"x",
        ));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer { len, .. } =
            reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a TRAILER data event");
        };
        drain_trailer_bytes(&mut frame, len).await;

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn first_trailer_fragment_starts_trailer_phase_immediately_with_empty_postcard() {
        let mut bytes = fragment_bytes(Flags::FIRST | Flags::TRAILER, 1, Kind::Request, b"ab");
        bytes.extend(fragment_bytes(
            Flags::TRAILER | Flags::LAST,
            1,
            Kind::Request,
            b"",
        ));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: Some(msg),
            len,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected FIRST|TRAILER to dispatch the message immediately");
        };
        assert_eq!(&msg.payload[..], b"");
        assert_eq!(&drain_trailer_bytes(&mut frame, len).await[..], b"ab");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            StreamEvent::None
        ));
    }

    #[tokio::test]
    async fn rejects_first_last_trailer_together() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(
            Flags::FIRST | Flags::LAST | Flags::TRAILER,
            1,
            Kind::Request,
            b"",
        ));
        let mut reassembler = StreamReassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_fragment_returning_to_postcard_after_trailer_started() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"a"));
        bytes.extend(fragment_bytes(Flags::NONE, 1, Kind::Request, b"b"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer { len, .. } =
            reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a TRAILER data event");
        };
        drain_trailer_bytes(&mut frame, len).await;

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn postcard_and_trailer_size_are_independent_budgets() {
        // A postcard payload that would itself exceed `max_trailer_size`
        // (but fits `max_payload_size`) doesn't count against the trailer
        // that follows it — the two limits are enforced independently, not
        // combined the way `max_message_size` used to combine them.
        let limits = Limits {
            max_fragment_size: 4,
            max_payload_size: 8,
            max_trailer_size: 3,
            ..Limits::default()
        };
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"abcd");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"ab"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer { len, .. } =
            reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a TRAILER data event, not rejection");
        };
        drain_trailer_bytes(&mut frame, len).await;
    }

    #[tokio::test]
    async fn rejects_trailer_exceeding_max_trailer_size() {
        let limits = Limits {
            max_fragment_size: 4,
            max_trailer_size: 3,
            ..Limits::default()
        };
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"abcd"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn abort_during_trailer_phase_discards_both_buffers() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"ab");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"cd"));
        bytes.extend(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer { len, .. } =
            reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a TRAILER data event");
        };
        drain_trailer_bytes(&mut frame, len).await;
        assert_eq!(reassembler.incomplete_trailers, 1);

        let header = read_fragment_header(&mut frame).await.unwrap();
        let event = reassembler.accept(header, &mut frame).await.unwrap();
        assert!(matches!(
            event,
            StreamEvent::Aborted {
                dispatched: true,
                ..
            }
        ));
        assert_eq!(reassembler.incomplete.len(), 0);
        assert_eq!(reassembler.incomplete_trailers, 0);
    }

    // --- Scheduler tests ---

    use tokio::io::AsyncReadExt;

    fn sender_pair() -> (AnySender, tokio::io::DuplexStream) {
        let (a, b) = tokio::io::duplex(64 * 1024);
        let (sender, _receiver) = crate::transport::generic_duplex(a);
        (AnySender::Generic(sender), b)
    }

    async fn read_wire_fragment(r: &mut tokio::io::DuplexStream) -> (Flags, Kind, u64, Vec<u8>) {
        let mut header_buf = [0u8; RawFragmentHeader::LEN];
        r.read_exact(&mut header_buf).await.unwrap();
        let (flags, kind, id, len) = RawFragmentHeader::decode(&header_buf).unwrap();
        let mut payload = vec![0u8; len];
        if len > 0 {
            r.read_exact(&mut payload).await.unwrap();
        }
        (flags, kind, id, payload)
    }

    #[test]
    fn fragment_shift_backs_off_on_short_write_and_decays_on_atomic_write() {
        let limits = Limits {
            max_fragment_size: 1024,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        assert_eq!(scheduler.effective_fragment_size(), 1024);

        scheduler.record_write_atomicity(false);
        assert_eq!(scheduler.effective_fragment_size(), 512);
        scheduler.record_write_atomicity(false);
        assert_eq!(scheduler.effective_fragment_size(), 256);

        scheduler.record_write_atomicity(true);
        assert_eq!(scheduler.effective_fragment_size(), 512);
        scheduler.record_write_atomicity(true);
        assert_eq!(scheduler.effective_fragment_size(), 1024);

        // Never decays past the negotiated maximum.
        scheduler.record_write_atomicity(true);
        assert_eq!(scheduler.effective_fragment_size(), 1024);
    }

    #[test]
    fn fragment_shift_is_capped_and_size_never_reaches_zero() {
        let limits = Limits {
            max_fragment_size: 1024,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        for _ in 0..20 {
            scheduler.record_write_atomicity(false);
        }
        assert_eq!(scheduler.fragment_shift, MAX_FRAGMENT_SHIFT);
        assert!(scheduler.effective_fragment_size() >= 1);
    }

    #[tokio::test]
    async fn scheduler_round_robins_between_active_messages() {
        let limits = Limits {
            max_fragment_size: 4,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Trailer::None,
        );
        let (mut sender, mut reader) = sender_pair();

        scheduler.advance(&mut sender).await.unwrap();
        let (_, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 1);

        scheduler.advance(&mut sender).await.unwrap();
        let (_, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(
            id, 2,
            "second turn should serve the other message, not repeat id 1"
        );
    }

    #[tokio::test]
    async fn scheduler_single_fragment_message_bypasses_concurrency_gate() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        // Occupies the only fragmented-concurrency slot.
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        // Fits in one fragment; must not be blocked by the slot above.
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"hi"), Trailer::None);
        let (mut sender, mut reader) = sender_pair();

        scheduler.advance(&mut sender).await.unwrap();
        let (_, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 1);

        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, payload) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 2);
        assert!(flags.contains(Flags::FIRST) && flags.contains(Flags::LAST));
        assert_eq!(payload, b"hi");
    }

    #[test]
    fn scheduler_defers_multi_fragment_message_when_active_fragmented_is_full() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Trailer::None,
        );
        assert_eq!(scheduler.active.len(), 1);
        assert_eq!(scheduler.waiting.len(), 1);
        assert_eq!(scheduler.active_fragmented, 1);
    }

    #[tokio::test]
    async fn scheduler_promotes_waiting_message_when_a_slot_frees() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Trailer::None,
        );
        assert_eq!(scheduler.waiting.len(), 1);
        let (mut sender, mut reader) = sender_pair();

        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 1);
        assert!(flags.contains(Flags::FIRST) && !flags.contains(Flags::LAST));
        assert_eq!(scheduler.waiting.len(), 1);

        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 1);
        assert!(flags.contains(Flags::LAST));
        assert_eq!(scheduler.waiting.len(), 0);
        assert_eq!(scheduler.active_fragmented, 1);
    }

    #[test]
    fn scheduler_try_cancel_active_reports_not_active_after_terminal_sent() {
        let mut scheduler = Scheduler::new(&Limits::default());
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"hi"), Trailer::None);
        scheduler.active.pop_front();
        assert!(matches!(
            scheduler.try_cancel_active(1),
            AbortOutcome::NotActive
        ));
    }

    #[test]
    fn scheduler_try_cancel_active_reports_not_started_before_any_fragment_sent() {
        let mut scheduler = Scheduler::new(&Limits::default());
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        match scheduler.try_cancel_active(1) {
            AbortOutcome::Discarded { started } => assert!(!started),
            AbortOutcome::NotActive => panic!("expected Discarded"),
        }
    }

    #[test]
    fn scheduler_try_cancel_active_reports_started_after_first_fragment() {
        let limits = Limits {
            max_fragment_size: 4,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        if let Some(send) = scheduler.active.front_mut() {
            send.offset = 4;
            send.started = true;
        }
        match scheduler.try_cancel_active(1) {
            AbortOutcome::Discarded { started } => assert!(started),
            AbortOutcome::NotActive => panic!("expected Discarded"),
        }
    }

    #[test]
    fn scheduler_try_cancel_active_discards_waiting_message_without_abort() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Trailer::None,
        );
        assert_eq!(scheduler.waiting.len(), 1);
        match scheduler.try_cancel_active(2) {
            AbortOutcome::Discarded { started } => assert!(!started),
            AbortOutcome::NotActive => panic!("expected Discarded"),
        }
        assert_eq!(scheduler.waiting.len(), 0);
        assert_eq!(scheduler.active_fragmented, 1);
    }

    // --- Trailer scheduling tests ---

    #[test]
    fn scheduler_trailer_forces_multi_fragment_even_with_small_payload() {
        let limits = Limits {
            max_fragment_size: 1024,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        // A trailer forces multi_fragment (and a terminal commit), occupying
        // the only concurrency slot even with a tiny postcard payload.
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"hi"),
            Trailer::Stream(SendShared::new(
                Kind::Request,
                1,
                &Limits {
                    max_trailer_size: usize::MAX,
                    ..limits
                },
            )),
        );
        assert_eq!(scheduler.active.len(), 1);
        assert_eq!(scheduler.active_fragmented, 1);
        // A second, ordinary small message with no trailer must not be
        // starved by the trailer message occupying the only slot.
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"hi"), Trailer::None);
        assert_eq!(scheduler.active.len(), 2);
        assert_eq!(scheduler.waiting.len(), 0);
    }

    #[tokio::test]
    async fn notify_discard_fires_only_on_a_subsequent_fragment_after_local_discard() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"");
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"a"));
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"b"));
        bytes.extend(fragment_bytes(Flags::TRAILER, 1, Kind::Request, b"c"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = StreamReassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        // First TRAILER fragment dispatches the message. `notify_discard`
        // must never fire here — the application hasn't had a chance to
        // discard anything yet.
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: Some(_),
            shared,
            len,
            notify_discard,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected the first TRAILER fragment to dispatch the message");
        };
        assert!(!notify_discard);
        drain_trailer_bytes(&mut frame, len).await;

        // The application decides to stop reading.
        RecvShared::discard(&shared);

        // Second TRAILER fragment: arrives after the discard, so this is
        // the point where the peer should be told to stop.
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: None,
            len,
            notify_discard,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a subsequent TRAILER data event");
        };
        assert!(notify_discard);
        drain_trailer_bytes(&mut frame, len).await;

        // Third TRAILER fragment: already notified once, must not fire
        // again for the same message.
        let header = read_fragment_header(&mut frame).await.unwrap();
        let StreamEvent::Trailer {
            message: None,
            len,
            notify_discard,
            ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a subsequent TRAILER data event");
        };
        assert!(!notify_discard);
        drain_trailer_bytes(&mut frame, len).await;
    }

    #[tokio::test]
    async fn scheduler_discard_active_trailer_errors_writer_and_sends_ordinary_terminal_commit() {
        use tokio::io::AsyncWriteExt;

        let limits = Limits::default();
        let mut scheduler = Scheduler::new(&limits);
        let shared = SendShared::new(
            Kind::Request,
            1,
            &Limits {
                max_trailer_size: usize::MAX,
                ..limits
            },
        );
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"hi"),
            Trailer::Stream(shared.clone()),
        );
        let mut trailer = crate::TrailerSend::new(shared, ());
        let (mut sender, mut reader) = sender_pair();

        // Postcard phase: FIRST, no LAST (a trailer is pending).
        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, payload) = read_wire_fragment(&mut reader).await;
        assert!(flags.contains(Flags::FIRST) && !flags.contains(Flags::LAST));
        assert_eq!(id, 1);
        assert_eq!(payload, b"hi");

        // One small trailer data fragment is staged without waiting for a
        // grant, then flushed by the scheduler.
        let writer = tokio::spawn(async move {
            trailer.write_all(b"data").await.unwrap();
            trailer
        });
        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, payload) = read_wire_fragment(&mut reader).await;
        assert!(flags.contains(Flags::TRAILER) && !flags.contains(Flags::LAST));
        assert_eq!(id, 1);
        assert_eq!(payload, b"data");
        let mut trailer = writer.await.unwrap();

        // The peer no longer wants the rest of the trailer.
        scheduler.discard_active_trailer(1);

        // The writer observes a clean failure on its next write, not a
        // hang, since nothing will ever grant it another lease.
        let error = trailer.write_all(b"more").await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);

        // The next turn is an ordinary zero-length TRAILER | LAST terminal
        // commit -- not an ABORT -- exactly as if the trailer had completed
        // normally.
        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, payload) = read_wire_fragment(&mut reader).await;
        assert!(flags.contains(Flags::TRAILER) && flags.contains(Flags::LAST));
        assert!(!flags.contains(Flags::ABORT));
        assert_eq!(id, 1);
        assert!(payload.is_empty());
    }
}
