use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use bytes::Buf;
use futures::{
    StreamExt,
    future::{AbortHandle, Abortable},
};
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot},
};

use crate::{
    Error, InvalidOpaque, Kind, Limits, Opaque, OpaqueGuard, OpaqueResource, Protocol, decode,
    encode_payload, fragment, opaque,
    transport::{self, Receiver, SendFrame, Sender},
};

/// A server endpoint for one connection.
pub struct Server<P: Protocol> {
    sender: transport::AnySender,
    receiver: transport::AnyReceiver,
    outgoing: mpsc::UnboundedSender<Message<P::Response>>,
    outgoing_rx: mpsc::UnboundedReceiver<Message<P::Response>>,
    inner: Arc<Mutex<Inner>>,
    limits: Limits,
    marker: PhantomData<fn() -> P>,
}

enum Message<R> {
    Response {
        id: u64,
        value: R,
        trailer: fragment::Trailer,
    },
    Error {
        id: u64,
    },
    Cancel {
        id: u64,
    },
    /// We stopped reading a request trailer (it arrived unwanted) and want
    /// to tell the peer to stop sending it. Always results in a wire
    /// `Kind::Discard` fragment.
    DiscardTrailer {
        id: u64,
    },
    /// A wire `Kind::Discard` fragment arrived, telling us the peer no
    /// longer wants our response trailer. Applied to our own active send;
    /// never re-sent to the peer.
    PeerDiscarded {
        id: u64,
    },
}

struct Inner {
    outstanding: HashMap<u64, Cancellation>,
    objects: opaque::ObjectTable,
    shutdown: Option<oneshot::Sender<()>>,
}

struct Cancellation {
    signal: Option<oneshot::Sender<()>>,
    abort: AbortHandle,
}

impl<P: Protocol> Server<P> {
    /// Creates a server over a bidirectional byte stream.
    pub fn new<T: AsyncRead + AsyncWrite + Unpin + Send + 'static>(stream: T) -> Self {
        let (sender, receiver) = transport::generic_duplex(stream);
        Self::from_transport(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
        )
    }

    /// Creates a server over separate byte-stream reader and writer halves.
    pub fn new_split<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        let (sender, receiver) = transport::generic(reader, writer);
        Self::from_transport(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
        )
    }

    #[cfg(unix)]
    pub fn from_unix_stream(stream: std::os::unix::net::UnixStream) -> std::io::Result<Self> {
        let (sender, receiver) = transport::unix::unix(stream)?;
        Ok(Self::from_transport(
            transport::AnySender::Unix(sender),
            transport::AnyReceiver::Unix(receiver),
        ))
    }

    #[cfg(windows)]
    pub fn from_named_pipe_server(pipe: NamedPipeServer) -> std::io::Result<Self> {
        let (sender, receiver) = transport::windows::server_pipe(pipe, true)?;
        Ok(Self::from_transport(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
        ))
    }

    #[cfg(windows)]
    pub fn from_named_pipe_client(pipe: NamedPipeClient) -> std::io::Result<Self> {
        let (sender, receiver) = transport::windows::client_pipe(pipe, true)?;
        Ok(Self::from_transport(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
        ))
    }

    fn from_transport(sender: transport::AnySender, receiver: transport::AnyReceiver) -> Self {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver,
            outgoing,
            outgoing_rx,
            inner: Arc::new(Mutex::new(Inner {
                outstanding: HashMap::new(),
                objects: opaque::ObjectTable::default(),
                shutdown: None,
            })),
            limits: Limits::default(),
            marker: PhantomData,
        }
    }

    /// Sets explicit size and concurrency limits. Must be called before
    /// [`Server::serve`].
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Serves requests until the peer disconnects or the session fails.
    pub async fn serve<H>(self, handler: H) -> Result<(), Error>
    where
        H: AsyncFn(CallContext<P>, P::Request) + Send + Sync + 'static,
    {
        let Server {
            sender,
            mut receiver,
            outgoing,
            outgoing_rx,
            inner,
            limits,
            marker: _,
        } = self;
        let (writer_shutdown, writer_stop) = oneshot::channel();
        let mut writer = tokio::spawn(writer::<P>(sender, outgoing_rx, writer_stop, limits));
        let (shutdown, mut shutdown_requested) = oneshot::channel();
        inner.lock().unwrap().shutdown = Some(shutdown);
        let handler = Arc::new(handler);
        let mut reassembler = fragment::StreamReassembler::new(limits);
        let mut tasks = futures::stream::FuturesUnordered::new();
        let (mut result, mut writer_finished, mut graceful) = 'main: loop {
            let mut frame = receiver.recv();
            // The header/payload reads must not be dropped and restarted
            // once they've begun: any bytes already consumed from the
            // transport into their local buffers would otherwise be lost,
            // desynchronizing the stream. `step` is polled repeatedly by
            // the inner loop below (never recreated) so that racing it
            // against `tasks.next()` and `continue`-ing loses no progress.
            let complete = {
                let step = async {
                    let header = fragment::read_fragment_header(&mut frame).await?;
                    reassembler.accept(header, &mut frame).await
                };
                tokio::pin!(step);
                loop {
                    tokio::select! {
                        result = &mut step => break result,
                        Some(_) = tasks.next(), if !tasks.is_empty() => continue,
                        _ = &mut shutdown_requested => break 'main (Ok(()), false, true),
                        result = &mut writer => {
                            let result = match result {
                                Ok(result) => result,
                                Err(error) => Err(Error::Protocol(format!("server writer task failed: {error}"))),
                            };
                            break 'main (result, true, false);
                        }
                    }
                }
            };
            let complete = match complete {
                Ok(complete) => complete,
                Err(error) => break 'main (Err(error), false, false),
            };
            let (message, live_trailer) = match complete {
                fragment::StreamEvent::None => (None, None),
                fragment::StreamEvent::Aborted {
                    kind: Kind::Request,
                    ..
                } => (None, None),
                fragment::StreamEvent::Aborted { kind, .. } => {
                    break 'main (
                        Err(Error::Protocol(format!(
                            "unexpected aborted {kind:?} message"
                        ))),
                        false,
                        false,
                    );
                }
                fragment::StreamEvent::Message(message) => (Some(message), None),
                fragment::StreamEvent::Trailer {
                    id,
                    message,
                    shared,
                    len,
                    notify_discard,
                } => (message, Some((id, shared, len, notify_discard))),
            };
            if let Some(fragment::StreamMessage {
                kind,
                id,
                payload,
                trailer,
            }) = message
            {
                match kind {
                    Kind::Request => {
                        let request = match decode(&payload, &mut frame) {
                            Ok(request) => request,
                            Err(error) => break (Err(error), false, false),
                        };
                        let trailer = trailer.map(crate::TrailerRecv::new);
                        let handler = handler.clone();
                        let task_inner = inner.clone();
                        let task_outgoing = outgoing.clone();
                        let (abort, registration) = AbortHandle::new_pair();
                        tasks.push(Abortable::new(
                            async move {
                                let context = CallContext {
                                    id,
                                    inner: task_inner.clone(),
                                    request_trailer: trailer,
                                    outgoing: task_outgoing,
                                    responded: false,
                                    shutdown_on_respond: AtomicBool::new(false),
                                    limits,
                                    marker: PhantomData,
                                };
                                handler(context, request).await;
                            },
                            registration,
                        ));
                        inner.lock().unwrap().outstanding.insert(
                            id,
                            Cancellation {
                                signal: None,
                                abort,
                            },
                        );
                    }
                    Kind::Cancel => {
                        let mut state = inner.lock().unwrap();
                        if let Some(signal) = state
                            .outstanding
                            .get_mut(&id)
                            .and_then(|cancel| cancel.signal.take())
                        {
                            let _ = signal.send(());
                        } else if let Some(cancel) = state.outstanding.remove(&id) {
                            cancel.abort.abort();
                        } else {
                            let _ = outgoing.send(Message::Cancel { id });
                        }
                    }
                    Kind::Discard => {
                        let _ = outgoing.send(Message::PeerDiscarded { id });
                    }
                    _ => {
                        break (
                            Err(Error::Protocol(format!("unexpected {kind:?} frame"))),
                            false,
                            false,
                        );
                    }
                }
            }
            if let Some((id, shared, len, notify_discard)) = live_trailer {
                if notify_discard {
                    let _ = outgoing.send(Message::DiscardTrailer { id });
                }
                // SAFETY: the lease retains the receiver borrow and clears
                // the erased token before it ends.
                let lease = unsafe { crate::trailer::RecvShared::grant(&shared, frame, len) };
                let result = loop {
                    tokio::select! {
                        result = crate::trailer::RecvShared::wait_fragment(&shared) => break result,
                        Some(_) = tasks.next(), if !tasks.is_empty() => continue,
                        _ = &mut shutdown_requested => break 'main (Ok(()), false, true),
                        result = &mut writer => {
                            let result = match result {
                                Ok(result) => result,
                                Err(error) => Err(Error::Protocol(format!("server writer task failed: {error}"))),
                            };
                            break 'main (result, true, false);
                        }
                    }
                };
                if let Err(error) = result {
                    break 'main (Err(error.into()), false, false);
                }
                lease.complete();
            }
        };
        drop(receiver);
        if graceful {
            while !tasks.is_empty() {
                tokio::select! {
                    Some(_) = tasks.next() => {}
                    writer_result = &mut writer => {
                        result = match writer_result {
                            Ok(result) => result,
                            Err(error) => Err(Error::Protocol(format!("server writer task failed: {error}"))),
                        };
                        writer_finished = true;
                        graceful = false;
                    }
                }
                if !graceful {
                    break;
                }
            }
        }
        drop(outgoing);
        drop(tasks);
        if !writer_finished {
            if graceful {
                result = match writer.await {
                    Ok(writer_result) => writer_result,
                    Err(error) => Err(Error::Protocol(format!(
                        "server writer task failed: {error}"
                    ))),
                };
            } else {
                let _ = writer_shutdown.send(());
                let _ = writer.await;
            }
        }
        result
    }
}

async fn writer<P: Protocol>(
    mut sender: transport::AnySender,
    mut outgoing: mpsc::UnboundedReceiver<Message<P::Response>>,
    mut shutdown: oneshot::Receiver<()>,
    limits: Limits,
) -> Result<(), Error> {
    let mut scheduler = fragment::Scheduler::new(&limits);
    loop {
        while let Ok(message) = outgoing.try_recv() {
            admit::<P>(&mut sender, &mut scheduler, message).await?;
        }
        if !scheduler.has_work() {
            let message = tokio::select! {
                message = outgoing.recv() => message,
                _ = &mut shutdown => return Ok(()),
            };
            let Some(message) = message else {
                return Ok(());
            };
            admit::<P>(&mut sender, &mut scheduler, message).await?;
            continue;
        }
        let ready = tokio::select! {
            _ = std::future::poll_fn(|cx| scheduler.poll_ready(cx)) => true,
            message = outgoing.recv() => {
                if let Some(message) = message {
                    admit::<P>(&mut sender, &mut scheduler, message).await?;
                    false
                } else {
                    // The last producer was dropped, but already-admitted
                    // responses still have to drain before the transport is
                    // closed.
                    true
                }
            }
            _ = &mut shutdown => return Ok(()),
        };
        if ready {
            tokio::select! {
                result = scheduler.advance(&mut sender) => { result?; }
                _ = &mut shutdown => return Ok(()),
            }
        }
    }
}

/// Admits one outgoing item. `Response` payloads are probed for native-
/// handle attachments and, if present, sent as a single atomic fragment
/// immediately (bypassing the round-robin scheduler); everything else is
/// handed to `scheduler`.
async fn admit<P: Protocol>(
    sender: &mut transport::AnySender,
    scheduler: &mut fragment::Scheduler,
    message: Message<P::Response>,
) -> Result<(), Error> {
    match message {
        Message::Response { id, value, trailer } => {
            let mut probe = sender.send();
            let payload = encode_payload(&value, &mut probe)?;
            if probe.has_attachments() {
                if !matches!(&trailer, fragment::Trailer::None) {
                    return Err(Error::Protocol(
                        "responses with both native-handle attachments and a trailer are not supported"
                            .into(),
                    ));
                }
                let header = fragment::FragmentHeader {
                    flags: fragment::Flags::FIRST | fragment::Flags::LAST,
                    kind: Kind::Response,
                    id,
                    payload_len: payload.len(),
                };
                let mut buffer = header.encode().chain(payload);
                probe.finish(&mut buffer).await?;
            } else {
                drop(probe);
                scheduler.admit_message(Kind::Response, id, payload, trailer);
            }
        }
        Message::Error { id } => scheduler.admit_empty(Kind::Error, id),
        Message::Cancel { id } => match scheduler.try_cancel_active(id) {
            fragment::AbortOutcome::NotActive => {}
            fragment::AbortOutcome::Discarded { started } => {
                if started {
                    scheduler.admit_abort(id);
                }
            }
        },
        Message::DiscardTrailer { id } => scheduler.admit_empty(Kind::Discard, id),
        Message::PeerDiscarded { id } => scheduler.discard_active_trailer(id),
    }
    Ok(())
}

/// Services available while processing one request.
pub struct CallContext<P: Protocol> {
    id: u64,
    inner: Arc<Mutex<Inner>>,
    request_trailer: Option<crate::TrailerRecv>,
    outgoing: mpsc::UnboundedSender<Message<P::Response>>,
    responded: bool,
    shutdown_on_respond: AtomicBool,
    limits: Limits,
    marker: PhantomData<fn() -> P>,
}

impl<P: Protocol> CallContext<P> {
    /// Returns this request's streaming trailer body, if present.
    pub fn request_trailer(&mut self) -> Option<&mut crate::TrailerRecv> {
        self.request_trailer.as_mut()
    }

    /// Sends an ordinary response and consumes this call context.
    pub fn respond(mut self, response: P::Response) {
        if let Some(trailer) = self.request_trailer.as_mut() {
            trailer.discard();
        }
        self.responded = true;
        self.inner.lock().unwrap().outstanding.remove(&self.id);
        let _ = self.outgoing.send(Message::Response {
            id: self.id,
            value: response,
            trailer: fragment::Trailer::None,
        });
        self.finish_shutdown();
    }

    /// Sends a response head and returns its streaming trailer body.
    pub fn respond_with_trailer(mut self, response: P::Response) -> crate::TrailerSend<()> {
        if let Some(trailer) = self.request_trailer.as_mut() {
            trailer.discard();
        }
        let shared = crate::trailer::SendShared::new(Kind::Response, self.id, &self.limits);
        self.responded = true;
        self.inner.lock().unwrap().outstanding.remove(&self.id);
        let _ = self.outgoing.send(Message::Response {
            id: self.id,
            value: response,
            trailer: fragment::Trailer::Stream(shared.clone()),
        });
        self.finish_shutdown();
        crate::TrailerSend::new(shared, ())
    }

    /// Stops accepting requests and gracefully drains the connection.
    pub fn shutdown(&self) {
        self.shutdown_on_respond.store(true, Ordering::Release);
    }

    fn finish_shutdown(&self) {
        if self.shutdown_on_respond.load(Ordering::Acquire)
            && let Some(shutdown) = self.inner.lock().unwrap().shutdown.take()
        {
            let _ = shutdown.send(());
        }
    }

    /// Runs an operation which can observe request cancellation without dropping the handler.
    pub async fn cancel_guard<T, F>(&mut self, operation: F) -> Result<T, RequestCancelled>
    where
        F: AsyncFnOnce(&mut CallContext<P>) -> T,
    {
        struct Reset {
            id: u64,
            inner: Arc<Mutex<Inner>>,
        }
        impl Drop for Reset {
            fn drop(&mut self) {
                if let Some(cancel) = self.inner.lock().unwrap().outstanding.get_mut(&self.id) {
                    cancel.signal = None;
                }
            }
        }
        let (signal, cancelled) = oneshot::channel();
        {
            let mut inner = self.inner.lock().unwrap();
            let cancel = inner
                .outstanding
                .get_mut(&self.id)
                .expect("call context is not registered");
            assert!(cancel.signal.is_none(), "cancel guard is already active");
            cancel.signal = Some(signal);
        }
        let _reset = Reset {
            id: self.id,
            inner: self.inner.clone(),
        };
        let future = operation(&mut *self);
        tokio::pin!(future);
        tokio::select! {
            value = &mut future => Ok(value),
            result = cancelled => match result { Ok(()) => Err(RequestCancelled), Err(_) => Ok(future.await) },
        }
    }

    pub fn register<T: OpaqueResource>(&self, value: T) -> Opaque<T::Marker> {
        self.inner.lock().unwrap().objects.register(value)
    }

    pub fn acquire<T: OpaqueResource>(
        &self,
        value: Opaque<T::Marker>,
    ) -> Result<OpaqueGuard<T>, InvalidOpaque> {
        self.inner.lock().unwrap().objects.acquire(value)
    }

    /// Removes a typed opaque resource from this session.
    ///
    /// Returns the resource when no acquired guards still share its ownership,
    /// or `None` after removing it when another call retains a guard.
    pub fn unregister<T: OpaqueResource>(
        &self,
        value: Opaque<T::Marker>,
    ) -> Result<Option<T>, InvalidOpaque> {
        self.inner.lock().unwrap().objects.unregister::<T>(value)
    }
}

impl<P: Protocol> Drop for CallContext<P> {
    fn drop(&mut self) {
        if !self.responded {
            self.inner.lock().unwrap().outstanding.remove(&self.id);
            let _ = self.outgoing.send(Message::Error { id: self.id });
        }
    }
}

/// Indicates that a guarded operation was interrupted by request cancellation.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("request cancelled")]
pub struct RequestCancelled;
