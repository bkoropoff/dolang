//! Session-wide credit pools.
//!
//! Two classes of receiver memory are governed by credit on this connection —
//! trailer bytes ([`Limits::trailer_session_window`](crate::Limits)) and
//! postcard payloads ([`Limits::max_outstanding_payload`](crate::Limits)) —
//! and they are deliberately **separate pools sharing one mechanism**.
//!
//! Merging them is a deadlock class rather than a tuning mistake. Take the
//! ordinary streaming-upload shape: a small descriptor payload, a large
//! trailer, and a handler that reads the trailer to completion before
//! responding. The payload's charge is held until the handler completes; the
//! handler cannot complete until it has consumed the trailer; the trailer needs
//! credit. One such call is fine because its own payload is small. `N` of them
//! whose payloads sum to a shared pool all wedge: each holds payload quota,
//! each needs trailer credit from the same place, and none can release.
//! Pricing a trailer fragment into the admission cost would only guarantee that
//! the trailer can *start*, not that it can continue.
//!
//! The dynamics do not mix either. Trailer credit recycles continuously as the
//! consumer reads; payload quota is held long and released once. The cost of
//! keeping them apart is that total peer-attributable receiver memory is the
//! sum of two numbers rather than one.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    task::Waker,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A per-message-id credit pool, shared by every stream of one class on one
/// connection.
///
/// Each endpoint holds one of these per direction per class. A send-side pool
/// tracks credit this endpoint may still spend; a receive-side pool tracks
/// unretired bytes owed by the peer, so the reassembler can catch a peer that
/// overruns what it was granted.
///
/// For trailers there is no negotiated per-trailer window. A sender that lets
/// one trailer consume the whole pool starves only its own other trailers,
/// never the peer's, so per-trailer fairness is a local scheduling concern
/// rather than something the protocol has to enforce — including a sender that
/// imposes a private budget per trailer, which this end cannot see. The
/// receiver never has to: it flushes coalesced credit whenever withholding it
/// could be what keeps a sender parked, both when the pool is provably drained
/// ([`SessionWindow::is_exhausted`]) and when a consumer is waiting on bytes
/// that have not come (`RecvShared::is_stalled`).
///
/// Outstanding bytes are tracked per message id rather than per
/// `SendShared`/`RecvShared`, because settlement outlives those. A trailer's
/// last `Credit` fragments routinely arrive after its send has finished and
/// left the scheduler; if the debt lived on the send, those refunds would be
/// dropped and the pool would shrink a little on every completed transfer
/// until nothing could be sent at all. Keying by id also makes settlement
/// idempotent, so an abort that returns the whole debt at once cannot be
/// double-counted by a `Credit` that crossed it on the wire.
///
/// The receive side of the payload quota uses the same type for the same
/// reason: a payload's charge is taken while its fragments arrive and released
/// when the application is done with the call, which is well after the
/// reassembler entry is gone.
pub(crate) struct SessionWindow {
    state: Mutex<SessionWindowState>,
}

struct SessionWindowState {
    /// Send side: bytes still spendable. Receive side: bytes of headroom
    /// before the peer has overrun its grant.
    available: usize,
    /// Bytes outstanding per message id. Entries are removed as they reach
    /// zero, so this is bounded by the number of live streams.
    debt: HashMap<u64, usize>,
    /// Writers parked because the *pool* was empty, as opposed to their own
    /// window. Woken all together on a refund; the parked set is bounded by
    /// the number of open streams, and no fairness is attempted beyond
    /// that — a starved pool is a misconfiguration, not a steady state.
    wakers: Vec<Waker>,
}

impl SessionWindow {
    pub(crate) fn new(available: usize) -> Self {
        Self {
            state: Mutex::new(SessionWindowState {
                available,
                debt: HashMap::new(),
                wakers: Vec::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        lock(&self.state).available
    }

    /// Receive side: true once the peer has spent every byte it was granted,
    /// so it is certainly parked waiting for credit. Forces a coalesced
    /// `Credit` out rather than stranding it below the threshold.
    pub(crate) fn is_exhausted(&self) -> bool {
        lock(&self.state).available == 0
    }

    /// Send side: spends up to `n` bytes on behalf of message `id`, returning
    /// how many were actually granted. Zero means the pool is empty.
    ///
    /// Clamping and accounting happen under one lock. They have to, now that
    /// the pool is the sole limiter: writers on different trailers hold
    /// different `SendShared` mutexes, so a separate read-then-debit would
    /// let two of them spend the same bytes.
    pub(crate) fn debit_up_to(&self, id: u64, n: usize) -> usize {
        let mut state = lock(&self.state);
        let granted = n.min(state.available);
        state.available -= granted;
        if granted > 0 {
            *state.debt.entry(id).or_default() += granted;
        }
        granted
    }

    /// Returns up to `n` of message `id`'s outstanding bytes and wakes every
    /// writer parked on the pool.
    ///
    /// Clamping to the recorded debt is what makes this safe to call for an
    /// id that has already been settled, or twice for the same bytes.
    pub(crate) fn refund(&self, id: u64, n: usize) {
        let mut state = lock(&self.state);
        let Some(debt) = state.debt.get_mut(&id) else {
            return;
        };
        let refund = n.min(*debt);
        *debt -= refund;
        if *debt == 0 {
            state.debt.remove(&id);
        }
        state.available += refund;
        let wakers = std::mem::take(&mut state.wakers);
        drop(state);
        for waker in wakers {
            waker.wake();
        }
    }

    /// Returns everything message `id` still owes, for a stream that ended
    /// without the rest of its bytes ever being retired (an abort, or a
    /// discard on the receive side). Idempotent.
    ///
    /// The amount released is reported back so a caller that must also tell
    /// the peer — the payload quota returns credit on exactly this path —
    /// can name the same number without keeping its own copy of the debt.
    pub(crate) fn settle(&self, id: u64) -> usize {
        let mut state = lock(&self.state);
        let Some(debt) = state.debt.remove(&id) else {
            return 0;
        };
        state.available += debt;
        let wakers = std::mem::take(&mut state.wakers);
        drop(state);
        for waker in wakers {
            waker.wake();
        }
        debt
    }

    pub(crate) fn park(&self, waker: &Waker) {
        let mut state = lock(&self.state);
        if !state.wakers.iter().any(|parked| parked.will_wake(waker)) {
            state.wakers.push(waker.clone());
        }
    }

    /// Receive side: accounts `n` bytes arriving from the peer under message
    /// `id`, returning `false` if that overruns the credit this endpoint
    /// actually granted.
    pub(crate) fn accept_bytes(&self, id: u64, n: usize) -> bool {
        let mut state = lock(&self.state);
        match state.available.checked_sub(n) {
            Some(remaining) => {
                state.available = remaining;
                *state.debt.entry(id).or_default() += n;
                true
            }
            None => false,
        }
    }
}

/// The send side of the payload quota: a plain byte counter with a parked-
/// writer list.
///
/// Unlike [`SessionWindow`] this keeps no per-id debt, because payload credit
/// comes back from the peer as a bare count rather than attributed to a
/// message. Nothing here ever has to answer "how much does message `n` owe" —
/// the scheduler knows what it charged each send, and a cancelled send
/// computes its own refund from how much of its payload actually reached the
/// wire.
pub(crate) struct PayloadBudget {
    state: Mutex<PayloadBudgetState>,
}

struct PayloadBudgetState {
    available: usize,
    wakers: Vec<Waker>,
}

impl PayloadBudget {
    pub(crate) fn new(available: usize) -> Self {
        Self {
            state: Mutex::new(PayloadBudgetState {
                available,
                wakers: Vec::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn available(&self) -> usize {
        lock(&self.state).available
    }

    /// Takes `n` bytes if the whole amount is available, reporting whether it
    /// was.
    ///
    /// All-or-nothing on purpose. A message is charged its entire payload
    /// before its first fragment goes out, so anything started can always be
    /// finished; partial debits would let several sends reach a half-written
    /// state that no remaining credit could drive to completion.
    pub(crate) fn try_debit(&self, n: usize) -> bool {
        let mut state = lock(&self.state);
        match state.available.checked_sub(n) {
            Some(remaining) => {
                state.available = remaining;
                true
            }
            None => false,
        }
    }

    /// Returns `n` bytes to the pool and wakes anything parked on it.
    ///
    /// Both the peer's `PayloadCredit` and a cancelled send's own unsent
    /// remainder land here. There is no clamp and none is needed: every byte
    /// returned corresponds to a byte this end charged, since the peer can
    /// only credit what it was actually sent and a cancelled send refunds only
    /// what it did not send.
    pub(crate) fn credit(&self, n: usize) {
        if n == 0 {
            return;
        }
        let mut state = lock(&self.state);
        state.available += n;
        let wakers = std::mem::take(&mut state.wakers);
        drop(state);
        for waker in wakers {
            waker.wake();
        }
    }

    pub(crate) fn park(&self, waker: &Waker) {
        let mut state = lock(&self.state);
        if !state.wakers.iter().any(|parked| parked.will_wake(waker)) {
            state.wakers.push(waker.clone());
        }
    }
}

/// The route from a credit-holding receiver back to its connection's outgoing
/// queue.
///
/// Trailer and payload state both live behind handles that know nothing about
/// the application protocol, so this mirrors `session::ReleaseSink`: the
/// endpoint implements it over its own outgoing channel and hands it in. All
/// three operations are fire-and-forget — a dead channel means the connection
/// is gone, which the reader will observe through its own error path.
pub(crate) trait ControlSink: Send + Sync + 'static {
    /// Returns `count` retired trailer bytes for message `id`.
    fn credit(&self, id: u64, count: u32);
    /// Returns `count` bytes of payload quota, for no particular message.
    fn payload_credit(&self, count: u32);
    /// Tells the peer to stop sending message `id`'s trailer.
    fn discard(&self, id: u64);
}

/// One message's charge against the receive-side payload quota, released when
/// this is dropped.
///
/// The charge is taken as the payload arrives and held until the application
/// is done with the call — which is the entire point of the limit, and also
/// what makes losing one so damaging. A missed release is not a delayed
/// release: it is a permanent subtraction from the pool, and enough of them
/// stall the connection with no diagnostic. There is no reconciliation
/// protocol worth building to recover from that, so **every** release path is
/// a drop rather than a call, and this guard is the only way a charge is ever
/// held. A dropped `CallResult`, a `CallContext` whose handler panicked, a
/// handler that returns without responding — each releases because each drops
/// this.
///
/// `SessionWindow::settle` is idempotent and clamps to the recorded debt, so
/// releasing early through an explicit API and then dropping the remains
/// cannot over-credit.
pub(crate) struct PayloadCharge {
    window: Arc<SessionWindow>,
    sink: Arc<dyn ControlSink>,
    id: u64,
}

impl PayloadCharge {
    pub(crate) fn new(window: Arc<SessionWindow>, sink: Arc<dyn ControlSink>, id: u64) -> Self {
        Self { window, sink, id }
    }

    /// Releases now rather than at drop, for a call that will pend far longer
    /// than it needs its request. Idempotent; dropping afterwards is a no-op.
    pub(crate) fn release(&self) {
        // The pool itself is negotiated through a `u32` handshake field, so
        // no single message's debt can exceed one; the saturation is for the
        // type system's benefit, not a case that arises.
        let count = u32::try_from(self.window.settle(self.id)).unwrap_or(u32::MAX);
        if count > 0 {
            self.sink.payload_credit(count);
        }
    }
}

impl Drop for PayloadCharge {
    fn drop(&mut self) {
        self.release();
    }
}
