//! Staged construction: negotiate first, choose a concrete [`Protocol`]
//! afterward.
//!
//! [`Client<P>`](crate::Client)/[`Server<P>`](crate::Server) are generic over
//! a statically known `P`, but which concrete `P` to use can depend on the
//! *negotiated* application-protocol version (e.g. a future protocol
//! revision might be represented as a distinct Rust type). [`UnboundClient`]
//! and [`UnboundServer`] negotiate an application protocol first, expose
//! what was negotiated, and only then let the caller [`bind`](UnboundClient::bind)
//! to a concrete `P`.
//!
//! [`Builder`] is the sole entry point for constructing either one: it takes
//! the mandatory application-protocol descriptor up front, offers chainable
//! setters for individual size/concurrency limits, and a terminal method per
//! transport shape (`client`/`client_split`/... or `server`/`server_split`/...).

use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

#[cfg(windows)]
use std::os::windows::io::OwnedHandle;

#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};

use crate::{Client, Error, Limits, Protocol, Server, fragment, transport};

/// Builds an [`UnboundClient`] or [`UnboundServer`].
///
/// A builder advertises one application-protocol name and supported versions.
/// Its terminal `client*` or `server*` method consumes it, performs the
/// handshake, and returns an unbound endpoint that can be inspected before
/// binding it to a concrete [`Protocol`]. Limit setters override defaults;
/// size and concurrency limits are negotiated to the more conservative value,
/// while copy thresholds are local performance settings.
pub struct Builder {
    name: String,
    versions: Vec<u16>,
    limits: Limits,
}

impl Builder {
    /// Starts a builder for an application-protocol name and supported versions.
    ///
    /// `versions` must be nonempty, unique, and in ascending order. The
    /// builder preserves the supplied order; violating this requirement can
    /// prevent negotiation from selecting the highest mutually supported
    /// version.
    pub fn new(name: &str, versions: &[u16]) -> Self {
        // FIXME: Validate the documented nonempty, unique, ascending input
        // rather than deferring a malformed list's consequences to handshake.
        Self {
            name: name.to_owned(),
            versions: versions.to_vec(),
            limits: Limits::default(),
        }
    }

    /// Sets the maximum complete wire-fragment size, including its header.
    ///
    /// This bounds one round-robin write of a fragmented message. Defaults to
    /// 512 KiB; the peer and local endpoint use the smaller advertised value.
    pub fn max_fragment_size(mut self, value: usize) -> Self {
        self.limits.max_fragment_size = value;
        self
    }

    /// Sets the maximum reassembled postcard payload, excluding a trailer.
    ///
    /// Defaults to 2 MiB; the peer and local endpoint use the smaller
    /// advertised value.
    pub fn max_payload_size(mut self, value: usize) -> Self {
        self.limits.max_payload_size = value;
        self
    }

    /// Sets the maximum total raw-byte trailer size for one message.
    ///
    /// Defaults to 2 MiB; the peer and local endpoint use the smaller
    /// advertised value.
    pub fn max_trailer_size(mut self, value: usize) -> Self {
        self.limits.max_trailer_size = value;
        self
    }

    /// Sets the receive-side eager-copy threshold for an undemanded fragment.
    ///
    /// A fragment at or below this size is copied immediately, allowing the
    /// connection receive loop to continue without waiting for the trailer
    /// reader. Defaults to 64 KiB. Set zero to disable nonempty eager copies.
    pub fn trailer_recv_copy_threshold(mut self, value: usize) -> Self {
        self.limits.trailer_recv_copy_threshold = value;
        self
    }

    /// Sets the receive-side eager-copy threshold for a demanded fragment.
    ///
    /// This applies when the trailer reader is already waiting for the next
    /// fragment. Defaults to 256 KiB. Set zero to disable nonempty eager
    /// copies on this path.
    pub fn trailer_recv_demand_copy_threshold(mut self, value: usize) -> Self {
        self.limits.trailer_recv_demand_copy_threshold = value;
        self
    }

    /// Sets the send-side staging threshold for a trailer fragment.
    ///
    /// A write at or below this size is copied into staging without waiting
    /// for a transport grant. Defaults to 64 KiB. Set zero to disable
    /// nonempty eager staging.
    pub fn trailer_send_copy_threshold(mut self, value: usize) -> Self {
        self.limits.trailer_send_copy_threshold = value;
        self
    }

    /// Sets the maximum number of messages with fragments in flight at once.
    ///
    /// Defaults to 64; the peer and local endpoint use the smaller advertised
    /// value.
    pub fn max_incomplete_messages(mut self, value: usize) -> Self {
        self.limits.max_incomplete_messages = value;
        self
    }

    /// Sets the maximum in-flight messages that may have an open trailer.
    ///
    /// This further restricts [`max_incomplete_messages`](Self::max_incomplete_messages).
    /// Defaults to 16; the peer and local endpoint use the smaller advertised
    /// value.
    pub fn max_incomplete_trailers(mut self, value: usize) -> Self {
        self.limits.max_incomplete_trailers = value;
        self
    }

    fn app_protocol(&self) -> (&str, &[u16]) {
        (&self.name, &self.versions)
    }

    /// Negotiates a client session over a bidirectional byte stream.
    pub async fn client<T>(self, stream: T) -> Result<UnboundClient, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (sender, receiver) = transport::generic_duplex(stream);
        negotiate_client(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            self.limits,
            false,
            #[cfg(windows)]
            None,
            self.app_protocol(),
        )
        .await
    }

    /// Negotiates a client session over separate byte-stream reader and writer
    /// halves.
    pub async fn client_split<R, W>(self, reader: R, writer: W) -> Result<UnboundClient, Error>
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        let (sender, receiver) = transport::generic(reader, writer);
        negotiate_client(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            self.limits,
            false,
            #[cfg(windows)]
            None,
            self.app_protocol(),
        )
        .await
    }

    #[cfg(unix)]
    /// Negotiates a client session over a connected Unix domain socket.
    ///
    /// Unlike [`client`](Self::client), this transport supports direct
    /// [`OsHandle`](crate::OsHandle) attachments.
    pub async fn client_unix(self, stream: UnixStream) -> Result<UnboundClient, Error> {
        let (sender, receiver) = transport::unix::unix(stream)?;
        negotiate_client(
            transport::AnySender::Unix(sender),
            transport::AnyReceiver::Unix(receiver),
            self.limits,
            false,
            #[cfg(windows)]
            None,
            self.app_protocol(),
        )
        .await
    }

    #[cfg(windows)]
    /// Starts a client session on the server end of a Windows named pipe.
    ///
    /// `peer_process` is retained for the lifetime of the session and must
    /// grant process-query and synchronization access. Construction fails if
    /// it does not identify the named-pipe peer.
    ///
    /// # Safety
    ///
    /// The identified peer must be trusted to send only handle values that it
    /// created in this process with `DuplicateHandle`. A malicious peer can
    /// otherwise cause this process to close arbitrary handles.
    pub async unsafe fn client_named_pipe_server(
        self,
        pipe: NamedPipeServer,
        peer_process: OwnedHandle,
    ) -> Result<UnboundClient, Error> {
        crate::client::validate_peer_process(
            &peer_process,
            transport::windows::server_pipe_peer_pid(&pipe)?,
        )?;
        let (sender, receiver) = transport::windows::server_pipe(pipe, false)?;
        negotiate_client(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            self.limits,
            true,
            Some(peer_process),
            self.app_protocol(),
        )
        .await
    }

    #[cfg(windows)]
    /// Starts a client session on the client end of a Windows named pipe.
    ///
    /// `peer_process` is retained for the lifetime of the session and must
    /// grant process-query and synchronization access. Construction fails if
    /// it does not identify the named-pipe peer.
    ///
    /// # Safety
    ///
    /// The identified peer must be trusted to send only handle values that it
    /// created in this process with `DuplicateHandle`. A malicious peer can
    /// otherwise cause this process to close arbitrary handles.
    pub async unsafe fn client_named_pipe_client(
        self,
        pipe: NamedPipeClient,
        peer_process: OwnedHandle,
    ) -> Result<UnboundClient, Error> {
        crate::client::validate_peer_process(
            &peer_process,
            transport::windows::client_pipe_peer_pid(&pipe)?,
        )?;
        let (sender, receiver) = transport::windows::client_pipe(pipe, false)?;
        negotiate_client(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            self.limits,
            true,
            Some(peer_process),
            self.app_protocol(),
        )
        .await
    }

    /// Negotiates a server session over a bidirectional byte stream.
    pub async fn server<T>(self, stream: T) -> Result<UnboundServer, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (sender, receiver) = transport::generic_duplex(stream);
        negotiate_server(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            self.limits,
            self.app_protocol(),
        )
        .await
    }

    /// Negotiates a server session over separate byte-stream reader and writer
    /// halves.
    pub async fn server_split<R, W>(self, reader: R, writer: W) -> Result<UnboundServer, Error>
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        let (sender, receiver) = transport::generic(reader, writer);
        negotiate_server(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            self.limits,
            self.app_protocol(),
        )
        .await
    }

    #[cfg(unix)]
    /// Negotiates a server session over a connected Unix domain socket.
    ///
    /// Unlike [`server`](Self::server), this transport supports direct
    /// [`OsHandle`](crate::OsHandle) attachments.
    pub async fn server_unix(self, stream: UnixStream) -> Result<UnboundServer, Error> {
        let (sender, receiver) = transport::unix::unix(stream)?;
        negotiate_server(
            transport::AnySender::Unix(sender),
            transport::AnyReceiver::Unix(receiver),
            self.limits,
            self.app_protocol(),
        )
        .await
    }

    #[cfg(windows)]
    /// Creates a server on the server end of a Windows named pipe.
    pub async fn server_named_pipe_server(
        self,
        pipe: NamedPipeServer,
    ) -> Result<UnboundServer, Error> {
        let (sender, receiver) = transport::windows::server_pipe(pipe, true)?;
        negotiate_server(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            self.limits,
            self.app_protocol(),
        )
        .await
    }

    #[cfg(windows)]
    /// Creates a server on the client end of a Windows named pipe.
    pub async fn server_named_pipe_client(
        self,
        pipe: NamedPipeClient,
    ) -> Result<UnboundServer, Error> {
        let (sender, receiver) = transport::windows::client_pipe(pipe, true)?;
        negotiate_server(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            self.limits,
            self.app_protocol(),
        )
        .await
    }
}

async fn negotiate_client(
    mut sender: transport::AnySender,
    mut receiver: transport::AnyReceiver,
    limits: Limits,
    keep_requests_alive: bool,
    #[cfg(windows)] peer_process: Option<OwnedHandle>,
    app_protocol: (&str, &[u16]),
) -> Result<UnboundClient, Error> {
    // The RPC framing version itself is an implementation detail,
    // uninteresting once binding to `P` — only the application-protocol
    // version negotiated below is surfaced.
    let negotiated = fragment::negotiate(&mut sender, &mut receiver, &limits, app_protocol).await?;
    Ok(UnboundClient {
        sender,
        receiver,
        limits: negotiated.limits,
        keep_requests_alive,
        #[cfg(windows)]
        peer_process,
        app_protocol: negotiated.app_protocol,
    })
}

async fn negotiate_server(
    mut sender: transport::AnySender,
    mut receiver: transport::AnyReceiver,
    limits: Limits,
    app_protocol: (&str, &[u16]),
) -> Result<UnboundServer, Error> {
    // The RPC framing version itself is an implementation detail,
    // uninteresting once binding to `P` — only the application-protocol
    // version negotiated below is surfaced.
    let negotiated = fragment::negotiate(&mut sender, &mut receiver, &limits, app_protocol).await?;
    Ok(UnboundServer {
        sender,
        receiver,
        limits: negotiated.limits,
        app_protocol: negotiated.app_protocol,
    })
}

/// A negotiated client endpoint that has not yet been bound to a [`Protocol`].
///
/// Inspect [`name`](Self::name) and [`version`](Self::version), select the
/// compatible Rust protocol type, then consume this value with [`bind`](Self::bind).
pub struct UnboundClient {
    sender: transport::AnySender,
    receiver: transport::AnyReceiver,
    limits: Limits,
    keep_requests_alive: bool,
    #[cfg(windows)]
    peer_process: Option<OwnedHandle>,
    app_protocol: (String, u16),
}

impl UnboundClient {
    /// The negotiated application-protocol name.
    pub fn name(&self) -> &str {
        &self.app_protocol.0
    }

    /// The negotiated application-protocol version. This is the
    /// application protocol's own version, not the underlying RPC framing
    /// version — the latter is an implementation detail of `dolang-rpc`
    /// uninteresting to callers choosing a `P` to bind to.
    pub fn version(&self) -> u16 {
        self.app_protocol.1
    }

    /// Consumes this endpoint and binds it to a concrete protocol type.
    ///
    /// The caller is responsible for choosing a `P` compatible with the
    /// negotiated application-protocol name and version.
    pub fn bind<P: Protocol>(self) -> Client<P> {
        Client::from_transport(
            self.sender,
            self.receiver,
            self.limits,
            self.keep_requests_alive,
            #[cfg(windows)]
            self.peer_process,
        )
    }
}

/// A negotiated server endpoint that has not yet been bound to a [`Protocol`].
///
/// Inspect [`name`](Self::name) and [`version`](Self::version), select the
/// compatible Rust protocol type, then consume this value with [`bind`](Self::bind).
pub struct UnboundServer {
    sender: transport::AnySender,
    receiver: transport::AnyReceiver,
    limits: Limits,
    app_protocol: (String, u16),
}

impl UnboundServer {
    /// The negotiated application-protocol name.
    pub fn name(&self) -> &str {
        &self.app_protocol.0
    }

    /// The negotiated application-protocol version. This is the
    /// application protocol's own version, not the underlying RPC framing
    /// version — the latter is an implementation detail of `dolang-rpc`
    /// uninteresting to callers choosing a `P` to bind to.
    pub fn version(&self) -> u16 {
        self.app_protocol.1
    }

    /// Consumes this endpoint and binds it to a concrete protocol type.
    ///
    /// The caller is responsible for choosing a `P` compatible with the
    /// negotiated application-protocol name and version.
    pub fn bind<P: Protocol>(self) -> Server<P> {
        Server::from_transport(self.sender, self.receiver, self.limits)
    }
}
