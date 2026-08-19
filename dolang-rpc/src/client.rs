use std::{
    collections::{HashMap, HashSet, VecDeque},
    future::Future,
    mem,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
};

#[cfg(windows)]
use std::{any::TypeId, io};

use tokio::sync::{mpsc, oneshot};

#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetProcessId;

#[cfg(unix)]
use crate::session::SessionHandles;
use crate::{
    Error, Limits, Protocol,
    fragment::{self, AbortOutcome, Event, Kind, Message, Reassembler, Scheduler, Trailer},
    serde::{decode_payload, encode_payload},
    session::{Ledger, Session, SessionFrame},
    trailer::{RecvShared, SendShared, SessionWindow, TrailerRecv, TrailerSend, TrailerSink},
    transport::{self, EncodeHandles, Receiver, Sender},
};
#[cfg(windows)]
use crate::{handle::TakeHandle, session::Inner as OpaqueInner};

/// A negotiated client endpoint that has not yet been bound to a [`Protocol`].
///
/// Inspect its negotiated application protocol, then consume it with
/// [`bind`](Unbound::bind) to obtain a [`Client`].
pub use crate::unbound::UnboundClient as Unbound;

type Pending<R> = HashMap<u64, oneshot::Sender<Result<CallResult<R>, Error>>>;

#[cfg(windows)]
struct DecodeHandles<'a> {
    consumed: HashSet<usize>,
    count: usize,
    max_handles: usize,
    session: &'a Arc<Session>,
}

#[cfg(windows)]
impl<'a> DecodeHandles<'a> {
    fn new(max_handles: usize, session: &'a Arc<Session>) -> Self {
        Self {
            consumed: HashSet::new(),
            count: 0,
            max_handles,
            session,
        }
    }
}

#[cfg(windows)]
impl TakeHandle for DecodeHandles<'_> {
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle> {
        if !self.consumed.insert(value) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "handle value was already consumed",
            ));
        }
        self.count += 1;
        // SAFETY: the trusted server created this value in our process with
        // DuplicateHandle before transmitting it.
        Ok(unsafe { OwnedHandle::from_raw_handle(value as _) })
    }

    fn finish(&mut self) -> io::Result<()> {
        if self.count > self.max_handles {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message contains too many handle attachments",
            ));
        }
        Ok(())
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

/// `(id, response receiver, cancel_sent)`, returned by `Client::begin`.
type BeginResult<P> = (
    u64,
    oneshot::Receiver<Result<CallResult<<P as Protocol>::Response>, Error>>,
    bool,
);

enum Outgoing<Q> {
    Request {
        id: u64,
        value: Q,
        trailer: Trailer,
    },
    Cancel {
        id: u64,
    },
    /// We stopped reading a response trailer (it arrived unwanted) and want
    /// to tell the peer to stop sending it. Always results in a wire
    /// `Kind::Discard` fragment — this connection never has an active
    /// outgoing send under a response id to abort locally instead.
    DiscardTrailer {
        id: u64,
    },
    /// A wire `Kind::Discard` fragment arrived, telling us the peer no
    /// longer wants our request trailer. Applied to our own active send;
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
    /// We retired `count` bytes of the response trailer on `id` and are
    /// returning that much credit. Always results in a wire `Kind::Credit`.
    Credit {
        id: u64,
        count: u32,
    },
    /// A terminal response or error arrived for an active request.
    Terminal {
        id: u64,
    },
}

impl<Q: Send + 'static> crate::session::ReleaseSink for mpsc::WeakUnboundedSender<Outgoing<Q>> {
    fn release(&self, id: u64, count: u32) {
        // Called from `Drop`, so a departed channel is not an error: the
        // writer is already gone and the peer's table dies with the session.
        if let Some(outgoing) = self.upgrade() {
            let _ = outgoing.send(Outgoing::Release { id, count });
        }
    }
}

impl<Q: Send + 'static> crate::trailer::TrailerSink for mpsc::WeakUnboundedSender<Outgoing<Q>> {
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

struct Inner<P: Protocol> {
    // Holding a clone of this sender represents the ability to still get a
    // message into the writer, so closing the channel — clearing this to
    // `None` — is itself the writer's shutdown signal (see `Writer::run`):
    // no separate oneshot needed. This is the only strong sender; the
    // session's release sink holds a weak one so that it cannot keep the
    // channel open past this point.
    outgoing: Mutex<Option<mpsc::UnboundedSender<Outgoing<P::Request>>>>,
    pending: Mutex<Pending<P::Response>>,
    next_id: Mutex<u64>,
    tasks: Mutex<Option<Tasks>>,
    #[cfg(windows)]
    handle_escrow: Mutex<HashMap<u64, Vec<OwnedHandle>>>,
    #[cfg(target_os = "macos")]
    fd_escrow: Mutex<crate::escrow::FdEscrow>,
    session: Arc<Session>,
    /// Send-side trailer credit shared by every outgoing trailer on this
    /// connection. Bounds what the peer must buffer for us in aggregate.
    trailer_session: Arc<SessionWindow>,
    limits: Limits,
    #[cfg(windows)]
    _peer_process: Option<OwnedHandle>,
}

struct Writer<P: Protocol> {
    transport: transport::AnySender,
    outgoing: mpsc::UnboundedReceiver<Outgoing<P::Request>>,
    inner: Weak<Inner<P>>,
    limits: Limits,
    scheduler: Scheduler,
    /// Calls admitted to the scheduler and not yet terminated, which is what
    /// `max_concurrent_calls` bounds on this end. An entry is added where the
    /// request actually reaches the scheduler (`admit_request`) and removed
    /// on the reader's `Outgoing::Terminal`, or on a cancel that got there
    /// before the request did.
    active_calls: HashSet<u64>,
    /// Requests held back because `active_calls` is at the limit, promoted in
    /// arrival order as slots free up.
    waiting: VecDeque<Outgoing<P::Request>>,
}

struct Reader<P: Protocol> {
    transport: transport::AnyReceiver,
    inner: Weak<Inner<P>>,
    /// The route trailers use to credit and discard themselves. Weak, since
    /// a `TrailerRecv` can outlive the session and must not keep the writer
    /// alive just to say it is going away.
    trailer_sink: Arc<dyn TrailerSink>,
    limits: Limits,
}

struct Tasks {
    reader_shutdown: Option<oneshot::Sender<()>>,
    writer: tokio::task::JoinHandle<Result<(), Error>>,
    reader: tokio::task::JoinHandle<()>,
}

impl Tasks {
    fn shutdown(&mut self) {
        if let Some(shutdown) = self.reader_shutdown.take() {
            let _ = shutdown.send(());
        }
    }

    async fn join(mut self) {
        self.shutdown();
        let _ = tokio::join!(self.writer, self.reader);
    }
}

impl<P: Protocol> Drop for Inner<P> {
    fn drop(&mut self) {
        // Close the writer's channel first — see the comment on `outgoing`.
        self.outgoing.lock().unwrap().take();
        if let Some(tasks) = self.tasks.get_mut().unwrap().as_mut() {
            tasks.shutdown();
        }
        self.fail(Error::ConnectionClosed);
    }
}

impl<P: Protocol> Inner<P> {
    /// Best-effort send: silently dropped if the writer's channel has
    /// already been closed.
    fn send(&self, message: Outgoing<P::Request>) {
        if let Some(sender) = self.outgoing.lock().unwrap().as_ref() {
            let _ = sender.send(message);
        }
    }

    fn complete(&self, id: u64, result: Result<CallResult<P::Response>, Error>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    }

    fn fail(&self, error: Error) {
        for (_, tx) in mem::take(&mut *self.pending.lock().unwrap()) {
            let _ = tx.send(Err(error.copy()));
        }
    }
}

/// A cloneable endpoint for sending requests on one RPC session.
///
/// Clones share request IDs, pending calls, and session lifetime. Calling
/// [`close`](Self::close) on any clone closes the shared session.
pub struct Client<P: Protocol> {
    inner: Arc<Inner<P>>,
}

impl<P: Protocol> Clone for Client<P> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<P: Protocol> Client<P> {
    /// Returns whether both clients refer to the same RPC session.
    pub fn is_same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Builds a `Client` from an already-negotiated transport. Only reachable
    /// via [`Unbound::bind`] — `Client` has
    /// no public constructors of its own, so every `Client<P>` has already
    /// completed `fragment::negotiate` by the time it exists.
    pub(crate) fn from_transport(
        sender: transport::AnySender,
        receiver: transport::AnyReceiver,
        limits: Limits,
        #[cfg(windows)] peer_process: Option<OwnedHandle>,
    ) -> Self {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let session = Session::new(Box::new(outgoing.downgrade()));
        let trailer_sink = outgoing.downgrade();
        let inner = Arc::new(Inner {
            outgoing: Mutex::new(Some(outgoing)),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(0),
            tasks: Mutex::new(None),
            #[cfg(windows)]
            handle_escrow: Mutex::new(HashMap::new()),
            #[cfg(target_os = "macos")]
            fd_escrow: Mutex::new(Default::default()),
            session,
            trailer_session: Arc::new(SessionWindow::new(limits.trailer_session_window)),
            limits,
            #[cfg(windows)]
            _peer_process: peer_process,
        });
        let (reader_shutdown, reader_stop) = oneshot::channel();
        let writer = tokio::spawn(
            Writer {
                transport: sender,
                outgoing: outgoing_rx,
                inner: Arc::downgrade(&inner),
                limits,
                scheduler: Scheduler::new(&limits),
                active_calls: HashSet::new(),
                waiting: VecDeque::new(),
            }
            .run(),
        );
        let reader = tokio::spawn(
            Reader {
                transport: receiver,
                inner: Arc::downgrade(&inner),
                trailer_sink: Arc::new(trailer_sink),
                limits,
            }
            .run(reader_stop),
        );
        *inner.tasks.lock().unwrap() = Some(Tasks {
            reader_shutdown: Some(reader_shutdown),
            writer,
            reader,
        });
        Self { inner }
    }

    /// Closes the shared session and waits for its background tasks to exit.
    ///
    /// This prevents new calls from being sent and completes all pending calls
    /// with [`Error::ConnectionClosed`]. It affects every clone of this
    /// client.
    pub async fn close(self) {
        let tasks = self.inner.tasks.lock().unwrap().take();
        // Close the writer's channel first — see the comment on `outgoing`.
        self.inner.outgoing.lock().unwrap().take();
        self.inner.fail(Error::ConnectionClosed);
        if let Some(tasks) = tasks {
            tasks.join().await;
        }
    }

    /// Begins one request and returns a future for its response.
    ///
    /// Dropping the returned [`Call`] before it completes requests best-effort
    /// cancellation from the peer.
    pub fn call(&self, request: P::Request) -> Call<P> {
        let ((id, rx, cancel_sent), ()) = self.begin(|id| {
            (
                Outgoing::Request {
                    id,
                    value: request,
                    trailer: Trailer::None,
                },
                (),
            )
        });
        Call {
            id,
            rx,
            inner: self.inner.clone(),
            cancel_sent,
        }
    }

    /// Begins one request with a streaming raw-byte trailer.
    ///
    /// Write the trailer through the returned [`TrailerSend`], then call
    /// [`TrailerSend::finish`] (or asynchronously shut it down) to obtain the
    /// [`Call`]. Dropping the sender without finishing aborts the trailer and
    /// cancels the partially sent request.
    pub fn call_with_trailer(&self, request: P::Request) -> TrailerSend<Call<P>> {
        let ((id, rx, cancel_sent), shared) = self.begin(|id| {
            let shared = SendShared::new(
                Kind::Request,
                id,
                &self.inner.limits,
                self.inner.trailer_session.clone(),
            );
            (
                Outgoing::Request {
                    id,
                    value: request,
                    trailer: Trailer::Stream(shared.clone()),
                },
                shared,
            )
        });
        if cancel_sent {
            SendShared::discard(&shared);
        }
        TrailerSend::new(
            shared,
            Call {
                id,
                rx,
                inner: self.inner.clone(),
                cancel_sent,
            },
        )
    }

    /// Shared id-allocation/pending-registration logic for `call` and
    /// `call_with_trailer`. `build` constructs the outgoing message once the
    /// id is known. Returns the id, the response receiver, and whether a
    /// cancel has effectively already been sent (nothing left to cancel).
    fn begin<T>(
        &self,
        build: impl FnOnce(u64) -> (Outgoing<P::Request>, T),
    ) -> (BeginResult<P>, T) {
        let (tx, rx) = oneshot::channel();
        let id = {
            let mut next = self.inner.next_id.lock().unwrap();
            let id = *next;
            *next = id.checked_add(1).expect("request identifiers exhausted");
            id
        };
        let (message, value) = build(id);
        self.inner.pending.lock().unwrap().insert(id, tx);
        let queued = self
            .inner
            .outgoing
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|sender| sender.send(message).is_ok());
        if !queued {
            self.inner.complete(id, Err(Error::ConnectionClosed));
        }
        ((id, rx, !queued), value)
    }
}

#[cfg(windows)]
pub(crate) fn validate_peer_process(
    peer_process: &OwnedHandle,
    pipe_peer_pid: u32,
) -> io::Result<()> {
    let process_pid = unsafe { GetProcessId(peer_process.as_raw_handle() as _) };
    if process_pid == 0 {
        return Err(io::Error::last_os_error());
    }
    if process_pid != pipe_peer_pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named-pipe peer does not match the expected process",
        ));
    }
    Ok(())
}

/// A completed call's response and its optional raw-byte trailer.
///
/// Use [`into_response`](Self::into_response) when the trailer is not needed,
/// or [`into_response_trailer`](Self::into_response_trailer) to retain it —
/// or [`into_response_trailer_manual_credit`](Self::into_response_trailer_manual_credit)
/// to retain it and take charge of returning its credit.
pub struct CallResult<R> {
    response: R,
    trailer: Option<TrailerRecv>,
}

impl<R> CallResult<R> {
    /// Discards any response trailer and returns just the response.
    pub fn into_response(self) -> R {
        self.response
    }

    /// Decomposes into the response and its readable trailer, if present.
    pub fn into_response_trailer(self) -> (R, Option<TrailerRecv>) {
        (self.response, self.trailer)
    }

    /// Decomposes into the response and its trailer in manual-credit mode.
    ///
    /// The consumer then owes the peer an explicit
    /// [`TrailerRecv::release`](crate::trailer::TrailerRecv::release) for
    /// every chunk it finishes with, instead of credit being returned on
    /// read — worth it when the bytes are handed somewhere slower than this
    /// process, so the peer's send rate follows the real drain rate. Read
    /// [`release`](crate::trailer::TrailerRecv::release) first: manual mode
    /// moves a deadlock rule into calling code.
    ///
    /// The mode is fixed here rather than switchable afterwards, so a
    /// trailer cannot be half auto-credited and half not.
    pub fn into_response_trailer_manual_credit(self) -> (R, Option<TrailerRecv>) {
        let mut trailer = self.trailer;
        if let Some(trailer) = trailer.as_mut() {
            trailer.set_manual_credit();
        }
        (self.response, trailer)
    }
}

/// An in-progress RPC request.
///
/// Await this future to receive the response and its optional trailer, or an
/// [`Error`]. Dropping it before completion sends best-effort cancellation to
/// the peer.
pub struct Call<P: Protocol> {
    id: u64,
    rx: oneshot::Receiver<Result<CallResult<P::Response>, Error>>,
    inner: Arc<Inner<P>>,
    cancel_sent: bool,
}

impl<P: Protocol> Call<P> {
    /// Requests best-effort cancellation and leaves the call awaitable.
    ///
    /// This is idempotent. A response that races with cancellation may still
    /// complete successfully.
    pub fn cancel(&mut self) {
        if !self.cancel_sent {
            self.cancel_sent = true;
            self.inner.send(Outgoing::Cancel { id: self.id });
        }
    }
}

impl<P: Protocol> Future for Call<P> {
    type Output = Result<CallResult<P::Response>, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.rx).poll(cx) {
            Poll::Ready(Ok(Ok(result))) => Poll::Ready(Ok(result)),
            Poll::Ready(Ok(Err(e))) => Poll::Ready(Err(e)),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Error::ConnectionClosed)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<P: Protocol> Drop for Call<P> {
    fn drop(&mut self) {
        if self
            .inner
            .pending
            .lock()
            .unwrap()
            .remove(&self.id)
            .is_some()
        {
            self.cancel();
        }
    }
}

impl<P: Protocol> Writer<P> {
    /// Best-effort completion of a pending call with an error; a no-op if
    /// the session is already gone. Also drops any handles escrowed until
    /// the peer has had an opportunity to duplicate them.
    fn complete_err(&self, id: u64, error: Error) {
        if let Some(inner) = self.inner.upgrade() {
            #[cfg(windows)]
            inner.handle_escrow.lock().unwrap().remove(&id);
            inner.complete(id, Err(error));
        }
    }

    /// Admits one queued item into the scheduler. Returns `Err` on a fatal
    /// transport/protocol error, which the caller must treat as fatal for
    /// the whole session, not just this one message.
    async fn admit(&mut self, message: Outgoing<P::Request>) -> Result<(), Error> {
        match message {
            Outgoing::Request { id, value, trailer } => {
                self.admit_request(id, value, trailer).await
            }
            Outgoing::Cancel { id } => {
                self.admit_cancel(id);
                Ok(())
            }
            Outgoing::DiscardTrailer { id } => {
                self.scheduler.admit_empty(Kind::Discard, id);
                Ok(())
            }
            Outgoing::PeerDiscarded { id } => {
                // The peer will never credit what it just threw away, so
                // settle by id rather than through the send, which may
                // already have finished and left the scheduler.
                if let Some(inner) = self.inner.upgrade() {
                    inner.trailer_session.settle(id);
                }
                self.scheduler.discard_active_trailer(id);
                Ok(())
            }
            Outgoing::Ack { id } => {
                self.scheduler.admit_empty(Kind::Ack, id);
                Ok(())
            }
            Outgoing::Release { id, count } => {
                self.scheduler.admit_release(id, count);
                Ok(())
            }
            Outgoing::Credit { id, count } => {
                self.scheduler.admit_credit(id, count);
                Ok(())
            }
            Outgoing::Terminal { .. } => unreachable!("handled by the writer loop"),
        }
    }

    async fn admit_request(
        &mut self,
        id: u64,
        value: P::Request,
        trailer: Trailer,
    ) -> Result<(), Error> {
        #[cfg(unix)]
        let max_handles = if self.limits.max_handles_per_fragment == 0 {
            0
        } else {
            self.limits.max_handles_per_message
        };
        #[cfg(windows)]
        let max_handles = self.limits.max_handles_per_message;
        let Some(inner) = self.inner.upgrade() else {
            return Ok(());
        };
        let mut ledger = Ledger::default();
        let mut put_handles = SessionFrame {
            inner: EncodeHandles::new(&self.transport, max_handles),
            session: &inner.session,
            ledger: &mut ledger,
        };
        let payload = match encode_payload(&value, &mut put_handles) {
            Ok(payload) => payload,
            Err(err) => {
                // The ledger drops here without committing. Nothing of this
                // message reached the wire, so any gift it named is rescinded
                // by that drop path, not by a commit.
                drop(put_handles);
                ledger.rescind();
                self.complete_err(id, err);
                return Ok(());
            }
        };
        let put_handles = put_handles.inner;
        #[cfg(unix)]
        let handles = put_handles.finish();
        #[cfg(target_os = "macos")]
        if handles.needs_ack()
            && let Some(inner) = self.inner.upgrade()
        {
            inner.fd_escrow.lock().unwrap().register(id);
        }
        #[cfg(windows)]
        let (handles, escrow) = put_handles.finish();
        #[cfg(windows)]
        if !escrow.is_empty()
            && let Some(inner) = self.inner.upgrade()
        {
            inner.handle_escrow.lock().unwrap().insert(id, escrow);
        }
        self.scheduler
            .admit_message(Kind::Request, id, payload, handles, trailer, ledger);
        // The call takes its slot here, where its request actually reaches
        // the scheduler — never on the way in. A request that fails to encode
        // is completed locally above and never goes out, so no response and
        // no `Terminal` will ever come back to release a slot taken earlier.
        self.active_calls.insert(id);
        Ok(())
    }

    fn admit_cancel(&mut self, id: u64) -> bool {
        match self.scheduler.try_cancel_active(id) {
            AbortOutcome::NotActive => {
                self.scheduler.admit_empty(Kind::Cancel, id);
                false
            }
            AbortOutcome::Discarded {
                started,
                dispatched,
            } => {
                if started {
                    self.scheduler.admit_abort(id);
                }
                #[cfg(target_os = "macos")]
                if !started && let Some(inner) = self.inner.upgrade() {
                    inner.fd_escrow.lock().unwrap().discard_unsent(id);
                }
                if dispatched {
                    self.scheduler.admit_empty(Kind::Cancel, id);
                    false
                } else {
                    self.complete_err(id, Error::Cancelled);
                    true
                }
            }
        }
    }

    async fn promote_waiting(&mut self) -> Result<(), Error> {
        while self.active_calls.len() < self.limits.max_concurrent_calls {
            let Some(message) = self.waiting.pop_front() else {
                break;
            };
            let Outgoing::Request { id, .. } = &message else {
                unreachable!("only requests wait for call admission")
            };
            let id = *id;
            let Some(inner) = self.inner.upgrade() else {
                break;
            };
            if !inner.pending.lock().unwrap().contains_key(&id) {
                continue;
            }
            self.admit(message).await?;
        }
        Ok(())
    }

    async fn run(mut self) -> Result<(), Error> {
        // Holding a clone of `Inner::outgoing` is what represents the
        // ability to still get a message in (see its doc comment), so the
        // channel closing — every clone gone — doubles as the shutdown
        // signal: once `recv()` reports no more messages will ever arrive,
        // admission of new work stops, and the loop keeps advancing the
        // scheduler until it's fully drained before exiting, never
        // abandoning a write already committed to it.
        let mut closed = false;
        while !closed || self.scheduler.has_work() {
            tokio::select! {
                message = self.outgoing.recv(), if !closed => {
                    let Some(message) = message else {
                        closed = true;
                        self.waiting.clear();
                        continue;
                    };
                    // No blanket `fail_all` here: `admit` already fails just
                    // the one call whose request it couldn't get onto the
                    // transport (see `admit_request`). Every other pending
                    // call's request either already made it out, or is still
                    // queued for a later turn of this same loop — a write
                    // failure on one message doesn't mean every other one is
                    // doomed, only that this connection is. The reader is
                    // what authoritatively decides that (see the comment
                    // after this loop).
                    match message {
                        request @ Outgoing::Request { .. } => self.waiting.push_back(request),
                        Outgoing::Cancel { id } => {
                            if let Some(pos) = self.waiting.iter().position(
                                |message| matches!(message, Outgoing::Request { id: waiting_id, .. } if *waiting_id == id),
                            ) {
                                self.waiting.remove(pos);
                                self.complete_err(id, Error::Cancelled);
                            } else if self.active_calls.contains(&id) && self.admit_cancel(id) {
                                self.active_calls.remove(&id);
                            }
                        }
                        Outgoing::Terminal { id } => {
                            self.active_calls.remove(&id);
                        }
                        message => self.admit(message).await?,
                    }
                    self.promote_waiting().await?;
                }
                // Not raced against anything: once ready, a fragment write
                // is committed to the scheduler and must run to completion.
                // A dropped send future could otherwise leave a committed
                // partial fragment on the transport, or — on transports
                // whose writes are dispatched to a detached background task
                // (e.g. the blocking-pool-backed Windows pipe transport) —
                // let an abandoned write complete arbitrarily later,
                // potentially after the peer has already torn down its end.
                _ = self.scheduler.ready(), if self.scheduler.has_work() => {
                    let result = self.scheduler.advance(&mut self.transport).await;
                    // Flush anything sent by the scheduler
                    let _ = self.transport.flush().await;
                    match result {
                        // A streaming trailer producer was dropped mid-message.
                        Ok(fragment::AdvanceOutcome::Aborted(id)) => {
                            // The postcard payload has already reached the peer
                            // before a streaming trailer can abort. Cancel the
                            // dispatched handler and retain its call slot until
                            // the resulting terminal message arrives.
                            self.scheduler.admit_empty(Kind::Cancel, id);
                        }
                        Ok(fragment::AdvanceOutcome::None) => {}
                        #[cfg(target_os = "macos")]
                        Ok(fragment::AdvanceOutcome::Escrow { id, fds, handles_done }) => {
                            if let Some(inner) = self.inner.upgrade() {
                                inner.fd_escrow.lock().unwrap().sent(id, fds, handles_done);
                            }
                        }
                        // No blanket `fail_all`: a write failure here means
                        // this connection is broken, not that every pending
                        // call's already-sent request was never delivered.
                        // The reader observes the same broken connection
                        // (see the comment after this loop) and is what
                        // authoritatively fails pending calls.
                        Err(err) => {
                            return Err(err);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl<P: Protocol> Reader<P> {
    fn dispatch(&self, message: Message) -> Result<(), Error> {
        let Some(inner) = self.inner.upgrade() else {
            return Ok(());
        };
        let Message {
            kind,
            id,
            payload,
            handles,
            trailer,
        } = message;
        #[cfg(windows)]
        let _ = handles;
        match kind {
            Kind::Response => {
                // Decoding happens here, in the reader task, *before*
                // `complete` discovers whether anyone still wants this
                // response. Decoding is what mirrors any opaque the payload
                // carries; the resulting `CallResult` is then dropped
                // normally when the call was cancelled, which releases those
                // references back to the peer. Skipping the decode for a
                // response nobody is waiting on would look like an
                // optimization and would silently leak every handle in it.
                #[cfg(unix)]
                let response = decode_payload(
                    &payload,
                    &mut SessionHandles {
                        inner: handles,
                        session: &inner.session,
                    },
                )?;
                #[cfg(windows)]
                let response = decode_payload(
                    &payload,
                    &mut DecodeHandles::new(self.limits.max_handles_per_message, &inner.session),
                )?;
                let trailer = trailer.map(TrailerRecv::new);
                #[cfg(windows)]
                inner.handle_escrow.lock().unwrap().remove(&id);
                inner.complete(id, Ok(CallResult { response, trailer }));
                inner.send(Outgoing::Terminal { id });
            }
            Kind::Error => {
                #[cfg(windows)]
                inner.handle_escrow.lock().unwrap().remove(&id);
                inner.complete(id, Err(Error::Cancelled));
                inner.send(Outgoing::Terminal { id });
            }
            #[cfg(target_os = "macos")]
            Kind::Ack => {
                if !inner.fd_escrow.lock().unwrap().release(id) {
                    return Err(Error::Protocol(format!(
                        "Ack for request {id} with no active escrow"
                    )));
                }
            }
            Kind::Discard => inner.send(Outgoing::PeerDiscarded { id }),
            kind => return Err(Error::Protocol(format!("unexpected {kind:?} frame"))),
        }
        Ok(())
    }

    async fn run(mut self, mut shutdown: oneshot::Receiver<()>) {
        let mut reassembler = Reassembler::new(self.limits, self.trailer_sink.clone());
        loop {
            let mut frame = self.transport.recv();
            let header = tokio::select! {
                header = fragment::read_fragment_header(&mut frame) => header,
                _ = &mut shutdown => return,
            };
            let header = match header {
                Ok(header) => header,
                Err(error) => {
                    fail(&self.inner, error);
                    return;
                }
            };
            let accepted = tokio::select! {
                accepted = reassembler.accept(header, &mut frame) => accepted,
                _ = &mut shutdown => return,
            };
            let complete = match accepted {
                Ok(complete) => complete,
                Err(error) => {
                    fail(&self.inner, error);
                    return;
                }
            };
            match complete {
                Event::None => {}
                Event::Aborted {
                    kind,
                    id,
                    dispatched,
                } => {
                    if kind != Kind::Response {
                        fail(
                            &self.inner,
                            Error::Protocol(format!("unexpected aborted {kind:?} message")),
                        );
                        return;
                    }
                    if !dispatched && let Some(inner) = self.inner.upgrade() {
                        #[cfg(windows)]
                        inner.handle_escrow.lock().unwrap().remove(&id);
                        inner.complete(id, Err(Error::Cancelled));
                        inner.send(Outgoing::Terminal { id });
                    }
                }
                Event::Message(message) => {
                    if let Err(error) = self.dispatch(message) {
                        fail(&self.inner, error);
                        return;
                    }
                }
                Event::Ack { id, message } => {
                    if let Some(inner) = self.inner.upgrade() {
                        inner.send(Outgoing::Ack { id });
                    }
                    if let Some(message) = message
                        && let Err(error) = self.dispatch(message)
                    {
                        fail(&self.inner, error);
                        return;
                    }
                }
                Event::Trailer { shared, len, .. } => {
                    let frame = self.transport.recv();
                    // SAFETY: the lease retains the receiver borrow and
                    // clears the erased token before it ends.
                    let lease = unsafe { RecvShared::grant(&shared, frame, len) };
                    if let Err(error) = RecvShared::wait_fragment(&shared).await {
                        fail(&self.inner, error.into());
                        return;
                    }
                    lease.complete();
                }
                Event::Release { id, count } => {
                    if let Some(inner) = self.inner.upgrade() {
                        inner.session.release(id, count);
                    }
                }
                Event::Credit { id, count } => {
                    // Applied here rather than routed through the writer:
                    // the pool is shared state on `Inner`, and the refund
                    // needs nothing the writer owns. It is keyed by id and
                    // lands whether or not the send still exists, since a
                    // trailer's last credits routinely arrive after it has
                    // finished and left the scheduler, and dropping those
                    // would shrink the pool on every transfer.
                    if let Some(inner) = self.inner.upgrade() {
                        inner.trailer_session.refund(id, count as usize);
                    }
                }
            }
        }
    }
}

/// Fails every pending call. Takes `inner` by reference (rather than a
/// `Reader` method borrowing `&self`) so it can be called while another
/// field (e.g. a `RecvFrame` token borrowing `self.transport`) is still
/// mutably borrowed.
fn fail<P: Protocol>(inner: &Weak<Inner<P>>, error: Error) {
    if let Some(inner) = inner.upgrade() {
        inner.fail(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Test;

    impl Protocol for Test {
        type Request = u8;
        type Response = u8;
    }

    fn pending_call() -> (Call<Test>, mpsc::UnboundedReceiver<Outgoing<u8>>) {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            outgoing: Mutex::new(Some(outgoing.clone())),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            tasks: Mutex::new(None),
            #[cfg(windows)]
            handle_escrow: Mutex::new(HashMap::new()),
            #[cfg(target_os = "macos")]
            fd_escrow: Mutex::new(Default::default()),
            session: Session::new(Box::new(outgoing.downgrade())),
            trailer_session: Arc::new(SessionWindow::new(Limits::default().trailer_session_window)),
            limits: Limits::default(),
            #[cfg(windows)]
            _peer_process: None,
        });
        let (tx, rx) = oneshot::channel();
        inner.pending.lock().unwrap().insert(0, tx);
        (
            Call {
                id: 0,
                rx,
                inner,
                cancel_sent: false,
            },
            outgoing_rx,
        )
    }

    #[tokio::test]
    async fn completed_call_does_not_send_cancel_when_dropped() {
        let (call, mut outgoing) = pending_call();
        let inner = call.inner.clone();
        call.inner.complete(
            call.id,
            Ok(CallResult {
                response: 7,
                trailer: None,
            }),
        );
        assert_eq!(call.await.unwrap().into_response(), 7);
        assert!(matches!(
            outgoing.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        drop(inner);
    }

    #[test]
    fn dropped_pending_call_sends_cancel() {
        let (call, mut outgoing) = pending_call();
        drop(call);
        assert!(matches!(
            outgoing.try_recv(),
            Ok(Outgoing::Cancel { id: 0 })
        ));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn complete_err_clears_handle_escrow() {
        let (outgoing, _outgoing_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            outgoing: Mutex::new(Some(outgoing.clone())),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            tasks: Mutex::new(None),
            handle_escrow: Mutex::new(HashMap::new()),
            #[cfg(target_os = "macos")]
            fd_escrow: Mutex::new(Default::default()),
            session: Session::new(Box::new(outgoing.downgrade())),
            trailer_session: Arc::new(SessionWindow::new(Limits::default().trailer_session_window)),
            limits: Limits::default(),
            #[cfg(windows)]
            _peer_process: None,
        });
        let (tx, _rx) = oneshot::channel();
        inner.pending.lock().unwrap().insert(0, tx);
        inner.handle_escrow.lock().unwrap().insert(0, Vec::new());

        let (dummy_write, _unused) = tokio::io::duplex(64);
        let (sender, _unused) = transport::generic_duplex(dummy_write);
        let (_unused_tx, outgoing_rx) = mpsc::unbounded_channel();
        let writer = Writer::<Test> {
            transport: transport::AnySender::Generic(sender),
            outgoing: outgoing_rx,
            inner: Arc::downgrade(&inner),
            limits: Limits::default(),
        };

        writer.complete_err(0, Error::Cancelled);

        assert!(inner.handle_escrow.lock().unwrap().is_empty());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::os::windows::io::FromRawHandle;

    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    use super::*;

    fn current_process_handle() -> OwnedHandle {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                GetCurrentProcessId(),
            )
        };
        assert!(!handle.is_null());
        unsafe { OwnedHandle::from_raw_handle(handle as _) }
    }

    #[test]
    fn validates_named_pipe_peer_process() {
        let process = current_process_handle();
        let pid = unsafe { GetCurrentProcessId() };
        validate_peer_process(&process, pid).unwrap();
        assert_eq!(
            validate_peer_process(&process, !pid).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
