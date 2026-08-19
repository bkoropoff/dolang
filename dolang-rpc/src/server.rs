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
    trailer::{RecvShared, SendShared, SessionWindow, TrailerRecv, TrailerSend},
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
    shared: Arc<Shared>,
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
    /// We retired `count` bytes of the request trailer on `id` and are
    /// returning that much credit. Always results in a wire `Kind::Credit`.
    Credit {
        id: u64,
        count: u32,
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

impl<R: Send + 'static> crate::trailer::TrailerSink for mpsc::WeakUnboundedSender<Message<R>> {
    fn credit(&self, id: u64, count: u32) {
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Message::Credit { id, count });
        }
    }

    fn discard(&self, id: u64) {
        // Reached from `TrailerRecv::drop`, so a departed channel just means
        // the connection is already gone and the peer has nothing to stop.
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Message::DiscardTrailer { id });
        }
    }
}

/// State the reader loop, the writer task, and every live handler share.
///
/// One `Arc` rather than a bundle of them, and held strongly by all three:
/// nothing in here can keep the session alive on its own, because the ability
/// to still get a message out is the `outgoing` sender, which stays outside
/// it. Closing that channel is still what shuts the writer down.
struct Shared {
    inner: Mutex<Inner>,
    session: Arc<Session>,
    /// Send-side trailer credit shared by every outgoing response trailer on
    /// this connection. Bounds what the peer must buffer for us in aggregate.
    trailer_session: Arc<SessionWindow>,
    limits: Limits,
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

/// Refuses a fragment the peer had no business sending, before the
/// reassembler can allocate anything for it.
///
/// Unlike the client's gate, this needs no id: a client never asks this end
/// for anything, so a response in this direction names nothing at all.
fn check_header(header: &fragment::FragmentHeader) -> Result<(), Error> {
    if header.kind == Kind::Response {
        return Err(Error::Protocol(
            "server received a Response fragment".into(),
        ));
    }
    Ok(())
}

/// Applies `max_concurrent_calls` to a call that is arriving or starting to
/// arrive.
///
/// A concurrent call is one this end has begun receiving and has not yet
/// answered, and it passes through two custodians on the way: the reassembler
/// holds it while its payload is still fragmented, and `outstanding` holds it
/// from dispatch until the response head. The limit is on the *sum* — the two
/// counts are disjoint, since a message leaves payload phase in the same
/// `accept` call that dispatches it — so neither custodian can enforce it
/// alone, and checking them separately would admit twice the limit.
///
/// `incomplete` is the reassembler's count *including* the call being
/// admitted, so callers add one for a call that has already left payload
/// phase.
fn check_call_admission(
    outstanding: &HashMap<u64, Cancellation>,
    incomplete: usize,
    max_concurrent_calls: usize,
    id: u64,
) -> Result<(), Error> {
    if outstanding.contains_key(&id) {
        return Err(Error::Protocol(format!("duplicate active request id {id}")));
    }
    if outstanding.len() + incomplete > max_concurrent_calls {
        return Err(Error::Protocol("too many concurrent calls".into()));
    }
    Ok(())
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
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    outstanding: HashMap::new(),
                    shutdown: None,
                    #[cfg(target_os = "macos")]
                    fd_escrow: Default::default(),
                }),
                session: Session::new(Box::new(outgoing.downgrade())),
                trailer_session: Arc::new(SessionWindow::new(limits.trailer_session_window)),
                limits,
            }),
            outgoing,
            outgoing_rx,
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
            shared,
            marker: _,
        } = self;
        let limits = shared.limits;
        let mut writer = tokio::spawn(writer::<P>(sender, outgoing_rx, shared.clone()));
        let (shutdown, mut shutdown_requested) = oneshot::channel();
        shared.inner.lock().unwrap().shutdown = Some(shutdown);
        let handler = Arc::new(handler);
        let mut reassembler = fragment::Reassembler::new(limits, Arc::new(outgoing.downgrade()));
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
                    check_header(&header)?;
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
                // A request has started arriving. It occupies the same
                // budget as one already dispatched, so it is admitted on the
                // same rule, at the earliest point this end knows about it.
                fragment::Event::PayloadIncomplete { id } => {
                    if let Err(error) = check_call_admission(
                        &shared.inner.lock().unwrap().outstanding,
                        reassembler.payload_incomplete(),
                        limits.max_concurrent_calls,
                        id,
                    ) {
                        break 'main (Err(error), false, false);
                    }
                    (None, None)
                }
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
                    shared: trailer,
                    len,
                    ..
                } => (None, Some((trailer, len))),
                fragment::Event::Release { id, count } => {
                    shared.session.release(id, count);
                    (None, None)
                }
                fragment::Event::Credit { id, count } => {
                    // Applied here rather than routed through the writer;
                    // see the client's matching arm.
                    shared.trailer_session.refund(id, count as usize);
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
                        if let Err(error) = check_call_admission(
                            &shared.inner.lock().unwrap().outstanding,
                            // This message has already left payload phase, so
                            // it is no longer in the reassembler's count and
                            // has to be added back.
                            reassembler.payload_incomplete() + 1,
                            limits.max_concurrent_calls,
                            id,
                        ) {
                            break (Err(error), false, false);
                        }
                        #[cfg(unix)]
                        let request = decode_payload(
                            &payload,
                            &mut session::SessionHandles {
                                inner: handles,
                                session: &shared.session,
                            },
                        );
                        #[cfg(windows)]
                        let request = decode_payload(
                            &payload,
                            &mut DecodeHandles {
                                receiver: &receiver,
                                session: &shared.session,
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
                        let task_shared = shared.clone();
                        let task_outgoing = outgoing.clone();
                        let (abort, registration) = AbortHandle::new_pair();
                        tasks.push(Abortable::new(
                            async move {
                                let context = CallContext {
                                    id,
                                    shared: task_shared,
                                    request_trailer: trailer,
                                    outgoing: task_outgoing,
                                    responded: false,
                                    shutdown_on_respond: false,
                                    marker: PhantomData,
                                };
                                handler(context, request).await;
                            },
                            registration,
                        ));
                        shared.inner.lock().unwrap().outstanding.insert(
                            id,
                            Cancellation {
                                signal: None,
                                abort,
                            },
                        );
                    }
                    Kind::Cancel => {
                        let mut state = shared.inner.lock().unwrap();
                        if let Some(signal) = state
                            .outstanding
                            .get_mut(&id)
                            .and_then(|cancel| cancel.signal.take())
                        {
                            let _ = signal.send(());
                        } else if let Some(cancel) = state.outstanding.get(&id) {
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
                        if !shared.inner.lock().unwrap().fd_escrow.release(id) {
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
            if let Some((trailer, len)) = live_trailer {
                let frame = receiver.recv();
                // SAFETY: the lease retains the receiver borrow and clears
                // the erased token before it ends.
                let lease = unsafe { RecvShared::grant(&trailer, frame, len) };
                let result = loop {
                    tokio::select! {
                        result = RecvShared::wait_fragment(&trailer) => break result,
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
    shared: Arc<Shared>,
) -> Result<(), Error> {
    let limits = shared.limits;
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
                admit::<P>(&mut sender, &mut scheduler, &shared, message).await?;
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
                        shared.inner.lock().unwrap().fd_escrow.sent(id, fds, handles_done);
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
    shared: &Arc<Shared>,
    message: Message<P::Response>,
) -> Result<(), Error> {
    let limits = &shared.limits;
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
                session: &shared.session,
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
                shared.inner.lock().unwrap().fd_escrow.register(id);
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
            fragment::AbortOutcome::Discarded { started, .. } => {
                if started {
                    scheduler.admit_abort(id);
                }
                #[cfg(target_os = "macos")]
                if !started {
                    shared.inner.lock().unwrap().fd_escrow.discard_unsent(id);
                }
            }
        },
        Message::DiscardTrailer { id } => scheduler.admit_empty(Kind::Discard, id),
        Message::PeerDiscarded { id } => {
            // The peer will never credit what it just threw away; see the
            // client's matching arm.
            shared.trailer_session.settle(id);
            scheduler.discard_active_trailer(id);
        }
        Message::Ack { id } => scheduler.admit_empty(Kind::Ack, id),
        Message::Release { id, count } => scheduler.admit_release(id, count),
        Message::Credit { id, count } => scheduler.admit_credit(id, count),
    }
    Ok(())
}

/// Request-scoped services supplied to a server handler.
///
/// A context is not cloneable and must be consumed to send a response.
pub struct CallContext<P: Protocol> {
    id: u64,
    shared: Arc<Shared>,
    request_trailer: Option<TrailerRecv>,
    /// A strong sender, so a live handler keeps the writer's channel — and
    /// with it the connection — open until it has answered.
    outgoing: mpsc::UnboundedSender<Message<P::Response>>,
    responded: bool,
    shutdown_on_respond: bool,
    marker: PhantomData<fn() -> P>,
}

impl<P: Protocol> CallContext<P> {
    /// Takes this request's raw-byte trailer, if present.
    ///
    /// The returned value implements [`AsyncRead`](tokio::io::AsyncRead).
    /// Dropping it or calling
    /// [`TrailerRecv::discard`](crate::trailer::TrailerRecv::discard) stops
    /// local consumption and immediately tells the peer to stop sending, as
    /// does responding while the context still holds it.
    ///
    /// Taken rather than borrowed, so a handler may keep reading after it
    /// has responded. Paired with
    /// [`respond_with_trailer`](Self::respond_with_trailer) that gives a
    /// duplex byte pipe over one call: each direction is an independent
    /// stream that ends when its own end says so, and the call itself is
    /// complete as soon as the response head goes out. Neither direction
    /// holds a call slot after that, so the pipes are bounded by trailer
    /// credit rather than by `max_concurrent_calls` — and, as with a socket,
    /// nothing ties the two halves together: closing one does not close the
    /// other, and a peer that vanishes is noticed through the transport.
    pub fn trailer(&mut self) -> Option<TrailerRecv> {
        self.request_trailer.take()
    }

    /// Returns this request's raw-byte trailer in manual-credit mode.
    ///
    /// The consumer then owes the peer an explicit
    /// [`TrailerRecv::release`](crate::trailer::TrailerRecv::release) for
    /// every chunk it finishes with, instead of credit being returned on
    /// read. Use this when the bytes are being handed somewhere slower than
    /// this process, so that the peer's send rate follows the real drain
    /// rate; read [`release`](crate::trailer::TrailerRecv::release) first,
    /// since manual mode moves a deadlock rule into calling code.
    ///
    /// The mode is fixed here rather than switchable afterwards, so a
    /// trailer cannot be half auto-credited and half not. Taken rather than
    /// borrowed, exactly as in [`trailer`](Self::trailer).
    pub fn trailer_manual_credit(&mut self) -> Option<TrailerRecv> {
        let mut trailer = self.request_trailer.take()?;
        trailer.set_manual_credit();
        Some(trailer)
    }

    /// Sends a response without a trailer and consumes this call context.
    ///
    /// A request trailer this context still holds is discarded; one already
    /// taken by [`trailer`](Self::trailer) is untouched and stays readable.
    pub fn respond(mut self, response: P::Response) {
        drop(self.request_trailer.take());
        self.responded = true;
        self.shared
            .inner
            .lock()
            .unwrap()
            .outstanding
            .remove(&self.id);
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
    /// Dropping it without finishing aborts the trailer. A request trailer
    /// this context still holds is discarded; one already taken by
    /// [`trailer`](Self::trailer) is untouched, which is what makes the two
    /// directions a duplex pipe.
    pub fn respond_with_trailer(mut self, response: P::Response) -> TrailerSend<()> {
        drop(self.request_trailer.take());
        let shared = SendShared::new(
            Kind::Response,
            self.id,
            &self.shared.limits,
            self.shared.trailer_session.clone(),
        );
        self.responded = true;
        self.shared
            .inner
            .lock()
            .unwrap()
            .outstanding
            .remove(&self.id);
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
            && let Some(shutdown) = self.shared.inner.lock().unwrap().shutdown.take()
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
            shared: Arc<Shared>,
        }
        impl Drop for Reset {
            fn drop(&mut self) {
                if let Some(cancel) = self
                    .shared
                    .inner
                    .lock()
                    .unwrap()
                    .outstanding
                    .get_mut(&self.id)
                {
                    cancel.signal = None;
                }
            }
        }
        let (signal, cancelled) = oneshot::channel();
        {
            let mut inner = self.shared.inner.lock().unwrap();
            let cancel = inner
                .outstanding
                .get_mut(&self.id)
                .expect("call context is not registered");
            assert!(cancel.signal.is_none(), "cancel guard is already active");
            cancel.signal = Some(signal);
        }
        let _reset = Reset {
            id: self.id,
            shared: self.shared.clone(),
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
        self.shared.session.register(value)
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
        self.shared.session.acquire(value)
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
        self.shared.session.unregister::<T>(value)
    }
}

impl<P: Protocol> Drop for CallContext<P> {
    fn drop(&mut self) {
        if !self.responded {
            self.shared
                .inner
                .lock()
                .unwrap()
                .outstanding
                .remove(&self.id);
            let _ = self.outgoing.send(Message::Error { id: self.id });
        }
    }
}

/// Indicates that a guarded operation was interrupted by request cancellation.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("request cancelled")]
pub struct RequestCancelled;

#[cfg(test)]
mod tests {
    use super::*;

    fn cancellation() -> Cancellation {
        let (abort, _registration) = AbortHandle::new_pair();
        Cancellation {
            signal: None,
            abort,
        }
    }

    /// The gate in front of the reassembler: nobody calls a client, so a
    /// response arriving here answers nothing.
    #[test]
    fn header_gate_refuses_responses() {
        let header = |kind| fragment::FragmentHeader {
            flags: fragment::Flags::FIRST | fragment::Flags::LAST,
            kind,
            id: 7,
            payload_len: 0,
        };
        assert!(matches!(
            check_header(&header(Kind::Response)),
            Err(Error::Protocol(_))
        ));
        assert!(check_header(&header(Kind::Request)).is_ok());
        assert!(check_header(&header(Kind::Cancel)).is_ok());
    }

    #[test]
    fn call_admission_rejects_excess_and_duplicate_requests() {
        let mut outstanding = HashMap::new();
        assert!(check_call_admission(&outstanding, 1, 1, 7).is_ok());
        outstanding.insert(7, cancellation());
        assert!(matches!(
            check_call_admission(&outstanding, 1, 1, 8),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));
        assert!(matches!(
            check_call_admission(&outstanding, 1, 2, 7),
            Err(Error::Protocol(message)) if message == "duplicate active request id 7"
        ));
    }

    /// The two halves share one budget: a call still arriving counts against
    /// the same limit as one already dispatched, so neither half may be
    /// admitted on its own count.
    #[test]
    fn call_admission_counts_arriving_and_dispatched_calls_together() {
        let mut outstanding = HashMap::new();
        outstanding.insert(7, cancellation());
        // One dispatched, one arriving, limit of two: exactly full, and the
        // arriving one is what the count already includes.
        assert!(check_call_admission(&outstanding, 1, 2, 8).is_ok());
        assert!(matches!(
            check_call_admission(&outstanding, 2, 2, 8),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));
    }

    #[test]
    fn zero_call_limit_rejects_the_first_request() {
        assert!(matches!(
            check_call_admission(&HashMap::new(), 1, 0, 0),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));
    }
}
