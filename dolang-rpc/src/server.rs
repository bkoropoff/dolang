use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use bytes::Buf;
use futures::{
    StreamExt,
    future::{AbortHandle, Abortable},
};
use tokio::sync::{mpsc, oneshot};

#[cfg(unix)]
use crate::attach_handles;

use crate::{
    Error, Kind, Limits, Protocol, decode, encode_payload, fragment,
    session::{self, InvalidOpaque, Opaque, OpaqueGuard, OpaqueResource},
    transport::{self, Receiver, SendFrame, Sender},
};

/// A negotiated server endpoint that has not yet been bound to a [`Protocol`].
///
/// Inspect its negotiated application protocol, then consume it with
/// [`bind`](Unbound::bind) to obtain a [`Server`].
pub use crate::unbound::UnboundServer as Unbound;

/// A server endpoint for one connection.
///
/// Consume it with [`serve`](Self::serve) to dispatch requests from the peer.
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
    objects: session::ObjectTable,
    shutdown: Option<oneshot::Sender<()>>,
}

struct Cancellation {
    signal: Option<oneshot::Sender<()>>,
    abort: AbortHandle,
}

impl<P: Protocol> Server<P> {
    /// Builds a `Server` from an already-negotiated transport. Only reachable
    /// via [`Unbound::bind`] — `Server` has
    /// no public constructors of its own, so it's never possible to hold one
    /// that hasn't already completed `fragment::negotiate`, and `serve`
    /// never needs to negotiate itself.
    pub(crate) fn from_transport(
        sender: transport::AnySender,
        receiver: transport::AnyReceiver,
        limits: Limits,
    ) -> Self {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver,
            outgoing,
            outgoing_rx,
            inner: Arc::new(Mutex::new(Inner {
                outstanding: HashMap::new(),
                objects: session::ObjectTable::default(),
                shutdown: None,
            })),
            limits,
            marker: PhantomData,
        }
    }

    /// Serves requests until the peer disconnects, the session fails, or a
    /// handler requests graceful shutdown.
    ///
    /// The handler may be called concurrently for independent requests. Each
    /// invocation must consume its [`CallContext`] with [`CallContext::respond`]
    /// or [`CallContext::respond_with_trailer`]; dropping the context without
    /// responding reports a per-request error to the peer.
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
        let mut writer = tokio::spawn(writer::<P>(sender, outgoing_rx, limits));
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
                        let trailer = trailer.map(crate::trailer::TrailerRecv::new);
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
                                    shutdown_on_respond: false,
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
        // Dropping `outgoing` and every task's clone of it (via `tasks`)
        // closes the writer's channel — see the comment on `writer` below —
        // which is what tells it to stop and, once drained, exit.
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
                let _ = writer.await;
            }
        }
        result
    }
}

async fn writer<P: Protocol>(
    mut sender: transport::AnySender,
    mut outgoing: mpsc::UnboundedReceiver<Message<P::Response>>,
    limits: Limits,
) -> Result<(), Error> {
    let mut scheduler = fragment::Scheduler::new(&limits);
    // Holding a clone of `outgoing`'s sender half (the local `outgoing` in
    // `serve`, or a `CallContext`'s) is what represents the ability to
    // still get a message in, so the channel closing — every clone gone —
    // doubles as this task's shutdown signal: once `recv()` reports no more
    // messages will ever arrive, admission of new work stops, and the loop
    // keeps advancing the scheduler until it's fully drained before
    // exiting, never abandoning a write already committed to it.
    let mut closed = false;
    while !closed || scheduler.has_work() {
        tokio::select! {
            message = outgoing.recv(), if !closed => {
                let Some(message) = message else {
                    closed = true;
                    continue;
                };
                admit::<P>(&mut sender, &mut scheduler, message).await?;
            }
            // Not raced against anything — see the matching comment in
            // client.rs's writer loop. A dropped send future could leave a
            // committed partial fragment on the transport, or — on
            // transports whose writes are dispatched to a detached
            // background task — let an abandoned write complete arbitrarily
            // later, after the peer has already torn down its end.
            _ = scheduler.ready(), if scheduler.has_work() => {
                scheduler.advance(&mut sender).await?;
                // Flush anything sent by the scheduler
                let _ = sender.flush().await;
            }
        }
    }
    Ok(())
}

/// Admits one outgoing item. Responses with native-handle attachments are
/// sent as a single atomic fragment immediately (bypassing the round-robin
/// scheduler); everything else is handed to `scheduler`.
async fn admit<P: Protocol>(
    sender: &mut transport::AnySender,
    scheduler: &mut fragment::Scheduler,
    message: Message<P::Response>,
) -> Result<(), Error> {
    match message {
        Message::Response { id, value, trailer } => {
            #[cfg(unix)]
            let (payload, handles) = encode_payload(&value)?;
            #[cfg(windows)]
            let mut frame = sender.send();
            #[cfg(windows)]
            let (payload, handles) = encode_payload(&value, &mut frame)?;
            if !handles.is_empty() {
                if !matches!(&trailer, fragment::Trailer::None) {
                    return Err(Error::Protocol(
                        "responses with both native-handle attachments and a trailer are not supported"
                        .into(),
                    ));
                }
                #[cfg(unix)]
                let mut frame = sender.send();
                #[cfg(unix)]
                attach_handles(&handles, &mut frame)?;
                let header = fragment::FragmentHeader {
                    flags: fragment::Flags::FIRST | fragment::Flags::LAST,
                    kind: Kind::Response,
                    id,
                    payload_len: payload.len(),
                };
                let mut buffer = header.encode().chain(payload);
                frame.finish(&mut buffer).await?;
                sender.flush().await?;
            } else {
                #[cfg(windows)]
                drop(frame);
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

/// Request-scoped services supplied to a server handler.
///
/// A context is not cloneable and must be consumed to send a response.
pub struct CallContext<P: Protocol> {
    id: u64,
    inner: Arc<Mutex<Inner>>,
    request_trailer: Option<crate::trailer::TrailerRecv>,
    outgoing: mpsc::UnboundedSender<Message<P::Response>>,
    responded: bool,
    shutdown_on_respond: bool,
    limits: Limits,
    marker: PhantomData<fn() -> P>,
}

impl<P: Protocol> CallContext<P> {
    /// Returns this request's raw-byte trailer, if present.
    ///
    /// The returned value implements [`AsyncRead`](tokio::io::AsyncRead).
    /// Dropping it or calling [`TrailerRecv::discard`](crate::trailer::TrailerRecv::discard)
    /// stops local consumption; the peer is notified only if it continues to
    /// send trailer fragments.
    pub fn request_trailer(&mut self) -> Option<&mut crate::trailer::TrailerRecv> {
        self.request_trailer.as_mut()
    }

    /// Sends a response without a trailer and consumes this call context.
    ///
    /// Any unread request trailer is discarded.
    pub fn respond(mut self, response: P::Response) {
        drop(self.request_trailer.take());
        self.responded = true;
        self.inner.lock().unwrap().outstanding.remove(&self.id);
        let _ = self.outgoing.send(Message::Response {
            id: self.id,
            value: response,
            trailer: fragment::Trailer::None,
        });
        self.finish_shutdown();
    }

    /// Sends a response head and returns a writer for its raw-byte trailer.
    ///
    /// Call [`TrailerSend::finish`](crate::trailer::TrailerSend::finish), or
    /// asynchronously shut down the returned writer, to commit the trailer.
    /// Dropping it without finishing aborts the trailer. A response cannot
    /// carry both a trailer and a direct [`OsHandle`](crate::handle::OsHandle)
    /// attachment. Any unread request trailer is discarded.
    pub fn respond_with_trailer(
        mut self,
        response: P::Response,
    ) -> crate::trailer::TrailerSend<()> {
        drop(self.request_trailer.take());
        let shared = crate::trailer::SendShared::new(Kind::Response, self.id, &self.limits);
        self.responded = true;
        self.inner.lock().unwrap().outstanding.remove(&self.id);
        let _ = self.outgoing.send(Message::Response {
            id: self.id,
            value: response,
            trailer: fragment::Trailer::Stream(shared.clone()),
        });
        self.finish_shutdown();
        crate::trailer::TrailerSend::new(shared, ())
    }

    /// Requests graceful shutdown after this handler sends its response.
    ///
    /// The server stops accepting requests once this context is consumed by
    /// [`respond`](Self::respond) or [`respond_with_trailer`](Self::respond_with_trailer),
    /// then lets already-running handlers finish.
    pub fn shutdown(&mut self) {
        self.shutdown_on_respond = true;
    }

    fn finish_shutdown(&self) {
        if self.shutdown_on_respond
            && let Some(shutdown) = self.inner.lock().unwrap().shutdown.take()
        {
            let _ = shutdown.send(());
        }
    }

    /// Runs an operation that can observe request cancellation without dropping
    /// the handler itself.
    ///
    /// If the peer cancels while `operation` is running, its future is dropped
    /// and this method returns [`RequestCancelled`]. The handler regains the
    /// context and may perform cleanup or send an application-level response.
    /// Only one cancellation guard may be active at a time; nesting guards
    /// panics.
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

    /// Registers a session-scoped resource and returns its serializable handle.
    ///
    /// The caller owns the resource's lifetime and must eventually unregister
    /// it. Cloning, sending, or dropping the returned [`Opaque`] does not
    /// affect the registration.
    pub fn register<T: OpaqueResource>(&self, value: T) -> Opaque<T::Marker> {
        self.inner.lock().unwrap().objects.register(value)
    }

    /// Acquires a typed shared guard for a registered opaque resource.
    ///
    /// Returns [`InvalidOpaque`] if the handle belongs to another session, was
    /// unregistered, or does not refer to a resource with concrete type `T`.
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
