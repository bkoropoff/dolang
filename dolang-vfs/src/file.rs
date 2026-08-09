//! File-specific values shared by VFS implementations.

use serde::{Deserialize, Serialize};

/// Access permission flags accepted by the Unix `access` VFS extension method.
#[cfg(unix)]
pub use nix::unistd::AccessFlags;

/// Lock access requested for a byte range of a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FileLockMode {
    Exclusive,
    Shared,
}

/// Whether acquiring a file lock may wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FileLockBehavior {
    Blocking,
    Try,
}

/// A half-open byte range used for a file lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileLockRange {
    pub start: u64,
    pub end: Option<u64>,
}

impl FileLockRange {
    /// Returns whether this range contains no bytes.
    pub fn is_empty(self) -> bool {
        self.end == Some(self.start)
    }

    pub(crate) fn conflicts(self, other: Self) -> bool {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => return false,
            (true, false) => {
                return other.start < self.start && self.start < other.end.unwrap_or(u64::MAX);
            }
            (false, true) => {
                return self.start < other.start && other.start < self.end.unwrap_or(u64::MAX);
            }
            (false, false) => {}
        }
        self.start < other.end.unwrap_or(u64::MAX) && other.start < self.end.unwrap_or(u64::MAX)
    }
}

/// A complete request to acquire a file lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileLockRequest {
    pub range: FileLockRange,
    pub mode: FileLockMode,
    pub behavior: FileLockBehavior,
}

/// A held file lock released explicitly or when dropped.
pub struct FileLock {
    inner: Option<FileLockInner>,
}

enum FileLockInner {
    Direct(crate::direct::DirectFileLock),
    Remote(crate::client::RemoteFileLock),
}

impl FileLock {
    pub(crate) fn direct(lock: crate::direct::DirectFileLock) -> Self {
        Self {
            inner: Some(FileLockInner::Direct(lock)),
        }
    }

    pub(crate) fn remote(lock: crate::client::RemoteFileLock) -> Self {
        Self {
            inner: Some(FileLockInner::Remote(lock)),
        }
    }

    /// Releases the lock. Calling this after a successful release is a no-op.
    pub async fn release(&mut self) -> crate::Result<()> {
        let Some(lock) = self.inner.as_mut() else {
            return Ok(());
        };
        let result = match lock {
            FileLockInner::Direct(lock) => lock.release().await,
            FileLockInner::Remote(lock) => lock.release().await,
        };
        if result.is_ok() {
            self.inner = None;
        }
        result
    }
}

impl std::fmt::Debug for FileLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileLock")
            .field("released", &self.inner.is_none())
            .finish()
    }
}
