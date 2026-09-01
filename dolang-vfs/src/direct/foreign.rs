//! Local backend for [`crate::process::Process`] and
//! [`crate::process::Processes`].
//!
//! The types live here and the per-platform behavior arrives as `impl_*`
//! methods from the cfg-gated modules, same as [`super::Child`]. What each
//! platform can *hold* differs enough to show up in the fields themselves:
//! Linux and Windows own a kernel reference that keeps the PID from being
//! reused, and the BSD-derived targets have nothing to own, so they carry the
//! start time and re-check it instead.

use std::collections::VecDeque;

use uuid::Uuid;

use crate::{
    error::{Error, ErrorKind, Result},
    process::{ProcessExit, ProcessInfo, Signal, StartTime},
};

#[cfg(any(target_os = "freebsd", target_os = "macos"))]
mod bsd;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// What one listed-but-not-yet-described process carries.
///
/// Linux needs nothing but the PID, since everything else comes out of
/// `/proc/<pid>` when the entry is actually reached. The targets whose
/// enumeration is a single call arrive with part of the record already built,
/// and would have to throw it away to be re-fetched otherwise.
#[cfg(target_os = "linux")]
type Candidate = u32;
#[cfg(any(target_os = "freebsd", target_os = "macos"))]
type Candidate = bsd::Listed;
#[cfg(windows)]
type Candidate = windows::Listed;

/// How many candidates one blocking hop turns into records.
///
/// Building a record costs several small reads per process, so doing one per
/// [`tokio::task::spawn_blocking`] would spend more on scheduling than on the
/// work. Batching also absorbs runs of processes that exit between being
/// listed and being read, which would otherwise each cost a hop and yield
/// nothing.
const SCAN_CHUNK: usize = 64;

/// A forward enumeration of the local process table.
pub(crate) struct Processes {
    session: Uuid,
    /// Processes listed but not yet described, in reverse so taking the next
    /// batch truncates the end rather than shifting everything after it.
    pending: Vec<Candidate>,
    ready: VecDeque<ProcessInfo>,
}

impl Processes {
    pub(crate) async fn open(session: Uuid) -> Result<Self> {
        let mut pending = Self::impl_scan().await?;
        pending.reverse();
        Ok(Self {
            session,
            pending,
            ready: VecDeque::new(),
        })
    }

    /// Returns the next process, skipping any that have gone away.
    ///
    /// A batch can come back empty without the enumeration being over, so this
    /// loops rather than returning what one batch happened to produce.
    pub(crate) async fn next_entry(&mut self) -> Result<Option<ProcessInfo>> {
        loop {
            if let Some(entry) = self.ready.pop_front() {
                return Ok(Some(entry));
            }
            if self.pending.is_empty() {
                return Ok(None);
            }
            let rest = self.pending.len().saturating_sub(SCAN_CHUNK);
            let mut batch = self.pending.split_off(rest);
            batch.reverse();
            self.ready = Self::impl_describe(self.session, batch).await?;
        }
    }
}

/// A handle to a local process this VFS did not spawn.
pub(crate) struct Process {
    pid: u32,
    session: Uuid,
    /// Keeps the PID reserved for as long as this handle lives, so every
    /// operation below refers to the process that was opened.
    #[cfg(target_os = "linux")]
    pidfd: std::os::fd::OwnedFd,
    /// As for `pidfd`: an open process handle blocks PID reuse on Windows.
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
    /// What the BSD-derived targets have instead of a reference.
    ///
    /// There is no descriptor to hold for a process this one did not fork —
    /// `pdfork` is the only source of process descriptors, by Capsicum's
    /// design — so identity is established at open and cannot be maintained.
    /// It is re-checked before anything that acts on the process, which
    /// narrows the window without closing it.
    #[cfg(any(target_os = "freebsd", target_os = "macos"))]
    start: StartTime,
}

impl Process {
    pub(crate) async fn open(session: Uuid, pid: u32, start: Option<StartTime>) -> Result<Self> {
        Self::impl_open(session, pid, start).await
    }

    pub(crate) async fn info(&self) -> Result<ProcessInfo> {
        self.impl_info().await
    }

    pub(crate) async fn signal(&self, signal: Signal) -> Result<()> {
        self.impl_signal(signal).await
    }

    pub(crate) async fn terminate(&self) -> Result<()> {
        self.impl_terminate().await
    }

    pub(crate) async fn kill(&self) -> Result<()> {
        self.impl_kill().await
    }

    pub(crate) async fn wait(&self) -> Result<ProcessExit> {
        self.impl_wait().await
    }

    pub(crate) async fn close(self) -> Result<()> {
        self.impl_close().await
    }
}

/// Reports a PID whose identity no longer matches what the caller expected.
///
/// Distinct from a plain "no such process": the PID exists, it is simply not
/// the process the caller had in mind, and retrying will not help.
fn recycled(pid: u32) -> Error {
    Error::new(
        ErrorKind::NotFound,
        format!("process {pid} is not the process it was when it was captured"),
    )
}

/// Reports a process that went away between being found and being asked about.
fn gone(pid: u32) -> Error {
    Error::new(ErrorKind::NotFound, format!("process {pid} has exited"))
}
