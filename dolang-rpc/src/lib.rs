#![deny(warnings)]
//! Framed, multiplexed RPC sessions over asynchronous byte streams.
//!
//! Define a [`Protocol`], negotiate a transport with [`Builder`], then bind
//! the negotiated endpoint to that protocol. The client may issue concurrent
//! calls; [`server::Server::serve`] dispatches concurrent request handlers.
//!
//! ```no_run
//! use dolang_rpc::{Builder, Protocol, server::CallContext};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Deserialize, Serialize)]
//! enum Request { Ping }
//! #[derive(Deserialize, Serialize)]
//! enum Response { Pong }
//! struct Example;
//! impl Protocol for Example {
//!     type Request = Request;
//!     type Response = Response;
//! }
//!
//! async fn run() -> Result<(), Box<dyn std::error::Error>> {
//!     let (client_io, server_io) = tokio::io::duplex(16 * 1024);
//!     let (client, server) = tokio::try_join!(
//!         Builder::new("example", &[1]).client(client_io),
//!         Builder::new("example", &[1]).server(server_io),
//!     )?;
//!
//!     let server = async {
//!         server.bind::<Example>().serve(async |mut context: CallContext<Example>, request| {
//!             context.shutdown();
//!             match request {
//!                 Request::Ping => context.respond(Response::Pong),
//!             }
//!         }).await
//!     };
//!     let client = async {
//!         let response = client.bind::<Example>().call(Request::Ping).await?.into_response();
//!         assert!(matches!(response, Response::Pong));
//!         Ok::<_, dolang_rpc::Error>(())
//!     };
//!     let (server, client) = tokio::join!(server, client);
//!     server?;
//!     client?;
//!     Ok(())
//! }
//! ```

pub mod client;
mod fragment;
pub mod handle;
mod serde;
pub mod server;
pub mod session;
pub mod trailer;
mod transport;
mod unbound;

use ::serde::{Serialize, de::DeserializeOwned};
use bytes::Bytes;
use transport::{RecvFrame, SendFrame};
pub use unbound::Builder;

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

/// A family of messages exchanged by one RPC session.
///
/// Implement this marker trait once for each application protocol version
/// represented by distinct Rust request and response types. Both peers must
/// bind the negotiated connection to compatible implementations.
pub trait Protocol: Send + Sync + 'static {
    /// Messages sent by [`client::Client`] calls and received by
    /// [`server::Server`] handlers.
    type Request: Serialize + DeserializeOwned + Send + 'static;
    /// Messages returned by [`server::Server`] handlers and yielded by
    /// completed [`client::Call`]s.
    type Response: Serialize + DeserializeOwned + Send + 'static;
}

/// An error from session establishment, transport, or an individual call.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The underlying transport failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Serializing an outgoing request or response failed.
    #[error("serialization error: {0}")]
    Serialize(String),
    /// Deserializing an incoming request or response failed.
    #[error("deserialization error: {0}")]
    Deserialize(String),
    /// The peer sent data that violates the RPC protocol.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The local or peer session closed before the operation completed.
    #[error("connection closed")]
    ConnectionClosed,
    /// The peer cancelled this call before it received a response.
    #[error("request cancelled")]
    Cancelled,
    /// A requested transport capability is unavailable.
    ///
    /// This variant is reserved for capability-reporting APIs; direct handle
    /// serialization on an unsupported generic transport currently panics
    /// instead.
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
