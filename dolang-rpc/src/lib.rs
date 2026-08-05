#![deny(warnings)]
//! Framed, multiplexed RPC sessions over asynchronous byte streams.

mod client;
mod fragment;
mod handle;
mod opaque;
mod serde;
mod server;
mod trailer;
mod transport;
mod unbound;

use ::serde::{Serialize, de::DeserializeOwned};
use bytes::Bytes;
pub use client::{Call, Client};
pub use handle::{DefaultHandle, OsHandle};
pub use opaque::{InvalidOpaque, Opaque, OpaqueGuard, OpaqueResource};
pub use server::{CallContext, RequestCancelled, Server};
pub use trailer::{TrailerRecv, TrailerSend};
use transport::{RecvFrame, SendFrame};
pub use unbound::{Builder, UnboundClient, UnboundServer};

/// Configurable size and concurrency limits for a session. Not public — set
/// via [`Builder`]'s chainable setters instead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// Maximum size of one whole wire fragment, header included. Bounds how
    /// much of a large message is written per round-robin turn; the header
    /// is subtracted from this to get the actual payload budget per write.
    pub max_fragment_size: usize,
    /// Maximum size of one complete (reassembled) message's postcard
    /// payload, excluding any trailer.
    pub max_payload_size: usize,
    /// Maximum size of one message's trailer. Trailers stream a known,
    /// bounded suffix in chunks rather than acting as an open-ended
    /// channel, so this should be reasonably bounded.
    pub max_trailer_size: usize,
    /// Maximum trailer fragment payload copied immediately by the receive
    /// driver when the consumer has not yet requested that fragment. Set to
    /// zero to disable copying nonempty fragments on this path.
    pub trailer_recv_copy_threshold: usize,
    /// Maximum trailer fragment payload copied immediately by the receive
    /// driver when the consumer is already waiting for that fragment. Set to
    /// zero to disable copying nonempty fragments on this path.
    pub trailer_recv_demand_copy_threshold: usize,
    /// Maximum trailer fragment payload copied into staging by
    /// `TrailerSend::poll_write` without first waiting for a transport grant.
    /// Set to zero to disable copying nonempty fragments on this path.
    pub trailer_send_copy_threshold: usize,
    /// Maximum number of messages with fragments in flight at once.
    pub max_incomplete_messages: usize,
    /// Maximum number of those messages that may have an open trailer at
    /// once, further restricting `max_incomplete_messages`.
    pub max_incomplete_trailers: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_fragment_size: 512 * 1024,
            max_payload_size: 2 * 1024 * 1024,
            max_trailer_size: 2 * 1024 * 1024,
            trailer_recv_copy_threshold: 64 * 1024,
            trailer_recv_demand_copy_threshold: 256 * 1024,
            trailer_send_copy_threshold: 64 * 1024,
            max_incomplete_messages: 64,
            max_incomplete_trailers: 16,
        }
    }
}

/// Maximum size of a `Kind::Negotiate` fragment, tolerated by both ends of a
/// connection regardless of their configured `Limits`. Negotiation must use a
/// fixed, transport-independent bound rather than `Limits::max_fragment_size`
/// because neither side knows what the peer will actually enforce until
/// negotiation completes. Not configurable — a future refactor must not tie
/// this to `Limits`.
pub(crate) const NEGOTIATE_FRAGMENT_SIZE: usize = 1024;

/// Maximum total size of a reassembled `Kind::Negotiate` message payload,
/// across all of its fragments. Bounds how much a peer can make the
/// receiving end buffer before negotiation (and with it, the negotiated
/// `Limits`) is in force. A real handshake payload — version blobs plus an
/// application-protocol name and version list — is at most a few hundred
/// bytes; this leaves generous headroom without allowing unbounded growth.
/// Not configurable, for the same reason as `NEGOTIATE_FRAGMENT_SIZE`.
pub(crate) const NEGOTIATE_MAX_PAYLOAD_SIZE: usize = 64 * 1024;

/// A family of request and response messages.
pub trait Protocol: Send + Sync + 'static {
    type Request: Serialize + DeserializeOwned + Send + 'static;
    type Response: Serialize + DeserializeOwned + Send + 'static;
}

/// An RPC session error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialize(String),
    #[error("deserialization error: {0}")]
    Deserialize(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("connection closed")]
    ConnectionClosed,
    #[error("request cancelled")]
    Cancelled,
    #[error("transport does not support direct handles")]
    UnsupportedCapability,
}

impl Error {
    pub(crate) fn copy(&self) -> Self {
        match self {
            Self::Io(e) => Self::Io(std::io::Error::new(e.kind(), e.to_string())),
            Self::Serialize(e) => Self::Serialize(e.clone()),
            Self::Deserialize(e) => Self::Deserialize(e.clone()),
            Self::Protocol(e) => Self::Protocol(e.clone()),
            Self::ConnectionClosed => Self::ConnectionClosed,
            Self::Cancelled => Self::Cancelled,
            Self::UnsupportedCapability => Self::UnsupportedCapability,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Kind {
    Request = 1,
    Response = 2,
    Error = 3,
    Cancel = 4,
    Notify = 5,
    /// Advisory: the sender no longer wants any more `TRAILER` fragments for
    /// the given message id. Unlike `Cancel`, this never affects the
    /// message's own request/response outcome — it only tells the peer to
    /// stop streaming a trailer it already committed to sending.
    Discard = 6,
    /// Protocol handshake/version negotiation. Sent unprompted and first by
    /// both ends of a connection before any other `Kind` is valid; see
    /// `fragment::negotiate`.
    Negotiate = 7,
}

impl TryFrom<u8> for Kind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Request),
            2 => Ok(Self::Response),
            3 => Ok(Self::Error),
            4 => Ok(Self::Cancel),
            5 => Ok(Self::Notify),
            6 => Ok(Self::Discard),
            7 => Ok(Self::Negotiate),
            _ => Err(Error::Protocol(format!("unknown frame kind {value}"))),
        }
    }
}

/// Serializes `value` into a plain payload buffer (no fragment header).
/// `frame` is a probe token: if serialization attaches any native-handle
/// descriptors to it, the caller must send the resulting payload as a
/// single atomic fragment via that same token, bypassing the round-robin
/// scheduler entirely.
fn encode_payload<'frame, T: Serialize, F: SendFrame<'frame>>(
    value: &'frame T,
    frame: &mut F,
) -> Result<Bytes, Error> {
    let buffer =
        serde::to_extend(value, frame, Vec::new()).map_err(|e| Error::Serialize(e.to_string()))?;
    Ok(buffer.into())
}

fn decode<T: DeserializeOwned>(bytes: &[u8], frame: &mut impl RecvFrame) -> Result<T, Error> {
    serde::from_bytes(bytes, frame).map_err(|e| Error::Deserialize(e.to_string()))
}
