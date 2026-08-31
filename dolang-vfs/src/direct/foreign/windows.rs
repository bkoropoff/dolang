//! Windows process backend.
//!
//! A live process handle is the identity: Windows will not recycle a PID while
//! any handle to it remains open, so every operation below refers to the
//! process that was opened, without a re-check.
//!
//! Handles are opened with `MAXIMUM_ALLOWED` rather than a caller-chosen access
//! mask. The rights actually needed differ per operation — terminating needs
//! `PROCESS_TERMINATE`, which has to be present from the *open* because
//! reopening later would reintroduce the reuse race the handle exists to close
//! — and an explicit mask would have to name the union up front, denying a
//! handle to a caller who only wanted to read. `MAXIMUM_ALLOWED` asks for
//! whatever the caller is entitled to instead, which also degrades correctly on
//! protected processes, where `PROCESS_QUERY_LIMITED_INFORMATION` is granted
//! but `PROCESS_QUERY_INFORMATION` is not. The cost is that a missing right
//! surfaces at use time rather than at open time.

use std::{
    collections::VecDeque,
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Mutex,
};

use tokio::sync::oneshot;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
            QueryFullProcessImageNameW, RegisterWaitForSingleObject, TerminateProcess,
            UnregisterWaitEx, WT_EXECUTEONLYONCE,
        },
    },
};

use typed_path::Utf8TypedPath;

use crate::{
    error::{Error, ErrorKind, Result},
    path::typed_path,
    process::{ProcessExit, ProcessFamily, ProcessInfo, Signal, StartTime},
    protocol::WirePath,
    security::WindowsTokenInfo,
};

use super::{Candidate, Process, Processes, gone, recycled};

/// `MAXIMUM_ALLOWED`, which `windows-sys` does not expose as a
/// `PROCESS_ACCESS_RIGHTS` constant because it is not one: it is a generic
/// access request understood by every securable object.
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;

/// What a Toolhelp snapshot already knows about one process.
///
/// Unlike Linux, the enumeration call returns real data rather than a directory
/// of names, so discarding it and re-reading per entry would cost an
/// `OpenProcess` for information already in hand.
pub(super) struct Listed {
    pid: u32,
    ppid: Option<u32>,
    name: String,
}

/// Turns a `FILETIME` into a start time.
///
/// 100-nanosecond intervals since 1601, kept in those units: this is compared
/// for equality only, and converting would lose precision for nothing.
fn start_time(time: FILETIME) -> StartTime {
    StartTime((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime))
}

/// Opens a process handle, mapping "no such process" onto a not-found.
///
/// `OpenProcess` reports an absent PID as `ERROR_INVALID_PARAMETER`, which is
/// indistinguishable from a genuinely malformed call at the API level but never
/// arises from one here — the only parameter that varies is the PID.
fn open_handle(pid: u32) -> Result<OwnedHandle> {
    // SAFETY: `OpenProcess` takes scalars and returns a new handle or null.
    let handle = unsafe { OpenProcess(MAXIMUM_ALLOWED, 0, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER) => Err(gone(pid)),
            _ => Err(error.into()),
        };
    }
    // SAFETY: the handle is fresh and owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Reads a process's creation time, and with it proves the handle carries query
/// access.
///
/// Because `MAXIMUM_ALLOWED` does not say what it granted, something has to
/// establish that the handle is useful at all; this is that check, and the
/// identity check needs the value anyway.
fn creation_time(handle: HANDLE) -> io::Result<StartTime> {
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    // SAFETY: all four out-parameters are live, correctly typed, and distinct.
    if unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(start_time(creation))
}

/// Reads a process's image path.
///
/// The Win32 form rather than `PROCESS_NAME_NATIVE`: both are answerable with
/// `PROCESS_QUERY_LIMITED_INFORMATION`, but the native form names the device
/// (`\\Device\\HarddiskVolume3\\...`), which no caller can compare against a
/// path they hold.
fn image_path(handle: HANDLE) -> Option<WirePath> {
    let mut buffer = vec![0u16; 32768];
    let mut len = buffer.len() as u32;
    // SAFETY: `buffer` is `len` u16s long and `len` is updated in place.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    };
    if ok == 0 {
        return None;
    }
    buffer.truncate(len as usize);
    let path = String::from_utf16(&buffer).ok()?;
    typed_path(path.into()).ok().map(Into::into)
}

/// Builds a snapshot from a handle plus whatever enumeration already knew.
///
/// `cmdline` and `cwd` are always absent: Windows has no documented interface
/// for reading another process's command line or working directory, and the
/// undocumented `PEB` walk is not something to rely on.
fn describe(
    session: Uuid,
    handle: HANDLE,
    pid: u32,
    ppid: Option<u32>,
    name: String,
    exe: Option<WirePath>,
    token: Option<WindowsTokenInfo>,
) -> Result<ProcessInfo> {
    Ok(ProcessInfo {
        session,
        pid,
        ppid,
        name,
        start: creation_time(handle)?,
        exe,
        cmdline: None,
        cwd: None,
        family: ProcessFamily::Windows(token),
    })
}

impl Processes {
    /// Takes a Toolhelp snapshot of the process table.
    ///
    /// One call returns the whole table, already consistent, so there is
    /// nothing to gain from re-querying per page.
    pub(super) async fn impl_scan() -> Result<Vec<Candidate>> {
        tokio::task::spawn_blocking(|| {
            // SAFETY: returns a handle or `INVALID_HANDLE_VALUE`.
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(Error::from(io::Error::last_os_error()));
            }
            // SAFETY: a snapshot handle is owned and closed exactly once below.
            let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };

            let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
            entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
            let raw = snapshot.as_raw_handle();
            // SAFETY: `entry` is sized as the API requires.
            if unsafe { Process32FirstW(raw, &mut entry) } == 0 {
                return Err(io::Error::last_os_error().into());
            }

            let mut listed = Vec::new();
            loop {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                listed.push(Listed {
                    pid: entry.th32ProcessID,
                    // The idle process reports itself as its own parent, which
                    // is not a parentage anyone can follow.
                    ppid: (entry.th32ParentProcessID != entry.th32ProcessID)
                        .then_some(entry.th32ParentProcessID),
                    name: String::from_utf16_lossy(&entry.szExeFile[..name_len]),
                });
                // SAFETY: as for `Process32FirstW`.
                if unsafe { Process32NextW(raw, &mut entry) } == 0 {
                    break;
                }
            }
            Ok(listed)
        })
        .await
        .map_err(Error::other)?
    }

    /// Fills in what the snapshot could not carry.
    ///
    /// Deliberately does **not** read the token: that would cost an
    /// `OpenProcessToken` per entry and be denied for most of the table to a
    /// non-elevated caller. [`Process::info`](crate::process::Process::info)
    /// fills it in, having already paid for a handle.
    ///
    /// A process without an openable handle is still reported, with only what
    /// the snapshot knew — this is the common case for system processes, and
    /// dropping them would make the table look emptier than it is. Such a
    /// record carries no start time, so opening it cannot confirm identity;
    /// in practice the open fails for the same reason this one did.
    pub(super) async fn impl_describe(
        session: Uuid,
        batch: Vec<Candidate>,
    ) -> Result<VecDeque<ProcessInfo>> {
        tokio::task::spawn_blocking(move || {
            Ok(batch
                .into_iter()
                .map(|listed| {
                    let Listed { pid, ppid, name } = listed;
                    let opened = open_handle(pid).ok().and_then(|handle| {
                        let raw = handle.as_raw_handle();
                        describe(session, raw, pid, ppid, name.clone(), image_path(raw), None).ok()
                    });
                    opened.unwrap_or(ProcessInfo {
                        session,
                        pid,
                        ppid,
                        name,
                        start: StartTime(0),
                        exe: None,
                        cmdline: None,
                        cwd: None,
                        family: ProcessFamily::Windows(None),
                    })
                })
                .collect())
        })
        .await
        .map_err(Error::other)?
    }
}

impl Process {
    pub(super) async fn impl_open(
        session: Uuid,
        pid: u32,
        start: Option<StartTime>,
    ) -> Result<Self> {
        tokio::task::spawn_blocking(move || {
            let handle = open_handle(pid)?;
            // The handle already blocks PID reuse, so this compares against a
            // process that cannot change underneath it. It doubles as the proof
            // that `MAXIMUM_ALLOWED` granted query access at all.
            let observed = creation_time(handle.as_raw_handle())?;
            if let Some(expected) = start
                && observed != expected
            {
                return Err(recycled(pid));
            }
            Ok(Self {
                pid,
                session,
                handle,
            })
        })
        .await
        .map_err(Error::other)?
    }

    pub(super) async fn impl_info(&self) -> Result<ProcessInfo> {
        let (pid, session) = (self.pid, self.session);
        let handle = self.handle.try_clone()?;
        tokio::task::spawn_blocking(move || {
            let raw = handle.as_raw_handle();
            let exe = image_path(raw);
            // The snapshot's `szExeFile` is not available here, so the name is
            // recovered from the image path — the same string Toolhelp reports.
            let name = exe
                .as_ref()
                .and_then(|path| Utf8TypedPath::from(path).file_name().map(ToOwned::to_owned))
                .unwrap_or_default();
            // Best-effort: reading the token needs rights `MAXIMUM_ALLOWED` may
            // not have granted, and a denial is not a reason to fail the call.
            let token = unsafe { WindowsTokenInfo::from_process_handle(raw) }.ok();
            describe(session, raw, pid, None, name, exe, token)
        })
        .await
        .map_err(Error::other)?
    }

    pub(super) async fn impl_signal(&self, _signal: Signal) -> Result<()> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "signals are not supported on Windows",
        ))
    }

    /// Terminates the process outright.
    ///
    /// Deliberately unlike `Child::impl_terminate`, which asks first: that path
    /// works only because children are spawned with `CREATE_NEW_PROCESS_GROUP`,
    /// so `pid == pgid` holds by construction. For a foreign PID there is no
    /// way to ask whether it heads a group, and `GenerateConsoleCtrlEvent`
    /// reaches only the caller's own console — which a service host does not
    /// have. So there is no graceful half to offer here.
    pub(super) async fn impl_terminate(&self) -> Result<()> {
        // SAFETY: a live handle; the exit code is an ordinary scalar.
        if unsafe { TerminateProcess(self.handle.as_raw_handle(), 1) } == 0 {
            let error = io::Error::last_os_error();
            // Terminating something that already exited is the outcome the
            // caller asked for, not a failure.
            if error.raw_os_error().map(|code| code as u32) == Some(ERROR_ACCESS_DENIED)
                && self.exit_code().is_some()
            {
                return Ok(());
            }
            return Err(error.into());
        }
        Ok(())
    }

    /// Kills the process.
    ///
    /// Identical to [`impl_terminate`](Self::impl_terminate): `TerminateProcess`
    /// is already the unconditional form, and Windows has no second, harder
    /// stop to escalate to the way `SIGKILL` escalates past `SIGTERM`.
    pub(super) async fn impl_kill(&self) -> Result<()> {
        self.impl_terminate().await
    }

    /// Returns the exit code if the process has already exited.
    fn exit_code(&self) -> Option<i32> {
        // `STILL_ACTIVE` (259) is indistinguishable from a process that exited
        // with 259, which is why this is only consulted after a wait has
        // reported the process gone, or alongside a failure that implies it.
        const STILL_ACTIVE: u32 = 259;
        let mut code = 0u32;
        // SAFETY: a live handle and a live out-parameter.
        if unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } == 0 {
            return None;
        }
        (code != STILL_ACTIVE).then_some(code as i32)
    }

    /// Waits for the process to exit.
    ///
    /// A thread-pool wait rather than a blocking one: waits can be numerous and
    /// long-lived, and `spawn_blocking` would pin an OS thread each.
    pub(super) async fn impl_wait(&self) -> Result<ProcessExit> {
        let (sender, receiver) = oneshot::channel();
        let registration = WaitRegistration::new(self.handle.as_raw_handle(), sender)?;
        let _ = receiver.await;
        // Unregistering before reading is what makes the read safe: it blocks
        // until the callback has finished, so nothing is still touching the
        // sender when the registration is torn down.
        drop(registration);
        Ok(ProcessExit {
            code: self.exit_code(),
        })
    }

    pub(super) async fn impl_close(self) -> Result<()> {
        drop(self.handle);
        Ok(())
    }
}

/// A registered thread-pool wait, unregistered on drop.
struct WaitRegistration {
    wait: HANDLE,
    /// Never read directly: it exists so the address handed to the callback
    /// stays valid until [`Drop`] has unregistered the wait.
    _sender: Box<Mutex<Option<oneshot::Sender<()>>>>,
}

// SAFETY: the wait handle is only ever passed back to `UnregisterWaitEx`, and
// the sender is behind a mutex.
unsafe impl Send for WaitRegistration {}
unsafe impl Sync for WaitRegistration {}

impl WaitRegistration {
    fn new(handle: HANDLE, sender: oneshot::Sender<()>) -> Result<Self> {
        let sender = Box::new(Mutex::new(Some(sender)));
        let context = (&*sender as *const Mutex<Option<oneshot::Sender<()>>>).cast_mut();
        let mut wait = ptr::null_mut();
        // `WT_EXECUTEONLYONCE` is mandatory, not an optimization: a process
        // handle stays signaled forever once the process exits, so without it
        // the callback would re-fire against a consumed channel indefinitely.
        //
        // SAFETY: `context` points into `sender`, which this struct owns and
        // outlives the registration, since `Drop` unregisters first.
        let ok = unsafe {
            RegisterWaitForSingleObject(
                &mut wait,
                handle,
                Some(wait_callback),
                context.cast(),
                u32::MAX,
                WT_EXECUTEONLYONCE,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(Self {
            wait,
            _sender: sender,
        })
    }
}

impl Drop for WaitRegistration {
    fn drop(&mut self) {
        // `INVALID_HANDLE_VALUE` means "block until any running callback has
        // finished", which is what makes dropping the sender afterwards safe.
        // It must never be called from inside the callback itself — that
        // self-deadlocks — and this type is only ever dropped by the waiter.
        //
        // SAFETY: `self.wait` came from a successful registration and is
        // unregistered exactly once.
        unsafe {
            UnregisterWaitEx(self.wait, INVALID_HANDLE_VALUE);
        }
    }
}

/// Fired by the thread pool when the process handle becomes signaled.
unsafe extern "system" fn wait_callback(context: *mut std::ffi::c_void, _timed_out: bool) {
    let sender = unsafe { &*context.cast::<Mutex<Option<oneshot::Sender<()>>>>() };
    if let Ok(mut sender) = sender.lock()
        && let Some(sender) = sender.take()
    {
        let _ = sender.send(());
    }
}
