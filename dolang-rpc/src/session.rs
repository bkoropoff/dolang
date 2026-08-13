//! Session-scoped opaque resources.
//!
//! [`Opaque`] is a reference to a resource retained by one endpoint of a
//! session. It can be redeemed only through that endpoint, and only while the
//! resource remains registered.
//!
//! # Reference counting
//!
//! Opaques are counted at two independent levels, and conflating them is a
//! bug:
//!
//! * The **protocol count** is per session table entry: how many references
//!   the owner has handed the peer. The owner increments it when it serializes
//!   a gift; the peer mirrors it when it deserializes one. Only a
//!   [`Kind::Release`](crate::fragment::Kind::Release) moves it down.
//! * The **local handle count** is the `Arc` inside [`Opaque`] itself: how
//!   many live values in this process name the resource. Cloning an `Opaque`
//!   grants the peer nothing, so it must not touch the protocol count — a
//!   shared counter would inflate the eventual release by every local clone
//!   and make the owner decrement more than it ever granted.
//!
//! Because the counts are plain totals rather than a handshake, a gift racing
//! a release needs no generation number or echo. The owner goes 1 -> 2 -> 1
//! while the peer goes 1 -> 0 -> 1, and both arrive at the same total whatever
//! the interleaving.
//!
//! # Direction is load-bearing
//!
//! Serializing an opaque means one of two entirely different things depending
//! on which side owns the resource:
//!
//! * A **gift** travels away from its owner (a response handing back a freshly
//!   opened file). It grants the peer a reference, so it takes one.
//! * A **citation** travels back toward its owner (`FileRead { file }`), which
//!   is the hot path. It must have no protocol effect whatsoever. Citations
//!   are safe unconditionally: the caller necessarily holds a reference for
//!   the duration of the call it is citing the opaque in.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(unix)]
use std::os::fd::OwnedFd;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt, io,
    marker::PhantomData,
    sync::{Arc, Mutex, Weak},
};

#[cfg(unix)]
use crate::{handle::TakeHandle, transport::ReceivedHandles};
use crate::{
    handle::{ErasedHandle, PutHandle},
    transport::EncodeHandles,
};

/// The owner bit rides the low bit of the id.
pub(crate) fn pack_wire(owner: u8, id: u64) -> u64 {
    debug_assert!(id < (1 << 63), "opaque identifier is too large to pack");
    (id << 1) | u64::from(owner & 1)
}

pub(crate) fn unpack_wire(packed: u64) -> (u8, u64) {
    ((packed & 1) as u8, packed >> 1)
}

/// Wire discriminant: the resource belongs to the sender.
const WIRE_GIFT: u8 = 0;
/// Wire discriminant: the resource belongs to the receiver.
const WIRE_CITATION: u8 = 1;

/// A value that can be registered in a session's opaque-object table.
///
/// `Marker` is the public protocol-level type carried by [`Opaque`]. The
/// concrete resource type may remain private. A marker does not by itself
/// authorize a downcast: [`Session::acquire`] also verifies the concrete type.
pub trait OpaqueResource: Send + Sync + 'static {
    type Marker: ?Sized + 'static;
}

/// Emits release frames for opaques whose last local handle has dropped.
///
/// Implemented on each endpoint's `WeakUnboundedSender` for its own outgoing
/// message type. Sending must not block or fail loudly: this is called from
/// `Drop`. Deliberately weak: a writer task treats "every sender dropped" as
/// its shutdown signal and transitively holds the `Session` that owns its
/// sink, so a strong sender here would be a cycle — the writer waiting on a
/// channel it is itself keeping open.
pub(crate) trait ReleaseSink: Send + Sync + 'static {
    fn release(&self, id: u64, count: u32);
}

/// A handle on a resource this endpoint owns.
///
/// The `Arc` around it is the local handle count. The resource itself lives in
/// the session table, never in here, so that [`Session::unregister`] can empty
/// the slot and have every outstanding `Opaque` observe the revocation. That
/// is the whole reason [`Session::acquire`] is fallible: resolving an opaque is
/// `open()` on a descriptor number, not a pointer dereference.
pub(crate) struct LocalRef {
    id: u64,
    session: Weak<Session>,
}

/// A handle on a resource the peer owns.
///
/// The protocol count lives in the table entry, not here: it is the total the
/// peer has granted for the id, and whichever handle is alive owns that whole
/// total. See [`RemoteRef::drop`] for how a handle that loses a race forfeits
/// it rather than splitting it.
pub(crate) struct RemoteRef {
    id: u64,
    session: Weak<Session>,
}

pub(crate) enum Ref {
    Local(Arc<LocalRef>),
    Remote(Arc<RemoteRef>),
}

impl Clone for Ref {
    fn clone(&self) -> Self {
        // Purely a local handle count bump; the protocol count is untouched.
        match self {
            Self::Local(local) => Self::Local(local.clone()),
            Self::Remote(remote) => Self::Remote(remote.clone()),
        }
    }
}

impl Ref {
    fn id(&self) -> u64 {
        match self {
            Self::Local(local) => local.id,
            Self::Remote(remote) => remote.id,
        }
    }

    fn owner(&self) -> u8 {
        match self {
            Self::Local(_) => WIRE_GIFT,
            Self::Remote(_) => WIRE_CITATION,
        }
    }
}

impl Drop for LocalRef {
    fn drop(&mut self) {
        // A dead `Weak<Session>` means the connection itself is tearing down,
        // which retires every table wholesale. Nothing to do.
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let mut tables = session.tables.lock().unwrap();
        let Some(entry) = tables.local.get(&self.id) else {
            return;
        };
        // Only act if the entry still points at *us*: a citation that arrived
        // while this handle was dying installed a fresh one (see `cite`), and
        // that one now owns the registration.
        if !entry.points_at(self) {
            return;
        }
        // The peer may still name this resource even though we no longer hold
        // a handle on it, in which case the entry (and the resource) has to
        // outlive us and is retired by the final release instead.
        if entry.granted == 0 {
            tables.local.remove(&self.id);
        }
    }
}

impl Drop for RemoteRef {
    fn drop(&mut self) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let granted = {
            let mut tables = session.tables.lock().unwrap();
            // Only act if the slot still points at *us*. A gift that failed to
            // upgrade this handle mid-drop installed a fresh one and folded
            // our references into the entry's running total; that handle now
            // owns the whole total, so this one releases nothing.
            if !tables
                .remote
                .get(&self.id)
                .is_some_and(|entry| entry.points_at(self))
            {
                return;
            }
            tables
                .remote
                .remove(&self.id)
                .expect("just matched")
                .granted
        };
        if granted > 0 {
            session.sink.release(self.id, granted);
        }
    }
}

/// A session-scoped reference to a registered resource.
///
/// This is a reference, not the resource itself. It becomes invalid when its
/// owner unregisters the resource or the session ends. Dropping the last
/// `Opaque` naming a resource the peer owns releases the endpoint's references
/// to it automatically — no explicit protocol close is required to avoid
/// leaking it.
pub struct Opaque<M: ?Sized> {
    pub(crate) inner: Ref,
    marker: PhantomData<fn() -> M>,
}

impl<M: ?Sized> Clone for Opaque<M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            marker: PhantomData,
        }
    }
}

impl<M: ?Sized> PartialEq for Opaque<M> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.owner() == other.inner.owner() && self.inner.id() == other.inner.id()
    }
}

impl<M: ?Sized> Eq for Opaque<M> {}

impl<M: ?Sized> fmt::Debug for Opaque<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opaque")
            .field("owner", &self.inner.owner())
            .field("id", &self.inner.id())
            .finish_non_exhaustive()
    }
}

impl<M: ?Sized> Serialize for Opaque<M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::serde::serialize_opaque(&self.inner, serializer)
    }
}

impl<'de, M: ?Sized> Deserialize<'de> for Opaque<M> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        crate::serde::deserialize_opaque(deserializer).map(|inner| Opaque {
            inner,
            marker: PhantomData,
        })
    }
}

/// A retained, typed guard for a registered opaque resource.
///
/// The resource remains alive until every guard is dropped, even if its owner
/// unregisters it in the meantime.
pub struct OpaqueGuard<T>(Arc<T>);
impl<T> std::ops::Deref for OpaqueGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

/// A handle that is stale, belongs to another session, or has the wrong type.
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("invalid opaque object")]
pub struct InvalidOpaque;

struct LocalEntry {
    ty: TypeId,
    /// `None` once [`Session::unregister`] has emptied the handle. The entry
    /// itself survives so that a citation still in flight from the peer is
    /// distinguishable from an unknown id, and so the id cannot be reused
    /// while the peer might still name it.
    resource: Option<Arc<dyn Any + Send + Sync>>,
    /// Protocol count: references handed to the peer.
    granted: u32,
    handle: Weak<LocalRef>,
}

impl LocalEntry {
    fn points_at(&self, handle: &LocalRef) -> bool {
        std::ptr::eq(self.handle.as_ptr(), handle as *const LocalRef)
    }
}

struct RemoteEntry {
    /// Protocol count: references the peer has granted for this id. Owned by
    /// the entry rather than by any one handle, so a gift racing the last
    /// handle's drop folds into a single running total.
    granted: u32,
    handle: Weak<RemoteRef>,
}

impl RemoteEntry {
    fn points_at(&self, handle: &RemoteRef) -> bool {
        std::ptr::eq(self.handle.as_ptr(), handle as *const RemoteRef)
    }
}

#[derive(Default)]
struct Tables {
    next: u64,
    local: HashMap<u64, LocalEntry>,
    remote: HashMap<u64, RemoteEntry>,
}

/// One endpoint's half of a session's opaque bookkeeping.
///
/// Both endpoints run the same structure: `local` holds resources this side
/// owns, `remote` mirrors references the peer has granted this side. A client
/// that only ever receives opaques still needs `remote`, because dropping
/// those references is what frees the peer's resources.
pub(crate) struct Session {
    tables: Mutex<Tables>,
    sink: Box<dyn ReleaseSink>,
}

impl Session {
    pub(crate) fn new(sink: Box<dyn ReleaseSink>) -> Arc<Self> {
        Arc::new(Self {
            tables: Mutex::new(Tables::default()),
            sink,
        })
    }

    pub(crate) fn register<T: OpaqueResource>(self: &Arc<Self>, value: T) -> Opaque<T::Marker> {
        let mut tables = self.tables.lock().unwrap();
        let id = tables.next;
        tables.next = tables
            .next
            .checked_add(1)
            .expect("opaque identifiers exhausted");
        let handle = Arc::new(LocalRef {
            id,
            session: Arc::downgrade(self),
        });
        tables.local.insert(
            id,
            LocalEntry {
                ty: TypeId::of::<T>(),
                resource: Some(Arc::new(value)),
                granted: 0,
                handle: Arc::downgrade(&handle),
            },
        );
        Opaque {
            inner: Ref::Local(handle),
            marker: PhantomData,
        }
    }

    pub(crate) fn acquire<T: OpaqueResource>(
        &self,
        value: Opaque<T::Marker>,
    ) -> Result<OpaqueGuard<T>, InvalidOpaque> {
        let Ref::Local(local) = &value.inner else {
            return Err(InvalidOpaque);
        };
        let tables = self.tables.lock().unwrap();
        let entry = tables.local.get(&local.id).ok_or(InvalidOpaque)?;
        if entry.ty != TypeId::of::<T>() {
            return Err(InvalidOpaque);
        }
        let resource = entry.resource.as_ref().ok_or(InvalidOpaque)?;
        Ok(OpaqueGuard(
            resource
                .clone()
                .downcast::<T>()
                .map_err(|_| InvalidOpaque)?,
        ))
    }

    /// Empties the handle, returning the resource if this call held the last
    /// reference to it.
    ///
    /// The registration itself survives until the peer has released every
    /// reference; only the resource slot is cleared. On the `None` path the
    /// resource is *not* restored to the table — outstanding [`OpaqueGuard`]s
    /// keep it alive and it dies with the last one. Restoring it would
    /// resurrect the table's own reference so the resource outlived every
    /// guard, silently turning a close that races an in-flight write into a
    /// no-op; on a pipe's send end that is a missing EOF and a hung reader.
    pub(crate) fn unregister<T: OpaqueResource>(
        &self,
        value: Opaque<T::Marker>,
    ) -> Result<Option<T>, InvalidOpaque> {
        let Ref::Local(local) = &value.inner else {
            return Err(InvalidOpaque);
        };
        let mut tables = self.tables.lock().unwrap();
        let entry = tables.local.get_mut(&local.id).ok_or(InvalidOpaque)?;
        if entry.ty != TypeId::of::<T>() {
            return Err(InvalidOpaque);
        }
        let resource = entry.resource.take().ok_or(InvalidOpaque)?;
        let resource = resource.downcast::<T>().map_err(|_| InvalidOpaque)?;
        Ok(Arc::try_unwrap(resource).ok())
    }

    /// Applies a release frame from the peer. Unknown ids are ignored: a
    /// consuming operation races the peer's release by construction.
    pub(crate) fn release(&self, id: u64, count: u32) {
        let mut tables = self.tables.lock().unwrap();
        let Some(entry) = tables.local.get_mut(&id) else {
            return;
        };
        entry.granted = entry.granted.saturating_sub(count);
        if entry.granted == 0 && entry.handle.upgrade().is_none() {
            tables.local.remove(&id);
        }
    }

    /// Records that a gift for `id` is being serialized, and returns the
    /// escrow item holding the reference until the payload is committed.
    fn gift(&self, handle: &Arc<LocalRef>) -> Escrowed {
        let mut tables = self.tables.lock().unwrap();
        if let Some(entry) = tables.local.get_mut(&handle.id) {
            entry.granted += 1;
        }
        Escrowed::Gift(handle.clone())
    }

    /// Mirrors an arriving gift for `id`, merging into the handle this
    /// endpoint already holds when there is one.
    fn receive(self: &Arc<Self>, id: u64) -> Ref {
        let mut tables = self.tables.lock().unwrap();
        let session = Arc::downgrade(self);
        let entry = tables.remote.entry(id).or_insert_with(|| RemoteEntry {
            granted: 0,
            handle: Weak::new(),
        });
        entry.granted += 1;
        // A failed upgrade means the previous handle is mid-`Drop`. It will
        // find the slot no longer pointing at it and leave the running total —
        // including its own references and the one arriving now — to the fresh
        // handle installed here.
        if let Some(handle) = entry.handle.upgrade() {
            return Ref::Remote(handle);
        }
        let handle = Arc::new(RemoteRef { id, session });
        entry.handle = Arc::downgrade(&handle);
        Ref::Remote(handle)
    }

    /// Resolves an arriving citation back to a handle on the resource this
    /// endpoint owns.
    ///
    /// An unknown id yields a handle with no table entry rather than an error:
    /// the peer may legitimately be citing something we have just retired, and
    /// failing here would tear down the whole session over a benign race.
    /// [`acquire`](Self::acquire) reports it as [`InvalidOpaque`] instead.
    fn cite(self: &Arc<Self>, id: u64) -> Ref {
        let mut tables = self.tables.lock().unwrap();
        if let Some(entry) = tables.local.get_mut(&id) {
            if let Some(handle) = entry.handle.upgrade() {
                return Ref::Local(handle);
            }
            let handle = Arc::new(LocalRef {
                id,
                session: Arc::downgrade(self),
            });
            entry.handle = Arc::downgrade(&handle);
            return Ref::Local(handle);
        }
        Ref::Local(Arc::new(LocalRef {
            id,
            session: Arc::downgrade(self),
        }))
    }

    /// Resolves an arriving `(owner, id)` pair against this endpoint.
    pub(crate) fn take(self: &Arc<Self>, owner: u8, id: u64) -> Result<Ref, InvalidOpaque> {
        match owner {
            // The sender owns it: a gift, which we mirror.
            WIRE_GIFT => Ok(self.receive(id)),
            // We own it: a citation coming home.
            WIRE_CITATION => Ok(self.cite(id)),
            _ => Err(InvalidOpaque),
        }
    }
}

/// A reference held on behalf of a message that is still being written.
enum Escrowed {
    /// A gift whose protocol increment is provisional until the payload is
    /// fully written.
    Gift(Arc<LocalRef>),
    /// A citation. Never read: holding the `Arc` *is* the point, since that
    /// is what keeps the last local handle alive and so orders any resulting
    /// release strictly after the last payload fragment of the message that
    /// cited it. Without it a small release frame could overtake a large body
    /// under round-robin fragmentation, and the peer would retire the entry
    /// before reassembling the message naming it.
    Citation(#[allow(dead_code)] Arc<RemoteRef>),
}

/// The opaque references one outgoing message is holding.
///
/// Serializing moves references in here; the message's terminal outcome
/// decides between [`commit`](Self::commit) and [`rescind`](Self::rescind).
#[derive(Default)]
pub(crate) struct Ledger {
    items: Vec<Escrowed>,
}

impl Ledger {
    /// Records an opaque encountered during serialization, returning its wire
    /// `(owner, id)`.
    pub(crate) fn put(&mut self, value: &Ref, session: &Arc<Session>) -> (u8, u64) {
        match value {
            Ref::Local(local) => {
                self.items.push(session.gift(local));
                (WIRE_GIFT, local.id)
            }
            Ref::Remote(remote) => {
                self.items.push(Escrowed::Citation(remote.clone()));
                (WIRE_CITATION, remote.id)
            }
        }
    }

    /// The payload was fully written, so every gift in it is irrevocably
    /// transmitted. Dropping the citations here is what orders any release
    /// they were suppressing after the message that cited them.
    pub(crate) fn commit(self) {
        // Every held reference drops here, on the far side of the payload.
    }

    /// The message was abandoned before its payload completed, so the peer
    /// cannot have decoded it and no gift in it ever landed.
    ///
    /// Only ever correct for an abort that precedes payload completion.
    /// Guessing "delivered" when it was not strands a reference until the
    /// session ends; guessing "not delivered" when it was leaves the peer
    /// holding a freed handle. Leak beats corruption.
    pub(crate) fn rescind(self) {
        for item in &self.items {
            let Escrowed::Gift(handle) = item else {
                continue;
            };
            let Some(session) = handle.session.upgrade() else {
                continue;
            };
            let mut tables = session.tables.lock().unwrap();
            if let Some(entry) = tables.local.get_mut(&handle.id) {
                entry.granted = entry.granted.saturating_sub(1);
            }
        }
    }
}

/// Wraps a transport's handle sink with the session context an [`Opaque`]
/// needs, so that serialization has exactly one threaded context rather than
/// two parallel ones.
pub(crate) struct SessionFrame<'a> {
    pub(crate) inner: EncodeHandles,
    pub(crate) session: &'a Arc<Session>,
    pub(crate) ledger: &'a mut Ledger,
}

impl PutHandle for SessionFrame<'_> {
    #[cfg(unix)]
    fn put_handle(&mut self, handle: &dyn ErasedHandle) -> io::Result<u32> {
        self.inner.put_handle(handle)
    }

    #[cfg(windows)]
    fn put_handle(&mut self, handle: &dyn ErasedHandle) -> io::Result<usize> {
        self.inner.put_handle(handle)
    }

    fn put_opaque(&mut self, opaque: &Ref) -> io::Result<(u8, u64)> {
        Ok(self.ledger.put(opaque, self.session))
    }
}

/// The deserialization counterpart of [`SessionFrame`].
#[cfg(unix)]
pub(crate) struct SessionHandles<'a> {
    pub(crate) inner: ReceivedHandles,
    pub(crate) session: &'a Arc<Session>,
}

#[cfg(unix)]
impl TakeHandle for SessionHandles<'_> {
    fn take_handle(&mut self, index: u32) -> io::Result<OwnedFd> {
        self.inner.take_handle(index)
    }

    fn finish(&mut self) -> io::Result<()> {
        self.inner.finish()
    }
    fn take_opaque(&mut self, owner: u8, id: u64) -> io::Result<Ref> {
        self.session
            .take(owner, id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid opaque reference"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct Recorder(Mutex<Vec<(u64, u32)>>);
    impl ReleaseSink for Arc<Recorder> {
        fn release(&self, id: u64, count: u32) {
            self.0.lock().unwrap().push((id, count));
        }
    }

    /// A session whose emitted releases the test can inspect.
    fn session() -> (Arc<Session>, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        (Session::new(Box::new(recorder.clone())), recorder)
    }

    struct Marker;
    struct OtherMarker;
    struct Value(u32);
    struct OtherValue;
    struct DropValue(Arc<AtomicBool>);
    impl OpaqueResource for Value {
        type Marker = Marker;
    }
    impl OpaqueResource for OtherValue {
        type Marker = OtherMarker;
    }
    impl OpaqueResource for DropValue {
        type Marker = Marker;
    }
    impl Drop for DropValue {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn guards_outlive_registration() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let guard = session.acquire::<Value>(opaque.clone()).unwrap();
        assert!(
            session
                .unregister::<Value>(opaque.clone())
                .unwrap()
                .is_none()
        );
        assert_eq!(guard.0.0, 42);
        assert!(session.acquire::<Value>(opaque).is_err());
    }

    #[test]
    fn unregister_returns_exclusively_owned_value() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let value = session.unregister::<Value>(opaque).unwrap().unwrap();
        assert_eq!(value.0, 42);
    }

    #[test]
    fn wrong_type_does_not_remove_value() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let wrong: Opaque<OtherMarker> = Opaque {
            inner: opaque.inner.clone(),
            marker: PhantomData,
        };
        assert!(session.unregister::<OtherValue>(wrong).is_err());
        assert_eq!(session.acquire::<Value>(opaque.clone()).unwrap().0.0, 42);
    }

    #[test]
    fn dropping_session_drops_registered_values() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (session, _) = session();
        let opaque = session.register(DropValue(dropped.clone()));
        drop(opaque);
        drop(session);
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn dropping_the_last_local_handle_retires_an_ungifted_entry() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        drop(opaque);
        assert!(session.tables.lock().unwrap().local.is_empty());
    }

    #[test]
    fn a_gifted_entry_outlives_its_local_handle_until_released() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let Ref::Local(handle) = &opaque.inner else {
            unreachable!()
        };
        let escrow = session.gift(handle);
        drop(escrow);
        drop(opaque);
        // The peer still holds the reference, so the resource must survive.
        assert_eq!(session.tables.lock().unwrap().local.len(), 1);
        session.release(0, 1);
        assert!(session.tables.lock().unwrap().local.is_empty());
    }

    #[test]
    fn cloning_an_opaque_does_not_grant_a_protocol_reference() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let clones: Vec<_> = (0..8).map(|_| opaque.clone()).collect();
        assert_eq!(session.tables.lock().unwrap().local[&0].granted, 0);
        drop(clones);
        drop(opaque);
        assert!(session.tables.lock().unwrap().local.is_empty());
    }

    #[test]
    fn rescinding_undoes_the_gift_increment() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let mut ledger = Ledger::default();
        assert_eq!(ledger.put(&opaque.inner, &session), (WIRE_GIFT, 0));
        assert_eq!(session.tables.lock().unwrap().local[&0].granted, 1);
        ledger.rescind();
        assert_eq!(session.tables.lock().unwrap().local[&0].granted, 0);
    }

    #[test]
    fn committing_leaves_the_gift_increment_in_place() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        let mut ledger = Ledger::default();
        ledger.put(&opaque.inner, &session);
        ledger.commit();
        assert_eq!(session.tables.lock().unwrap().local[&0].granted, 1);
    }

    #[test]
    fn citing_an_opaque_has_no_protocol_effect() {
        let (session, recorder) = session();
        let mirrored = session.take(WIRE_GIFT, 7).unwrap();
        let opaque: Opaque<Marker> = Opaque {
            inner: mirrored,
            marker: PhantomData,
        };
        let mut ledger = Ledger::default();
        assert_eq!(ledger.put(&opaque.inner, &session), (WIRE_CITATION, 7));
        ledger.commit();
        // Still exactly the one reference the gift granted.
        drop(opaque);
        assert_eq!(*recorder.0.lock().unwrap(), vec![(7, 1)]);
    }

    #[test]
    fn repeated_gifts_of_one_id_accumulate_into_a_single_release() {
        let (session, recorder) = session();
        let first: Opaque<Marker> = Opaque {
            inner: session.take(WIRE_GIFT, 3).unwrap(),
            marker: PhantomData,
        };
        let second: Opaque<Marker> = Opaque {
            inner: session.take(WIRE_GIFT, 3).unwrap(),
            marker: PhantomData,
        };
        assert_eq!(first, second);
        drop(first);
        assert!(recorder.0.lock().unwrap().is_empty());
        drop(second);
        assert_eq!(*recorder.0.lock().unwrap(), vec![(3, 2)]);
    }

    #[test]
    fn a_citation_for_an_unknown_id_fails_to_acquire_rather_than_erroring() {
        let (session, _) = session();
        let opaque: Opaque<Marker> = Opaque {
            inner: session.take(WIRE_CITATION, 99).unwrap(),
            marker: PhantomData,
        };
        assert!(session.acquire::<Value>(opaque).is_err());
    }

    #[test]
    fn plain_postcard_use_panics() {
        let (session, _) = session();
        let opaque = session.register(Value(42));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                postcard::to_allocvec(&opaque)
            }))
            .is_err()
        );
        assert!(std::panic::catch_unwind(|| postcard::from_bytes::<Opaque<Marker>>(&[0])).is_err());
    }

    #[test]
    fn wire_form_survives_packing_both_owners() {
        for owner in [WIRE_GIFT, WIRE_CITATION] {
            for id in [0, 1, 42, u32::MAX as u64, (1 << 62) - 1] {
                assert_eq!(unpack_wire(pack_wire(owner, id)), (owner, id));
            }
        }
    }

    #[test]
    fn releasing_an_unknown_id_is_ignored() {
        let (session, _) = session();
        session.release(1234, 5);
    }
}
