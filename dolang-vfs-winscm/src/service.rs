//! Backend service resource.
//!
//! Only exists under `#[cfg(windows)]`: the stub backend never calls
//! [`dolang_vfs::extension::ExtContext::register`], so there is nothing to hold
//! on other platforms.

#![cfg(windows)]

use std::sync::Arc;

use dolang_vfs::error::{Error, ErrorKind};
use dolang_vfs::extension::ExtResource;
use dolang_winterop::apc::Reactor;
use windows_sys::Win32::System::Services::{CloseServiceHandle, SC_HANDLE};

use crate::wire::ServiceMarker;

/// Wraps a value so it can cross a [`tokio::task::spawn_blocking`] boundary
/// even when it isn't `Send` — `SC_HANDLE` is an opaque pointer type Rust
/// doesn't know is safe to move between threads, even though Microsoft
/// documents it as such.
struct SendHandle(SC_HANDLE);
// SAFETY: only used to ferry a live SC handle value to the blocking pool for
// `CloseServiceHandle`, which is documented as usable from any thread.
unsafe impl Send for SendHandle {}

impl SendHandle {
    /// Consumes the wrapper, forcing a whole-value closure capture rather
    /// than a disjoint capture of the (non-`Send`) inner field.
    fn into_inner(self) -> SC_HANDLE {
        self.0
    }
}

/// An open handle to a specific service.
///
/// Retains the [`Reactor`] it was opened/created through so
/// [`crate::backend::windows::wait_for_status_change`] can submit work to it
/// directly, without touching the process-wide reactor cache on every call.
/// This is also what keeps the reactor's background thread alive for
/// exactly as long as at least one `Service` handle referencing it exists —
/// see `crate::backend::windows::reactor` for the cache/quiescence design.
///
/// Also retains the service's own `name`: a status-change wait never
/// registers `NotifyServiceStatusChangeW` on `handle` itself. Instead it
/// opens a second, dedicated handle to the same service purely for that one
/// notification (see `crate::backend::windows::wait_for_status_change`) —
/// `name` is what lets it reopen the service on demand. This matters
/// because SCM has no "unregister notification" API: the only documented
/// way to cancel a still-outstanding request is to close the handle it was
/// registered on, and closing `handle` itself would invalidate every other
/// operation this `Service` supports. A dedicated, wait-scoped handle can
/// be closed freely on cancellation without disturbing anything else, and
/// — just as importantly — leaves `handle` free to be used for another
/// `wait_for_status_change` call later, since a handle that has ever had a
/// notification registered on it refuses a second registration
/// (`ERROR_ALREADY_REGISTERED`) until it's closed and reopened.
///
/// `handle` is `Option`-wrapped so [`Service::close`] can take it out and
/// hand it to the blocking pool while leaving [`Drop`] able to tell (via
/// `None`) that there's nothing left for it to do.
pub(crate) struct Service {
    handle: Option<SC_HANDLE>,
    pub(crate) reactor: Arc<Reactor>,
    pub(crate) name: String,
}

// SAFETY: Win32 service handles are valid to use from any thread.
unsafe impl Send for Service {}
unsafe impl Sync for Service {}

impl ExtResource for Service {
    type Marker = ServiceMarker;
}

impl Service {
    pub(crate) fn new(handle: SC_HANDLE, reactor: Arc<Reactor>, name: String) -> Self {
        Self {
            handle: Some(handle),
            reactor,
            name,
        }
    }

    pub(crate) fn handle(&self) -> SC_HANDLE {
        self.handle.expect("service handle used after close")
    }

    /// Closes the handle on the blocking pool and awaits completion, then
    /// disarms [`Drop`] so it becomes a no-op afterward.
    ///
    /// Used for explicit [`crate::wire::WinScmRequest::CloseService`]
    /// handling, where the caller wants to know the close has actually
    /// completed before the request returns — unlike [`Drop`]'s
    /// fire-and-forget close, which exists only to catch handles dropped
    /// through some other path (e.g. session teardown).
    pub(crate) async fn close(mut self) -> Result<(), Error> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        let handle = SendHandle(handle);
        tokio::task::spawn_blocking(move || {
            // SAFETY: `handle` is a live handle owned by this `Service`;
            // taking it above ensures `Drop` won't also try to close it.
            unsafe {
                CloseServiceHandle(handle.into_inner());
            }
        })
        .await
        .map_err(|_| Error::new(ErrorKind::Other, "SCM service close task panicked"))
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
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
            let handle = SendHandle(handle);
            drop(tokio::task::spawn_blocking(move || {
                // SAFETY: see the comment on the `SendHandle` field above.
                unsafe {
                    CloseServiceHandle(handle.into_inner());
                }
            }));
        } else {
            // SAFETY: `handle` is a live handle owned by this value; nothing
            // else can close it first since `Service` isn't `Clone`.
            unsafe {
                CloseServiceHandle(handle);
            }
        }
    }
}
