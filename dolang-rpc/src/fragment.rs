//! Fragmented framing: wire header, receive-side reassembly, and the
//! send-side round-robin scheduler. Shared between [`crate::client`] and
//! [`crate::server`], which differ only in which [`Kind`]s they originate
//! and dispatch.

use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::Arc,
    task::Poll,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use ::serde::{Deserialize, Serialize};

use crate::{
    Error, Limits, NEGOTIATE_FRAGMENT_SIZE, NEGOTIATE_MAX_PAYLOAD_SIZE,
    trailer::{RecvShared, SendAction, SendShared},
    transport::{
        AnyReceiver, AnySender, OutgoingHandles, ReceivedHandles, Receiver, RecvFrame, SendFrame,
        Sender,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Kind {
    Request = 0,
    Response = 1,
    Error = 2,
    Cancel = 3,
    /// Advisory: the sender no longer wants any more `TRAILER` fragments for
    /// the given message id. Unlike `Cancel`, this never affects the
    /// message's own request/response outcome — it only tells the peer to
    /// stop streaming a trailer it already committed to sending.
    Discard = 4,
    /// Protocol handshake/version negotiation. Sent unprompted and first by
    /// both ends of a connection before any other `Kind` is valid; see
    /// [`negotiate`].
    Negotiate = 5,
}

impl TryFrom<u8> for Kind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Request),
            1 => Ok(Self::Response),
            2 => Ok(Self::Error),
            3 => Ok(Self::Cancel),
            4 => Ok(Self::Discard),
            5 => Ok(Self::Negotiate),
            _ => Err(Error::Protocol(format!("unknown frame kind {value}"))),
        }
    }
}

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
    // Reserved for future use; always zero-filled on write, never validated
    // or surfaced on read (a peer sending non-zero reserved bytes today must
    // not be rejected). Fields are ordered by ascending size so each falls
    // on a naturally aligned offset (0, 1, 2, 4, 8) if ever read from an
    // aligned buffer, though decoding today is manual `from_le_bytes` and
    // doesn't rely on that.
    reserved: [u8; 2],
    payload_len: [u8; 4],
    id: [u8; 8],
}

impl RawFragmentHeader {
    const LEN: usize = size_of::<Self>();

    fn new(flags: Flags, kind: Kind, id: u64, payload_len: u32) -> Self {
        Self {
            flags: [flags.bits()],
            kind: [kind as u8],
            reserved: [0; 2],
            payload_len: payload_len.to_le_bytes(),
            id: id.to_le_bytes(),
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
        // Intentionally read but discarded: `reserved` is forward-compatible
        // padding, never validated or surfaced.
        let _ = header.reserved;
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

/// The protocol version this build speaks. `negotiate` advertises this as
/// its sole supported version; a future version bump adds another entry
/// here (and to the version-selection logic below) rather than replacing it.
const PROTOCOL_VERSION: u8 = 1;

/// Version 1's handshake payload: the limits this endpoint enforces on
/// incoming traffic. `negotiate` reduces each field of the local `Limits` to
/// the minimum of the local and peer values, so both ends converge on the
/// same effective limits — one side raising a limit has no effect unless the
/// peer also raises it, and either side can unilaterally cap what actually
/// gets used on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HandshakeV1 {
    max_fragment_size: u32,
    max_payload_size: u32,
    max_trailer_size: u32,
    max_handles_per_fragment: u32,
    max_handles_per_message: u32,
    max_incomplete_messages: u32,
    max_incomplete_trailers: u32,
}

impl HandshakeV1 {
    fn from_limits(limits: &Limits) -> Self {
        let clamp = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        #[cfg(unix)]
        let max_handles_per_fragment = limits
            .max_handles_per_fragment
            .min(crate::transport::unix::MAX_FDS_PER_FRAGMENT);
        #[cfg(not(unix))]
        let max_handles_per_fragment = limits.max_handles_per_fragment;
        Self {
            max_fragment_size: clamp(limits.max_fragment_size),
            max_payload_size: clamp(limits.max_payload_size),
            max_trailer_size: clamp(limits.max_trailer_size),
            max_handles_per_fragment: clamp(max_handles_per_fragment),
            max_handles_per_message: clamp(limits.max_handles_per_message),
            max_incomplete_messages: clamp(limits.max_incomplete_messages),
            max_incomplete_trailers: clamp(limits.max_incomplete_trailers),
        }
    }

    /// Reduces the size/concurrency fields of `limits` to the minimum of
    /// their current value and this (peer-advertised) handshake's value.
    /// The buffering-threshold fields (`trailer_*_copy_threshold`) aren't
    /// part of the handshake at all — they only affect local behavior and
    /// don't need agreement.
    fn clamp_limits(&self, limits: &mut Limits) {
        limits.max_fragment_size = limits
            .max_fragment_size
            .min(self.max_fragment_size as usize);
        limits.max_payload_size = limits.max_payload_size.min(self.max_payload_size as usize);
        limits.max_trailer_size = limits.max_trailer_size.min(self.max_trailer_size as usize);
        limits.max_handles_per_fragment = limits
            .max_handles_per_fragment
            .min(self.max_handles_per_fragment as usize);
        #[cfg(unix)]
        {
            limits.max_handles_per_fragment = limits
                .max_handles_per_fragment
                .min(crate::transport::unix::MAX_FDS_PER_FRAGMENT);
        }
        limits.max_handles_per_message = limits
            .max_handles_per_message
            .min(self.max_handles_per_message as usize);
        limits.max_incomplete_messages = limits
            .max_incomplete_messages
            .min(self.max_incomplete_messages as usize);
        limits.max_incomplete_trailers = limits
            .max_incomplete_trailers
            .min(self.max_incomplete_trailers as usize);
    }
}

fn postcard_err(error: postcard::Error) -> Error {
    Error::Protocol(format!("negotiate: {error}"))
}

/// Result of a successful [`negotiate`] call.
#[derive(Debug)]
pub(crate) struct NegotiationResult {
    /// The negotiated RPC framing version. Not yet consulted by any caller
    /// (mirrors `HandshakeV1`, see its doc comment) — only version 1 exists,
    /// so there is nothing to branch on yet.
    #[allow(dead_code)]
    pub(crate) version: u8,
    /// The negotiated application-protocol name and version.
    pub(crate) app_protocol: (String, u16),
    /// The local `Limits` passed to `negotiate`, with each size/concurrency
    /// field reduced to the minimum of the local and peer values. See
    /// `HandshakeV1::clamp_limits`.
    pub(crate) limits: Limits,
}

/// The negotiate payload's outer shape: RPC-framing-version blobs (see
/// `negotiate`'s doc comment) alongside the mandatory application-protocol
/// name + sorted ascending supported-version list. Application-protocol
/// versions are `u16` rather than `u8` since application protocols are
/// expected to revise far more often than the RPC framing format, and they
/// travel in the payload rather than the 8-slot wire `id` field, so they
/// aren't bound by its capacity.
///
/// postcard serializes a struct as the plain sequence of its fields, the
/// same as a tuple of the same types in the same order — using a struct here
/// is purely for readability at the call sites and does not change the wire
/// format.
#[derive(Debug, Serialize, Deserialize)]
struct NegotiatePayload {
    version_blobs: Vec<Vec<u8>>,
    app_protocol: (String, Vec<u16>),
}

/// Performs the protocol handshake: exchanges supported-version lists and
/// per-version metadata, then returns the highest mutually supported
/// version. Must run to completion before any other `Kind` is sent or
/// accepted on `sender`/`receiver`.
///
/// The wire `id` field of a `Negotiate` fragment is repurposed to hold this
/// endpoint's sorted, ascending, zero-terminated list of supported 8-bit
/// RPC framing version numbers (at most 8 fit in the 8-byte field). The
/// payload is a postcard-encoded [`NegotiatePayload`]: the first element is
/// a `Vec<Vec<u8>>` of one length-prefixed, version-specific blob per
/// non-zero entry in the id array, in the same order; the second is this
/// endpoint's optional application-protocol descriptor. Because postcard
/// already length-prefixes `Vec<u8>`, a receiver can decode the outer vector
/// — and so locate any entry — without knowing the schema of versions it
/// doesn't support.
///
/// Both ends select the same RPC framing version independently (the maximum
/// of the intersection of the two advertised lists), so no acknowledgement
/// round trip is needed. If there is no overlap, this sends a `FIRST|ABORT`
/// fragment as a failsafe/diagnostic signal and returns an error.
///
/// `app_protocol` is a mandatory `(name, sorted ascending supported
/// versions)` pair for the application protocol layered on top of the RPC
/// framing — every caller has one to offer (there is no raw-RPC-only path;
/// see the [module documentation](crate::unbound)), so there is no skip/opt-out
/// case to represent. The peer's name must match exactly and there must be a
/// mutually supported version; either failure sends the `FIRST|ABORT` signal
/// and returns a distinct error from the RPC-version-mismatch case.
pub(crate) async fn negotiate(
    sender: &mut AnySender,
    receiver: &mut AnyReceiver,
    limits: &Limits,
    app_protocol: (&str, &[u16]),
) -> Result<NegotiationResult, Error> {
    let local_blob =
        postcard::to_stdvec(&HandshakeV1::from_limits(limits)).map_err(postcard_err)?;
    let (local_name, local_versions) = app_protocol;
    let local_app_protocol = (local_name.to_string(), local_versions.to_vec());
    let local_payload = NegotiatePayload {
        version_blobs: vec![local_blob],
        app_protocol: local_app_protocol,
    };
    let local_payload = postcard::to_stdvec(&local_payload).map_err(postcard_err)?;
    let mut local_id = [0u8; 8];
    local_id[0] = PROTOCOL_VERSION;

    // Drive the local write and the peer read concurrently: sequencing them
    // (write fully, then read) risks a deadlock if either side's handshake
    // payload is large enough to fill transport buffering before its peer
    // starts draining it.
    let (write_result, read_result) = tokio::join!(
        write_negotiate_message(sender, local_id, &local_payload),
        read_negotiate_message(receiver),
    );
    write_result?;
    let (peer_id, peer_payload) = read_result?;

    let peer_versions: Vec<u8> = peer_id.into_iter().take_while(|&v| v != 0).collect();
    let Some(negotiated) = [PROTOCOL_VERSION]
        .into_iter()
        .rev()
        .find(|version| peer_versions.contains(version))
    else {
        // Best-effort: the peer may already have reached the same
        // conclusion and closed its end, in which case this send fails.
        // That doesn't change what error we return here — we already know
        // why negotiation failed, and a symmetric peer that's also aborting
        // doesn't need the signal anyway.
        let _ = send_negotiate_abort(sender).await;
        return Err(Error::Protocol(
            "no mutually supported RPC protocol version".into(),
        ));
    };

    let NegotiatePayload {
        version_blobs: blobs,
        app_protocol: peer_app_protocol,
    } = postcard::from_bytes(&peer_payload).map_err(postcard_err)?;
    let index = peer_versions
        .iter()
        .position(|&version| version == negotiated)
        .expect("negotiated version was found in peer_versions");
    let blob = blobs.get(index).ok_or_else(|| {
        Error::Protocol("missing handshake payload for negotiated version".into())
    })?;
    let peer_handshake: HandshakeV1 = postcard::from_bytes(blob).map_err(postcard_err)?;
    let mut effective_limits = *limits;
    peer_handshake.clamp_limits(&mut effective_limits);

    let (peer_name, peer_app_versions) = peer_app_protocol;
    if local_name != peer_name {
        // Best-effort; see the RPC-version-mismatch case above.
        let _ = send_negotiate_abort(sender).await;
        return Err(Error::Protocol(format!(
            "mismatched application protocol: local {local_name:?}, peer {peer_name:?}"
        )));
    }
    let Some(&negotiated_app_version) = local_versions
        .iter()
        .rev()
        .find(|version| peer_app_versions.contains(version))
    else {
        let _ = send_negotiate_abort(sender).await;
        return Err(Error::Protocol(format!(
            "no mutually supported version of application protocol {local_name:?}"
        )));
    };
    let app_protocol = (local_name.to_string(), negotiated_app_version);

    Ok(NegotiationResult {
        version: negotiated,
        app_protocol,
        limits: effective_limits,
    })
}

/// Writes one `Kind::Negotiate` message, chunked into
/// `NEGOTIATE_FRAGMENT_SIZE`-bounded fragments with `FIRST`/`LAST` flags.
async fn write_negotiate_message(
    sender: &mut AnySender,
    id: [u8; 8],
    payload: &[u8],
) -> Result<(), Error> {
    let id = u64::from_le_bytes(id);
    let total = payload.len();
    let mut offset = 0;
    loop {
        let end = (offset + NEGOTIATE_FRAGMENT_SIZE).min(total);
        let chunk = &payload[offset..end];
        let first = offset == 0;
        let last = end == total;
        let mut flags = Flags::NONE;
        if first {
            flags = flags | Flags::FIRST;
        }
        if last {
            flags = flags | Flags::LAST;
        }
        let header = FragmentHeader {
            flags,
            kind: Kind::Negotiate,
            id,
            payload_len: chunk.len(),
        };
        let mut buffer = BytesMut::with_capacity(RawFragmentHeader::LEN + chunk.len());
        header.encode_into(&mut buffer);
        buffer.put_slice(chunk);
        let mut buffer = buffer.freeze();
        sender.send().finish(&mut buffer).await.map_err(Error::Io)?;
        sender.flush().await?;
        offset = end;
        if last {
            break;
        }
    }
    Ok(())
}

/// Sends the `FIRST|ABORT` no-compatible-version failsafe signal.
async fn send_negotiate_abort(sender: &mut AnySender) -> Result<(), Error> {
    let header = FragmentHeader {
        flags: Flags::FIRST | Flags::ABORT,
        kind: Kind::Negotiate,
        id: 0,
        payload_len: 0,
    };
    let mut buffer = header.encode();
    sender.send().finish(&mut buffer).await.map_err(Error::Io)?;
    sender.flush().await?;
    Ok(())
}

/// Reads one `Kind::Negotiate` message, accumulating payload bytes across
/// continuation fragments until `LAST`. Returns the peer's id-array bytes
/// and full payload. Treats a `FIRST|ABORT` fragment as the peer signaling
/// incompatible versions, surfaced as an error.
async fn read_negotiate_message(receiver: &mut AnyReceiver) -> Result<([u8; 8], Vec<u8>), Error> {
    let mut payload = BytesMut::new();
    let mut id = [0u8; 8];
    let mut started = false;
    loop {
        let mut frame = receiver.recv();
        let header = read_fragment_header(&mut frame).await?;
        if header.kind != Kind::Negotiate {
            return Err(Error::Protocol(format!(
                "expected a Negotiate frame, got {:?}",
                header.kind
            )));
        }
        let first = header.flags.contains(Flags::FIRST);
        let last = header.flags.contains(Flags::LAST);
        let abort = header.flags.contains(Flags::ABORT);
        if abort {
            if !first || last || header.flags.contains(Flags::TRAILER) || header.payload_len != 0 {
                return Err(Error::Protocol("invalid negotiate ABORT fragment".into()));
            }
            return Err(Error::Protocol(
                "peer aborted RPC protocol negotiation (no mutually supported version)".into(),
            ));
        }
        if header.payload_len > NEGOTIATE_FRAGMENT_SIZE {
            return Err(Error::Protocol(
                "negotiate fragment exceeds the minimum tolerated size".into(),
            ));
        }
        if payload.len() + header.payload_len > NEGOTIATE_MAX_PAYLOAD_SIZE {
            return Err(Error::Protocol(
                "negotiate message exceeds the maximum tolerated total size".into(),
            ));
        }
        if first {
            if started {
                return Err(Error::Protocol("duplicate FIRST negotiate fragment".into()));
            }
            started = true;
            id = header.id.to_le_bytes();
        } else if !started {
            return Err(Error::Protocol(
                "negotiate fragment received before FIRST".into(),
            ));
        }
        read_payload(&mut frame, &mut payload, header.payload_len).await?;
        if last {
            break;
        }
    }
    Ok((id, payload.to_vec()))
}

pub(crate) struct Message {
    pub(crate) kind: Kind,
    pub(crate) id: u64,
    pub(crate) payload: Bytes,
    pub(crate) handles: ReceivedHandles,
    pub(crate) trailer: Option<Arc<std::sync::Mutex<RecvShared>>>,
}

pub(crate) enum Event {
    None,
    Aborted {
        kind: Kind,
        id: u64,
        dispatched: bool,
    },
    Message(Message),
    Trailer {
        id: u64,
        message: Option<Message>,
        shared: Arc<std::sync::Mutex<RecvShared>>,
        len: usize,
        /// Set when the local consumer had already discarded this trailer
        /// (via [`crate::trailer::TrailerRecv::discard`] or by dropping it) before
        /// this *subsequent* fragment arrived — i.e. the peer is still
        /// sending more than we want. Never set on the fragment that first
        /// hands the trailer to the application. The caller should tell the
        /// peer to stop (`Kind::Discard`) exactly once per message when
        /// this is set.
        notify_discard: bool,
    },
}

struct Incomplete {
    kind: Kind,
    postcard: BytesMut,
    handles: ReceivedHandles,
    trailer: Option<Arc<std::sync::Mutex<RecvShared>>>,
    trailer_len: usize,
    dispatched: bool,
    discard_notified: bool,
}

/// Reassembles postcard data while handing trailer fragments to a live
/// [`RecvShared`] without buffering their bytes.
pub(crate) struct Reassembler {
    limits: Limits,
    incomplete: HashMap<u64, Incomplete>,
    /// Number of `incomplete` entries whose `trailer` is (or was, at some
    /// point while incomplete) `Some`. Enforces `max_incomplete_trailers`
    /// independent of `max_incomplete_messages`.
    incomplete_trailers: usize,
}

impl Reassembler {
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
    ) -> Result<Event, Error> {
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
        #[cfg(unix)]
        let mut fragment_handles = frame.drain_fds();
        #[cfg(unix)]
        if fragment_handles.len() > self.limits.max_handles_per_fragment {
            return Err(Error::Protocol(format!(
                "fragment for message {id} exceeds the maximum native-handle count"
            )));
        }

        if abort {
            #[cfg(unix)]
            if !fragment_handles.is_empty() {
                return Err(Error::Protocol(
                    "ABORT fragment contains file descriptor attachments".into(),
                ));
            }
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
            return Ok(Event::Aborted {
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

        #[cfg(unix)]
        if trailer && !fragment_handles.is_empty() {
            return Err(Error::Protocol(
                "trailer fragment contains file descriptor attachments".into(),
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
                return Ok(Event::None);
            }
            return Ok(Event::Message(Message {
                kind,
                id,
                payload: entry.postcard.freeze(),
                handles: entry.handles,
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
            #[cfg(unix)]
            fragment_handles.extend(frame.drain_fds());
            #[cfg(unix)]
            if fragment_handles.len() > self.limits.max_handles_per_fragment {
                return Err(Error::Protocol(format!(
                    "fragment for message {id} exceeds the maximum native-handle count"
                )));
            }
            #[cfg(unix)]
            if fragment_handles.len() > self.limits.max_handles_per_message {
                return Err(Error::Protocol(format!(
                    "message {id} exceeds the maximum native-handle count"
                )));
            }
            #[cfg(unix)]
            if !matches!(kind, Kind::Request | Kind::Response) && !fragment_handles.is_empty() {
                return Err(Error::Protocol(format!(
                    "{kind:?} fragment contains file descriptor attachments"
                )));
            }
            #[allow(unused_mut)]
            let mut handles: ReceivedHandles = Default::default();
            #[cfg(unix)]
            handles.extend(fragment_handles);
            return Ok(Event::Message(Message {
                kind,
                id,
                payload: payload.freeze(),
                handles,
                trailer: None,
            }));
        }

        if first {
            self.incomplete.insert(
                id,
                Incomplete {
                    kind,
                    postcard: BytesMut::new(),
                    handles: Default::default(),
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
                Some(Message {
                    kind,
                    id,
                    payload: entry.postcard.clone().freeze(),
                    handles: std::mem::take(&mut entry.handles),
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
            return Ok(Event::Trailer {
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
        #[cfg(unix)]
        {
            fragment_handles.extend(frame.drain_fds());
            if fragment_handles.len() > self.limits.max_handles_per_fragment {
                return Err(Error::Protocol(format!(
                    "fragment for message {id} exceeds the maximum native-handle count"
                )));
            }
            if entry.handles.len() + fragment_handles.len() > self.limits.max_handles_per_message {
                return Err(Error::Protocol(format!(
                    "message {id} exceeds the maximum native-handle count"
                )));
            }
            if !matches!(kind, Kind::Request | Kind::Response) && !fragment_handles.is_empty() {
                return Err(Error::Protocol(format!(
                    "{kind:?} fragment contains file descriptor attachments"
                )));
            }
            entry.handles.extend(fragment_handles);
        }

        if last {
            let entry = self.incomplete.remove(&id).unwrap();
            return Ok(Event::Message(Message {
                kind,
                id,
                payload: entry.postcard.freeze(),
                handles: entry.handles,
                trailer: None,
            }));
        }
        Ok(Event::None)
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
    #[cfg(unix)]
    handles: OutgoingHandles,
    #[cfg(unix)]
    handle_offset: usize,
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
/// Constructed from the negotiated `Limits` (see `HandshakeV1::clamp_limits`),
/// which is already the minimum of the local and peer values, so throttling
/// against it here is throttling against whichever side is more
/// conservative — the peer's `Reassembler` never sees more concurrency than
/// it asked for.
pub(crate) struct Scheduler {
    active: VecDeque<ActiveSend>,
    /// Multi-fragment sends admitted but not yet started (no concurrency
    /// slot, or no byte budget, free at admission time).
    waiting: VecDeque<ActiveSend>,
    control: VecDeque<ControlSend>,
    active_fragmented: usize,
    max_active_fragmented: usize,
    /// Payload budget per fragment write, already reduced from
    /// `limits.max_fragment_size` by `RawFragmentHeader::LEN` so that
    /// `limits.max_fragment_size` bounds the whole wire fragment (header +
    /// payload) actually written per round-robin turn, not just the payload.
    max_fragment_size: usize,
    #[cfg(unix)]
    max_handles_per_fragment: usize,
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
            max_fragment_size: limits
                .max_fragment_size
                .saturating_sub(RawFragmentHeader::LEN)
                .max(1),
            #[cfg(unix)]
            max_handles_per_fragment: limits.max_handles_per_fragment,
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

    pub(crate) fn admit_message(
        &mut self,
        kind: Kind,
        id: u64,
        payload: Bytes,
        handles: OutgoingHandles,
        trailer: Trailer,
    ) {
        #[cfg(unix)]
        let handles_fit = handles.fds.len() <= self.max_handles_per_fragment;
        #[cfg(not(unix))]
        let handles_fit = true;
        #[cfg(not(unix))]
        let _ = handles;
        if trailer.is_none() && payload.len() <= self.max_fragment_size && handles_fit {
            self.active.push_back(ActiveSend {
                id,
                kind,
                payload,
                offset: 0,
                #[cfg(unix)]
                handles,
                #[cfg(unix)]
                handle_offset: 0,
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
            #[cfg(unix)]
            handles,
            #[cfg(unix)]
            handle_offset: 0,
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
    /// producer. Once this resolves, `advance` must be driven to completion
    /// without racing ordinary message admission: it may commit part of a
    /// fragment before yielding on transport readiness.
    pub(crate) async fn ready(&self) {
        std::future::poll_fn(|cx| {
            if !self.control.is_empty() {
                return Poll::Ready(());
            }
            for send in &self.active {
                if send.offset != send.payload.len() {
                    return Poll::Ready(());
                }
                match &send.trailer {
                    Trailer::Stream(shared) if SendShared::poll_action(shared, cx).is_pending() => {
                    }
                    _ => return Poll::Ready(()),
                }
            }
            Poll::Pending
        })
        .await
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

        #[cfg(unix)]
        let handles_pending = send.handle_offset < send.handles.fds.len();
        #[cfg(not(unix))]
        let handles_pending = false;
        if send.offset < send.payload.len() || handles_pending || must_open_with_postcard {
            let start = send.offset;
            let end = (start + self.effective_fragment_size()).min(send.payload.len());
            let postcard_done = end == send.payload.len();
            #[allow(unused_mut)]
            let mut frame = transport.send();
            #[cfg(unix)]
            let attached = if handles_pending {
                let batch_end = (send.handle_offset + self.max_handles_per_fragment)
                    .min(send.handles.fds.len());
                let attached =
                    frame.attach_fds(&send.handles.fds[send.handle_offset..batch_end])?;
                if attached == 0 || attached > batch_end - send.handle_offset {
                    return Err(Error::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "transport accepted an invalid native-handle batch size",
                    )));
                }
                attached
            } else {
                0
            };
            #[cfg(unix)]
            let handles_done = send.handle_offset + attached == send.handles.fds.len();
            #[cfg(not(unix))]
            let handles_done = true;
            let mut flags = Flags::NONE;
            if first {
                flags = flags | Flags::FIRST;
            }
            if postcard_done && handles_done && send.trailer.is_none() {
                flags = flags | Flags::LAST;
            }
            let header = FragmentHeader {
                flags,
                kind: send.kind,
                id: send.id,
                payload_len: end - start,
            };
            let mut buffer = header.encode().chain(send.payload.slice(start..end));
            let atomic = frame.finish(&mut buffer).await?;
            self.record_write_atomicity(atomic);
            send.offset = end;
            #[cfg(unix)]
            {
                send.handle_offset += attached;
            }
            send.started = true;
            if postcard_done && handles_done && send.trailer.is_none() {
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
        fn drain_fds(&mut self) -> Vec<OwnedFd> {
            Vec::new()
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

    #[test]
    fn raw_fragment_header_len_is_16_bytes() {
        assert_eq!(RawFragmentHeader::LEN, 16);
    }

    #[test]
    fn raw_fragment_header_round_trips_with_reserved_bytes_ignored() {
        let header = FragmentHeader {
            flags: Flags::FIRST | Flags::LAST,
            kind: Kind::Request,
            id: 0x0102_0304_0506_0708,
            payload_len: 42,
        };
        let mut bytes = header.encode().to_vec();
        // Corrupt the reserved bytes (offset 2..4) with a non-zero pattern;
        // decode must still succeed and ignore them.
        bytes[2] = 0xAA;
        bytes[3] = 0xBB;
        let bytes: [u8; RawFragmentHeader::LEN] = bytes.try_into().unwrap();
        let (flags, kind, id, payload_len) = RawFragmentHeader::decode(&bytes).unwrap();
        assert_eq!(flags, header.flags);
        assert_eq!(kind, header.kind);
        assert_eq!(id, header.id);
        assert_eq!(payload_len, header.payload_len);
    }

    #[tokio::test]
    async fn first_last_fragment_is_the_fast_path_and_bypasses_incomplete_bookkeeping() {
        let mut frame = FakeRecvFrame::new(fast_path_bytes(1, Kind::Request, b"hello"));
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
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
        let mut reassembler = Reassembler::new(Limits::default());

        for _ in 0..2 {
            let header = read_fragment_header(&mut frame).await.unwrap();
            assert!(matches!(
                reassembler.accept(header, &mut frame).await.unwrap(),
                Event::None
            ));
        }
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"one");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert_eq!(header.id, 2);
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
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
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
            panic!("expected a completed message");
        };
        assert_eq!(&msg.payload[..], b"hello");
    }

    #[tokio::test]
    async fn rejects_duplicate_first_fragment() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::FIRST, 1, Kind::Request, b"a"));
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(limits);
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
        let mut reassembler = Reassembler::new(limits);
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
        let mut reassembler = Reassembler::new(limits);
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
        let mut reassembler = Reassembler::new(limits);
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
        let mut reassembler = Reassembler::new(limits);
        let mut frame = FakeRecvFrame::new(fragment_bytes(
            Flags::FIRST | Flags::TRAILER,
            1,
            Kind::Request,
            b"a",
        ));
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            Event::Trailer { .. }
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
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        let event = reassembler.accept(header, &mut frame).await.unwrap();
        assert!(matches!(
            event,
            Event::Aborted {
                dispatched: false,
                ..
            }
        ));
        assert_eq!(reassembler.incomplete.len(), 0);
    }

    #[tokio::test]
    async fn rejects_abort_for_unknown_message() {
        let mut frame = FakeRecvFrame::new(fragment_bytes(Flags::ABORT, 1, Kind::Request, b""));
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
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
        let mut reassembler = Reassembler::new(Limits::default());
        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            Event::None
        ));
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Message(msg) = reassembler.accept(header, &mut frame).await.unwrap() else {
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            Event::None
        ));

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer {
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
            Event::None
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            Event::None
        ));

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer {
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
        let Event::Trailer {
            message: None, len, ..
        } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a subsequent TRAILER fragment to not redispatch the message");
        };
        assert_eq!(&drain_trailer_bytes(&mut frame, len).await[..], b"cd");

        let header = read_fragment_header(&mut frame).await.unwrap();
        assert!(matches!(
            reassembler.accept(header, &mut frame).await.unwrap(),
            Event::None
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer { len, .. } = reassembler.accept(header, &mut frame).await.unwrap()
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer {
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
            Event::None
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
        let mut reassembler = Reassembler::new(Limits::default());
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer { len, .. } = reassembler.accept(header, &mut frame).await.unwrap()
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
        let mut reassembler = Reassembler::new(limits);
        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer { len, .. } = reassembler.accept(header, &mut frame).await.unwrap()
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
        let mut reassembler = Reassembler::new(limits);
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer { len, .. } = reassembler.accept(header, &mut frame).await.unwrap()
        else {
            panic!("expected a TRAILER data event");
        };
        drain_trailer_bytes(&mut frame, len).await;
        assert_eq!(reassembler.incomplete_trailers, 1);

        let header = read_fragment_header(&mut frame).await.unwrap();
        let event = reassembler.accept(header, &mut frame).await.unwrap();
        assert!(matches!(
            event,
            Event::Aborted {
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
            max_fragment_size: 1024 + RawFragmentHeader::LEN,
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
            max_fragment_size: 1024 + RawFragmentHeader::LEN,
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
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Default::default(),
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
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        // Occupies the only fragmented-concurrency slot.
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        // Fits in one fragment; must not be blocked by the slot above.
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"hi"),
            Default::default(),
            Trailer::None,
        );
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
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Default::default(),
            Trailer::None,
        );
        assert_eq!(scheduler.active.len(), 1);
        assert_eq!(scheduler.waiting.len(), 1);
        assert_eq!(scheduler.active_fragmented, 1);
    }

    #[tokio::test]
    async fn scheduler_wire_fragment_never_exceeds_max_fragment_size() {
        let limits = Limits {
            max_fragment_size: 20,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        let (mut sender, mut reader) = sender_pair();
        loop {
            scheduler.advance(&mut sender).await.unwrap();
            let (flags, _, _, payload) = read_wire_fragment(&mut reader).await;
            assert!(RawFragmentHeader::LEN + payload.len() <= limits.max_fragment_size);
            if flags.contains(Flags::LAST) {
                break;
            }
        }
    }

    #[tokio::test]
    async fn scheduler_promotes_waiting_message_when_a_slot_frees() {
        let limits = Limits {
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Default::default(),
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
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"hi"),
            Default::default(),
            Trailer::None,
        );
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
            Default::default(),
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
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
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
            max_fragment_size: 4 + RawFragmentHeader::LEN,
            max_incomplete_messages: 1,
            ..Limits::default()
        };
        let mut scheduler = Scheduler::new(&limits);
        scheduler.admit_message(
            Kind::Request,
            1,
            Bytes::from_static(b"AAAAAAAA"),
            Default::default(),
            Trailer::None,
        );
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"BBBBBBBB"),
            Default::default(),
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
            max_fragment_size: 1024 + RawFragmentHeader::LEN,
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
            Default::default(),
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
        scheduler.admit_message(
            Kind::Request,
            2,
            Bytes::from_static(b"hi"),
            Default::default(),
            Trailer::None,
        );
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
        let mut reassembler = Reassembler::new(Limits::default());

        let header = read_fragment_header(&mut frame).await.unwrap();
        reassembler.accept(header, &mut frame).await.unwrap();

        // First TRAILER fragment dispatches the message. `notify_discard`
        // must never fire here — the application hasn't had a chance to
        // discard anything yet.
        let header = read_fragment_header(&mut frame).await.unwrap();
        let Event::Trailer {
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
        let Event::Trailer {
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
        let Event::Trailer {
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
            Default::default(),
            Trailer::Stream(shared.clone()),
        );
        let mut trailer = crate::trailer::TrailerSend::new(shared, ());
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

    // --- Negotiate tests ---

    /// A connected pair of full duplex (sender + receiver) endpoints, unlike
    /// `sender_pair` which only wires up one direction.
    fn duplex_endpoint_pair(buffer: usize) -> ((AnySender, AnyReceiver), (AnySender, AnyReceiver)) {
        let (a_to_b_write, a_to_b_read) = tokio::io::duplex(buffer);
        let (b_to_a_write, b_to_a_read) = tokio::io::duplex(buffer);
        let (a_sender, _unused) = crate::transport::generic_duplex(a_to_b_write);
        let (_unused, a_receiver) = crate::transport::generic_duplex(b_to_a_read);
        let (b_sender, _unused) = crate::transport::generic_duplex(b_to_a_write);
        let (_unused, b_receiver) = crate::transport::generic_duplex(a_to_b_read);
        (
            (
                AnySender::Generic(a_sender),
                AnyReceiver::Generic(a_receiver),
            ),
            (
                AnySender::Generic(b_sender),
                AnyReceiver::Generic(b_receiver),
            ),
        )
    }

    #[tokio::test]
    async fn negotiate_between_two_real_endpoints_selects_the_shared_version() {
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(4096);
        let limits = Limits::default();
        let (a_result, b_result) = tokio::join!(
            negotiate(&mut a_sender, &mut a_receiver, &limits, ("test", &[1])),
            negotiate(&mut b_sender, &mut b_receiver, &limits, ("test", &[1])),
        );
        let a_result = a_result.unwrap();
        let b_result = b_result.unwrap();
        assert_eq!(a_result.version, PROTOCOL_VERSION);
        assert_eq!(b_result.version, PROTOCOL_VERSION);
        assert_eq!(a_result.app_protocol, ("test".to_string(), 1));
        assert_eq!(b_result.app_protocol, ("test".to_string(), 1));
    }

    #[tokio::test]
    async fn negotiate_aborts_and_fails_when_there_is_no_mutual_version() {
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(4096);

        // `b` fakes a peer that only advertises a version `a` doesn't speak.
        let fake_peer = async move {
            let mut id = [0u8; 8];
            id[0] = PROTOCOL_VERSION.wrapping_add(1);
            let blob = postcard::to_stdvec(&0u8).unwrap();
            let payload = NegotiatePayload {
                version_blobs: vec![blob],
                app_protocol: ("test".to_string(), vec![1]),
            };
            let payload = postcard::to_stdvec(&payload).unwrap();
            write_negotiate_message(&mut b_sender, id, &payload)
                .await
                .unwrap();
            // First `a`'s own ordinary advertisement arrives (sent
            // concurrently with `a` reading ours); only after `a` processes
            // our list and finds no overlap does it send the ABORT failsafe.
            read_negotiate_message(&mut b_receiver).await.unwrap();
            let error = read_negotiate_message(&mut b_receiver).await.unwrap_err();
            assert!(matches!(error, Error::Protocol(_)));
        };

        let limits = Limits::default();
        let (a_result, ()) = tokio::join!(
            negotiate(&mut a_sender, &mut a_receiver, &limits, ("test", &[1])),
            fake_peer,
        );
        assert!(matches!(a_result, Err(Error::Protocol(_))));
    }

    #[tokio::test]
    async fn negotiate_selects_max_overlapping_app_protocol_version() {
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(4096);
        let limits = Limits::default();
        let (a_result, b_result) = tokio::join!(
            negotiate(&mut a_sender, &mut a_receiver, &limits, ("vfs", &[1, 2, 3])),
            negotiate(&mut b_sender, &mut b_receiver, &limits, ("vfs", &[2, 3, 4])),
        );
        let a_result = a_result.unwrap();
        let b_result = b_result.unwrap();
        assert_eq!(a_result.app_protocol, ("vfs".to_string(), 3));
        assert_eq!(b_result.app_protocol, ("vfs".to_string(), 3));
    }

    #[tokio::test]
    async fn negotiate_aborts_on_mismatched_app_protocol_name() {
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(4096);
        let limits = Limits::default();
        let (a_result, b_result) = tokio::join!(
            negotiate(&mut a_sender, &mut a_receiver, &limits, ("vfs", &[1])),
            negotiate(&mut b_sender, &mut b_receiver, &limits, ("other", &[1])),
        );
        let a_error = a_result.unwrap_err();
        let b_error = b_result.unwrap_err();
        assert!(
            matches!(a_error, Error::Protocol(ref msg) if msg.contains("mismatched application protocol"))
        );
        assert!(
            matches!(b_error, Error::Protocol(ref msg) if msg.contains("mismatched application protocol"))
        );
    }

    #[tokio::test]
    async fn negotiate_aborts_on_no_overlapping_app_protocol_version() {
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(4096);
        let limits = Limits::default();
        let (a_result, b_result) = tokio::join!(
            negotiate(&mut a_sender, &mut a_receiver, &limits, ("vfs", &[1])),
            negotiate(&mut b_sender, &mut b_receiver, &limits, ("vfs", &[2])),
        );
        let a_error = a_result.unwrap_err();
        let b_error = b_result.unwrap_err();
        assert!(
            matches!(a_error, Error::Protocol(ref msg) if msg.contains("no mutually supported version of application protocol"))
        );
        assert!(
            matches!(b_error, Error::Protocol(ref msg) if msg.contains("no mutually supported version of application protocol"))
        );
    }

    #[tokio::test]
    async fn negotiate_message_spanning_multiple_fragments_reassembles() {
        let ((mut sender, _unused_receiver), (_unused_sender, mut receiver)) =
            duplex_endpoint_pair(1 << 20);
        let payload: Vec<u8> = (0..(NEGOTIATE_FRAGMENT_SIZE * 3 + 17))
            .map(|i| i as u8)
            .collect();
        let mut id = [0u8; 8];
        id[0] = 7;

        let (write_result, read_result) = tokio::join!(
            write_negotiate_message(&mut sender, id, &payload),
            read_negotiate_message(&mut receiver),
        );
        write_result.unwrap();
        let (got_id, got_payload) = read_result.unwrap();
        assert_eq!(got_id, id);
        assert_eq!(got_payload, payload);
    }

    #[tokio::test]
    async fn negotiate_message_exceeding_max_total_size_is_rejected() {
        let ((mut sender, _unused_receiver), (_unused_sender, mut receiver)) =
            duplex_endpoint_pair(1 << 21);
        let payload = vec![0u8; NEGOTIATE_MAX_PAYLOAD_SIZE + 1];
        let mut id = [0u8; 8];
        id[0] = 7;

        let (write_result, read_result) = tokio::join!(
            write_negotiate_message(&mut sender, id, &payload),
            read_negotiate_message(&mut receiver),
        );
        // The writer has no size limit of its own; only the reader enforces
        // the cap, so its write may or may not fail depending on how far
        // the reader got before erroring out and dropping the connection.
        let _ = write_result;
        let error = read_result.unwrap_err();
        assert!(
            matches!(error, Error::Protocol(ref msg) if msg.contains("exceeds the maximum tolerated total size"))
        );
    }

    #[tokio::test]
    async fn negotiate_write_and_read_do_not_deadlock_on_a_small_transport_buffer() {
        // Smaller than the multi-fragment payload below, so a naive
        // write-fully-then-read implementation on both sides would deadlock:
        // each side's write blocks on the other side draining it, which
        // never happens because the other side is also still blocked
        // writing.
        let ((mut a_sender, mut a_receiver), (mut b_sender, mut b_receiver)) =
            duplex_endpoint_pair(64);
        let payload = vec![0xABu8; NEGOTIATE_FRAGMENT_SIZE * 4];
        let mut id = [0u8; 8];
        id[0] = 3;

        let a_side = async {
            let (write_result, read_result) = tokio::join!(
                write_negotiate_message(&mut a_sender, id, &payload),
                read_negotiate_message(&mut a_receiver),
            );
            write_result.unwrap();
            read_result.unwrap().1
        };
        let b_side = async {
            let (write_result, read_result) = tokio::join!(
                write_negotiate_message(&mut b_sender, id, &payload),
                read_negotiate_message(&mut b_receiver),
            );
            write_result.unwrap();
            read_result.unwrap().1
        };

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(a_side, b_side)
        })
        .await
        .expect("negotiate write/read deadlocked on a small transport buffer");
        assert_eq!(result.0, payload);
        assert_eq!(result.1, payload);
    }
}
