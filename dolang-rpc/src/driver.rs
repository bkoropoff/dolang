//! Shutdown coordination shared by the client and server connection drivers.
//!
//! Each endpoint runs two long-lived futures — a receive driver and a send
//! driver — raced against each other by the endpoint's entry point. They are
//! deliberately kept at arm's length: neither is cancel-safe, so each must be
//! polled as a whole rather than stepped, and merging them into one loop would
//! make a stalled write block reading (and deadlock against a peer that is
//! itself blocked writing).
//!
//! That independence leaves exactly one thing they cannot work out alone: when
//! the send driver is allowed to stop. Its own state says whether it has work
//! left; only the receive half knows whether more work can still *arrive*, and
//! whether the peer is still there to return the flow-control credit a queued
//! send may be waiting on. [`Drain`] is that missing bit, published by the
//! receive driver and observed by the send driver over a `watch` channel.
//!
//! # Why this exists
//!
//! Payload quota (see [`crate::window`]) made the send scheduler able to hold
//! a message back indefinitely: a send whose payload does not fit the session
//! budget waits until the peer returns credit, and that credit arrives through
//! the *receive* half. A writer that drains while its reader is already gone
//! can therefore be holding a send that can never proceed.
//!
//! Dropping such a send is not an option — a graceful shutdown promises that
//! work already accepted is finished, and silently discarding a queued request
//! or response breaks that. So the ordering has to be the other way around:
//! the receive driver stays alive until the send driver has drained, rather
//! than the send driver being torn down when the receive driver ends.
//!
//! # Termination
//!
//! [`Drain::Graceful`] is unbounded by construction: it waits for credit the
//! peer may never send. A peer that stops reading at its own shutdown will
//! hold this end open indefinitely, so a caller that cannot tolerate that
//! should impose its own deadline by wrapping the endpoint's driving future in
//! a timeout — dropping it aborts both halves immediately. This crate does not
//! depend on a timer, so the policy belongs to the caller rather than here.

use tokio::sync::watch;

/// How much the send driver still owes before it may stop.
///
/// Ordered by strictness rather than by value: a driver may be moved from
/// [`Running`](Drain::Running) to either terminal state, and from
/// [`Graceful`](Drain::Graceful) to [`Abrupt`](Drain::Abrupt), but never back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Drain {
    /// The session is live. The send driver stops only if its channel closes,
    /// which means every handle that could still queue work is gone.
    Running,
    /// Shutdown was requested and everything that was going to be queued has
    /// been. The send driver finishes *everything* it holds, including sends
    /// still waiting on payload quota — the receive half is still running, so
    /// the credit that releases them can still arrive.
    Graceful,
    /// The receive half is gone. No further credit can arrive, so a send still
    /// waiting on quota can never proceed; the send driver flushes only what
    /// it has already started and abandons the rest.
    Abrupt,
}

/// The receive driver's end of the drain signal.
pub(crate) struct DrainSignal(watch::Sender<Drain>);

/// The send driver's end of the drain signal.
pub(crate) struct DrainWatch(watch::Receiver<Drain>);

/// Creates a drain signal, already subscribed.
///
/// The receiver is created here rather than on demand so the sender always has
/// one: `watch::Sender::send` reports an error when nobody is subscribed, and
/// a signal that silently failed to publish would strand the send driver.
pub(crate) fn drain_signal() -> (DrainSignal, DrainWatch) {
    let (tx, rx) = watch::channel(Drain::Running);
    (DrainSignal(tx), DrainWatch(rx))
}

impl DrainSignal {
    /// Publishes `mode`, unless a stricter one is already in effect.
    ///
    /// `send_if_modified` rather than `send`, so this cannot fail on a dropped
    /// receiver and cannot downgrade [`Drain::Abrupt`] — which is set on the
    /// receive driver's way out and must stick, or the send driver would be
    /// left waiting for credit from a peer this end is no longer reading.
    pub(crate) fn set(&self, mode: Drain) {
        self.0.send_if_modified(|current| {
            if *current == Drain::Abrupt || *current == mode {
                return false;
            }
            *current = mode;
            true
        });
    }

    /// Publishes [`Drain::Graceful`] once a requested drain has nothing left
    /// to wait for: shutdown asked for, and every dispatched call answered.
    ///
    /// Call it wherever that pair can newly become true — when shutdown is
    /// first requested, and whenever a handler task completes. [`Self::set`]
    /// is idempotent, so repeating it is free.
    ///
    /// Takes the two conditions as plain `bool`s rather than reaching for
    /// them, so a caller can invoke it through a single field borrow while a
    /// read borrows the rest of its driver.
    pub(crate) fn seal_if_idle(&self, draining: bool, idle: bool) {
        if draining && idle {
            self.set(Drain::Graceful);
        }
    }
}

impl DrainWatch {
    /// The mode currently in effect.
    pub(crate) fn mode(&mut self) -> Drain {
        *self.0.borrow_and_update()
    }

    /// Waits for the mode to change.
    ///
    /// Cancel-safe, and safe to select on unconditionally: once the sender is
    /// gone this stays pending forever rather than resolving in a loop, since
    /// the last mode published is already visible through [`Self::mode`].
    pub(crate) async fn changed(&mut self) {
        if self.0.changed().await.is_err() {
            std::future::pending::<()>().await
        }
    }
}
