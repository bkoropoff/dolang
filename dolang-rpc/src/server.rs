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
    stream::FuturesUnordered,
};
use tokio::sync::{mpsc, oneshot};

use crate::{
    Error, Limits, Protocol,
    fragment::{self, Event, Kind, Message},
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
    outgoing: mpsc::UnboundedSender<Outgoing<P::Response>>,
    outgoing_rx: mpsc::UnboundedReceiver<Outgoing<P::Response>>,
    /// The other end of `Inner::shutdown`, held until `serve` hands it to
    /// the receive driver.
    shutdown_rx: oneshot::Receiver<()>,
    shared: Arc<Shared>,
    marker: PhantomData<fn() -> P>,
}

enum Outgoing<R> {
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
impl<R: Send + 'static> session::ReleaseSink for mpsc::WeakUnboundedSender<Outgoing<R>> {
    fn release(&self, id: u64, count: u32) {
        // Called from `Drop`, so a departed channel is not an error: the
        // writer is already gone and the peer's table dies with the session.
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Outgoing::Release { id, count });
        }
    }
}

impl<R: Send + 'static> crate::trailer::TrailerSink for mpsc::WeakUnboundedSender<Outgoing<R>> {
    fn credit(&self, id: u64, count: u32) {
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Outgoing::Credit { id, count });
        }
    }

    fn discard(&self, id: u64) {
        // Reached from `TrailerRecv::drop`, so a departed channel just means
        // the connection is already gone and the peer has nothing to stop.
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Outgoing::DiscardTrailer { id });
        }
    }
}

/// State both connection drivers and every live handler share.
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

impl Shared {
    /// Maximum handle attachments one message may carry.
    fn max_handles(&self) -> usize {
        // A transport configured to attach no handles to a fragment can
        // carry none at all.
        #[cfg(unix)]
        if self.limits.max_handles_per_fragment == 0 {
            return 0;
        }
        self.limits.max_handles_per_message
    }

    /// Finishes handle encoding for message `id`, taking custody of whatever
    /// this platform must keep alive once the message is on the wire.
    ///
    /// On macOS that is the file descriptors themselves, escrowed until the
    /// peer acknowledges receipt. Every other unix passes them with the
    /// fragment and is done with them.
    #[cfg(unix)]
    fn finish_handles(&self, id: u64, handles: EncodeHandles) -> transport::OutgoingHandles {
        let handles = handles.finish();
        #[cfg(target_os = "macos")]
        if handles.needs_ack() {
            self.inner.lock().unwrap().fd_escrow.register(id);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = id;
        handles
    }

    /// Finishes handle encoding for message `_id`. Windows duplicates each
    /// handle into the peer as it is encoded, so the originals are this
    /// end's to close and nothing is escrowed.
    #[cfg(windows)]
    fn finish_handles(&self, _id: u64, handles: EncodeHandles) -> transport::OutgoingHandles {
        let (handles, escrow) = handles.finish();
        drop(escrow);
        handles
    }

    /// Decodes a message payload, taking custody of every handle and opaque
    /// reference it carries.
    #[cfg(unix)]
    fn decode<T: ::serde::de::DeserializeOwned>(
        &self,
        payload: &[u8],
        handles: transport::ReceivedHandles,
        _receiver: &transport::AnyReceiver,
    ) -> Result<T, Error> {
        decode_payload(
            payload,
            &mut session::SessionHandles {
                inner: handles,
                session: &self.session,
            },
        )
    }

    /// Decodes a message payload, taking custody of every handle and opaque
    /// reference it carries. Windows handles are named by value in the
    /// payload and duplicated out of the peer as they are decoded, rather
    /// than arriving attached to the fragment, so `handles` is empty.
    #[cfg(windows)]
    fn decode<T: ::serde::de::DeserializeOwned>(
        &self,
        payload: &[u8],
        _handles: transport::ReceivedHandles,
        receiver: &transport::AnyReceiver,
    ) -> Result<T, Error> {
        decode_payload(
            payload,
            &mut DecodeHandles {
                receiver,
                session: &self.session,
                count: 0,
                max_handles: self.max_handles(),
            },
        )
    }

    /// Records the file descriptors for `id` that just reached the wire.
    #[cfg(target_os = "macos")]
    fn escrow_sent(&self, id: u64, fds: Vec<std::os::fd::OwnedFd>, done: bool) {
        self.inner.lock().unwrap().fd_escrow.sent(id, fds, done);
    }

    /// Forgets the escrow for a message that will never reach the wire.
    fn discard_unsent_escrow(&self, id: u64) {
        #[cfg(target_os = "macos")]
        self.inner.lock().unwrap().fd_escrow.discard_unsent(id);
        #[cfg(not(target_os = "macos"))]
        let _ = id;
    }

    /// Releases the escrow an `Ack` names, returning false when there is
    /// none — which is every `Ack` on a platform that escrows nothing.
    fn release_escrow(&self, id: u64) -> bool {
        #[cfg(target_os = "macos")]
        return self.inner.lock().unwrap().fd_escrow.release(id);
        #[cfg(not(target_os = "macos"))]
        {
            let _ = id;
            false
        }
    }
}

struct Inner {
    outstanding: HashMap<u64, Cancellation>,
    /// Signals the receive driver to stop accepting new work. Taken by the
    /// first handler to ask for shutdown, so later ones are no-ops.
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
        let (shutdown, shutdown_rx) = oneshot::channel();
        Self {
            sender,
            receiver,
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    outstanding: HashMap::new(),
                    shutdown: Some(shutdown),
                    #[cfg(target_os = "macos")]
                    fd_escrow: Default::default(),
                }),
                session: Session::new(Box::new(outgoing.downgrade())),
                trailer_session: Arc::new(SessionWindow::new(limits.trailer_session_window)),
                limits,
            }),
            outgoing,
            outgoing_rx,
            shutdown_rx,
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
        let send = SendDriver::<P>::new(self.sender, self.outgoing_rx, self.shared.clone()).run();
        tokio::pin!(send);
        let recv = RecvDriver::new(
            self.receiver,
            self.outgoing,
            self.shutdown_rx,
            self.shared,
            handler,
        )
        .run();
        let result = tokio::select! {
            result = recv => result,
            // The send driver cannot finish on its own while the receive
            // driver holds a sender into it, so reaching here means it
            // failed — which ends the session, and nothing else would
            // notice. Dropping the receive future is what stops it; it is
            // never resumed, so a half-read fragment costs nothing.
            result = &mut send => return result,
        };
        // The receive half has dropped its sender, so the send driver stops
        // once it has flushed whatever is still queued. Only then is the
        // session over — but a receive-side failure is what ended it, so
        // that is the error worth reporting.
        result.and(send.await)
    }
}

/// Drives the receive half of the connection: reassembles inbound fragments,
/// dispatches requests to the handler, and owns the tear-down that follows
/// whatever ends the session.
///
/// This is the half that runs on the caller's own future rather than a task
/// of its own, because it is what [`Server::serve`] returns the result of.
struct RecvDriver<P: Protocol, H> {
    transport: transport::AnyReceiver,
    reassembler: fragment::Reassembler,
    shared: Arc<Shared>,
    /// Kept alive for the whole run: dropping this and every handler task's
    /// clone is what closes the send driver's channel and shuts it down.
    outgoing: mpsc::UnboundedSender<Outgoing<P::Response>>,
    handler: Arc<H>,
    /// Fires when a handler has asked to shut down gracefully.
    shutdown: oneshot::Receiver<()>,
}

impl<P: Protocol, H> RecvDriver<P, H>
where
    H: AsyncFn(CallContext<P>, P::Request) + Send + Sync + 'static,
{
    fn new(
        transport: transport::AnyReceiver,
        outgoing: mpsc::UnboundedSender<Outgoing<P::Response>>,
        shutdown: oneshot::Receiver<()>,
        shared: Arc<Shared>,
        handler: H,
    ) -> Self {
        let reassembler = fragment::Reassembler::new(shared.limits, Arc::new(outgoing.downgrade()));
        Self {
            transport,
            reassembler,
            shared,
            outgoing,
            handler: Arc::new(handler),
            shutdown,
        }
    }

    /// Applies `max_concurrent_calls` to a call that is arriving or starting
    /// to arrive.
    ///
    /// A concurrent call is one this end has begun receiving and has not yet
    /// answered, and it passes through two custodians on the way: the
    /// reassembler holds it while its payload is still fragmented, and
    /// `outstanding` holds it from dispatch until the response head. The
    /// limit is on the *sum* — the two counts are disjoint, since a message
    /// leaves payload phase in the same `accept` call that dispatches it — so
    /// neither custodian can enforce it alone, and checking them separately
    /// would admit twice the limit.
    ///
    /// `incomplete` is the reassembler's count *including* the call being
    /// admitted, so callers add one for a call that has already left payload
    /// phase.
    fn check_call_admission(&self, id: u64, incomplete: usize) -> Result<(), Error> {
        let inner = self.shared.inner.lock().unwrap();
        let duplicate = inner.outstanding.contains_key(&id);
        let outstanding = inner.outstanding.len();
        if duplicate {
            return Err(Error::Protocol(format!("duplicate active request id {id}")));
        }
        if outstanding + incomplete > self.shared.limits.max_concurrent_calls {
            return Err(Error::Protocol("too many concurrent calls".into()));
        }
        Ok(())
    }

    /// Runs until the peer disconnects, the session fails, or a handler
    /// requests shutdown.
    ///
    /// Knows nothing of the send driver beyond holding a sender into it:
    /// whether that half is still alive is [`Server::serve`]'s concern, and
    /// it ends this one by dropping the future.
    async fn run(mut self) -> Result<(), Error> {
        let mut tasks = FuturesUnordered::new();
        let (result, graceful) = 'main: loop {
            let mut frame = self.transport.recv();
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
                    self.reassembler.accept(header, &mut frame).await
                };
                tokio::pin!(step);
                loop {
                    tokio::select! {
                        result = &mut step => break result,
                        Some(_) = tasks.next(), if !tasks.is_empty() => continue,
                        _ = &mut self.shutdown => break 'main (Ok(()), true),
                    }
                }
            };
            let complete = match complete {
                Ok(complete) => complete,
                Err(error) => break 'main (Err(error), false),
            };
            let (message, live_trailer) = match complete {
                Event::None => (None, None),
                // A request has started arriving. It occupies the same
                // budget as one already dispatched, so it is admitted on the
                // same rule, at the earliest point this end knows about it.
                Event::PayloadIncomplete { id } => {
                    if let Err(error) =
                        self.check_call_admission(id, self.reassembler.payload_incomplete())
                    {
                        break 'main (Err(error), false);
                    }
                    (None, None)
                }
                Event::Aborted {
                    kind: Kind::Request,
                    ..
                } => (None, None),
                Event::Aborted { kind, .. } => {
                    break 'main (
                        Err(Error::Protocol(format!(
                            "unexpected aborted {kind:?} message"
                        ))),
                        false,
                    );
                }
                Event::Message(message) => (Some(message), None),
                Event::Ack { id, message } => {
                    let _ = self.outgoing.send(Outgoing::Ack { id });
                    (message, None)
                }
                Event::Trailer {
                    shared: trailer,
                    len,
                    ..
                } => (None, Some((trailer, len))),
                Event::Release { id, count } => {
                    self.shared.session.release(id, count);
                    (None, None)
                }
                Event::Credit { id, count } => {
                    // Applied here rather than routed through the writer;
                    // see the client's matching arm.
                    self.shared.trailer_session.refund(id, count as usize);
                    (None, None)
                }
            };
            if let Some(Message {
                kind,
                id,
                payload,
                handles,
                trailer,
            }) = message
            {
                match kind {
                    Kind::Request => {
                        // This message has already left payload phase, so it
                        // is no longer in the reassembler's count and has to
                        // be added back.
                        if let Err(error) =
                            self.check_call_admission(id, self.reassembler.payload_incomplete() + 1)
                        {
                            break (Err(error), false);
                        }
                        let request = match self.shared.decode(&payload, handles, &self.transport) {
                            Ok(request) => request,
                            Err(error) => break (Err(error), false),
                        };
                        let trailer = trailer.map(TrailerRecv::new);
                        let handler = self.handler.clone();
                        let task_shared = self.shared.clone();
                        let task_outgoing = self.outgoing.clone();
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
                        self.shared.inner.lock().unwrap().outstanding.insert(
                            id,
                            Cancellation {
                                signal: None,
                                abort,
                            },
                        );
                    }
                    Kind::Cancel => {
                        let mut state = self.shared.inner.lock().unwrap();
                        if let Some(signal) = state
                            .outstanding
                            .get_mut(&id)
                            .and_then(|cancel| cancel.signal.take())
                        {
                            let _ = signal.send(());
                        } else if let Some(cancel) = state.outstanding.get(&id) {
                            cancel.abort.abort();
                        } else {
                            let _ = self.outgoing.send(Outgoing::Cancel { id });
                        }
                    }
                    Kind::Discard => {
                        let _ = self.outgoing.send(Outgoing::PeerDiscarded { id });
                    }
                    Kind::Ack => {
                        if !self.shared.release_escrow(id) {
                            break (
                                Err(Error::Protocol(format!(
                                    "Ack for response {id} with no active escrow"
                                ))),
                                false,
                            );
                        }
                    }
                    _ => {
                        break (
                            Err(Error::Protocol(format!("unexpected {kind:?} frame"))),
                            false,
                        );
                    }
                }
            }
            if let Some((trailer, len)) = live_trailer {
                let frame = self.transport.recv();
                // SAFETY: the lease retains the receiver borrow and clears
                // the erased token before it ends.
                let lease = unsafe { RecvShared::grant(&trailer, frame, len) };
                let result = loop {
                    tokio::select! {
                        result = RecvShared::wait_fragment(&trailer) => break result,
                        Some(_) = tasks.next(), if !tasks.is_empty() => continue,
                        _ = &mut self.shutdown => break 'main (Ok(()), true),
                    }
                };
                if let Err(error) = result {
                    break 'main (Err(error.into()), false);
                }
                lease.complete();
            }
        };
        drop(self.transport);
        if graceful {
            // A handler asked to shut down rather than the session breaking,
            // so every call already dispatched still gets to answer. Their
            // responses queue up behind the send driver's channel, which the
            // caller drains after this returns.
            while tasks.next().await.is_some() {}
        }
        result
    }
}

/// Drives the send half of the connection: admits queued messages into the
/// fragment scheduler and advances the scheduler onto the transport.
///
/// Runs on [`Server::serve`]'s own future, alongside the receive driver and
/// the handlers — a response is queued rather than written by the handler
/// that produced it, so nothing here blocks on anything there.
struct SendDriver<P: Protocol> {
    transport: transport::AnySender,
    outgoing: mpsc::UnboundedReceiver<Outgoing<P::Response>>,
    shared: Arc<Shared>,
    scheduler: fragment::Scheduler,
}

impl<P: Protocol> SendDriver<P> {
    fn new(
        transport: transport::AnySender,
        outgoing: mpsc::UnboundedReceiver<Outgoing<P::Response>>,
        shared: Arc<Shared>,
    ) -> Self {
        let scheduler = fragment::Scheduler::new(&shared.limits);
        Self {
            transport,
            outgoing,
            shared,
            scheduler,
        }
    }

    async fn run(mut self) -> Result<(), Error> {
        // Holding a clone of `outgoing`'s sender half (the local `outgoing` in
        // `serve`, or a `CallContext`'s) is what represents the ability to
        // still get a message in, so the channel closing — every clone gone —
        // doubles as this task's shutdown signal: once `recv()` reports no more
        // messages will ever arrive, admission of new work stops, and the loop
        // keeps advancing the scheduler until it's fully drained before
        // exiting, never abandoning a write already committed to it.
        let mut closed = false;
        while !closed || self.scheduler.has_work() {
            tokio::select! {
                message = self.outgoing.recv(), if !closed => {
                    let Some(message) = message else {
                        closed = true;
                        continue;
                    };
                    self.admit(message).await?;
                }
                // Not raced against anything — see the matching comment in
                // client.rs's writer loop. A dropped send future could leave a
                // committed partial fragment on the transport, or — on
                // transports whose writes are dispatched to a detached
                // background task — let an abandoned write complete arbitrarily
                // later, after the peer has already torn down its end.
                _ = self.scheduler.ready(), if self.scheduler.has_work() => {
                    match self.scheduler.advance(&mut self.transport).await? {
                        fragment::AdvanceOutcome::None | fragment::AdvanceOutcome::Aborted(_) => {}
                        #[cfg(target_os = "macos")]
                        fragment::AdvanceOutcome::Escrow { id, fds, handles_done } => {
                            self.shared.escrow_sent(id, fds, handles_done);
                        }
                    }
                    // Flush anything sent by the scheduler
                    let _ = self.transport.flush().await;
                }
            }
        }
        Ok(())
    }

    /// Admits one outgoing item to the fragment scheduler.
    async fn admit(&mut self, message: Outgoing<P::Response>) -> Result<(), Error> {
        match message {
            Outgoing::Response { id, value, trailer } => {
                let mut ledger = session::Ledger::default();
                let mut put_handles = session::SessionFrame {
                    inner: EncodeHandles::new(&self.transport, self.shared.max_handles()),
                    session: &self.shared.session,
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
                let handles = self.shared.finish_handles(id, put_handles.inner);
                self.scheduler
                    .admit_message(Kind::Response, id, payload, handles, trailer, ledger);
            }
            Outgoing::Error { id } => self.scheduler.admit_empty(Kind::Error, id),
            Outgoing::Cancel { id } => match self.scheduler.try_cancel_active(id) {
                fragment::AbortOutcome::NotActive => {}
                fragment::AbortOutcome::Discarded { started, .. } => {
                    if started {
                        self.scheduler.admit_abort(id);
                    }
                    if !started {
                        self.shared.discard_unsent_escrow(id);
                    }
                }
            },
            Outgoing::DiscardTrailer { id } => self.scheduler.admit_empty(Kind::Discard, id),
            Outgoing::PeerDiscarded { id } => {
                // The peer will never credit what it just threw away; see the
                // client's matching arm.
                self.shared.trailer_session.settle(id);
                self.scheduler.discard_active_trailer(id);
            }
            Outgoing::Ack { id } => self.scheduler.admit_empty(Kind::Ack, id),
            Outgoing::Release { id, count } => self.scheduler.admit_release(id, count),
            Outgoing::Credit { id, count } => self.scheduler.admit_credit(id, count),
        }
        Ok(())
    }
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
    outgoing: mpsc::UnboundedSender<Outgoing<P::Response>>,
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
        let _ = self.outgoing.send(Outgoing::Response {
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
        let _ = self.outgoing.send(Outgoing::Response {
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
            let _ = self.outgoing.send(Outgoing::Error { id: self.id });
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

    struct Test;

    impl Protocol for Test {
        type Request = u8;
        type Response = u8;
    }

    /// A receive driver over a transport nothing ever reads or writes:
    /// enough to exercise the checks that only consult `Shared`.
    fn test_driver(
        max_concurrent_calls: usize,
    ) -> RecvDriver<Test, impl AsyncFn(CallContext<Test>, u8) + Send + Sync + 'static> {
        let (stream, _peer) = tokio::io::duplex(64);
        let (_sender, receiver) = transport::generic_duplex(stream);
        let (outgoing, _outgoing_rx) = mpsc::unbounded_channel();
        let (shutdown, shutdown_requested) = oneshot::channel();
        let limits = Limits {
            max_concurrent_calls,
            ..Limits::default()
        };
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                outstanding: HashMap::new(),
                shutdown: Some(shutdown),
                #[cfg(target_os = "macos")]
                fd_escrow: Default::default(),
            }),
            session: Session::new(Box::new(outgoing.downgrade())),
            trailer_session: Arc::new(SessionWindow::new(limits.trailer_session_window)),
            limits,
        });
        RecvDriver::new(
            transport::AnyReceiver::Generic(receiver),
            outgoing,
            shutdown_requested,
            shared,
            async |_: CallContext<Test>, _: u8| {},
        )
    }

    fn dispatch<P: Protocol, H>(driver: &RecvDriver<P, H>, id: u64) {
        driver
            .shared
            .inner
            .lock()
            .unwrap()
            .outstanding
            .insert(id, cancellation());
    }

    #[tokio::test]
    async fn call_admission_rejects_excess_and_duplicate_requests() {
        let driver = test_driver(1);
        assert!(driver.check_call_admission(7, 1).is_ok());
        dispatch(&driver, 7);
        assert!(matches!(
            driver.check_call_admission(8, 1),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));

        // A duplicate is refused on its own account, not because the limit
        // happens to be full.
        let roomy = test_driver(8);
        dispatch(&roomy, 7);
        assert!(matches!(
            roomy.check_call_admission(7, 1),
            Err(Error::Protocol(message)) if message == "duplicate active request id 7"
        ));
    }

    /// The two halves share one budget: a call still arriving counts against
    /// the same limit as one already dispatched, so neither half may be
    /// admitted on its own count.
    #[tokio::test]
    async fn call_admission_counts_arriving_and_dispatched_calls_together() {
        let driver = test_driver(2);
        dispatch(&driver, 7);
        // One dispatched, one arriving, limit of two: exactly full, and the
        // arriving one is what the count already includes.
        assert!(driver.check_call_admission(8, 1).is_ok());
        assert!(matches!(
            driver.check_call_admission(8, 2),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));
    }

    #[tokio::test]
    async fn zero_call_limit_rejects_the_first_request() {
        assert!(matches!(
            test_driver(0).check_call_admission(0, 1),
            Err(Error::Protocol(message)) if message == "too many concurrent calls"
        ));
    }
}
