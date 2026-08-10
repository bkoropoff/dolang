//! Streaming RPC trailer bodies and their lifetime-erased transport leases.

use std::{
    io::{self, IoSlice},
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll, Waker},
};

use bytes::{Buf, BufMut, BytesMut, buf::UninitSlice};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::{
    Limits,
    fragment::Kind,
    fragment::{Flags, FragmentHeader},
    transport::{AnyRecv, AnySend, RecvFrame, SendFrame},
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn register_waker(slot: &mut Option<Waker>, waker: &Waker) {
    if !slot
        .as_ref()
        .is_some_and(|current| current.will_wake(waker))
    {
        *slot = Some(waker.clone());
    }
}

fn wake(waker: Option<Waker>) {
    if let Some(waker) = waker {
        waker.wake();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SendAction {
    Fragment,
    Finish,
    Abort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SendState {
    /// Nothing staged, no fragment in flight, no token requested.
    Idle,
    /// The writer wants to send a fragment and has asked to be scheduled,
    /// but no token has been granted yet.
    Demand,
    /// A token was just granted in response to `Demand`. `wait_fragment` is
    /// giving the writer one grace period to show up and use the zero-copy
    /// fast path directly against the token.
    Granted,
    /// The grace period following `Granted` expired without the writer
    /// reappearing. The token is still held; the *next* `poll_write` stages
    /// directly into `buffer` instead of attempting zero-copy, and wakes
    /// the driver itself — no further grant/round-trip is needed.
    Staging,
    /// `buffer` holds a fragment (header + payload, or whatever's left of
    /// one) ready to flush.
    Fragment,
    /// `buffer` still holds an unflushed fragment *and* the writer has
    /// more data ready to stage as soon as it drains.
    FragmentDemand,
    Finish,
    FragmentFinish,
    /// A clean, local abort (peer discard, cancellation, or the producer
    /// dropping `TrailerSend` without finishing): the driver observes this
    /// as an ordinary `SendAction::Abort` (wire `ABORT` fragment, no
    /// connection-level failure), while the producer observes it as
    /// `SendShared::error` on its next `poll_write`/`poll_flush`. `state`
    /// is authoritative for whether `error` is set — always paired with it,
    /// never checked independently.
    Abort,
    /// A genuine I/O failure (mid-flush or mid zero-copy write). Both the
    /// producer and the driver observe this as `Err` (via `error`, same
    /// pairing as `Abort`); for the driver it propagates out of
    /// `wait_fragment` as connection-fatal.
    Failed,
}

pub(crate) struct SendShared {
    token: Option<AnySend<'static>>,
    kind: Kind,
    id: u64,
    max_fragment_size: usize,
    copy_threshold: usize,
    /// Configured cap on the total bytes this trailer may carry (see
    /// `crate::Limits::max_trailer_size`). Fixed for the lifetime of this
    /// `SendShared` — unlike `max_fragment_size`, it isn't reset per grant.
    max_trailer_size: usize,
    /// Total bytes committed to fragments so far (staged or written
    /// zero-copy), regardless of whether they've actually reached the wire
    /// yet. Checked against `max_trailer_size` on every `poll_write`.
    written: usize,
    /// Unsent suffix of a committed fragment. While `poll_write` holds the
    /// mutex, this temporarily contains the header before it is committed.
    buffer: BytesMut,
    state: SendState,
    /// Set exactly when `state` is `Abort` or `Failed`, cleared never
    /// (states never revert). Only ever read once `state` has already
    /// established one of those two, so it doesn't need its own
    /// `is_some()`/`is_none()` check anywhere.
    error: Option<(io::ErrorKind, String)>,
    writer_waker: Option<Waker>,
    driver_waker: Option<Waker>,
}

impl SendShared {
    pub(crate) fn new(kind: Kind, id: u64, limits: &Limits) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            token: None,
            kind,
            id,
            max_fragment_size: limits.max_fragment_size,
            copy_threshold: limits.trailer_send_copy_threshold,
            max_trailer_size: limits.max_trailer_size,
            written: 0,
            buffer: BytesMut::new(),
            state: SendState::Idle,
            error: None,
            writer_waker: None,
            driver_waker: None,
        }))
    }

    pub(crate) fn poll_action(shared: &Mutex<Self>, cx: &mut Context<'_>) -> Poll<SendAction> {
        let mut inner = lock(shared);
        inner.driver_waker.take();
        match inner.state {
            SendState::Demand
            | SendState::Fragment
            | SendState::FragmentDemand
            | SendState::FragmentFinish => Poll::Ready(SendAction::Fragment),
            SendState::Finish => Poll::Ready(SendAction::Finish),
            SendState::Abort => Poll::Ready(SendAction::Abort),
            SendState::Idle | SendState::Granted | SendState::Staging => {
                register_waker(&mut inner.driver_waker, cx.waker());
                Poll::Pending
            }
            SendState::Failed => {
                // Defensive: `Failed` is only ever set while a lease from
                // `grant` is live (mid-flush, or by the producer's own
                // zero-copy write), and by the time it's live this
                // `ActiveSend` has already been popped out of the
                // scheduler's queue — so `poll_action` should never
                // observe it. Wait rather than treat it as unreachable.
                register_waker(&mut inner.driver_waker, cx.waker());
                Poll::Pending
            }
        }
    }

    /// Installs a frame token whose real borrow is retained by the returned
    /// lease. The token is only accessed while `inner` is locked.
    ///
    /// A fresh grant with nothing already staged (`buffer` empty — the
    /// ordinary case, `state` is `Demand`) starts the zero-copy grace period
    /// (`Granted`). A grant that arrives with `buffer` already non-empty
    /// (`Fragment`/`FragmentDemand`, data staged from an earlier lease)
    /// leaves `state` alone — there is nothing to wait for, `wait_fragment`
    /// should drain it immediately.
    pub(crate) unsafe fn grant<'a>(
        shared: &Arc<Mutex<Self>>,
        token: AnySend<'a>,
        max_fragment_size: usize,
    ) -> SendLease<'a> {
        // SAFETY: `SendLease` retains the source mutable borrow and clears the
        // token under the same mutex before that borrow ends.
        let token = unsafe { std::mem::transmute::<AnySend<'a>, AnySend<'static>>(token) };
        let mut inner = lock(shared);
        assert!(inner.token.is_none());
        if inner.buffer.is_empty() {
            inner.state = SendState::Granted;
        }
        inner.token = Some(token);
        inner.max_fragment_size = max_fragment_size;
        let writer = inner.writer_waker.take();
        drop(inner);
        wake(writer);
        SendLease {
            shared: shared.clone(),
            armed: true,
            _borrow: PhantomData,
        }
    }

    /// Waits for the next fragment/finish/abort decision, draining any
    /// bytes the writer couldn't hand the transport synchronously.
    ///
    /// While `Granted`, gives the writer one cooperative scheduling turn to
    /// show up and use the zero-copy fast path; if it doesn't, flips the
    /// state to `Staging` (still holding the token) and keeps waiting — the
    /// next `poll_write` will stage into `buffer` and wake this same wait
    /// directly, with no further grant needed.
    ///
    /// Returns, alongside the action, whether the fragment that just
    /// completed needed this draining at all (`false`) as opposed to having
    /// been written entirely within the writer's own `poll_write` call
    /// (`true`) — the same short-write signal `SendFrame::finish` reports,
    /// used by the scheduler to adapt fragment sizing.
    pub(crate) async fn wait_fragment(shared: &Mutex<Self>) -> io::Result<(SendAction, bool)> {
        let mut needed_drain = false;
        let mut yielded = false;
        loop {
            let outcome = std::future::poll_fn(|cx| {
                let mut inner = lock(shared);
                inner.driver_waker.take();
                if inner.state == SendState::Failed {
                    let (kind, message) = inner.error.clone().expect("error set for Failed");
                    return Poll::Ready(Err(io::Error::new(kind, message)));
                }
                if !inner.buffer.is_empty() {
                    needed_drain = true;
                    let result = poll_flush_buffer(&mut inner, cx);
                    match result {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(error)) => {
                            inner.state = SendState::Failed;
                            inner.error = Some((error.kind(), error.to_string()));
                            let writer = inner.writer_waker.take();
                            drop(inner);
                            wake(writer);
                            return Poll::Ready(Err(error));
                        }
                        Poll::Ready(Ok(())) => {}
                    }
                }
                let atomic = !needed_drain;
                match inner.state {
                    SendState::Fragment | SendState::FragmentDemand | SendState::FragmentFinish => {
                        Poll::Ready(Ok(Some((SendAction::Fragment, atomic))))
                    }
                    SendState::Finish => Poll::Ready(Ok(Some((SendAction::Finish, atomic)))),
                    SendState::Abort => Poll::Ready(Ok(Some((SendAction::Abort, atomic)))),
                    SendState::Granted if !yielded => {
                        // One cooperative scheduling turn for the writer to
                        // show up before we fall back to staging.
                        Poll::Ready(Ok(None))
                    }
                    SendState::Granted => {
                        inner.state = SendState::Staging;
                        register_waker(&mut inner.driver_waker, cx.waker());
                        Poll::Pending
                    }
                    SendState::Idle | SendState::Demand | SendState::Staging => {
                        // Defensive: none of these should be observable
                        // here (this function only runs while a lease from
                        // `grant` is live), but wait rather than treat it
                        // as unreachable.
                        register_waker(&mut inner.driver_waker, cx.waker());
                        Poll::Pending
                    }
                    SendState::Failed => unreachable!("handled above"),
                }
            })
            .await?;
            match outcome {
                Some(result) => return Ok(result),
                None => {
                    yielded = true;
                    tokio::task::yield_now().await;
                }
            }
        }
    }

    fn poll_flush(shared: &Mutex<Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut inner = lock(shared);
        inner.writer_waker.take();
        match inner.state {
            SendState::Abort | SendState::Failed => {
                let (kind, message) = inner.error.clone().expect("error set for Abort/Failed");
                Poll::Ready(Err(io::Error::new(kind, message)))
            }
            _ if !inner.buffer.is_empty() => {
                register_waker(&mut inner.writer_waker, cx.waker());
                Poll::Pending
            }
            _ => Poll::Ready(Ok(())),
        }
    }

    fn finish(shared: &Mutex<Self>) {
        let mut inner = lock(shared);
        inner.state = match inner.state {
            SendState::Fragment | SendState::FragmentDemand => SendState::FragmentFinish,
            SendState::FragmentFinish => SendState::FragmentFinish,
            // Preserve an existing abort/failure verbatim rather than
            // silently discarding it — `poll_write`/`poll_flush` must keep
            // reporting the original error even if `finish` is (unusually)
            // still called afterward.
            aborted @ (SendState::Abort | SendState::Failed) => aborted,
            _ => SendState::Finish,
        };
        let driver = inner.driver_waker.take();
        let writer = inner.writer_waker.take();
        drop(inner);
        wake(driver);
        wake(writer);
    }

    /// The writer dropped its `TrailerSend` without finishing.
    fn abandon(shared: &Mutex<Self>) {
        Self::set_aborted(shared, io::ErrorKind::BrokenPipe, "trailer is closed");
    }

    /// Cuts a still-open trailer send short from the scheduler's side:
    /// records an error so the *live* `TrailerSend`'s writer observes a
    /// clean failure on its next write instead of hanging, waiting for a
    /// lease that will never come again. Used both for genuine cancellation
    /// and for a peer-issued `Discard` notice — the two differ only in
    /// whether the surrounding message as a whole is still considered
    /// valid, which is a concern for the caller, not for this shared state.
    /// Never observed by `wait_fragment`: both call sites remove or replace
    /// the `ActiveSend`'s trailer before the scheduler could poll this
    /// `SendShared` again.
    pub(crate) fn discard(shared: &Mutex<Self>) {
        Self::set_aborted(
            shared,
            io::ErrorKind::BrokenPipe,
            "trailer discarded by peer",
        );
    }

    fn set_aborted(shared: &Mutex<Self>, kind: io::ErrorKind, message: &str) {
        let mut inner = lock(shared);
        if !matches!(
            inner.state,
            SendState::Finish | SendState::FragmentFinish | SendState::Failed
        ) {
            inner.state = SendState::Abort;
            inner.error = Some((kind, message.into()));
        }
        let driver = inner.driver_waker.take();
        let writer = inner.writer_waker.take();
        drop(inner);
        wake(driver);
        wake(writer);
    }
}

/// If the producer has already written `max_trailer_size` bytes, aborts the
/// send (same as a peer discard or cancellation — the driver observes an
/// ordinary wire `ABORT`, not a connection-fatal error) and returns the
/// error the caller should hand back from `poll_write`.
///
/// A well-behaved producer that respects this never causes the receiver's
/// own (connection-fatal) `max_trailer_size` check to trip — that check
/// only exists as a backstop for an asymmetrically configured or
/// misbehaving peer.
fn reject_if_trailer_size_exceeded(shared: &mut SendShared) -> Option<io::Error> {
    if shared.written < shared.max_trailer_size {
        return None;
    }
    let error = io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "trailer exceeds the maximum size of {} bytes",
            shared.max_trailer_size
        ),
    );
    shared.state = SendState::Abort;
    shared.error = Some((error.kind(), error.to_string()));
    let driver = shared.driver_waker.take();
    wake(driver);
    Some(error)
}

fn poll_flush_buffer(shared: &mut SendShared, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
    let Some(token) = shared.token.as_mut() else {
        return Poll::Ready(Err(io::Error::other("send lease has no frame token")));
    };
    loop {
        if shared.buffer.is_empty() {
            break Poll::Ready(Ok(()));
        }
        match token.poll_write_once(cx, &shared.buffer) {
            Poll::Ready(Ok(0)) => break Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
            Poll::Ready(Ok(n)) => shared.buffer.advance(n),
            Poll::Ready(Err(error)) => break Poll::Ready(Err(error)),
            Poll::Pending => break Poll::Pending,
        }
    }
}

pub(crate) struct SendLease<'a> {
    shared: Arc<Mutex<SendShared>>,
    armed: bool,
    _borrow: PhantomData<&'a mut ()>,
}

impl SendLease<'_> {
    pub(crate) fn complete(mut self) {
        let mut shared = lock(&self.shared);
        shared.token.take();
        shared.buffer.clear();
        shared.state = match shared.state {
            SendState::Fragment | SendState::FragmentDemand => SendState::Idle,
            SendState::FragmentFinish => SendState::Finish,
            state => state,
        };
        let writer = shared.writer_waker.take();
        self.armed = false;
        drop(shared);
        wake(writer);
    }
}

impl Drop for SendLease<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut shared = lock(&self.shared);
        shared.token.take();
        shared.buffer = BytesMut::new();
        // Preserve an existing `Failed` (e.g. `wait_fragment` already
        // observed a real I/O failure and this lease is being dropped
        // uncompleted as a result) rather than downgrading it to a generic
        // revocation message.
        if shared.state != SendState::Failed {
            shared.state = SendState::Abort;
            if shared.error.is_none() {
                shared.error = Some((
                    io::ErrorKind::ConnectionAborted,
                    "send grant was revoked".into(),
                ));
            }
        }
        let writer = shared.writer_waker.take();
        shared.driver_waker.take();
        drop(shared);
        wake(writer);
    }
}

/// A streaming request or response trailer.
///
/// This type implements [`AsyncWrite`]. Call
/// [`finish`](Self::finish), or asynchronously shut down the writer, to
/// commit the trailer; dropping it first aborts the trailer. `finish` returns
/// the value wrapped by the operation that created it, such as a
/// [`Call`](crate::client::Call).
pub struct TrailerSend<T> {
    shared: Arc<Mutex<SendShared>>,
    completion: Option<T>,
}

impl<T> TrailerSend<T> {
    pub(crate) fn new(shared: Arc<Mutex<SendShared>>, completion: T) -> Self {
        Self {
            shared,
            completion: Some(completion),
        }
    }

    /// Commits the trailer and returns the operation completed by it.
    ///
    /// This does not wait for buffered trailer bytes to reach the peer. Use
    /// [`AsyncWriteExt::shutdown`](tokio::io::AsyncWriteExt::shutdown) first
    /// when that ordering matters to the caller.
    pub fn finish(mut self) -> T {
        SendShared::finish(&self.shared);
        self.completion.take().unwrap()
    }
}

impl<T: Unpin> AsyncWrite for TrailerSend<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let this = self.get_mut();
        let mut inner = lock(&this.shared);
        inner.writer_waker.take();
        match inner.state {
            SendState::Finish | SendState::FragmentFinish => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "trailer is closed",
            ))),
            SendState::Abort | SendState::Failed => {
                let (kind, message) = inner.error.clone().expect("error set for Abort/Failed");
                Poll::Ready(Err(io::Error::new(kind, message)))
            }
            SendState::Fragment | SendState::FragmentDemand => {
                // A previously staged fragment hasn't been fully flushed
                // yet — wait for the driver to drain it before staging (or
                // writing) more. This is real backpressure (bounded to one
                // fragment's worth of staged data), not a wait for the
                // driver to win a scheduling race, so it's fine to block.
                inner.state = SendState::FragmentDemand;
                register_waker(&mut inner.writer_waker, cx.waker());
                let driver = inner.driver_waker.take();
                drop(inner);
                wake(driver);
                Poll::Pending
            }
            SendState::Idle => {
                if let Some(error) = reject_if_trailer_size_exceeded(&mut inner) {
                    return Poll::Ready(Err(error));
                }
                let len = buf
                    .len()
                    .min(inner.max_fragment_size.max(1))
                    .min(inner.max_trailer_size - inner.written);
                if len <= inner.copy_threshold {
                    FragmentHeader {
                        flags: Flags::TRAILER,
                        kind: inner.kind,
                        id: inner.id,
                        payload_len: len,
                    }
                    .encode_into(&mut inner.buffer);
                    inner.buffer.extend_from_slice(&buf[..len]);
                    inner.written += len;
                    inner.state = SendState::Fragment;
                    let driver = inner.driver_waker.take();
                    drop(inner);
                    wake(driver);
                    return Poll::Ready(Ok(len));
                }
                // A large write asks to be granted a token for direct I/O.
                inner.state = SendState::Demand;
                register_waker(&mut inner.writer_waker, cx.waker());
                let driver = inner.driver_waker.take();
                drop(inner);
                wake(driver);
                Poll::Pending
            }
            SendState::Demand => {
                register_waker(&mut inner.writer_waker, cx.waker());
                Poll::Pending
            }
            SendState::Staging => {
                if let Some(error) = reject_if_trailer_size_exceeded(&mut inner) {
                    return Poll::Ready(Err(error));
                }
                // The grace period for zero-copy already expired for this
                // grant: stage directly and wake the driver, which is
                // already waiting for exactly this.
                let len = buf
                    .len()
                    .min(inner.max_fragment_size.max(1))
                    .min(inner.max_trailer_size - inner.written);
                FragmentHeader {
                    flags: Flags::TRAILER,
                    kind: inner.kind,
                    id: inner.id,
                    payload_len: len,
                }
                .encode_into(&mut inner.buffer);
                inner.buffer.extend_from_slice(&buf[..len]);
                inner.written += len;
                inner.state = SendState::Fragment;
                let driver = inner.driver_waker.take();
                drop(inner);
                wake(driver);
                Poll::Ready(Ok(len))
            }
            SendState::Granted => {
                if let Some(error) = reject_if_trailer_size_exceeded(&mut inner) {
                    return Poll::Ready(Err(error));
                }
                // Zero-copy fast path: a token is granted and waiting on
                // us, so try writing directly instead of staging.
                let len = buf
                    .len()
                    .min(inner.max_fragment_size.max(1))
                    .min(inner.max_trailer_size - inner.written);
                FragmentHeader {
                    flags: Flags::TRAILER,
                    kind: inner.kind,
                    id: inner.id,
                    payload_len: len,
                }
                .encode_into(&mut inner.buffer);

                let header_len = inner.buffer.len();
                let write_result = {
                    let shared = &mut *inner;
                    let bufs = [IoSlice::new(&shared.buffer), IoSlice::new(&buf[..len])];
                    shared
                        .token
                        .as_mut()
                        .expect("installed send token")
                        .poll_write_vectored_once(cx, &bufs)
                };
                match write_result {
                    Poll::Ready(Ok(0)) => {
                        let error = io::Error::from(io::ErrorKind::WriteZero);
                        inner.buffer.clear();
                        inner.state = SendState::Failed;
                        inner.error = Some((error.kind(), error.to_string()));
                        let driver = inner.driver_waker.take();
                        drop(inner);
                        wake(driver);
                        Poll::Ready(Err(error))
                    }
                    Poll::Ready(Ok(n)) => {
                        debug_assert!(n <= header_len + len);
                        if n < header_len {
                            inner.buffer.advance(n);
                            inner.buffer.extend_from_slice(&buf[..len]);
                        } else {
                            inner.buffer.clear();
                            inner.buffer.extend_from_slice(&buf[n - header_len..len]);
                        }
                        inner.written += len;
                        inner.state = SendState::Fragment;
                        let driver = inner.driver_waker.take();
                        drop(inner);
                        wake(driver);
                        Poll::Ready(Ok(len))
                    }
                    Poll::Ready(Err(error)) => {
                        inner.buffer.clear();
                        inner.state = SendState::Failed;
                        inner.error = Some((error.kind(), error.to_string()));
                        let driver = inner.driver_waker.take();
                        drop(inner);
                        wake(driver);
                        Poll::Ready(Err(error))
                    }
                    Poll::Pending => {
                        // No header bytes were committed, so this write
                        // remains entirely the caller's and can be retried
                        // with a new buffer.
                        inner.buffer.clear();
                        Poll::Pending
                    }
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        SendShared::poll_flush(&self.get_mut().shared, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        SendShared::finish(&this.shared);
        SendShared::poll_flush(&this.shared, cx)
    }
}

impl<T> Drop for TrailerSend<T> {
    fn drop(&mut self) {
        if self.completion.is_some() {
            SendShared::abandon(&self.shared);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecvState {
    Idle,
    /// The consumer polled while no fragment was granted and is waiting for
    /// the next fragment specifically.
    Demand,
    /// A fragment is granted and a zero-copy read returned `Pending`, so the
    /// transport registered the consumer's waker. `wait_fragment` trusts
    /// that registration instead of taking over draining itself.
    Reading,
    /// A fragment is granted, but nothing yet guarantees the consumer will
    /// be polled again — either it hasn't asked for this fragment at all
    /// yet, or an earlier read happened to exactly satisfy the previous
    /// fragment and it never came back for this one. `wait_fragment` gives
    /// this state exactly one cooperative scheduling turn to resolve into
    /// `Reading` on its own before falling back to `Draining`.
    Unclaimed,
    /// The driver has taken over pulling the remainder of the fragment off
    /// the wire into `stage` — reached either from `Unclaimed` (grace
    /// turn passed uneventfully) or directly from `Reading`/`Unclaimed`
    /// when a consumer's own zero-copy read came up short and didn't ask
    /// for more. The consumer now only reads from `stage`.
    Draining,
    Fragment,
    /// The current fragment is complete, but its lease has not yet been
    /// released and the consumer has already polled for the next fragment.
    FragmentDemand,
    Eof,
    Discard,
    /// A read failed, or the grant/connection was aborted/revoked. `error`
    /// holds the `io::Error` to report; `state` is authoritative for
    /// whether it's set, never checked independently.
    Failed,
}

pub(crate) struct RecvShared {
    token: Option<AnyRecv<'static>>,
    remaining: usize,
    /// Bytes the driver pulled off the wire on the consumer's behalf while
    /// in `RecvState::Draining` (or discarded on the wire but not yet
    /// consumed via `RecvState::Discard`). Always drained to the consumer
    /// before anything else — see `TrailerRecv::poll_read`.
    stage: BytesMut,
    copy_threshold: usize,
    demand_copy_threshold: usize,
    state: RecvState,
    /// Set exactly when `state` is `Failed`, cleared never.
    error: Option<(io::ErrorKind, String)>,
    reader_waker: Option<Waker>,
    driver_waker: Option<Waker>,
}

impl RecvShared {
    pub(crate) fn new(copy_threshold: usize, demand_copy_threshold: usize) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            token: None,
            remaining: 0,
            stage: BytesMut::new(),
            copy_threshold,
            demand_copy_threshold,
            state: RecvState::Idle,
            error: None,
            reader_waker: None,
            driver_waker: None,
        }))
    }

    /// Installs a fresh fragment and selects copying or rendezvous according
    /// to whether the consumer demanded this specific fragment.
    pub(crate) unsafe fn grant<'a>(
        shared: &Arc<Mutex<Self>>,
        token: AnyRecv<'a>,
        remaining: usize,
    ) -> RecvLease<'a> {
        // SAFETY: `RecvLease` retains the source mutable borrow and clears
        // the token under the same mutex before that borrow ends.
        let token = unsafe { std::mem::transmute::<AnyRecv<'a>, AnyRecv<'static>>(token) };
        let mut inner = lock(shared);
        assert!(inner.token.is_none());
        if inner.state != RecvState::Discard {
            let demanded = inner.state == RecvState::Demand;
            let copy_threshold = if demanded {
                inner.demand_copy_threshold
            } else {
                inner.copy_threshold
            };
            inner.state = if remaining == 0 {
                if demanded {
                    RecvState::FragmentDemand
                } else {
                    RecvState::Fragment
                }
            } else if remaining <= copy_threshold {
                RecvState::Draining
            } else {
                RecvState::Unclaimed
            };
        }
        inner.token = Some(token);
        inner.remaining = remaining;
        let reader = if inner.state == RecvState::Unclaimed {
            inner.reader_waker.take()
        } else {
            None
        };
        drop(inner);
        wake(reader);
        RecvLease {
            shared: shared.clone(),
            armed: true,
            _borrow: PhantomData,
        }
    }

    /// Waits for the current fragment to be fully off the wire, driving the
    /// actual transport reads itself whenever the consumer isn't (state
    /// `Draining` or `Discard`) — see `TrailerRecv::poll_read` for how a
    /// consumer hands off to this. This is what guarantees forward
    /// progress independent of whether (or how promptly) the consumer
    /// polls: this function is only ever driven by the connection's single
    /// receiver loop, which is always being polled as long as the
    /// connection is alive.
    ///
    /// Returns `Ok(true)` if the trailer was discarded.
    pub(crate) async fn wait_fragment(shared: &Mutex<Self>) -> io::Result<bool> {
        // Persists across multiple polls of the single `poll_fn` future
        // below (for as long as this `wait_fragment` call remains
        // unresolved), same as a struct field would, but scoped to this
        // one grant with no separate reset needed.
        let mut grace_given = false;
        std::future::poll_fn(|cx| {
            let mut inner = lock(shared);
            inner.driver_waker.take();
            loop {
                match inner.state {
                    RecvState::Fragment | RecvState::FragmentDemand => {
                        return Poll::Ready(Ok(false));
                    }
                    RecvState::Discard if inner.remaining == 0 => return Poll::Ready(Ok(true)),
                    RecvState::Draining | RecvState::Discard => {}
                    RecvState::Reading => {
                        // Something already guarantees a future poll —
                        // trust it to make progress and call back.
                        register_waker(&mut inner.driver_waker, cx.waker());
                        return Poll::Pending;
                    }
                    RecvState::Unclaimed => {
                        if !grace_given {
                            // Nothing guarantees the consumer will call
                            // `poll_read` again (e.g. it hasn't asked for
                            // this fragment yet, or a buffered reader's
                            // read happened to land exactly on the previous
                            // fragment boundary). Give it one cooperative
                            // scheduling turn to show up on its own before
                            // taking over.
                            grace_given = true;
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        inner.state = RecvState::Draining;
                    }
                    RecvState::Idle | RecvState::Demand | RecvState::Eof => {
                        register_waker(&mut inner.driver_waker, cx.waker());
                        return Poll::Pending;
                    }
                    RecvState::Failed => {
                        // Defensive: `fail`/`RecvLease::Drop` only ever set
                        // this at a point where this function isn't
                        // concurrently driving the same `RecvShared` (see
                        // their doc comments), so this should be
                        // unreachable — wait rather than treat it as such.
                        register_waker(&mut inner.driver_waker, cx.waker());
                        return Poll::Pending;
                    }
                }
                let discard = inner.state == RecvState::Discard;
                let result = if discard {
                    let mut sink = [0u8; 8192];
                    let n = inner.remaining.min(sink.len());
                    let mut dest = &mut sink[..n];
                    inner
                        .token
                        .as_mut()
                        .expect("installed receive token")
                        .poll_read_once(cx, &mut dest)
                } else {
                    let remaining = inner.remaining;
                    inner.stage.reserve(remaining);
                    let RecvShared { token, stage, .. } = &mut *inner;
                    // `stage` may have more spare capacity than `remaining`
                    // left over from an earlier, larger fragment — cap the
                    // read so a transport with more already-buffered bytes
                    // available (e.g. the next fragment, already sitting in
                    // the OS receive buffer) can't overrun this fragment's
                    // boundary.
                    let mut limited = stage.limit(remaining);
                    token
                        .as_mut()
                        .expect("installed receive token")
                        .poll_read_once(cx, &mut limited)
                };
                match result {
                    Poll::Ready(Ok(0)) => {
                        return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
                    }
                    Poll::Ready(Ok(n)) => {
                        inner.remaining -= n;
                        if inner.remaining == 0 && inner.state == RecvState::Draining {
                            inner.state = RecvState::Fragment;
                        }
                        if !discard {
                            let reader = inner.reader_waker.take();
                            wake(reader);
                        }
                        // Loop back around: reassess state (may now be
                        // `Fragment`/still-zero `Discard`) or keep draining.
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => {
                        register_waker(&mut inner.driver_waker, cx.waker());
                        return Poll::Pending;
                    }
                }
            }
        })
        .await
    }

    pub(crate) fn finish(shared: &Mutex<Self>) {
        let mut inner = lock(shared);
        inner.state = RecvState::Eof;
        let reader = inner.reader_waker.take();
        drop(inner);
        wake(reader);
    }

    pub(crate) fn fail(shared: &Mutex<Self>, error: io::Error) {
        let mut inner = lock(shared);
        inner.state = RecvState::Failed;
        inner.error = Some((error.kind(), error.to_string()));
        let reader = inner.reader_waker.take();
        drop(inner);
        wake(reader);
    }

    pub(crate) fn discard(shared: &Mutex<Self>) {
        let mut inner = lock(shared);
        inner.state = RecvState::Discard;
        let driver = inner.driver_waker.take();
        drop(inner);
        wake(driver);
    }

    /// Peeks whether the local consumer has already stopped wanting this
    /// trailer's bytes, without changing anything. Used to decide, when a
    /// *subsequent* `TRAILER` fragment arrives, whether it's worth telling
    /// the peer to stop — never on the fragment that first hands the
    /// trailer to the application, since nothing has had a chance to
    /// discard it yet at that point.
    pub(crate) fn is_discarded(shared: &Mutex<Self>) -> bool {
        lock(shared).state == RecvState::Discard
    }
}

pub(crate) struct RecvLease<'a> {
    shared: Arc<Mutex<RecvShared>>,
    armed: bool,
    _borrow: PhantomData<&'a mut ()>,
}

impl RecvLease<'_> {
    pub(crate) fn complete(mut self) {
        let mut shared = lock(&self.shared);
        shared.token.take();
        shared.remaining = 0;
        shared.state = match shared.state {
            RecvState::Fragment => RecvState::Idle,
            RecvState::FragmentDemand => RecvState::Demand,
            state => state,
        };
        self.armed = false;
    }
}

impl Drop for RecvLease<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut shared = lock(&self.shared);
        shared.token.take();
        shared.remaining = 0;
        // Preserve an existing error (e.g. `fail` already ran for an
        // earlier ABORT on this message) rather than downgrading it to a
        // generic revocation message.
        shared.state = RecvState::Failed;
        if shared.error.is_none() {
            shared.error = Some((
                io::ErrorKind::ConnectionAborted,
                "receive grant was revoked".into(),
            ));
        }
        let reader = shared.reader_waker.take();
        shared.driver_waker.take();
        drop(shared);
        wake(reader);
    }
}

/// A streaming request or response trailer.
///
/// This type implements [`AsyncRead`]. End of file
/// means the peer finished the trailer.
///
/// Dropping or [`discard`](TrailerRecv::discard)ing a `TrailerRecv` before
/// reading it to completion never itself sends anything to the peer: it
/// only stops the local reader from waiting on further fragments. If the
/// peer is still (or later starts) streaming more `TRAILER` data for this
/// message, the read loop that notices it arriving unwanted is what tells
/// the peer to stop — see `notify_discard` on `StreamEvent::Trailer`. This
/// keeps the overwhelmingly common case (a consumer reads exactly what it
/// expects, then drops the handle right as the trailer naturally ends)
/// silent, since nothing more is ever going to arrive for it anyway.
pub struct TrailerRecv {
    pub(crate) shared: Arc<Mutex<RecvShared>>,
}

impl TrailerRecv {
    pub(crate) fn new(shared: Arc<Mutex<RecvShared>>) -> Self {
        Self { shared }
    }

    /// Stops waiting for any more of this trailer's bytes. Idempotent, and
    /// safe to call even if the trailer has already finished.
    pub fn discard(&mut self) {
        RecvShared::discard(&self.shared);
    }
}

impl AsyncRead for TrailerRecv {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let mut inner = lock(&this.shared);
        inner.reader_waker.take();
        // Bytes the driver already pulled off the wire on our behalf (see
        // `RecvState::Draining` below) always go out first, ahead of even a
        // recorded error or EOF — they were legitimately received and the
        // error/EOF is only discovered on the poll after they run out.
        //
        // Unlike `token`/`remaining`, whether `stage` has a backlog isn't
        // implied by `state`: the driver deliberately doesn't wait for us
        // to drain it before completing this lease and granting the next
        // fragment (that's the whole point of `Draining` — it lets the
        // driver keep pipelining ahead of a slow or absent reader), so
        // `state` can already describe a *later* fragment's grant while
        // `stage` still holds an *earlier* fragment's undelivered tail.
        // These are two genuinely independent facts, so `stage` has to be
        // checked directly rather than folded into `state`.
        if !inner.stage.is_empty() {
            let n = buf.remaining().min(inner.stage.len());
            buf.put_slice(&inner.stage[..n]);
            let _ = inner.stage.split_to(n);
            return Poll::Ready(Ok(()));
        }
        match inner.state {
            RecvState::Failed => {
                let (kind, message) = inner.error.clone().expect("error set for Failed");
                Poll::Ready(Err(io::Error::new(kind, message)))
            }
            RecvState::Eof => Poll::Ready(Ok(())),
            RecvState::Idle => {
                inner.state = RecvState::Demand;
                register_waker(&mut inner.reader_waker, cx.waker());
                Poll::Pending
            }
            RecvState::Fragment => {
                inner.state = RecvState::FragmentDemand;
                register_waker(&mut inner.reader_waker, cx.waker());
                Poll::Pending
            }
            RecvState::Demand | RecvState::FragmentDemand => {
                register_waker(&mut inner.reader_waker, cx.waker());
                Poll::Pending
            }
            RecvState::Draining | RecvState::Discard => {
                // The driver owns pulling bytes off the wire, so `stage`
                // (checked above) is the only thing we can serve from until
                // it is refilled or the fragment/trailer completes.
                register_waker(&mut inner.reader_waker, cx.waker());
                Poll::Pending
            }
            RecvState::Reading | RecvState::Unclaimed => {
                // Zero-copy path: read directly into the caller's buffer.
                // Both states guarantee a live token and `remaining > 0`
                // (set together by `grant`).
                let before = buf.filled().len();
                let mut adapter = ReadBufMut(buf);
                let mut limited = (&mut adapter).limit(inner.remaining);
                let result = inner
                    .token
                    .as_mut()
                    .expect("installed receive token")
                    .poll_read_once(cx, &mut limited);
                match result {
                    Poll::Ready(Ok(0)) => Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into())),
                    Poll::Ready(Ok(n)) => {
                        inner.remaining -= n;
                        if inner.remaining == 0 {
                            inner.state = RecvState::Fragment;
                        } else {
                            // A short read here says nothing about whether
                            // the consumer intends to ask for more soon — a
                            // buffered reader upstream (e.g. the zip
                            // crate's `BufReader`) routinely over-requests
                            // for read-ahead, then goes quiet once its
                            // immediate caller is satisfied, potentially
                            // forever. Hand the token off to the driver
                            // (`wait_fragment`), which is always being
                            // polled independent of this consumer and can
                            // therefore be relied on to finish draining the
                            // fragment into `stage` regardless. Leaving the
                            // remainder on the wire would instead stall the
                            // connection's single sequential reader — which
                            // must fully drain this fragment before it can
                            // read the *next* fragment, for any message —
                            // on a consumer poll that may never come,
                            // wedging every other in-flight call on the
                            // connection.
                            inner.state = RecvState::Draining;
                        }
                        let driver = inner.driver_waker.take();
                        drop(inner);
                        wake(driver);
                        debug_assert_eq!(buf.filled().len() - before, n);
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                    Poll::Pending => {
                        // The transport registered its own readiness waker
                        // for this task, so the consumer will be polled
                        // again once more data arrives — `wait_fragment`
                        // can trust that.
                        inner.state = RecvState::Reading;
                        Poll::Pending
                    }
                }
            }
        }
    }
}

impl Drop for TrailerRecv {
    fn drop(&mut self) {
        RecvShared::discard(&self.shared);
    }
}

struct ReadBufMut<'a, 'b>(&'a mut ReadBuf<'b>);

unsafe impl BufMut for ReadBufMut<'_, '_> {
    fn remaining_mut(&self) -> usize {
        self.0.remaining()
    }

    unsafe fn advance_mut(&mut self, cnt: usize) {
        // SAFETY: delegated to the caller of this unsafe method.
        unsafe { self.0.assume_init(cnt) };
        self.0.advance(cnt);
    }

    fn chunk_mut(&mut self) -> &mut UninitSlice {
        // SAFETY: `BufMut` exposes this region only as uninitialized storage.
        let unfilled = unsafe { self.0.unfilled_mut() };
        // SAFETY: `UninitSlice` has the same representation as a slice of
        // `MaybeUninit<u8>` and cannot initialize beyond this region.
        unsafe { UninitSlice::from_raw_parts_mut(unfilled.as_mut_ptr().cast(), unfilled.len()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{AnyReceiver, AnySender, Receiver, Sender, generic};

    fn poll_read_once(trailer: &mut TrailerRecv, output: &mut [u8]) -> Poll<io::Result<usize>> {
        let mut read = ReadBuf::new(output);
        let mut cx = Context::from_waker(Waker::noop());
        match Pin::new(trailer).poll_read(&mut cx, &mut read) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(read.filled().len())),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }

    struct CappedSink {
        bytes: Arc<Mutex<Vec<u8>>>,
        max_write: usize,
    }

    impl AsyncWrite for CappedSink {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let len = buf.len().min(self.max_write);
            lock(&self.bytes).extend_from_slice(&buf[..len]);
            Poll::Ready(Ok(len))
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            let mut remaining = self.max_write;
            let mut written = 0;
            let mut output = lock(&self.bytes);
            for buf in bufs {
                let len = buf.len().min(remaining);
                output.extend_from_slice(&buf[..len]);
                written += len;
                remaining -= len;
                if remaining == 0 {
                    break;
                }
            }
            Poll::Ready(Ok(written))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn small_send_stages_without_a_grant_and_large_send_demands_one() {
        let limits = Limits {
            max_fragment_size: 8,
            trailer_send_copy_threshold: 4,
            ..Limits::default()
        };

        let small_shared = SendShared::new(Kind::Request, 1, &limits);
        let mut small = TrailerSend::new(small_shared.clone(), ());
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            Pin::new(&mut small).poll_write(&mut cx, b"data"),
            Poll::Ready(Ok(4))
        ));
        let header = FragmentHeader {
            flags: Flags::TRAILER,
            kind: Kind::Request,
            id: 1,
            payload_len: 4,
        }
        .encode();
        assert_eq!(
            &small_shared.lock().unwrap().buffer[..],
            [&header[..], b"data"].concat()
        );
        assert_eq!(small_shared.lock().unwrap().state, SendState::Fragment);

        let large_shared = SendShared::new(Kind::Request, 2, &limits);
        let mut large = TrailerSend::new(large_shared.clone(), ());
        assert!(
            Pin::new(&mut large)
                .poll_write(&mut cx, b"large")
                .is_pending()
        );
        assert_eq!(large_shared.lock().unwrap().state, SendState::Demand);
        assert!(large_shared.lock().unwrap().buffer.is_empty());
    }

    #[test]
    fn receive_copy_threshold_depends_on_demand_for_this_fragment() {
        let undemanded = RecvShared::new(1, 4);
        let (_, receiver) = generic(tokio::io::empty(), tokio::io::sink());
        let mut receiver = AnyReceiver::Generic(receiver);
        let lease = unsafe { RecvShared::grant(&undemanded, receiver.recv(), 4) };
        assert_eq!(undemanded.lock().unwrap().state, RecvState::Unclaimed);
        drop(lease);

        let demanded = RecvShared::new(1, 4);
        let mut trailer = TrailerRecv::new(demanded.clone());
        let mut output = [0; 4];
        assert!(poll_read_once(&mut trailer, &mut output).is_pending());
        assert_eq!(demanded.lock().unwrap().state, RecvState::Demand);
        let (_, receiver) = generic(tokio::io::empty(), tokio::io::sink());
        let mut receiver = AnyReceiver::Generic(receiver);
        let lease = unsafe { RecvShared::grant(&demanded, receiver.recv(), 4) };
        assert_eq!(demanded.lock().unwrap().state, RecvState::Draining);
        drop(lease);
    }

    #[test]
    fn demand_at_a_completed_fragment_boundary_applies_to_the_next_fragment() {
        let shared = RecvShared::new(0, 0);
        let mut trailer = TrailerRecv::new(shared.clone());
        let (_, receiver) = generic(tokio::io::empty(), tokio::io::sink());
        let mut receiver = AnyReceiver::Generic(receiver);
        let lease = unsafe { RecvShared::grant(&shared, receiver.recv(), 0) };
        assert_eq!(shared.lock().unwrap().state, RecvState::Fragment);

        let mut output = [0; 1];
        assert!(poll_read_once(&mut trailer, &mut output).is_pending());
        assert_eq!(shared.lock().unwrap().state, RecvState::FragmentDemand);
        lease.complete();
        assert_eq!(shared.lock().unwrap().state, RecvState::Demand);
    }

    #[tokio::test]
    async fn unclaimed_large_receive_falls_back_to_driver_draining() {
        use tokio::io::AsyncWriteExt;

        let shared = RecvShared::new(0, 0);
        let (mut writer, reader) = tokio::io::duplex(16);
        writer.write_all(b"data").await.unwrap();
        let (_, receiver) = generic(reader, tokio::io::sink());
        let mut receiver = AnyReceiver::Generic(receiver);
        let lease = unsafe { RecvShared::grant(&shared, receiver.recv(), 4) };
        assert_eq!(shared.lock().unwrap().state, RecvState::Unclaimed);

        assert!(!RecvShared::wait_fragment(&shared).await.unwrap());
        assert_eq!(shared.lock().unwrap().state, RecvState::Fragment);
        assert_eq!(&shared.lock().unwrap().stage[..], b"data");
        lease.complete();
    }

    #[tokio::test]
    async fn demanded_large_receive_can_claim_the_grant_directly() {
        use tokio::io::AsyncWriteExt;

        let shared = RecvShared::new(0, 0);
        let mut trailer = TrailerRecv::new(shared.clone());
        let mut output = [0; 4];
        assert!(poll_read_once(&mut trailer, &mut output).is_pending());

        let (mut writer, reader) = tokio::io::duplex(16);
        writer.write_all(b"data").await.unwrap();
        let (_, receiver) = generic(reader, tokio::io::sink());
        let mut receiver = AnyReceiver::Generic(receiver);
        let lease = unsafe { RecvShared::grant(&shared, receiver.recv(), 4) };
        assert_eq!(shared.lock().unwrap().state, RecvState::Unclaimed);
        assert!(matches!(
            poll_read_once(&mut trailer, &mut output),
            Poll::Ready(Ok(4))
        ));
        assert_eq!(&output, b"data");
        assert!(!RecvShared::wait_fragment(&shared).await.unwrap());
        lease.complete();
        assert_eq!(shared.lock().unwrap().state, RecvState::Idle);
    }

    #[tokio::test]
    async fn abandoned_fragment_flushes_only_its_real_staged_suffix() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let (sender, _) = generic(
            tokio::io::empty(),
            CappedSink {
                bytes: output.clone(),
                max_write: 16,
            },
        );
        let mut sender = AnySender::Generic(sender);
        let shared = SendShared::new(
            Kind::Request,
            1,
            &Limits {
                max_trailer_size: usize::MAX,
                ..Limits::default()
            },
        );
        let lease = unsafe { SendShared::grant(&shared, sender.send(), 1024) };
        let data = (0..100).map(|value| value as u8).collect::<Vec<_>>();
        let mut trailer = TrailerSend::new(shared.clone(), ());

        let written = std::future::poll_fn(|cx| Pin::new(&mut trailer).poll_write(cx, &data))
            .await
            .unwrap();
        assert_eq!(written, data.len());
        {
            let inner = lock(&shared);
            let header_len = FragmentHeader {
                flags: Flags::TRAILER,
                kind: Kind::Request,
                id: 1,
                payload_len: data.len(),
            }
            .encode()
            .len();
            assert_eq!(&inner.buffer[..], &data[16 - header_len..]);
        }

        drop(trailer);
        assert_eq!(
            SendShared::wait_fragment(&shared).await.unwrap().0,
            SendAction::Abort
        );
        let header_len = FragmentHeader {
            flags: Flags::TRAILER,
            kind: Kind::Request,
            id: 1,
            payload_len: data.len(),
        }
        .encode()
        .len();
        assert_eq!(&lock(&output)[header_len..], data);
        lease.complete();
    }

    #[tokio::test]
    async fn partial_header_and_payload_share_the_stage_buffer() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let (sender, _) = generic(
            tokio::io::empty(),
            CappedSink {
                bytes: output.clone(),
                max_write: 5,
            },
        );
        let mut sender = AnySender::Generic(sender);
        let shared = SendShared::new(
            Kind::Request,
            7,
            &Limits {
                max_trailer_size: usize::MAX,
                ..Limits::default()
            },
        );
        let lease = unsafe { SendShared::grant(&shared, sender.send(), 1024) };
        let data = (0..32).map(|value| value as u8).collect::<Vec<_>>();
        let mut trailer = TrailerSend::new(shared.clone(), ());

        let written = std::future::poll_fn(|cx| Pin::new(&mut trailer).poll_write(cx, &data))
            .await
            .unwrap();
        assert_eq!(written, data.len());

        let header = FragmentHeader {
            flags: Flags::TRAILER,
            kind: Kind::Request,
            id: 7,
            payload_len: data.len(),
        }
        .encode();
        let mut expected_stage = Vec::from(&header[5..]);
        expected_stage.extend_from_slice(&data);
        assert_eq!(&lock(&shared).buffer[..], expected_stage);

        assert_eq!(
            SendShared::wait_fragment(&shared).await.unwrap().0,
            SendAction::Fragment
        );
        assert_eq!(&lock(&output)[..], [&header[..], &data].concat());
        lease.complete();
    }

    #[tokio::test]
    async fn finish_releases_an_unused_live_grant() {
        let (sender, _) = generic(tokio::io::empty(), tokio::io::sink());
        let mut sender = AnySender::Generic(sender);
        let shared = SendShared::new(
            Kind::Request,
            9,
            &Limits {
                max_trailer_size: usize::MAX,
                ..Limits::default()
            },
        );
        let lease = unsafe { SendShared::grant(&shared, sender.send(), 1024) };

        TrailerSend::new(shared.clone(), ()).finish();
        assert_eq!(
            SendShared::wait_fragment(&shared).await.unwrap().0,
            SendAction::Finish
        );
        lease.complete();
    }

    #[tokio::test]
    async fn write_past_max_trailer_size_aborts_instead_of_silently_truncating() {
        let (sender, _) = generic(tokio::io::empty(), tokio::io::sink());
        let mut sender = AnySender::Generic(sender);
        let shared = SendShared::new(
            Kind::Request,
            1,
            &Limits {
                max_trailer_size: 4,
                ..Limits::default()
            },
        );
        let lease = unsafe { SendShared::grant(&shared, sender.send(), 1024) };
        let mut trailer = TrailerSend::new(shared.clone(), ());

        // Exhaust the 4-byte budget in a single write.
        let written = std::future::poll_fn(|cx| Pin::new(&mut trailer).poll_write(cx, b"abcd"))
            .await
            .unwrap();
        assert_eq!(written, 4);
        let (action, _) = SendShared::wait_fragment(&shared).await.unwrap();
        assert_eq!(action, SendAction::Fragment);
        lease.complete();

        // A fresh grant still has nothing staged, so `poll_write` goes
        // through the zero-copy `Granted` path — which must reject
        // immediately rather than trying to write anything, since the
        // budget is already spent.
        let lease = unsafe { SendShared::grant(&shared, sender.send(), 1024) };
        let error = std::future::poll_fn(|cx| Pin::new(&mut trailer).poll_write(cx, b"e"))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);

        // The peer must observe a real `ABORT`, not a clean completion that
        // would look like the trailer just ended early (a plain EOF).
        assert_eq!(
            SendShared::wait_fragment(&shared).await.unwrap().0,
            SendAction::Abort
        );
        lease.complete();
    }
}
