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
#[cfg(unix)]
use handle::ErasedHandle;
use handle::{PutHandle, TakeHandle};
use transport::RecvFrame;
#[cfg(unix)]
use transport::SendFrame;
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
/// Native handles are stolen from `value` only after serialization succeeds.
#[cfg(unix)]
struct FramePutHandle<'handle> {
    handles: Vec<&'handle dyn ErasedHandle>,
}

#[cfg(unix)]
impl<'frame> PutHandle<'frame> for FramePutHandle<'frame> {
    fn put_handle(&mut self, handle: &'frame dyn ErasedHandle) -> std::io::Result<u32> {
        if self
            .handles
            .iter()
            .any(|existing| std::ptr::eq(*existing, handle))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the same operating-system handle was serialized more than once",
            ));
        }
        let index = u32::try_from(self.handles.len()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "too many operating-system handles in one message",
            )
        })?;
        self.handles.push(handle);
        Ok(index)
    }
}

#[cfg(unix)]
type OwnedHandles = Vec<std::os::fd::OwnedFd>;

#[cfg(unix)]
fn steal_handles(handles: Vec<&dyn ErasedHandle>) -> OwnedHandles {
    handles
        .into_iter()
        .map(ErasedHandle::steal_handle)
        .collect()
}

#[cfg(unix)]
fn attach_handles<'frame, F: SendFrame<'frame>>(
    handles: &'frame OwnedHandles,
    frame: &mut F,
) -> std::io::Result<()> {
    use std::os::fd::AsFd;

    for (index, handle) in handles.iter().enumerate() {
        let attached = frame.attach_fd(handle.as_fd())?;
        if attached as usize != index {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "transport returned an unexpected handle attachment index",
            ));
        }
    }
    Ok(())
}

struct FrameTakeHandle<'borrow, F>(&'borrow mut F);

impl<F: RecvFrame> TakeHandle for FrameTakeHandle<'_, F> {
    #[cfg(unix)]
    fn take_handle(&mut self, index: u32) -> std::io::Result<std::os::fd::OwnedFd> {
        self.0.take_fd(index)
    }

    #[cfg(windows)]
    fn take_handle(&mut self, value: usize) -> std::io::Result<std::os::windows::io::OwnedHandle> {
        self.0.take_handle(value)
    }
}

#[cfg(unix)]
fn encode_payload<T: Serialize>(value: &T) -> Result<(Bytes, OwnedHandles), Error> {
    let mut handles = FramePutHandle {
        handles: Vec::new(),
    };
    let buffer = serde::to_extend(value, &mut handles, Vec::new())
        .map_err(|e| Error::Serialize(e.to_string()))?;
    let handles = steal_handles(handles.handles);
    Ok((buffer.into(), handles))
}

#[cfg(windows)]
fn encode_payload<'handle, T: Serialize, H: PutHandle<'handle>>(
    value: &'handle T,
    handles: &mut H,
) -> Result<Bytes, Error> {
    let buffer = serde::to_extend(value, handles, Vec::new())
        .map_err(|e| Error::Serialize(e.to_string()))?;
    Ok(buffer.into())
}

fn decode<T: DeserializeOwned>(bytes: &[u8], frame: &mut impl RecvFrame) -> Result<T, Error> {
    serde::from_bytes(bytes, &mut FrameTakeHandle(frame))
        .map_err(|e| Error::Deserialize(e.to_string()))
}

#[cfg(all(test, unix))]
mod handle_tests {
    use std::{
        io,
        os::fd::{BorrowedFd, OwnedFd},
        task::{Context, Poll},
    };

    use ::serde::{Serialize, ser::SerializeStruct};
    use nix::unistd::pipe;

    use super::*;
    use crate::handle::OsHandle;

    struct TestFrame(Vec<i32>);

    impl<'frame> SendFrame<'frame> for TestFrame {
        fn attach_fd(&mut self, fd: BorrowedFd<'frame>) -> io::Result<u32> {
            use std::os::fd::AsRawFd;

            let index = self.0.len() as u32;
            self.0.push(fd.as_raw_fd());
            Ok(index)
        }

        fn poll_write_once(
            &mut self,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            unreachable!()
        }
    }

    #[derive(Serialize)]
    struct OneHandle<'a> {
        handle: &'a OsHandle<OwnedFd>,
    }

    #[derive(Serialize)]
    struct RepeatedHandle<'a> {
        first: &'a OsHandle<OwnedFd>,
        second: &'a OsHandle<OwnedFd>,
    }

    struct FailsAfterHandle<'a>(&'a OsHandle<OwnedFd>);

    impl Serialize for FailsAfterHandle<'_> {
        fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut state = serializer.serialize_struct("FailsAfterHandle", 2)?;
            state.serialize_field("handle", self.0)?;
            Err(::serde::ser::Error::custom("intentional failure"))
        }
    }

    #[test]
    fn successful_serialization_steals_handle() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let (_, owned) = encode_payload(&OneHandle { handle: &handle }).unwrap();
        assert_eq!(owned.len(), 1);
        let mut frame = TestFrame(Vec::new());
        attach_handles(&owned, &mut frame).unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { handle.into_inner() }))
                .is_err()
        );
    }

    #[test]
    fn failed_serialization_does_not_steal_handle() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        assert!(encode_payload(&FailsAfterHandle(&handle)).is_err());
        drop(handle.into_inner());
    }

    #[test]
    fn repeated_handle_is_rejected_without_being_stolen() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let value = RepeatedHandle {
            first: &handle,
            second: &handle,
        };
        assert!(encode_payload(&value).is_err());
        drop(handle.into_inner());
    }
}
