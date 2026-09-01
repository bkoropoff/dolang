//! Inspection and control of processes this VFS did not spawn.
//!
//! Where [`Child`](super::Child) addresses a process by parentage, everything
//! here addresses one that already exists. The distinction is not cosmetic: a
//! parent has a reaping relationship with its child that nothing here can
//! reproduce, so [`ProcessExit`] carries strictly less than
//! [`ProcessStatus`](super::ProcessStatus).
//!
//! A [`Process`] holds a kernel handle rather than a PID. A bare PID makes
//! every operation check-then-act — enumerate, then signal, and in between the
//! PID may have been recycled onto something else — so [`ProcessInfo`] carries
//! a [`StartTime`] that [`Vfs::open_process_info`](crate::Vfs::open_process_info)
//! compares after opening. A recycled PID necessarily has a later start time,
//! so a match proves the handle refers to the intended process.

use std::fmt;

use serde::{Deserialize, Serialize};
use typed_path::Utf8TypedPath;
use uuid::Uuid;

use crate::{
    client, direct,
    error::{Error, ErrorKind, Result},
    protocol::WirePath,
    security::{UnixSecurityInfo, WindowsTokenInfo},
};

/// When a process started, in whatever units its platform reports.
///
/// Deliberately opaque, and compared only for equality: it exists to
/// distinguish a process from a later one that inherited its PID, and
/// normalizing it to a wall-clock time would cost precision on every platform
/// for no gain. Linux reports clock ticks since boot, the BSDs and macOS a
/// microsecond timestamp, and Windows 100-nanosecond intervals since 1601.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StartTime(pub(crate) u64);

/// A snapshot of one process, as of when it was taken.
///
/// Every field but `pid`, `name`, and `start` is optional, because no field
/// beyond those is available on every platform, for every target process, to
/// every caller. See the accessors for what limits each one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub(crate) session: Uuid,
    pub(crate) pid: u32,
    pub(crate) ppid: Option<u32>,
    pub(crate) name: String,
    pub(crate) start: StartTime,
    pub(crate) exe: Option<WirePath>,
    pub(crate) cmdline: Option<Vec<String>>,
    pub(crate) cwd: Option<WirePath>,
    pub(crate) family: ProcessFamily,
}

/// The platform-specific half of a [`ProcessInfo`].
///
/// Which variant a record carries follows from the target it was captured on,
/// so the enum answers "does this platform have such a thing at all" once, and
/// the `Option` inside each variant is left to mean only "not obtained for this
/// process".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ProcessFamily {
    /// Unix credentials, absent if they could not be read.
    Unix(Option<UnixSecurityInfo>),
    /// Windows access token information, absent if it was not read.
    Windows(Option<WindowsTokenInfo>),
}

impl ProcessInfo {
    /// Returns the process ID.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Returns the parent process ID.
    pub fn parent_pid(&self) -> Option<u32> {
        self.ppid
    }

    /// Returns the process name.
    ///
    /// The kernel's short name for the process, not its executable path: Linux
    /// and the BSDs truncate it, and it reflects whatever the process last set
    /// rather than what it was launched as.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns when the process started.
    pub fn start_time(&self) -> StartTime {
        self.start
    }

    /// Returns the path to the process executable.
    pub fn exe(&self) -> Option<Utf8TypedPath<'_>> {
        self.exe.as_ref().map(Into::into)
    }

    /// Returns the process command line.
    ///
    /// Best-effort. macOS restricts it to processes owned by the same user,
    /// and Windows has no documented interface for reading another process's
    /// command line at all.
    pub fn command_line(&self) -> Option<&[String]> {
        self.cmdline.as_deref()
    }

    /// Returns the process working directory.
    ///
    /// Available on Linux and macOS only. FreeBSD exposes it through
    /// `libprocstat`, and Windows not at all.
    pub fn cwd(&self) -> Option<Utf8TypedPath<'_>> {
        self.cwd.as_ref().map(Into::into)
    }

    /// Returns the process's Unix credentials.
    ///
    /// On macOS the supplementary group list is the kernel credential list,
    /// capped at `NGROUPS` (16). That is narrower than what
    /// [`SecurityInfo::current`](crate::security::SecurityInfo::current)
    /// reports for this process, which resolves extended memberships through
    /// opendirectoryd — an interface that answers only for the caller — so a
    /// foreign macOS group list can be a truncated view where the
    /// current-process one is not.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Unsupported`](crate::error::ErrorKind::Unsupported) if the
    /// record came from a Windows target, which has no such credentials to
    /// report.
    pub fn identity(&self) -> Result<Option<&UnixSecurityInfo>> {
        match &self.family {
            ProcessFamily::Unix(identity) => Ok(identity.as_ref()),
            ProcessFamily::Windows(_) => Err(Error::new(
                ErrorKind::Unsupported,
                "Windows processes have no Unix credentials",
            )),
        }
    }

    /// Returns the process's access token information.
    ///
    /// `None` for a record produced by
    /// [`Vfs::processes`](crate::Vfs::processes): reading it costs a process
    /// open and a token open per entry, and is denied for most of the table to
    /// an unelevated caller. Fetch it through [`Process::info`], which already
    /// holds a handle.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Unsupported`](crate::error::ErrorKind::Unsupported) if the
    /// record came from a Unix target, which has no access tokens.
    pub fn token(&self) -> Result<Option<&WindowsTokenInfo>> {
        match &self.family {
            ProcessFamily::Windows(token) => Ok(token.as_ref()),
            ProcessFamily::Unix(_) => Err(Error::new(
                ErrorKind::Unsupported,
                "Unix processes have no access token",
            )),
        }
    }

    /// Returns the identity of the target session this was captured from.
    pub fn session(&self) -> Uuid {
        self.session
    }
}

/// How a process this VFS did not spawn ended.
///
/// Weaker than [`ProcessStatus`](super::ProcessStatus), which describes a
/// child. Only Windows can report an exit code for an arbitrary process;
/// `waitid(P_PIDFD)` and `EVFILT_PROC`'s `NOTE_EXITSTATUS` are both restricted
/// to the parent, so a Unix target can report only that the process is gone.
///
/// The type is the same on every platform rather than being absent on Unix, so
/// that portable code does not have to branch on the target OS to call
/// [`Process::wait`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExit {
    pub(crate) code: Option<i32>,
}

impl ProcessExit {
    /// Returns the exit code, on a Windows target.
    pub fn code(self) -> Option<i32> {
        self.code
    }
}

pub(crate) enum ProcessesInner {
    Client(client::Processes),
    Direct(direct::Processes),
}

impl fmt::Debug for Processes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Processes").finish_non_exhaustive()
    }
}

/// A forward enumeration of the target's process table.
///
/// Entries are produced lazily. This matters on Linux, where the per-process
/// cost is a pair of `/proc` reads, so a search that stops early does not pay
/// for the whole table.
pub struct Processes {
    inner: ProcessesInner,
}

impl Processes {
    pub(crate) fn client(processes: client::Processes) -> Self {
        Self {
            inner: ProcessesInner::Client(processes),
        }
    }

    pub(crate) fn direct(processes: direct::Processes) -> Self {
        Self {
            inner: ProcessesInner::Direct(processes),
        }
    }

    /// Returns the next process, or `None` once the table is exhausted.
    ///
    /// Processes that exit partway through enumeration are skipped rather than
    /// reported as errors: the table is a moving target on every platform, and
    /// a caller cannot act on the difference.
    pub async fn next_entry(&mut self) -> Result<Option<ProcessInfo>> {
        match &mut self.inner {
            ProcessesInner::Client(processes) => processes.next_entry().await,
            ProcessesInner::Direct(processes) => processes.next_entry().await,
        }
    }
}

pub(crate) enum ProcessInner {
    Client(client::Process),
    Direct(direct::Process),
}

impl fmt::Debug for Process {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Process").field("pid", &self.pid).finish()
    }
}

/// A handle to a process this VFS did not spawn.
///
/// Holding one is what makes the operations below refer to a single process
/// rather than to whatever currently owns a PID. How strong that guarantee is
/// varies:
///
/// - Linux (`pidfd`) and Windows (a process handle pins the PID against reuse)
///   are race-free for the whole life of the handle.
/// - FreeBSD and macOS validate identity at open and cannot maintain it:
///   `kill(2)` takes a PID, and Capsicum mints process descriptors only through
///   `pdfork`, so there is no handle to hold for a process this one did not
///   fork. [`Process::signal`] and [`Process::terminate`] are racy there.
pub struct Process {
    pid: u32,
    inner: ProcessInner,
}

impl Process {
    pub(crate) fn client(pid: u32, process: client::Process) -> Self {
        Self {
            pid,
            inner: ProcessInner::Client(process),
        }
    }

    pub(crate) fn direct(pid: u32, process: direct::Process) -> Self {
        Self {
            pid,
            inner: ProcessInner::Direct(process),
        }
    }

    /// Returns the process ID.
    ///
    /// The only attribute projected directly: it is fixed for the life of the
    /// handle, where everything else has to be re-read to be true.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Takes a fresh snapshot of the process.
    pub async fn info(&self) -> Result<ProcessInfo> {
        match &self.inner {
            ProcessInner::Client(process) => process.info().await,
            ProcessInner::Direct(process) => process.info().await,
        }
    }

    /// Sends a signal to the process.
    ///
    /// Fails with [`ErrorKind::Unsupported`](crate::error::ErrorKind::Unsupported)
    /// on a Windows target. The method exists there regardless, because the
    /// target may be remote: whether signals exist is a property of the target,
    /// not of the host this code was compiled for.
    pub async fn signal(&self, signal: super::Signal) -> Result<()> {
        match &self.inner {
            ProcessInner::Client(process) => process.signal(signal).await,
            ProcessInner::Direct(process) => process.signal(signal).await,
        }
    }

    /// Asks the process to terminate.
    ///
    /// `SIGTERM` on Unix, `TerminateProcess` on Windows. Unconditional, with no
    /// grace period and no escalation, unlike
    /// [`Child::terminate`](super::Child::terminate): the graceful half of that
    /// path relies on children being spawned into a process group of their own,
    /// which is not something that can be arranged after the fact. Compose
    /// `terminate`, [`wait`](Self::wait), a timeout, and [`kill`](Self::kill)
    /// for grace with escalation.
    ///
    /// Note the asymmetry this leaves on Windows, where `TerminateProcess` is
    /// not a request the target can decline or clean up after.
    pub async fn terminate(&self) -> Result<()> {
        match &self.inner {
            ProcessInner::Client(process) => process.terminate().await,
            ProcessInner::Direct(process) => process.terminate().await,
        }
    }

    /// Kills the process.
    ///
    /// `SIGKILL` on Unix, `TerminateProcess` on Windows — the strongest stop
    /// the target offers, and one the process cannot catch or clean up after.
    ///
    /// On a Windows target this is [`terminate`](Self::terminate) under another
    /// name, since `TerminateProcess` is already unconditional and there is
    /// nothing harder to escalate to. The pair exists because on Unix the
    /// distinction is real, and a portable caller should be able to ask for
    /// either without branching on the target.
    pub async fn kill(&self) -> Result<()> {
        match &self.inner {
            ProcessInner::Client(process) => process.kill().await,
            ProcessInner::Direct(process) => process.kill().await,
        }
    }

    /// Waits for the process to exit.
    pub async fn wait(&self) -> Result<ProcessExit> {
        match &self.inner {
            ProcessInner::Client(process) => process.wait().await,
            ProcessInner::Direct(process) => process.wait().await,
        }
    }

    /// Closes the handle.
    ///
    /// Not required: a dropped [`Process`] closes itself, and a remote one is
    /// released when the session's object table is torn down. This exists so a
    /// caller can observe a close failure instead of discarding it.
    pub async fn close(self) -> Result<()> {
        match self.inner {
            ProcessInner::Client(process) => process.close().await,
            ProcessInner::Direct(process) => process.close().await,
        }
    }
}
