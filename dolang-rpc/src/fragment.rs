//! Fragmented framing: wire header, receive-side reassembly, and the
//! send-side round-robin scheduler. Shared between [`crate::client`] and
//! [`crate::server`], which differ only in which [`Kind`]s they originate
//! and dispatch.

use std::collections::{HashMap, VecDeque};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::{
    Error, Kind, Limits,
    transport::{AnySender, RecvFrame, SendFrame, Sender},
};

/// Fragment header flag bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Flags(u8);

impl Flags {
    pub(crate) const NONE: Flags = Flags(0);
    pub(crate) const FIRST: Flags = Flags(0b001);
    pub(crate) const LAST: Flags = Flags(0b010);
    pub(crate) const ABORT: Flags = Flags(0b100);
    const VALID: u8 = Self::FIRST.0 | Self::LAST.0 | Self::ABORT.0;

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
    pub(crate) fn encode(&self) -> Bytes {
        let payload_len = u32::try_from(self.payload_len).expect("fragment payload is too large");
        let raw = RawFragmentHeader::new(self.flags, self.kind, self.id, payload_len);
        Bytes::copy_from_slice(&raw.as_bytes())
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

struct Incomplete {
    kind: Kind,
    buffer: BytesMut,
}

/// A message whose fragments have all been received and reassembled.
pub(crate) struct CompleteMessage {
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    pub(crate) payload: Bytes,
}

/// Receive-side fragment reassembly, keyed by message ID.
pub(crate) struct Reassembler {
    limits: Limits,
    incomplete: HashMap<u64, Incomplete>,
    incomplete_bytes: usize,
}

impl Reassembler {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            incomplete: HashMap::new(),
            incomplete_bytes: 0,
        }
    }

    /// Validates `header`, reads exactly `header.payload_len` bytes directly
    /// off `frame` (appending them into this message's accumulation
    /// buffer), and updates per-id bookkeeping. Returns the completed
    /// message once its LAST (or a fast-path FIRST|LAST) fragment has been
    /// read.
    pub(crate) async fn accept_fragment<F: RecvFrame>(
        &mut self,
        header: FragmentHeader,
        frame: &mut F,
    ) -> Result<Option<CompleteMessage>, Error> {
        let FragmentHeader {
            flags,
            kind,
            id,
            payload_len,
        } = header;
        let first = flags.contains(Flags::FIRST);
        let last = flags.contains(Flags::LAST);
        let abort = flags.contains(Flags::ABORT);

        if abort {
            if first || last {
                return Err(Error::Protocol(
                    "ABORT fragment must not also set FIRST or LAST".into(),
                ));
            }
            if payload_len != 0 {
                return Err(Error::Protocol(
                    "ABORT fragment must not carry a payload".into(),
                ));
            }
            let entry = self.incomplete.remove(&id).ok_or_else(|| {
                Error::Protocol(format!("ABORT for message {id} with no active fragments"))
            })?;
            self.incomplete_bytes -= entry.buffer.len();
            return Ok(None);
        }

        if first {
            if self.incomplete.contains_key(&id) {
                return Err(Error::Protocol(format!(
                    "duplicate FIRST fragment for message {id}"
                )));
            }
        } else {
            match self.incomplete.get(&id) {
                None => {
                    return Err(Error::Protocol(format!(
                        "fragment for message {id} without an active message"
                    )));
                }
                Some(entry) if entry.kind != kind => {
                    return Err(Error::Protocol(format!(
                        "inconsistent message kind for message {id}"
                    )));
                }
                Some(_) => {}
            }
        }

        let fast_path = first && last;
        let fragment_limit = if fast_path {
            self.limits.max_message_size
        } else {
            self.limits.max_fragment_size
        };
        if payload_len > fragment_limit {
            return Err(Error::Protocol(format!(
                "fragment of {payload_len} bytes for message {id} exceeds the limit of {fragment_limit}"
            )));
        }

        if fast_path {
            let mut dest = BytesMut::with_capacity(payload_len);
            read_payload(frame, &mut dest, payload_len).await?;
            return Ok(Some(CompleteMessage {
                kind,
                id,
                payload: dest.freeze(),
            }));
        }

        if first {
            if self.incomplete.len() >= self.limits.max_incomplete_messages {
                return Err(Error::Protocol("too many incomplete messages".into()));
            }
            self.incomplete.insert(
                id,
                Incomplete {
                    kind,
                    buffer: BytesMut::with_capacity(payload_len),
                },
            );
        }

        let entry = self
            .incomplete
            .get_mut(&id)
            .expect("presence already validated above");
        if entry.buffer.len() + payload_len > self.limits.max_message_size {
            let removed = self.incomplete.remove(&id).expect("looked up above");
            self.incomplete_bytes -= removed.buffer.len();
            return Err(Error::Protocol(format!(
                "message {id} exceeds the maximum message size"
            )));
        }
        if self.incomplete_bytes + payload_len > self.limits.max_incomplete_bytes {
            return Err(Error::Protocol("too many incomplete bytes".into()));
        }
        entry.buffer.reserve(payload_len);
        read_payload(frame, &mut entry.buffer, payload_len).await?;
        self.incomplete_bytes += payload_len;

        if last {
            let entry = self
                .incomplete
                .remove(&id)
                .expect("entry was just populated");
            self.incomplete_bytes -= entry.buffer.len();
            return Ok(Some(CompleteMessage {
                kind: entry.kind,
                id,
                payload: entry.buffer.freeze(),
            }));
        }

        Ok(None)
    }
}

/// One outbound message the scheduler is actively (or about to be) sending.
struct ActiveSend {
    id: u64,
    kind: Kind,
    payload: Bytes,
    offset: usize,
    /// Whether this send occupies a concurrency slot (`payload` did not fit
    /// in one fragment at admission time).
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
    /// Sum of the full payload length of every started (FIRST sent), not
    /// yet completed (LAST sent) multi-fragment send — mirrors the peer
    /// `Reassembler`'s `incomplete_bytes` counter, so starting a message
    /// never commits the peer to buffering more than `max_incomplete_bytes`.
    outstanding_bytes: usize,
    max_incomplete_bytes: usize,
    max_fragment_size: usize,
}

impl Scheduler {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            active: VecDeque::new(),
            waiting: VecDeque::new(),
            control: VecDeque::new(),
            active_fragmented: 0,
            max_active_fragmented: limits.max_incomplete_messages.max(1),
            outstanding_bytes: 0,
            max_incomplete_bytes: limits.max_incomplete_bytes,
            max_fragment_size: limits.max_fragment_size,
        }
    }

    /// Admits a request/response payload for sending.
    pub(crate) fn admit_message(&mut self, kind: Kind, id: u64, payload: Bytes) {
        if payload.len() <= self.max_fragment_size {
            self.active.push_back(ActiveSend {
                id,
                kind,
                payload,
                offset: 0,
                multi_fragment: false,
            });
            return;
        }
        let payload_len = payload.len();
        let send = ActiveSend {
            id,
            kind,
            payload,
            offset: 0,
            multi_fragment: true,
        };
        if self.active_fragmented < self.max_active_fragmented
            && self.outstanding_bytes + payload_len <= self.max_incomplete_bytes
        {
            self.active_fragmented += 1;
            self.outstanding_bytes += payload_len;
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
    pub(crate) fn try_cancel_active(&mut self, id: u64) -> AbortOutcome {
        if let Some(pos) = self.waiting.iter().position(|s| s.id == id) {
            self.waiting.remove(pos);
            return AbortOutcome::Discarded { started: false };
        }
        if let Some(pos) = self.active.iter().position(|s| s.id == id) {
            let send = self.active.remove(pos).expect("position was just found");
            let started = send.offset > 0;
            if send.multi_fragment {
                self.free_fragmented_slot(send.payload.len());
            }
            return AbortOutcome::Discarded { started };
        }
        AbortOutcome::NotActive
    }

    /// Releases the concurrency slot and byte budget held by a completed or
    /// cancelled multi-fragment send, then promotes the next waiting send if
    /// it now fits within both the slot and byte budget.
    fn free_fragmented_slot(&mut self, payload_len: usize) {
        self.active_fragmented -= 1;
        self.outstanding_bytes -= payload_len;
        let outstanding_bytes = self.outstanding_bytes;
        let max_incomplete_bytes = self.max_incomplete_bytes;
        if let Some(next) = self
            .waiting
            .pop_front_if(|next| outstanding_bytes + next.payload.len() <= max_incomplete_bytes)
        {
            self.active_fragmented += 1;
            self.outstanding_bytes += next.payload.len();
            self.active.push_back(next);
        }
    }

    /// Whether `advance` has anything to send right now.
    pub(crate) fn has_work(&self) -> bool {
        !self.control.is_empty() || !self.active.is_empty()
    }

    /// Sends one control fragment if any are queued (priority); otherwise
    /// sends up to `max_fragment_size` bytes from the front of `active` as
    /// one fragment, re-queuing it at the back if not yet complete.
    pub(crate) async fn advance(&mut self, transport: &mut AnySender) -> Result<(), Error> {
        if let Some(control) = self.control.pop_front() {
            return self.send_control(transport, control).await;
        }
        let Some(mut send) = self.active.pop_front() else {
            return Ok(());
        };
        let start = send.offset;
        let end = (start + self.max_fragment_size).min(send.payload.len());
        let is_last = end == send.payload.len();
        let mut flags = Flags::NONE;
        if start == 0 {
            flags = flags | Flags::FIRST;
        }
        if is_last {
            flags = flags | Flags::LAST;
        }
        let header = FragmentHeader {
            flags,
            kind: send.kind,
            id: send.id,
            payload_len: end - start,
        };
        let mut buffer = header.encode().chain(send.payload.slice(start..end));
        transport.send().finish(&mut buffer).await?;
        send.offset = end;
        if is_last {
            if send.multi_fragment {
                self.free_fragmented_slot(send.payload.len());
            }
        } else {
            self.active.push_back(send);
        }
        Ok(())
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

    #[tokio::test]
    async fn first_last_fragment_is_the_fast_path_and_bypasses_incomplete_bookkeeping() {
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let msg = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap()
            .expect("fast path completes immediately");
        assert_eq!(msg.kind, Kind::Request);
        assert_eq!(&msg.payload[..], b"hello");
        assert_eq!(reassembler.incomplete.len(), 0);
        assert_eq!(reassembler.incomplete_bytes, 0);
    }

    #[tokio::test]
    async fn continuation_fragments_append_directly_into_the_same_buffer() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"hello, ");
        bytes.extend(fragment_bytes(Flags::NONE, 1, Kind::Request, b"world"));
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Request, b"!"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(Limits::default());

        for _ in 0..2 {
            let header = read_fragment_header(&mut frame).await.unwrap();
            assert!(
                reassembler
                    .accept_fragment(header, &mut frame)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        let header = read_fragment_header(&mut frame).await.unwrap();
        let msg = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap()
            .expect("LAST completes the message");
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        let msg = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&msg.payload[..], b"one");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert_eq!(header.id, 2);
        let msg = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&msg.payload[..], b"two");
    }

    #[tokio::test]
    async fn payload_read_handles_partial_chunked_delivery() {
        let bytes = fragment_bytes(Flags::FIRST | Flags::LAST, 1, Kind::Request, b"hello");
        // Split the wire bytes into single-byte chunks to force many partial
        // `recv()` calls across both the header and payload reads.
        let pieces = bytes.into_iter().map(|b| vec![b]).collect();
        let mut frame = FakeRecvFrame::chunked(pieces);
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let msg = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&msg.payload[..], b"hello");
    }

    #[tokio::test]
    async fn rejects_duplicate_first_fragment() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a"));
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();

        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"b"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_continuation_without_active_message() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::NONE, 1, Kind::Request, b"a"));
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_fragment_after_terminal_fragment() {
        let mut bytes = fragment_bytes(Flags::FIRST | Flags::LAST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::NONE, 1, Kind::Request, b"b"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_inconsistent_kind_in_continuation() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Response, b"b"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
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
        let mut reassembler = Reassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_single_fragment_exceeding_max_message_size() {
        let limits = Limits {
            max_message_size: 4,
            ..Limits::default()
        };
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = Reassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_reassembled_message_exceeding_max_message_size() {
        let limits = Limits {
            max_fragment_size: 4,
            max_message_size: 6,
            ..Limits::default()
        };
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"abcd");
        bytes.extend(fragment_bytes(Flags::LAST, 1, Kind::Request, b"abcd"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
        assert_eq!(reassembler.incomplete_bytes, 0);
    }

    #[tokio::test]
    async fn rejects_too_many_incomplete_messages() {
        let limits = Limits {
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut reassembler = Reassembler::new(limits);
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();

        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 2, Kind::Request, b"a"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_too_many_incomplete_bytes() {
        let limits = Limits {
            max_incomplete_bytes: 4,
            ..Limits::default()
        };
        let mut reassembler = Reassembler::new(limits);
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"ab"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();

        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 2, Kind::Request, b"abcd"));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn rejects_nonzero_payload_on_abort() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a");
        bytes.extend(fragment_bytes(Flags::ABORT, 1, Kind::Request, b"x"));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn abort_discards_accumulated_buffer_without_completing() {
        let mut bytes = fragment_bytes(Flags::FIRST, 1, Kind::Request, b"abcd");
        bytes.extend(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut frame = FakeRecvFrame::new(bytes);
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        assert_eq!(reassembler.incomplete_bytes, 4);
        let header = read_fragment_header(&mut frame).await.unwrap();
        let result = reassembler
            .accept_fragment(header, &mut frame)
            .await
            .unwrap();
        assert!(result.is_none());
        assert_eq!(reassembler.incomplete.len(), 0);
        assert_eq!(reassembler.incomplete_bytes, 0);
    }

    #[tokio::test]
    async fn rejects_abort_for_unknown_message() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept_fragment(header, &mut frame).await,
            Err(Error::Protocol(_))
        ));
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

    #[tokio::test]
    async fn scheduler_round_robins_between_active_messages() {
        let limits = Limits {
            max_fragment_size: 4,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
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
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        // Fits in one fragment; must not be blocked by the slot above.
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"hi"));
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
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
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
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
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
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"hi"));
        scheduler.active.pop_front();
        assert!(matches!(
            scheduler.try_cancel_active(1),
            AbortOutcome::NotActive
        ));
    }

    #[test]
    fn scheduler_try_cancel_active_reports_not_started_before_any_fragment_sent() {
        let mut scheduler = Scheduler::new(&Limits::default());
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
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
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        if let Some(send) = scheduler.active.front_mut() {
            send.offset = 4;
        }
        match scheduler.try_cancel_active(1) {
            AbortOutcome::Discarded { started } => assert!(started),
            AbortOutcome::NotActive => panic!("expected Discarded"),
        }
    }

    #[test]
    fn scheduler_defers_multi_fragment_message_when_incomplete_byte_budget_is_full() {
        let limits = Limits {
            max_fragment_size: 4,
            // Plenty of concurrency slots, but only enough byte budget for
            // one 8-byte message at a time.
            max_incomplete_messages: 8,
            max_incomplete_bytes: 8,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
        assert_eq!(scheduler.active.len(), 1);
        assert_eq!(scheduler.waiting.len(), 1);
        assert_eq!(scheduler.outstanding_bytes, 8);
    }

    #[tokio::test]
    async fn scheduler_promotes_waiting_message_when_byte_budget_frees() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 8,
            max_incomplete_bytes: 8,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
        assert_eq!(scheduler.waiting.len(), 1);
        let (mut sender, mut reader) = sender_pair();

        // Drive message 1 to completion; only then should message 2's
        // 8 bytes fit within the 8-byte budget.
        scheduler.advance(&mut sender).await.unwrap();
        let _ = read_wire_fragment(&mut reader).await;
        assert_eq!(scheduler.waiting.len(), 1);

        scheduler.advance(&mut sender).await.unwrap();
        let (flags, _, id, _) = read_wire_fragment(&mut reader).await;
        assert_eq!(id, 1);
        assert!(flags.contains(Flags::LAST));
        assert_eq!(scheduler.waiting.len(), 0);
        assert_eq!(scheduler.outstanding_bytes, 8);
    }

    #[test]
    fn scheduler_try_cancel_active_discards_waiting_message_without_abort() {
        let limits = Limits {
            max_fragment_size: 4,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(Kind::Request, 1, Bytes::from_static(b"AAAAAAAA"));
        scheduler.admit_message(Kind::Request, 2, Bytes::from_static(b"BBBBBBBB"));
        assert_eq!(scheduler.waiting.len(), 1);
        match scheduler.try_cancel_active(2) {
            AbortOutcome::Discarded { started } => assert!(!started),
            AbortOutcome::NotActive => panic!("expected Discarded"),
        }
        assert_eq!(scheduler.waiting.len(), 0);
        assert_eq!(scheduler.active_fragmented, 1);
    }
}
