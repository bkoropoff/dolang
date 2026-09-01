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
    ffi::c_void,
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr, slice,
    sync::{Mutex, OnceLock},
};

use tokio::sync::oneshot;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
        UNICODE_STRING, WAIT_OBJECT_0,
    },
    System::{
        Diagnostics::Debug::ReadProcessMemory,
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
        Threading::{
            GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_BASIC_INFORMATION,
            PROCESS_NAME_WIN32, QueryFullProcessImageNameW, RegisterWaitForSingleObject,
            TerminateProcess, UnregisterWaitEx, WT_EXECUTEONLYONCE, WaitForSingleObject,
        },
    },
};
use windows_sys::core::w;

use dolang_winterop::process::split_arguments;
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

/// `ProcessCommandLineInformation`, which `PROCESSINFOCLASS` does not name.
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

/// `STATUS_INFO_LENGTH_MISMATCH`, the "your buffer is too small, here is the
/// size" answer every `NtQueryInformationProcess` class gives.
const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

type NtQueryInformationProcess =
    unsafe extern "system" fn(HANDLE, u32, *mut c_void, u32, *mut u32) -> i32;

/// Resolves `NtQueryInformationProcess`, once per process.
///
/// `ntdll` has no import library in `windows-sys` and is always already mapped,
/// so this is a lookup in a module that cannot fail to be present — but the
/// *symbol* can be absent under an `ntdll` that does not implement it, which is
/// why the result is an `Option` rather than an unwrap.
fn nt_query_information_process() -> Option<NtQueryInformationProcess> {
    static RESOLVED: OnceLock<Option<NtQueryInformationProcess>> = OnceLock::new();
    *RESOLVED.get_or_init(|| {
        const NAME: &[u8] = b"NtQueryInformationProcess\0";
        // SAFETY: `ntdll` is mapped into every process; the name is NUL-terminated.
        let ntdll = unsafe { GetModuleHandleW(w!("ntdll.dll")) };
        if ntdll.is_null() {
            return None;
        }
        let symbol = unsafe { GetProcAddress(ntdll, NAME.as_ptr()) }?;
        // SAFETY: this is the documented signature of the named export, and the
        // signature is what every caller of it in the wild assumes.
        Some(unsafe {
            mem::transmute::<unsafe extern "system" fn() -> isize, NtQueryInformationProcess>(
                symbol,
            )
        })
    })
}

/// What a live handle can tell about a process beyond the snapshot fields.
#[derive(Default)]
struct Parameters {
    cmdline: Option<String>,
    cwd: Option<String>,
    ppid: Option<u32>,
}

/// Reads everything a handle affords that the enumeration call does not.
///
/// The command line and working directory both live in the target's own memory
/// and Windows offers no supported way to read another process's copy, so every
/// route to them here is an undocumented one. Individually absent, rather than
/// an error, when a route does not work: these are fields of a record worth
/// returning without them.
///
/// The command line is read through the kernel where that is possible, since
/// [`queried_command_line`] cannot observe a half-written `PEB` the way the
/// walk can. There is no equivalent for the working directory: no information
/// class reaches it, because unlike the command line it is not something the
/// kernel knows. NT has no per-process current directory at all — native calls
/// take an absolute path or a directory handle — so what `SetCurrentDirectory`
/// maintains is a Win32 convention living in the process's own bookkeeping,
/// and reading it is as accurate as that bookkeeping happens to be.
fn parameters(handle: HANDLE) -> Parameters {
    let basic = basic_information(handle);
    let cmdline = queried_command_line(handle);
    let mut walked = peb_parameters(handle, basic).unwrap_or_default();
    if cmdline.is_some() {
        walked.cmdline = cmdline;
    }
    // Read from the basic information rather than the walk, so that a handle
    // without `PROCESS_VM_READ` still reports a parent.
    walked.ppid = basic.and_then(|basic| {
        u32::try_from(basic.InheritedFromUniqueProcessId)
            .ok()
            .filter(|ppid| *ppid != 0)
    });
    walked
}

/// Reads a process's `PROCESS_BASIC_INFORMATION`.
///
/// Undocumented in the same sense as the other information classes used here:
/// absent from the public `PROCESSINFOCLASS`, stable in practice for as long as
/// the API has existed. Needs only query access.
fn basic_information(handle: HANDLE) -> Option<PROCESS_BASIC_INFORMATION> {
    const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;

    let query = nt_query_information_process()?;
    let mut basic = PROCESS_BASIC_INFORMATION::default();
    let mut len = 0u32;
    // SAFETY: the out-parameter is a live, correctly sized structure.
    let status = unsafe {
        query(
            handle,
            PROCESS_BASIC_INFORMATION_CLASS,
            (&raw mut basic).cast(),
            mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut len,
        )
    };
    (status >= 0).then_some(basic)
}

/// Asks the kernel for the command line directly.
///
/// `ProcessCommandLineInformation` has existed since Windows 8.1: the kernel
/// walks the target's `PEB` in its own address space and copies the result back
/// as a `UNICODE_STRING`, atomically, needing only the
/// `PROCESS_QUERY_LIMITED_INFORMATION` that `MAXIMUM_ALLOWED` grants for nearly
/// anything. Undocumented in that the information class is not in the public
/// header, not in that it is unstable — it is what every process viewer on the
/// platform uses.
///
/// Wine does not implement the class, answering `STATUS_INVALID_INFO_CLASS`.
fn queried_command_line(handle: HANDLE) -> Option<String> {
    let query = nt_query_information_process()?;

    let mut len = 0u32;
    // SAFETY: a null buffer of length zero is how the required size is asked
    // for; `len` is the only thing written.
    let status = unsafe {
        query(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            ptr::null_mut(),
            0,
            &mut len,
        )
    };
    if status != STATUS_INFO_LENGTH_MISMATCH || len == 0 {
        return None;
    }

    // `u64` rather than `u8` for the alignment: the buffer's first bytes are a
    // `UNICODE_STRING`, which contains a pointer.
    let mut buffer = vec![0u64; (len as usize).div_ceil(mem::size_of::<u64>())];
    // SAFETY: the buffer is at least `len` bytes and correctly aligned.
    let status = unsafe {
        query(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            len,
            &mut len,
        )
    };
    if status < 0 {
        return None;
    }

    // SAFETY: a successful call wrote a `UNICODE_STRING` at the start of the
    // buffer whose `Buffer` points at the characters that follow it, in this
    // same allocation.
    let string = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    if string.Buffer.is_null() {
        return None;
    }
    let chars = unsafe {
        slice::from_raw_parts(
            string.Buffer,
            string.Length as usize / mem::size_of::<u16>(),
        )
    };
    // A zero-length answer is no command line rather than an empty one: every
    // process has at least its own image name there, so nothing is a failure to
    // read it. Reporting that as absent also keeps this route and the walk,
    // which returns nothing outright, from disagreeing about the same process.
    String::from_utf16(chars)
        .ok()
        .filter(|line| !line.is_empty())
}

/// Field offsets within a process's `PEB` and its `RTL_USER_PROCESS_PARAMETERS`.
///
/// Spelled out rather than taken from `windows-sys`, which declares the fields
/// ahead of `CommandLine` as opaque padding and stops before the current
/// directory — and which describes only the layout of the process doing the
/// reading, when half the point here is to read the other one.
///
/// These are the same on x64 and on ARM64: the layout follows the pointer
/// width, not the architecture.
#[derive(Clone, Copy)]
struct Layout {
    /// Width of a pointer in the target, in bytes.
    pointer: u64,
    /// `PEB.ProcessParameters`.
    process_parameters: u64,
    /// `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory.DosPath`.
    current_directory: u64,
    /// `RTL_USER_PROCESS_PARAMETERS.CommandLine`.
    command_line: u64,
}

impl Layout {
    /// The layout of a process of the same bitness as this one.
    const NATIVE: Self = Self {
        pointer: 8,
        process_parameters: 0x20,
        current_directory: 0x38,
        command_line: 0x70,
    };

    /// The layout of a 32-bit process seen from a 64-bit one.
    const WOW64: Self = Self {
        pointer: 4,
        process_parameters: 0x10,
        current_directory: 0x24,
        command_line: 0x40,
    };
}

/// Reads one plain-data value out of another process.
fn read_remote<T>(handle: HANDLE, address: u64) -> Option<T> {
    let mut value = mem::MaybeUninit::<T>::uninit();
    let mut read = 0usize;
    // SAFETY: the destination is a live allocation of exactly `size_of::<T>()`
    // bytes; a short or failed read leaves it untouched and unread.
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            address as *const c_void,
            value.as_mut_ptr().cast(),
            mem::size_of::<T>(),
            &mut read,
        )
    };
    // SAFETY: a full-length successful read initialized every byte, and `T` is
    // only ever a plain-data structure here.
    (ok != 0 && read == mem::size_of::<T>()).then(|| unsafe { value.assume_init() })
}

/// Reads a pointer out of another process, in that process's width.
fn read_pointer(handle: HANDLE, address: u64, layout: Layout) -> Option<u64> {
    match layout.pointer {
        4 => read_remote::<u32>(handle, address).map(u64::from),
        _ => read_remote::<u64>(handle, address),
    }
}

/// Reads a `UNICODE_STRING` out of another process, and then its characters.
///
/// `Length` is at offset zero either way; the buffer pointer follows the
/// `MaximumLength` that pads out to the target's pointer alignment.
fn read_remote_string(handle: HANDLE, address: u64, layout: Layout) -> Option<String> {
    let length: u16 = read_remote(handle, address)?;
    let buffer = read_pointer(handle, address + layout.pointer, layout)?;
    if length == 0 || buffer == 0 {
        return None;
    }

    let mut chars = vec![0u16; length as usize / mem::size_of::<u16>()];
    let mut read = 0usize;
    // SAFETY: the destination holds exactly `length` bytes.
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            buffer as *const c_void,
            chars.as_mut_ptr().cast(),
            length as usize,
            &mut read,
        )
    };
    if ok == 0 || read != length as usize {
        return None;
    }
    String::from_utf16(&chars).ok()
}

/// Walks the target's `PEB` for its command line and working directory.
///
/// Needs `PROCESS_VM_READ` on top of query access, and — being several reads of
/// memory the target is free to rewrite — can catch a field mid-update.
/// `ProcessBasicInformation` and `ProcessWow64Information` are undocumented in
/// the same sense as the other class: absent from the public `PROCESSINFOCLASS`,
/// stable in practice for as long as the API has existed.
///
/// A 32-bit target is read through its 32-bit `PEB`, not the native one that
/// `ProcessBasicInformation` reports. A WOW64 process has both, and the 32-bit
/// half is the one it maintains itself: its own `SetCurrentDirectory` writes
/// there, leaving the native copy holding whatever the directory was at
/// startup. The command line is not rewritten by anyone in practice, so it
/// agrees either way, but the working directory does not.
///
/// The reverse mismatch — a 32-bit reader against a 64-bit target, which needs
/// `NtWow64ReadVirtualMemory64` — cannot arise, as nothing here is built 32-bit.
fn peb_parameters(handle: HANDLE, basic: Option<PROCESS_BASIC_INFORMATION>) -> Option<Parameters> {
    const PROCESS_WOW64_INFORMATION: u32 = 26;

    let query = nt_query_information_process()?;

    // A non-zero answer is the address of the target's 32-bit `PEB`, and by
    // implication the statement that it has one.
    let mut wow64_peb = 0u64;
    let mut len = 0u32;
    // SAFETY: the out-parameter is a live, pointer-sized integer.
    let status = unsafe {
        query(
            handle,
            PROCESS_WOW64_INFORMATION,
            (&raw mut wow64_peb).cast(),
            mem::size_of::<u64>() as u32,
            &mut len,
        )
    };
    let (peb, layout) = if status >= 0 && wow64_peb != 0 {
        (wow64_peb, Layout::WOW64)
    } else {
        let peb = basic?.PebBaseAddress;
        if peb.is_null() {
            return None;
        }
        (peb as u64, Layout::NATIVE)
    };

    let params = read_pointer(handle, peb + layout.process_parameters, layout)?;
    if params == 0 {
        return None;
    }
    Some(Parameters {
        cmdline: read_remote_string(handle, params + layout.command_line, layout),
        cwd: read_remote_string(handle, params + layout.current_directory, layout),
        // Filled in by the caller, which reads it without needing the walk.
        ppid: None,
    })
}

/// Turns a `PEB` working directory into a path.
///
/// Windows stores it with a trailing separator, which no other path this VFS
/// produces carries. A root keeps one, since `C:` alone means the drive's
/// current directory rather than its root.
fn cwd_path(dos_path: String) -> Option<WirePath> {
    let trimmed = dos_path.trim_end_matches('\\');
    let path = if trimmed.ends_with(':') || trimmed.is_empty() {
        dos_path.as_str()
    } else {
        trimmed
    };
    typed_path(path.into()).ok().map(Into::into)
}

/// Reports how the process ended, if it has.
///
/// A zero-timeout wait rather than `GetExitCodeProcess` alone: a process handle
/// signals on exit, and testing that is what separates a process still running
/// from one that exited with `STILL_ACTIVE` (259), which the exit code by
/// itself cannot.
fn exit_status(handle: HANDLE) -> Option<ProcessExit> {
    // SAFETY: a live handle; a zero timeout makes this a poll.
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
        return None;
    }
    let mut code = 0u32;
    // SAFETY: a live handle and a live out-parameter.
    if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
        return Some(ProcessExit { code: None });
    }
    Some(ProcessExit {
        code: Some(code as i32),
    })
}

/// Builds a snapshot from a handle plus whatever enumeration already knew.
fn describe(
    session: Uuid,
    handle: HANDLE,
    pid: u32,
    ppid: Option<u32>,
    name: String,
    exe: Option<WirePath>,
    token: Option<WindowsTokenInfo>,
) -> Result<ProcessInfo> {
    let Parameters {
        cmdline,
        cwd,
        ppid: inherited,
    } = parameters(handle);
    Ok(ProcessInfo {
        session,
        // The Toolhelp snapshot has this during enumeration; a handle opened
        // by PID does not, and reads it from the process itself.
        ppid: ppid.or(inherited),
        pid,
        name,
        start: creation_time(handle)?,
        exe,
        // Windows stores one string and leaves splitting it to the process, so
        // the vector is a reconstruction by the convention the C runtime uses.
        // The original is kept alongside it rather than thrown away.
        cmdline: cmdline.as_deref().map(split_arguments),
        cwd: cwd.and_then(cwd_path),
        family: ProcessFamily::Windows { token, cmdline },
        // An exited process stays addressable for as long as a handle to it is
        // open, so a record can outlive its process — and then everything read
        // out of its address space above is legitimately absent.
        exit: exit_status(handle),
    })
}

/// The record a process that could not be opened is reported as.
///
/// Everything the snapshot itself carried, and nothing else: the rest is
/// behind a handle. The absent start time is the visible consequence — such a
/// record cannot be used to confirm identity later, and in practice the open
/// that would do so fails for the same reason this one did.
fn snapshot_only(session: Uuid, listed: Listed) -> ProcessInfo {
    let Listed { pid, ppid, name } = listed;
    ProcessInfo {
        session,
        pid,
        ppid,
        name,
        start: StartTime(0),
        exe: None,
        cmdline: None,
        cwd: None,
        family: ProcessFamily::Windows {
            token: None,
            cmdline: None,
        },
        // Reporting this needs the handle that could not be opened; the
        // snapshot alone does not say.
        exit: None,
    }
}

impl Processes {
    /// Takes a Toolhelp snapshot of the process table.
    ///
    /// One call returns the whole table, already consistent, so there is
    /// nothing to gain from re-querying per page.
    fn snapshot() -> Result<Vec<Listed>> {
        {
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
        }
    }

    pub(super) async fn impl_scan() -> Result<Vec<Candidate>> {
        tokio::task::spawn_blocking(Self::snapshot)
            .await
            .map_err(Error::other)?
    }

    /// Describes one process by PID.
    ///
    /// The snapshot is of the whole table either way — Toolhelp has no
    /// per-process query, and neither does the `NtQuerySystemInformation` route
    /// — but it is one call with no per-entry work behind it, which is what
    /// separates this from enumerating and filtering.
    pub(super) async fn impl_describe_one(session: Uuid, pid: u32) -> Result<ProcessInfo> {
        tokio::task::spawn_blocking(move || {
            let listed = Self::snapshot()?
                .into_iter()
                .find(|listed| listed.pid == pid)
                .ok_or_else(|| gone(pid))?;
            let Some(handle) = open_handle(pid).ok() else {
                return Ok(snapshot_only(session, listed));
            };
            let raw = handle.as_raw_handle();
            let token = unsafe { WindowsTokenInfo::from_process_handle(raw) }.ok();
            let Listed { pid, ppid, name } = listed;
            describe(session, raw, pid, ppid, name, image_path(raw), token)
        })
        .await
        .map_err(Error::other)?
    }

    /// Fills in what the snapshot could not carry.
    ///
    /// The token is read here like everything else. It is not the expense it
    /// looks like: reaching the start time and the PEB has already cost a
    /// process open, and on a handle already held the token is an
    /// `OpenProcessToken` and a handful of `GetTokenInformation` calls, none
    /// of which resolve a SID to a name. What it does cost is bytes on the
    /// wire, which the page budget already accounts for.
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
                    let Listed { pid, ppid, name } = &listed;
                    let opened = open_handle(*pid).ok().and_then(|handle| {
                        let raw = handle.as_raw_handle();
                        let (pid, ppid, name) = (*pid, *ppid, name.clone());
                        // Best-effort: `MAXIMUM_ALLOWED` may not have granted
                        // the rights this needs, and a denial is no reason to
                        // drop the rest of the record.
                        let token = unsafe { WindowsTokenInfo::from_process_handle(raw) }.ok();
                        describe(session, raw, pid, ppid, name, image_path(raw), token).ok()
                    });
                    opened.unwrap_or_else(|| snapshot_only(session, listed))
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
