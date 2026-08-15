//! Low-level async integration with Windows APCs
//!
//! Some Win32 APIs deliver their completion as a user-mode APC to whichever thread made the call,
//! and require that thread to periodically enter an alertable wait
//! (`SleepEx`/`WaitForSingleObjectEx` with `bAlertable = TRUE`). This module is deliberately
//! agnostic to any particular such API: it only provides the alertable thread, task creation, and
//! cooperative cancellation.
//!
//! # Cancellation
//!
//! Dropping the [`Task`] returned by [`Reactor::submit`] cancels the task. By default this simply
//! drops the corresponding future in the reactor thread. A task that needs to do something before
//! being torn down can call [`Context::cancel_guard`], which turns a cancellation request arriving
//! during that region into a cooperative `Err` instead of a drop, so the task's own code can run
//! async cleanup before finishing normally.

use std::{error, fmt, io};

/// A future boxed for storage on the reactor thread. Deliberately not
/// `Send`: it is only ever constructed and polled on the reactor thread
/// itself (see [`Reactor::submit`]), never transported across a thread
/// boundary — which also sidesteps `AsyncFnOnce`'s associated future type
/// not being nameable as `Send` on stable Rust.
#[cfg(all(windows, not(docsrs)))]
mod imp {
    use super::*;
    pub(super) use futures::{
        channel::oneshot,
        future::{self, Either},
        task::ArcWake,
    };
    pub(super) use std::{
        cell::RefCell,
        collections::HashMap,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        panic::{AssertUnwindSafe, catch_unwind},
        pin::Pin,
        ptr,
        sync::{
            Arc, Mutex, Weak,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        task::{self, Poll, Waker},
        thread,
    };
    pub(super) use windows_sys::Win32::{
        Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, TRUE},
        System::Threading::{GetCurrentProcess, GetCurrentThread, INFINITE, QueueUserAPC, SleepEx},
    };

    pub(super) type BoxedTask = Pin<Box<dyn Future<Output = ()>>>;

    /// Uniquely identifies a task within a single [`Reactor`]'s registry.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub(super) struct TaskId(pub(super) u64);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Closed;

    impl fmt::Display for Closed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc reactor is closed")
        }
    }

    impl error::Error for Closed {}

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TaskCanceled;

    impl fmt::Display for TaskCanceled {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc task was cancelled")
        }
    }

    impl error::Error for TaskCanceled {}

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Canceled;

    impl fmt::Display for Canceled {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc task was cancelled inside a cancel_guard")
        }
    }

    impl error::Error for Canceled {}

    /// A per-task registry slot. Only ever touched by the reactor thread.
    pub(super) struct TaskSlot {
        /// Taken out for the duration of each poll so that reentrant access to
        /// this same slot's other fields (e.g. from `cancel_guard`, which runs
        /// as part of polling the task's own future) doesn't need a reentrant
        /// `RefCell` borrow.
        pub(super) future: Option<BoxedTask>,
        pub(super) in_guard: bool,
        pub(super) guard_signal: Option<oneshot::Sender<()>>,
    }

    #[derive(Default)]
    pub(super) struct Registry {
        pub(super) tasks: HashMap<TaskId, TaskSlot>,
        /// Set by the reactor's own flush-marker APC (see `run`) to whatever
        /// `tasks`'s emptiness actually was at the moment that marker ran.
        /// This, not some value independently recomputed by the main loop, is
        /// the real exit condition: a task-insertion (or any other) APC can be
        /// durably queued without having run yet, in which case `tasks`
        /// doesn't reflect it yet and looks empty when it isn't really. The
        /// flush marker always runs strictly after anything queued before it
        /// (the OS's per-thread APC queue is FIFO), so its own check is
        /// authoritative for that instant.
        pub(super) should_exit: bool,
    }

    #[cfg(windows)]
    thread_local! {
        /// Owned by the reactor thread's loop. Cross-thread requests
        /// (submission, cancellation) always arrive as a closure posted via a
        /// real `QueueUserAPC`, which only actually runs once executing on this
        /// thread — so nothing here needs locking.
        pub(super) static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
    }

    /// Queues `f` to run on the thread identified by `handle`, via a real
    /// `QueueUserAPC`. This is the raw mechanism — no synchronization against
    /// the reactor thread's own shutdown decision. Only [`ReactorInner::drop`],
    /// [`Control::close`], and `run`'s own flush marker (see `run`) are
    /// allowed to call this directly, since they can each prove nothing else
    /// could be racing them; every other caller must go through [`post`], which
    /// guards against a "successfully" queued APC being silently discarded when
    /// the thread terminates before ever running it.
    ///
    /// Takes a raw `HANDLE` rather than `&OwnedHandle` so `run` can pass the
    /// pseudo-handle from `GetCurrentThread()` (valid only for the calling
    /// thread to refer to itself, not an owned resource to be duplicated or
    /// closed) when posting its own flush marker.
    ///
    /// # Safety
    ///
    /// `handle` must be a valid thread handle (with `THREAD_SET_CONTEXT`
    /// access) for the entire duration of this call — either a real `HANDLE`
    /// the caller keeps open (e.g. via a live `OwnedHandle` it's borrowing
    /// from), or the `GetCurrentThread()` pseudo-handle used from within the
    /// thread it refers to.
    pub(super) unsafe fn queue_apc(
        handle: HANDLE,
        f: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        unsafe extern "system" fn trampoline(param: usize) {
            // SAFETY: `param` was produced by `Box::into_raw` below, from a
            // `Box<Box<dyn FnOnce() + Send>>` that hasn't been freed yet (this
            // is the only place that ever reconstructs or frees it).
            let boxed = unsafe { Box::from_raw(param as *mut Box<dyn FnOnce() + Send>) };
            // Catch panics here: this runs across an `extern "system"`
            // boundary, where unwinding is undefined behavior. A panicking
            // closure (a bug in an injected task-insertion or cancel-dispatch
            // closure, say) shouldn't be able to bring down the whole process.
            let _ = catch_unwind(AssertUnwindSafe(move || (*boxed)()));
        }

        let boxed: Box<dyn FnOnce() + Send> = Box::new(f);
        let raw = Box::into_raw(Box::new(boxed));
        // SAFETY: `raw` is a valid, uniquely-owned pointer we just created;
        // `trampoline` reconstructs and consumes it exactly once, whenever the
        // OS actually delivers this APC.
        let ok = unsafe {
            QueueUserAPC(
                Some(trampoline as unsafe extern "system" fn(usize)),
                handle,
                raw as usize,
            )
        };
        if ok == 0 {
            // The APC will never run; reclaim the box instead of leaking it.
            drop(unsafe { Box::from_raw(raw) });
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Mutex-guarded state shared by every handle to a given reactor. Bundles
    /// the thread handle together with `closed` so that touching the handle
    /// for anything that needs to be synchronized with `closed` — which turns
    /// out to be everything (see [`post`] and `run`) — is structurally forced
    /// to go through the same lock, rather than relying on each call site to
    /// separately remember to.
    pub(super) struct ReactorState {
        pub(super) thread_handle: OwnedHandle,
        /// Set by [`Control::close`]. Once true, [`Reactor::submit`]
        /// rejects new work with [`Closed`].
        pub(super) closed: bool,
    }

    /// State shared by every handle to a given reactor ([`Reactor`] clones,
    /// [`Control`], and the [`Context`]/[`Task`] belonging to each
    /// live task). Wrapped in a single `Arc` so there's one allocation and one
    /// refcount for the whole reactor rather than three.
    pub(super) struct ReactorInner {
        pub(super) state: Mutex<ReactorState>,
        pub(super) next_id: AtomicU64,
    }

    impl Drop for ReactorInner {
        fn drop(&mut self) {
            // This only runs once every strong reference — every `Reactor`
            // clone, `Control`, and live task — is gone. [`post`] wraps
            // every closure it queues to hold its own `Arc<ReactorInner>` for
            // as long as it's queued-but-undelivered, so nothing can possibly
            // still be in flight at this point — a plain, unconditional wake is
            // enough to get the reactor thread to notice, via `Weak::upgrade`
            // failing in `run`, and exit. Unlike the explicit-`close` path, no
            // flush-and-recheck is needed here: nothing can still be racing us
            // (the only other way the reactor thread could be gone is this
            // very drop, which can't have run twice), and the handle is still
            // open at this point — only after this function returns does Rust
            // drop it (closing it) as an ordinary field.
            let guard = self.state.lock().unwrap();
            // SAFETY: `guard.thread_handle` is a live `OwnedHandle`, kept open
            // by holding `guard` (this field isn't dropped until this
            // function returns) for the duration of this call.
            let _ = unsafe { queue_apc(guard.thread_handle.as_raw_handle() as HANDLE, || {}) };
        }
    }

    /// Queues `f` to run on the reactor thread.
    ///
    /// Doesn't need to check `closed` or otherwise synchronize with the reactor
    /// thread's own shutdown decision, nor keep the reactor alive itself while
    /// queued-but-undelivered: `run`'s loop never actually stops until its own
    /// flush marker confirms the task registry is empty, and that marker is
    /// always processed strictly after anything already durably queued at the
    /// time it's posted (the OS's per-thread APC queue is FIFO) — so an APC
    /// queued while the reactor thread is still willing to accept it is
    /// guaranteed to run before the reactor exits, full stop.
    pub(super) fn post(
        inner: &Arc<ReactorInner>,
        f: impl FnOnce() + Send + 'static,
    ) -> io::Result<()> {
        let guard = inner.state.lock().unwrap();
        // SAFETY: `guard.thread_handle` is a live `OwnedHandle`, kept open by
        // holding `guard` for the duration of this call.
        unsafe { queue_apc(guard.thread_handle.as_raw_handle() as HANDLE, f) }
    }

    pub(super) fn close_reactor(inner: &ReactorInner) {
        let mut guard = inner.state.lock().unwrap();
        if guard.closed {
            return;
        }
        guard.closed = true;
        // SAFETY: `guard.thread_handle` is a live `OwnedHandle`, kept open by
        // holding `guard` for the duration of this call.
        let _ = unsafe { queue_apc(guard.thread_handle.as_raw_handle() as HANDLE, || {}) };
    }

    /// Wakes the reactor thread's alertable wait so it re-polls its task set.
    /// Shared by every task's [`Context`] — the reactor re-polls its whole
    /// registry after every wake regardless of cause, so there is no need for
    /// per-task wake identity.
    ///
    /// Holds a `Weak` reference rather than a strong one: a waker can end up
    /// cloned into and held by some external resource (e.g. a channel a task is
    /// blocked on) for longer than the task itself, and a strong reference
    /// there would keep the whole reactor alive even after every real handle to
    /// it (`Reactor`, `Control`, the task itself) is gone.
    pub(super) struct WakeSignal {
        pub(super) inner: Weak<ReactorInner>,
    }

    impl ArcWake for WakeSignal {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            // Best effort: failure, or the upgrade failing, means the reactor
            // thread has already exited (or is about to), so there is nothing
            // left to wake.
            if let Some(inner) = arc_self.inner.upgrade() {
                let _ = post(&inner, || {});
            }
        }
    }

    #[derive(Clone)]
    pub struct Reactor {
        pub(super) inner: Arc<ReactorInner>,
    }

    pub struct Control {
        pub(super) inner: Arc<ReactorInner>,
        pub(super) exit_rx: oneshot::Receiver<()>,
    }

    pub struct Join {
        pub(super) inner: Option<Weak<ReactorInner>>,
        pub(super) exit_rx: oneshot::Receiver<()>,
        pub(super) _pin: std::marker::PhantomPinned,
    }

    pub struct Context {
        pub(super) id: TaskId,
        pub(super) inner: Arc<ReactorInner>,
    }

    pub struct Task<T> {
        pub(super) id: Option<TaskId>,
        pub(super) rx: oneshot::Receiver<T>,
        pub(super) inner: Arc<ReactorInner>,
        pub(super) _pin: std::marker::PhantomPinned,
    }

    pub(super) fn run(weak_inner: Weak<ReactorInner>) {
        let waker = futures::task::waker(Arc::new(WakeSignal {
            inner: weak_inner.clone(),
        }));
        loop {
            // SAFETY: plain alertable wait; no preconditions beyond a valid
            // calling thread.
            unsafe {
                SleepEx(INFINITE, TRUE);
            }
            poll_all(&waker);

            if REGISTRY.with(|r| r.borrow().should_exit) {
                break;
            }

            // `closed` is either genuinely true (`Control::close` was
            // called), or *effectively* true because nobody could possibly
            // call it — or `Reactor::submit` — ever again: `weak_inner.upgrade`
            // failing means every `Reactor`, `Control`, and live task
            // reference is gone. Either way this alone doesn't mean it's safe
            // to stop: there could still be a live task in `registry`, or an
            // APC already durably queued but not yet reflected there. Keep
            // looping normally (the `if` below is only a cheap pre-filter, not
            // the real exit decision) until a flush marker actually confirms
            // it.
            let closed = match weak_inner.upgrade() {
                Some(inner) => inner.state.lock().unwrap().closed,
                None => true,
            };
            if closed && REGISTRY.with(|r| r.borrow().tasks.is_empty()) {
                // Posted via the `GetCurrentThread()` pseudo-handle, not
                // `ReactorInner`'s — which may already be gone in the
                // natural-quiescence case — since this must keep working
                // regardless. Its own check of `registry`, made at the moment
                // it actually runs (strictly after anything already durably
                // queued, per FIFO order — see `post`), is what's actually
                // authoritative; if something did sneak in, `should_exit`
                // simply comes out false and the loop above keeps running
                // normally until this is attempted again once things settle.
                // SAFETY: `GetCurrentThread()`'s pseudo-handle is always valid
                // for the thread it refers to, which is the one making this
                // call.
                let _ = unsafe {
                    queue_apc(GetCurrentThread(), || {
                        REGISTRY.with(|r| {
                            let mut r = r.borrow_mut();
                            r.should_exit = r.tasks.is_empty();
                        });
                    })
                };
            }
        }
    }

    fn poll_all(waker: &Waker) {
        let ids: Vec<TaskId> = REGISTRY.with(|r| r.borrow().tasks.keys().copied().collect());
        for id in ids {
            let future = REGISTRY.with(|r| {
                r.borrow_mut()
                    .tasks
                    .get_mut(&id)
                    .and_then(|slot| slot.future.take())
            });
            let Some(mut future) = future else {
                // Not present (already retired) or its future was already
                // taken by an earlier iteration of this same pass — neither
                // can happen today since nothing re-enters `poll_all`
                // mid-pass, but skip defensively rather than panic.
                continue;
            };

            let mut cx = task::Context::from_waker(waker);
            let outcome = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));

            match outcome {
                Ok(Poll::Pending) => {
                    REGISTRY.with(|r| {
                        if let Some(slot) = r.borrow_mut().tasks.get_mut(&id) {
                            slot.future = Some(future);
                        }
                        // else: the slot was removed while its future was
                        // checked out above. Can't happen today (see above),
                        // but if it did, just let `future` drop here.
                    });
                }
                Ok(Poll::Ready(())) | Err(_) => {
                    // A panic while polling this task shouldn't take down the
                    // shared reactor thread or strand other in-flight tasks —
                    // just drop this one and move on.
                    let removed = REGISTRY.with(|r| r.borrow_mut().tasks.remove(&id));
                    drop(removed);
                    drop(future);
                }
            }
        }
    }
}

#[cfg(docsrs)]
mod imp {
    use std::{error, fmt};

    struct ReceiverInner {
        _inner: std::cell::UnsafeCell<()>,
    }

    unsafe impl Sync for ReceiverInner {}

    struct ReceiverMarker {
        _inner: std::sync::Arc<ReceiverInner>,
    }

    // Mirrors the synchronization around the real oneshot receiver: it can
    // be shared across threads, but is neither `UnwindSafe` nor
    // `RefUnwindSafe`.

    /// A handle for submitting work to a reactor.
    ///
    /// Cloneable.  The reactor thread implicitly shuts down after
    /// all clones are dropped and no work remains.
    #[derive(Clone)]
    pub struct Reactor {
        _dummy: (),
    }

    /// Allows closing and awaiting exit of a [`Reactor`].
    pub struct Control {
        _marker: ReceiverMarker,
    }

    /// Future returned by [`Control::join`].
    pub struct Join {
        _marker: ReceiverMarker,
        _pin: std::marker::PhantomPinned,
    }

    /// Context handle provided to APC tasks.
    pub struct Context {
        _dummy: (),
    }

    /// A future result of a task.
    ///
    /// Dropping it before it resolves cancels the task.
    pub struct Task<T> {
        _value: std::marker::PhantomData<T>,
        _marker: ReceiverMarker,
        _pin: std::marker::PhantomPinned,
    }

    // The real oneshot receiver is `Sync` when `T: Send`; `T` need not be
    // `Sync` because access to it is synchronized internally.
    unsafe impl<T: Send> Sync for Task<T> {}

    /// Error returned by [`Reactor::submit`] when the reactor is closed.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Closed;

    impl fmt::Display for Closed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc reactor is closed")
        }
    }

    impl error::Error for Closed {}

    /// Error returned when a reactor task is cancelled before producing a value.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TaskCanceled;

    impl fmt::Display for TaskCanceled {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc task was cancelled")
        }
    }

    impl error::Error for TaskCanceled {}

    /// Error returned when cancellation reaches [`Context::cancel_guard`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Canceled;

    impl fmt::Display for Canceled {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("apc task was cancelled inside a cancel_guard")
        }
    }

    impl error::Error for Canceled {}
}

#[cfg(windows)]
use imp::*;
pub use imp::{Canceled, Closed, Context, Control, Join, Reactor, Task, TaskCanceled};

#[cfg(any(windows, docsrs))]
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send_sync_unpin<T: Send + Sync + Unpin>() {}
    fn assert_unwind_safe<T: std::panic::UnwindSafe>() {}
    fn assert_ref_unwind_safe<T: std::panic::RefUnwindSafe>() {}

    assert_send_sync_unpin::<Reactor>();
    assert_send_sync_unpin::<Control>();
    assert_send_sync::<Join>();
    assert_send_sync_unpin::<Context>();
    // `Cell<()>` is `Send` but not `Sync`. The oneshot receiver, and therefore
    // `Task<T>`, is nevertheless `Sync` when `T: Send`.
    assert_send_sync::<Task<std::cell::Cell<()>>>();

    assert_unwind_safe::<Reactor>();
    assert_unwind_safe::<Context>();
    assert_ref_unwind_safe::<Reactor>();
    assert_ref_unwind_safe::<Context>();
};

impl Reactor {
    /// Spawns the reactor thread, returning a cloneable submission handle
    /// alongside the unique handle that controls its lifecycle.
    pub async fn new() -> io::Result<(Reactor, Control)> {
        #[cfg(windows)]
        {
            let (exit_tx, exit_rx) = oneshot::channel();
            let (ready_tx, ready_rx) = oneshot::channel::<()>();
            let (handle_tx, handle_rx) = mpsc::channel::<Weak<ReactorInner>>();

            let join_handle = thread::Builder::new()
                .name("dolang-winterop-apc".into())
                .spawn(move || {
                    // Signal that we are actually executing our own code before
                    // doing anything else. A freshly created Windows thread can
                    // still be inside the OS's own thread-startup sequence
                    // (loader/CRT init) for a little while after `CreateThread`
                    // returns a valid, already-usable handle; delivering a
                    // `QueueUserAPC` to it during that window races that
                    // startup and can corrupt it. Once *any* of our own code
                    // has run, that window is guaranteed to be over, so the
                    // spawning thread waits for this signal before it (or
                    // anyone else) is allowed to post anything to us.
                    let _ = ready_tx.send(());

                    // Wait for a weak reference to the shared state, sent by
                    // the spawning thread right after this thread was created
                    // (see below — it can only be produced once the OS thread
                    // exists, since it wraps this thread's own duplicated
                    // handle). If the sender was dropped instead (spawning
                    // failed after this thread was already created), just exit
                    // without ever entering the alertable wait.
                    let Ok(weak_inner) = handle_rx.recv() else {
                        return;
                    };
                    run(weak_inner);
                    let _ = exit_tx.send(());
                })
                .map_err(io::Error::other)?;

            if ready_rx.await.is_err() {
                return Err(io::Error::other("apc reactor thread failed to start"));
            }

            // Duplicate a handle to the new thread that we own independent of
            // the `JoinHandle` — we never block-join the OS thread (`join()`
            // instead awaits `exit_rx`, signaled right before the thread's
            // closure returns), and detach it below.
            let mut dup: HANDLE = ptr::null_mut();
            // SAFETY: `join_handle.as_raw_handle()` is a valid, currently-open
            // thread handle for the thread we just spawned.
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    join_handle.as_raw_handle() as HANDLE,
                    GetCurrentProcess(),
                    &mut dup,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 {
                let err = io::Error::last_os_error();
                // Unblock the thread (it's parked on `handle_rx.recv()`) so it
                // exits immediately instead of waiting forever.
                drop(handle_tx);
                return Err(err);
            }
            // SAFETY: `dup` is a valid, uniquely-owned handle from a successful
            // `DuplicateHandle` call above.
            let thread_handle = unsafe { OwnedHandle::from_raw_handle(dup as _) };

            // Detach: dropping a `JoinHandle` without joining it just forfeits
            // the ability to block-join or observe a panic through it; the OS
            // thread keeps running independently, driven from here on by our
            // duplicated `thread_handle`.
            drop(join_handle);

            let inner = Arc::new(ReactorInner {
                state: Mutex::new(ReactorState {
                    thread_handle,
                    closed: false,
                }),
                next_id: AtomicU64::new(0),
            });

            // The receive end can only fail if the thread already exited
            // (e.g. it panicked before reaching `handle_rx.recv()`), in which
            // case there's nothing more to do — `exit_rx` will observe that on
            // its own once `Control` gets used.
            let _ = handle_tx.send(Arc::downgrade(&inner));

            Ok((
                Reactor {
                    inner: inner.clone(),
                },
                Control { inner, exit_rx },
            ))
        }
        #[cfg(all(docsrs, not(windows)))]
        unreachable!()
    }

    /// Submits `f` to run on the reactor thread, returning a future for its
    /// result.
    ///
    /// `f` receives a [`Context`] for cooperative cancellation
    /// ([`Context::cancel_guard`]) and task self-submission
    /// ([`Context::submit`]).
    ///
    /// Fails with [`Closed`] once [`Control::close`] has been called.
    pub fn submit<T, F>(&self, f: F) -> Result<Task<T>, Closed>
    where
        T: Send + 'static,
        F: AsyncFnOnce(&mut Context) -> T + Send + 'static,
    {
        #[cfg(windows)]
        {
            // `closed` is read here, separately from the `post` call below, on
            // purpose: a submission that narrowly beats `cancel` (checks
            // `closed` just before it's set) isn't a bug — its task-insertion
            // APC simply shows up during the reactor's flush-and-recheck (see
            // `run`), which correctly aborts the exit rather than dropping it.
            if self.inner.state.lock().unwrap().closed {
                return Err(Closed);
            }

            let id = TaskId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
            let (result_tx, result_rx) = oneshot::channel();

            let task_inner = self.inner.clone();
            let posted = post(&self.inner, move || {
                // Only construct (and box) the task's future once actually
                // running on the reactor thread — see `BoxedTask`'s doc comment
                // for why that matters.
                let task: BoxedTask = Box::pin(async move {
                    let mut ctx = Context {
                        id,
                        inner: task_inner,
                    };
                    let value = f(&mut ctx).await;
                    let _ = result_tx.send(value);
                });
                REGISTRY.with(|r| {
                    r.borrow_mut().tasks.insert(
                        id,
                        TaskSlot {
                            future: Some(task),
                            in_guard: false,
                            guard_signal: None,
                        },
                    );
                });
            });

            match posted {
                Ok(()) => Ok(Task {
                    id: Some(id),
                    rx: result_rx,
                    inner: self.inner.clone(),
                    _pin: std::marker::PhantomPinned,
                }),
                Err(_) => Err(Closed),
            }
        }
        #[cfg(all(docsrs, not(windows)))]
        {
            let _ = f;
            unreachable!()
        }
    }
}

impl Control {
    /// Stops accepting new [`Reactor::submit`] calls on every clone of the
    /// corresponding [`Reactor`].
    pub fn close(&self) {
        #[cfg(windows)]
        {
            // `closed` is set and the wake is posted in the *same* critical
            // section — not two separate calls — because the reactor thread's
            // own "read `closed`, and if true, start exiting" step (`run`)
            // takes the same lock. Without that, the reactor could observe a
            // stale `closed == false` via some unrelated wake, decide to keep
            // looping, and go back to sleep with nothing left to ever wake it
            // again before this call's own post — permanently hanging a
            // reactor that had nothing else going on.
            close_reactor(&self.inner);
        }
        #[cfg(all(docsrs, not(windows)))]
        unreachable!()
    }

    /// Returns a future which awaits the reactor thread's exit.
    ///
    /// The reactor thread will not exit until all work completes and no more can be submitted. This
    /// means that if [`close`](Self::close) is not called first, this function implicitly waits for
    /// every `Reactor` clone to be dropped in addition to all work completing. [`Join::close`] on
    /// the returned join handle can be used to make a late-binding decision to close the reactor.
    pub fn join(self) -> Join {
        #[cfg(windows)]
        {
            let Control { inner, exit_rx } = self;
            let weak_inner = Arc::downgrade(&inner);
            drop(inner);
            Join {
                inner: Some(weak_inner),
                exit_rx,
                _pin: std::marker::PhantomPinned,
            }
        }
        #[cfg(all(docsrs, not(windows)))]
        unreachable!()
    }
}

impl Join {
    /// Closes the reactor while continuing to await its exit.
    pub fn close(self: std::pin::Pin<&mut Self>) {
        #[cfg(windows)]
        {
            // SAFETY: `inner` is only mutated in place and is not structurally
            // pinned.
            let this = unsafe { self.get_unchecked_mut() };
            if let Some(inner) = this.inner.take().and_then(|inner| inner.upgrade()) {
                close_reactor(&inner);
            }
        }
        #[cfg(all(docsrs, not(windows)))]
        unreachable!()
    }
}

#[cfg(windows)]
impl Future for Join {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we do not move any field out of the pinned join future, and
        // `exit_rx` is `Unpin`.
        let this = unsafe { self.get_unchecked_mut() };
        Pin::new(&mut this.exit_rx).poll(cx).map(|_| ())
    }
}

#[cfg(all(docsrs, not(windows)))]
impl std::future::Future for Join {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let _ = (self, cx);
        std::task::Poll::Pending
    }
}

impl Context {
    /// Runs `f` and awaits its future, converting a cancellation request for this task into
    /// `Err(Canceled)`, with `f`'s future dropped instead of the task future.  The surrounding task
    /// can run async cleanup before finishing.
    ///
    /// Only one `cancel_guard` may be active for a task at a time — calling this re-entrantly
    /// panics.
    pub async fn cancel_guard<T, F>(&mut self, f: F) -> Result<T, Canceled>
    where
        F: AsyncFnOnce(&mut Context) -> T,
    {
        #[cfg(windows)]
        {
            let id = self.id;
            let (tx, rx) = oneshot::channel::<()>();

            REGISTRY.with(|r| {
                let mut reg = r.borrow_mut();
                let slot = reg
                    .tasks
                    .get_mut(&id)
                    .expect("cancel_guard: task slot missing for the currently running task");
                assert!(
                    !slot.in_guard,
                    "cancel_guard: already inside a guard for this task"
                );
                slot.in_guard = true;
                slot.guard_signal = Some(tx);
            });

            struct Reset(TaskId);
            impl Drop for Reset {
                fn drop(&mut self) {
                    REGISTRY.with(|r| {
                        if let Some(slot) = r.borrow_mut().tasks.get_mut(&self.0) {
                            slot.in_guard = false;
                            slot.guard_signal = None;
                        }
                    });
                }
            }
            let _reset = Reset(id);

            let fut = f(&mut *self);
            futures::pin_mut!(fut);
            match future::select(fut, rx).await {
                Either::Left((value, _)) => Ok(value),
                Either::Right(_) => Err(Canceled),
            }
        }
        #[cfg(all(docsrs, not(windows)))]
        {
            let _ = f;
            unreachable!()
        }
    }

    /// Submits `f` to the reactor thread as a new task and returns a
    /// future for its result.
    ///
    /// Because APCs are queued in a FIFO manner, self-submitting and awaiting a
    /// task can be used to guarantee that all previously pending APCs, including
    /// those generated by Win32 APIs, have run.
    pub fn submit<T, F>(&self, f: F) -> Task<T>
    where
        T: Send + 'static,
        F: AsyncFnOnce(&mut Context) -> T + Send + 'static,
    {
        #[cfg(windows)]
        {
            let id = TaskId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
            let (result_tx, result_rx) = oneshot::channel();
            let task_inner = self.inner.clone();

            // SAFETY: `Context` methods only ever run from within a task's
            // poll, which only ever happens on the reactor thread — so
            // `GetCurrentThread()`'s pseudo-handle correctly refers to it.
            let result = unsafe {
                queue_apc(GetCurrentThread(), move || {
                    let task: BoxedTask = Box::pin(async move {
                        let mut ctx = Context {
                            id,
                            inner: task_inner,
                        };
                        let value = f(&mut ctx).await;
                        let _ = result_tx.send(value);
                    });
                    REGISTRY.with(|r| {
                        r.borrow_mut().tasks.insert(
                            id,
                            TaskSlot {
                                future: Some(task),
                                in_guard: false,
                                guard_signal: None,
                            },
                        );
                    });
                })
            };
            result.expect("submitting to the live APC reactor thread should succeed");

            Task {
                id: Some(id),
                rx: result_rx,
                inner: self.inner.clone(),
                _pin: std::marker::PhantomPinned,
            }
        }
        #[cfg(all(docsrs, not(windows)))]
        {
            let _ = f;
            unreachable!()
        }
    }
}

#[cfg(windows)]
impl<T> Future for Task<T> {
    type Output = Result<T, TaskCanceled>;

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we do not move any field out of the pinned task. `rx` is
        // `Unpin`, and `id` is only mutated in place.
        let this = unsafe { self.get_unchecked_mut() };
        match Pin::new(&mut this.rx).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                this.id = None;
                Poll::Ready(result.map_err(|_| TaskCanceled))
            }
        }
    }
}

#[cfg(all(docsrs, not(windows)))]
impl<T> std::future::Future for Task<T> {
    type Output = Result<T, TaskCanceled>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let _ = (self, cx);
        std::task::Poll::Pending
    }
}

impl<T> Task<T> {
    /// Requests cancellation without dropping the task.
    ///
    /// The task can be awaited afterward to join it. An unguarded task
    /// resolves with [`TaskCanceled`]; a task inside
    /// [`Context::cancel_guard`] can run cooperative cleanup and produce its
    /// normal result instead.
    pub fn cancel(self: std::pin::Pin<&mut Self>) {
        #[cfg(windows)]
        {
            // SAFETY: cancellation only mutates `id`, which is not structurally
            // pinned.
            let this = unsafe { self.get_unchecked_mut() };
            let Some(id) = this.id.take() else {
                return;
            };
            // Best effort: failure means the reactor thread (and thus the
            // task) has already gone away, so there is nothing to cancel.
            let _ = post(&this.inner, move || {
                let removed = REGISTRY.with(|r| {
                    let mut reg = r.borrow_mut();
                    match reg.tasks.get_mut(&id) {
                        None => None,
                        Some(slot) if slot.in_guard => {
                            if let Some(tx) = slot.guard_signal.take() {
                                let _ = tx.send(());
                            }
                            None
                        }
                        Some(_) => reg.tasks.remove(&id),
                    }
                });
                // Drop outside the RefCell borrow above, in case the future's
                // own teardown happens to touch the registry.
                drop(removed);
            });
        }
        #[cfg(all(docsrs, not(windows)))]
        unreachable!()
    }
}

#[cfg(windows)]
impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        // SAFETY: a value is pinned in place for the duration of its destructor.
        unsafe { Pin::new_unchecked(self) }.cancel();
    }
}

#[cfg(all(docsrs, not(windows)))]
impl<T> Drop for Task<T> {
    fn drop(&mut self) {}
}

#[cfg(all(windows, test))]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    use super::*;

    /// Sends on `tx` when dropped — lets a test observe exactly when a
    /// task's future was actually torn down.
    struct DropSignal(Option<mpsc::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    const TIMEOUT: Duration = Duration::from_secs(5);

    /// Joins `control` on a helper thread with a bounded wait, so a bug
    /// that makes `join()` hang doesn't hang the whole test suite.
    fn join_with_timeout(control: Control) {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            futures::executor::block_on(control.join());
            let _ = tx.send(());
        });
        rx.recv_timeout(TIMEOUT)
            .expect("reactor did not shut down in time");
    }

    /// Runs `fut` to completion on a helper thread with a bounded wait, so a
    /// bug that stalls the reactor doesn't hang the whole test suite.
    fn block_on_with_timeout<T: Send + 'static>(
        fut: impl Future<Output = T> + Send + 'static,
    ) -> T {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(futures::executor::block_on(fut));
        });
        rx.recv_timeout(TIMEOUT)
            .expect("future did not resolve in time")
    }

    #[test]
    fn submit_and_await_result() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let task = reactor.submit(async move |_| 42).unwrap();
        assert_eq!(block_on_with_timeout(task).unwrap(), 42);
        control.close();
        join_with_timeout(control);
    }

    #[test]
    fn dropping_unguarded_task_force_drops_it() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let task = reactor
            .submit(async move |_| {
                let _signal = DropSignal(Some(tx));
                let _ = started_tx.send(());
                future::pending::<()>().await
            })
            .unwrap();

        // Wait for the task to actually start running (and reach the
        // pending await) before cancelling it — otherwise we'd just be
        // testing that dropping a never-polled task drops it, which is a
        // trivially different (and trivially true) case.
        started_rx
            .recv_timeout(TIMEOUT)
            .expect("task should have started running");
        drop(task);

        rx.recv_timeout(TIMEOUT)
            .expect("unguarded task should be force-dropped promptly");
        control.close();
        join_with_timeout(control);
    }

    #[test]
    fn cancel_and_await_unguarded_task() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let task = reactor
            .submit(async move |_| {
                let _ = started_tx.send(());
                future::pending::<()>().await
            })
            .unwrap();

        started_rx
            .recv_timeout(TIMEOUT)
            .expect("task should have started running");
        let mut task = Box::pin(task);
        task.as_mut().cancel();
        task.as_mut().cancel();
        assert_eq!(block_on_with_timeout(task), Err(TaskCanceled));

        control.close();
        join_with_timeout(control);
    }

    #[test]
    fn dropping_guarded_task_runs_cooperative_cleanup() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let task = reactor
            .submit(async move |ctx| {
                let result = ctx
                    .cancel_guard(async move |_| {
                        let _ = started_tx.send(());
                        future::pending::<()>().await
                    })
                    .await;
                assert!(result.is_err(), "expected Canceled");
                let _ = tx.send(());
            })
            .unwrap();

        // Wait for the task to actually enter its guard before cancelling
        // it, so this exercises the cooperative path rather than racing a
        // force-drop against the task's very first poll.
        started_rx
            .recv_timeout(TIMEOUT)
            .expect("task should have entered its guard");
        drop(task);

        rx.recv_timeout(TIMEOUT)
            .expect("guarded task should observe cancellation and clean up cooperatively");
        control.close();
        join_with_timeout(control);
    }

    #[test]
    fn close_rejects_new_submissions() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        control.close();

        let result = reactor.submit(async move |_| ());
        assert_eq!(result.err(), Some(Closed));

        join_with_timeout(control);
    }

    #[test]
    fn join_resolves_after_mixed_tasks_are_cancelled() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();

        let guarded = reactor
            .submit(async move |ctx| {
                let _ = ctx
                    .cancel_guard(async move |_| future::pending::<()>().await)
                    .await;
            })
            .unwrap();
        let plain = reactor
            .submit(async move |_| future::pending::<()>().await)
            .unwrap();

        drop(guarded);
        drop(plain);
        control.close();

        // If this returns at all, `join()` correctly observed both
        // cancellations and drained the registry.
        join_with_timeout(control);
    }

    #[test]
    fn join_resolves_without_close_once_every_handle_is_dropped() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();

        let task = reactor
            .submit(async move |_| future::pending::<()>().await)
            .unwrap();

        // Drop every `Reactor` clone and live task, but never call
        // `cancel()`. `join()` should still resolve — it drops its own
        // reference and waits, so this exercises the reactor noticing that
        // *nothing* references it anymore (not just that it was told to
        // close) and exiting on its own.
        drop(task);
        drop(reactor);

        join_with_timeout(control);
    }

    #[test]
    fn pending_join_can_close_reactor() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let mut join = Box::pin(control.join());

        let waker = futures::task::noop_waker();
        let mut cx = task::Context::from_waker(&waker);
        assert!(matches!(join.as_mut().poll(&mut cx), Poll::Pending));
        join.as_mut().close();
        block_on_with_timeout(join);

        assert_eq!(reactor.submit(async |_| ()).err(), Some(Closed));
    }

    #[test]
    fn context_submit_runs_on_reactor_thread() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();
        let (tx, rx) = mpsc::channel();
        let task = reactor
            .submit(async move |ctx| {
                let reactor_tid = unsafe { GetCurrentThreadId() };
                let posted_tid = ctx
                    .submit(async |_| unsafe { GetCurrentThreadId() })
                    .await
                    .unwrap();
                let _ = tx.send((reactor_tid, posted_tid));
            })
            .unwrap();

        let (reactor_tid, posted_tid) = rx.recv_timeout(TIMEOUT).unwrap();
        assert_eq!(reactor_tid, posted_tid);

        drop(task);
        control.close();
        join_with_timeout(control);
    }

    #[test]
    fn panic_in_one_task_does_not_affect_others() {
        let (reactor, control) = futures::executor::block_on(Reactor::new()).unwrap();

        // Suppress the default panic hook's stderr output for this
        // deliberately-triggered, caught panic.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicking = reactor
            .submit(async move |_| {
                panic!("intentional test panic");
            })
            .unwrap();
        let panicking_result = block_on_with_timeout(panicking);
        std::panic::set_hook(previous_hook);
        assert!(panicking_result.is_err());

        let ok = reactor.submit(async move |_| 7).unwrap();
        assert_eq!(block_on_with_timeout(ok).unwrap(), 7);

        control.close();
        join_with_timeout(control);
    }
}
