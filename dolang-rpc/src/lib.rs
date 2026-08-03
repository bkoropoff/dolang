//! Framed, multiplexed RPC sessions over asynchronous byte streams.

mod client;
mod fragment;
mod handle;
mod opaque;
mod serde;
mod server;
mod transport;

use ::serde::{Serialize, de::DeserializeOwned};
use bytes::Bytes;
pub use client::{Call, Client};
pub use handle::{DefaultHandle, OsHandle};
pub use opaque::{InvalidOpaque, Opaque, OpaqueGuard, OpaqueResource};
pub use server::{CallContext, RequestCancelled, Server};
use transport::{RecvFrame, SendFrame};

/// Configurable size and concurrency limits for a session.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum payload size of one fragment. Bounds how much of a large
    /// message is written per round-robin turn.
    pub max_fragment_size: usize,
    /// Maximum size of one complete (reassembled) message.
    pub max_message_size: usize,
    /// Maximum number of messages with fragments in flight at once.
    pub max_incomplete_messages: usize,
    /// Maximum aggregate bytes accumulated across all incomplete messages.
    pub max_incomplete_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_fragment_size: 64 * 1024,
            max_message_size: 16 * 1024 * 1024,
            max_incomplete_messages: 64,
            max_incomplete_bytes: 64 * 1024 * 1024,
        }
    }
}

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
    fn copy(&self) -> Self {
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
