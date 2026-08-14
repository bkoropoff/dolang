#[cfg(windows)]
use std::{any::TypeId, io, os::windows::io::OwnedHandle};
use std::{
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use futures::{
    StreamExt,
    future::{AbortHandle, Abortable},
};
use tokio::sync::{mpsc, oneshot};

use crate::{
    Error, Limits, Protocol,
    fragment::{self, Kind},
    serde::{decode_payload, encode_payload},
    session::{self, Cite, Gift, InvalidOpaque, OpaqueGuard, OpaqueResource, Session},
    trailer::{RecvShared, SendShared, TrailerRecv, TrailerSend},
    transport::{self, EncodeHandles, Receiver, Sender},
};
#[cfg(windows)]
use crate::{handle::TakeHandle, session::Inner as OpaqueInner};

#[cfg(windows)]
struct DecodeHandles<'a> {
    receiver: &'a transport::AnyReceiver,
    session: &'a Arc<Session>,
    count: usize,
    max_handles: usize,
}

#[cfg(windows)]
impl TakeHandle for DecodeHandles<'_> {
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle> {
        if self.count == self.max_handles {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message contains too many handle attachments",
            ));
        }
        self.count += 1;
        self.receiver.duplicate_peer_handle(value)
    }

    fn take_gift(&mut self, owner: u8, id: u64) -> io::Result<OpaqueInner> {
        self.session
            .take_gift(owner, id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid opaque reference"))
    }

    fn take_cite(&mut self, owner: u8, id: u64, marker: TypeId) -> io::Result<OpaqueInner> {
        self.session
            .take_cite(owner, id, marker)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid opaque reference"))
    }
}

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
    session: Arc<Session>,
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
    Ack {
        id: u64,
    },
    /// Drops `count` of this endpoint's references to the peer's opaque `id`.
    Release {
        id: u64,
        count: u32,
    },
}

/// Emits `Release` frames for opaques whose last local handle dropped. The
/// strong senders stay exactly what they were: `serve`'s own sender and each
/// live `CallContext`.
impl<R: Send + 'static> session::ReleaseSink for mpsc::WeakUnboundedSender<Message<R>> {
    fn release(&self, id: u64, count: u32) {
        // Called from `Drop`, so a departed channel is not an error: the
        // writer is already gone and the peer's table dies with the session.
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Message::Release { id, count });
        }
    }
}

struct Inner {
    outstanding: HashMap<u64, Cancellation>,
    shutdown: Option<oneshot::Sender<()>>,
    #[cfg(target_os = "macos")]
    fd_escrow: crate::escrow::FdEscrow,
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
        let session = Session::new(Box::new(outgoing.downgrade()));
        Self {
            sender,
            receiver,
            outgoing,
            outgoing_rx,
            session,
            inner: Arc::new(Mutex::new(Inner {
                outstanding: HashMap::new(),
                shutdown: None,
                #[cfg(target_os = "macos")]
                fd_escrow: Default::default(),
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
            session,
            limits,
            marker: _,
        } = self;
        let mut writer = tokio::spawn(writer::<P>(
            sender,
            outgoing_rx,
            inner.clone(),
            session.clone(),
            limits,
        ));
        let (shutdown, mut shutdown_requested) = oneshot::channel();
        inner.lock().unwrap().shutdown = Some(shutdown);
        let handler = Arc::new(handler);
        let mut reassembler = fragment::Reassembler::new(limits);
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
                fragment::Event::None => (None, None),
                fragment::Event::Aborted {
                    kind: Kind::Request,
                    ..
                } => (None, None),
                fragment::Event::Aborted { kind, .. } => {
                    break 'main (
                        Err(Error::Protocol(format!(
                            "unexpected aborted {kind:?} message"
                        ))),
                        false,
                        false,
                    );
                }
                fragment::Event::Message(message) => (Some(message), None),
                fragment::Event::Ack { id, message } => {
                    let _ = outgoing.send(Message::Ack { id });
                    (message, None)
                }
                fragment::Event::Trailer {
                    id,
                    shared,
                    len,
                    notify_discard,
                } => (None, Some((id, shared, len, notify_discard))),
                fragment::Event::Release { id, count } => {
                    session.release(id, count);
                    (None, None)
                }
            };
            if let Some(fragment::Message {
                kind,
                id,
                payload,
                handles,
                trailer,
            }) = message
            {
                #[cfg(windows)]
                let _ = handles;
                match kind {
                    Kind::Request => {
                        #[cfg(unix)]
                        let request = decode_payload(
                            &payload,
                            &mut session::SessionHandles {
                                inner: handles,
                                session: &session,
                            },
                        );
                        #[cfg(windows)]
                        let request = decode_payload(
                            &payload,
                            &mut DecodeHandles {
                                receiver: &receiver,
                                session: &session,
                                count: 0,
                                max_handles: limits.max_handles_per_message,
                            },
                        );
                        let request = match request {
                            Ok(request) => request,
                            Err(error) => break (Err(error), false, false),
                        };
                        let trailer = trailer.map(TrailerRecv::new);
                        let handler = handler.clone();
                        let task_inner = inner.clone();
                        let task_session = session.clone();
                        let task_outgoing = outgoing.clone();
                        let (abort, registration) = AbortHandle::new_pair();
                        tasks.push(Abortable::new(
                            async move {
                                let context = CallContext {
                                    id,
                                    inner: task_inner.clone(),
                                    session: task_session,
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
                    Kind::Ack => {
                        #[cfg(target_os = "macos")]
                        if !inner.lock().unwrap().fd_escrow.release(id) {
                            break (
                                Err(Error::Protocol(format!(
                                    "Ack for response {id} with no active file descriptor escrow"
                                ))),
                                false,
                                false,
                            );
                        }
                        #[cfg(not(target_os = "macos"))]
                        break (
                            Err(Error::Protocol(format!(
                                "Ack for response {id} with no active escrow"
                            ))),
                            false,
                            false,
                        );
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
                let frame = receiver.recv();
                // SAFETY: the lease retains the receiver borrow and clears
                // the erased token before it ends.
                let lease = unsafe { RecvShared::grant(&shared, frame, len) };
                let result = loop {
                    tokio::select! {
                        result = RecvShared::wait_fragment(&shared) => break result,
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
    inner: Arc<Mutex<Inner>>,
    session: Arc<Session>,
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
                admit::<P>(&mut sender, &mut scheduler, &inner, &session, &limits, message).await?;
            }
            // Not raced against anything — see the matching comment in
            // client.rs's writer loop. A dropped send future could leave a
            // committed partial fragment on the transport, or — on
            // transports whose writes are dispatched to a detached
            // background task — let an abandoned write complete arbitrarily
            // later, after the peer has already torn down its end.
            _ = scheduler.ready(), if scheduler.has_work() => {
                match scheduler.advance(&mut sender).await? {
                    fragment::AdvanceOutcome::None | fragment::AdvanceOutcome::Aborted(_) => {}
                    #[cfg(target_os = "macos")]
                    fragment::AdvanceOutcome::Escrow { id, fds, handles_done } => {
                        inner.lock().unwrap().fd_escrow.sent(id, fds, handles_done);
                    }
                }
                // Flush anything sent by the scheduler
                let _ = sender.flush().await;
            }
        }
    }
    Ok(())
}

/// Admits one outgoing item to the fragment scheduler.
async fn admit<P: Protocol>(
    sender: &mut transport::AnySender,
    scheduler: &mut fragment::Scheduler,
    inner: &Arc<Mutex<Inner>>,
    session: &Arc<Session>,
    limits: &Limits,
    message: Message<P::Response>,
) -> Result<(), Error> {
    #[cfg(not(target_os = "macos"))]
    let _ = inner;
    match message {
        Message::Response { id, value, trailer } => {
            #[cfg(unix)]
            let max_handles = if limits.max_handles_per_fragment == 0 {
                0
            } else {
                limits.max_handles_per_message
            };
            #[cfg(windows)]
            let max_handles = limits.max_handles_per_message;
            let mut ledger = session::Ledger::default();
            let mut put_handles = session::SessionFrame {
                inner: EncodeHandles::new(sender, max_handles),
                session,
                ledger: &mut ledger,
            };
            let payload = match encode_payload(&value, &mut put_handles) {
                Ok(payload) => payload,
                Err(error) => {
                    drop(put_handles);
                    // Nothing reached the wire, so undo the gift increments
                    // rather than letting the ledger's drop commit them.
                    ledger.rescind();
                    return Err(error);
                }
            };
            let put_handles = put_handles.inner;
            #[cfg(unix)]
            let handles = put_handles.finish();
            #[cfg(target_os = "macos")]
            if handles.needs_ack() {
                inner.lock().unwrap().fd_escrow.register(id);
            }
            #[cfg(windows)]
            let (handles, escrow) = put_handles.finish();
            #[cfg(windows)]
            drop(escrow);
            scheduler.admit_message(Kind::Response, id, payload, handles, trailer, ledger);
        }
        Message::Error { id } => scheduler.admit_empty(Kind::Error, id),
        Message::Cancel { id } => match scheduler.try_cancel_active(id) {
            fragment::AbortOutcome::NotActive => {}
            fragment::AbortOutcome::Discarded { started } => {
                if started {
                    scheduler.admit_abort(id);
                }
                #[cfg(target_os = "macos")]
                if !started {
                    inner.lock().unwrap().fd_escrow.discard_unsent(id);
                }
            }
        },
        Message::DiscardTrailer { id } => scheduler.admit_empty(Kind::Discard, id),
        Message::PeerDiscarded { id } => scheduler.discard_active_trailer(id),
        Message::Ack { id } => scheduler.admit_empty(Kind::Ack, id),
        Message::Release { id, count } => scheduler.admit_release(id, count),
    }
    Ok(())
}

/// Request-scoped services supplied to a server handler.
///
/// A context is not cloneable and must be consumed to send a response.
pub struct CallContext<P: Protocol> {
    id: u64,
    inner: Arc<Mutex<Inner>>,
    session: Arc<Session>,
    request_trailer: Option<TrailerRecv>,
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
    pub fn request_trailer(&mut self) -> Option<&mut TrailerRecv> {
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
    /// Dropping it without finishing aborts the trailer. Any unread request
    /// trailer is discarded.
    pub fn respond_with_trailer(mut self, response: P::Response) -> TrailerSend<()> {
        drop(self.request_trailer.take());
        let shared = SendShared::new(Kind::Response, self.id, &self.limits);
        self.responded = true;
        self.inner.lock().unwrap().outstanding.remove(&self.id);
        let _ = self.outgoing.send(Message::Response {
            id: self.id,
            value: response,
            trailer: fragment::Trailer::Stream(shared.clone()),
        });
        self.finish_shutdown();
        TrailerSend::new(shared, ())
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
    /// The registration lives as long as some handle naming it does — in this
    /// process or in the peer's mirror of it. Dropping the last one releases
    /// the resource, so a handle that is registered and then never sent
    /// (because the call failed, or was cancelled before responding) cleans
    /// itself up rather than stranding the entry.
    ///
    /// Registering yields a [`Gift`], which is what a wire position that grants
    /// the peer a reference holds. To name an already-granted resource back to
    /// the peer without granting another reference, send the same `Gift` again:
    /// the peer merges the arrival into the handle it holds, and the extra
    /// references collapse into one counted release.
    ///
    /// # Panics
    ///
    /// If a different concrete type has already been registered under
    /// `T::Marker` on this session. A marker is the only type information the
    /// wire carries, so it must name exactly one resource type.
    pub fn register<T: OpaqueResource>(&self, value: T) -> Gift<T::Marker> {
        self.session.register(value)
    }

    /// Acquires a typed shared guard for a registered opaque resource.
    ///
    /// Takes a [`Cite`], because only a citation names a resource this endpoint
    /// owns; a peer that puts a gift in a citation position is rejected during
    /// decode instead of arriving here.
    ///
    /// Returns [`InvalidOpaque`] if the resource was unregistered while the peer
    /// still held a reference to it, which an ordinary race with
    /// [`unregister`](Self::unregister) produces.
    ///
    /// # Panics
    ///
    /// If the handle was minted by a different session. Opaque ids are
    /// session-scoped, so redeeming one elsewhere is a local logic error that
    /// would otherwise resolve against an unrelated resource.
    pub fn acquire<T: OpaqueResource>(
        &self,
        value: Cite<T::Marker>,
    ) -> Result<OpaqueGuard<T>, InvalidOpaque> {
        self.session.acquire(value)
    }

    /// Empties a typed opaque resource, returning it when no acquired guards
    /// still share its ownership.
    ///
    /// The registration itself outlives this call until the peer has released
    /// its references, so a citation still in flight resolves to a revoked
    /// handle rather than an unknown one.
    ///
    /// # Panics
    ///
    /// If the handle was minted by a different session, as with
    /// [`acquire`](Self::acquire).
    /// Takes a [`Cite`], as [`acquire`](Self::acquire) does: a resource is
    /// closed because the peer named it and asked, so what arrives here is the
    /// citation that named it. An owner closing a resource of its own accord
    /// has no need of this — dropping its last handle retires the registration
    /// once the peer has released its references.
    pub fn unregister<T: OpaqueResource>(
        &self,
        value: Cite<T::Marker>,
    ) -> Result<Option<T>, InvalidOpaque> {
        self.session.unregister::<T>(value)
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
