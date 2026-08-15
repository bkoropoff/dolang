//! Backend SC manager resource.
//!
//! Only exists under `#[cfg(windows)]`: the stub backend never calls
//! [`dolang_vfs::extension::ExtContext::register`], so there is nothing to hold
//! on other platforms.

#![cfg(windows)]

use dolang_vfs::error::{Error, ErrorKind};
use dolang_vfs::extension::ExtResource;
use windows_sys::Win32::System::Services::{CloseServiceHandle, SC_HANDLE};

use crate::wire::ScManagerMarker;

/// Wraps a value so it can cross a [`tokio::task::spawn_blocking`] boundary
/// even when it isn't `Send` — `SC_HANDLE` is an opaque pointer type Rust
/// doesn't know is safe to move between threads, even though Microsoft
/// documents it as such.
struct SendHandle(SC_HANDLE);
// SAFETY: only used to ferry a live SC handle value to the blocking pool for
// `CloseServiceHandle`, which is documented as usable from any thread.
unsafe impl Send for SendHandle {}

impl SendHandle {
    /// Takes temporary ownership of a live SC manager handle.
    ///
    /// # Safety
    ///
    /// `handle` must be a live, owned SC manager handle. Ownership transfers
    /// to the returned wrapper.
    unsafe fn new(handle: SC_HANDLE) -> Self {
        Self(handle)
    }

    /// Consumes the wrapper, forcing a whole-value closure capture rather
    /// than a disjoint capture of the (non-`Send`) inner field.
    fn into_inner(self) -> SC_HANDLE {
        self.0
    }
}

/// An open handle to the Service Control Manager database.
///
/// Unlike `dolang-vfs-winreg`'s `Key`, there is no cursor-affecting
/// operation that shares state across calls on the same handle, so no
/// `Mutex` is needed — the handle is simply safe to use from any thread.
///
/// The handle is `Option`-wrapped so [`ScManager::close`] can take it out
/// and hand it to the blocking pool while leaving [`Drop`] able to tell
/// (via `None`) that there's nothing left for it to do.
pub(crate) struct ScManager(Option<SC_HANDLE>);

// SAFETY: Win32 SC manager handles are valid to use from any thread.
unsafe impl Send for ScManager {}
unsafe impl Sync for ScManager {}

impl ExtResource for ScManager {
    type Marker = ScManagerMarker;
}

impl ScManager {
    /// Takes ownership of an open SC manager handle.
    ///
    /// # Safety
    ///
    /// `handle` must be a live, owned SC manager handle that may be passed to
    /// `CloseServiceHandle` exactly once. Ownership transfers to this value.
    pub(crate) unsafe fn new(handle: SC_HANDLE) -> Self {
        Self(Some(handle))
    }

    pub(crate) fn handle(&self) -> SC_HANDLE {
        self.0.expect("SC manager handle used after close")
    }

    /// Closes the handle on the blocking pool and awaits completion, then
    /// disarms [`Drop`] so it becomes a no-op afterward.
    ///
    /// Used for explicit [`crate::wire::WinScmRequest::CloseManager`]
    /// handling, where the caller wants to know the close has actually
    /// completed before the request returns — unlike [`Drop`]'s
    /// fire-and-forget close, which exists only to catch handles dropped
    /// through some other path (e.g. session teardown).
    pub(crate) async fn close(mut self) -> Result<(), Error> {
        let Some(handle) = self.0.take() else {
            return Ok(());
        };
        // SAFETY: the handle was taken from this owning resource.
        let handle = unsafe { SendHandle::new(handle) };
        tokio::task::spawn_blocking(move || {
            // SAFETY: `handle` is a live handle owned by this `ScManager`;
            // taking it above ensures `Drop` won't also try to close it.
            unsafe {
                CloseServiceHandle(handle.into_inner());
            }
        })
        .await
        .map_err(|_| Error::new(ErrorKind::Other, "SCM manager close task panicked"))
    }
}

impl Drop for ScManager {
    fn drop(&mut self) {
        let Some(handle) = self.0.take() else {
            return;
        };
        // Closing an SC handle can involve an RPC to the services.exe SCM
        // process, not just local object-manager bookkeeping, so it
        // shouldn't run inline on an async executor thread. Route it
        // through the blocking pool when one is available; fall back to an
        // inline close outside any runtime (e.g. the last `Arc` reference
        // is dropped during teardown with no runtime alive), mirroring
        // `dolang_vfs::direct::lock::DirectFileLock`'s `Drop` impl.
        if tokio::runtime::Handle::try_current().is_ok() {
            // SAFETY: the handle was taken from this owning resource.
            let handle = unsafe { SendHandle::new(handle) };
            drop(tokio::task::spawn_blocking(move || {
                // SAFETY: see the comment on the `SendHandle` field above.
                unsafe {
                    CloseServiceHandle(handle.into_inner());
                }
            }));
        } else {
            // SAFETY: `handle` is a live handle owned by this value; nothing
            // else can close it first since `ScManager` isn't `Clone`.
            unsafe {
                CloseServiceHandle(handle);
            }
        }
    }
}
