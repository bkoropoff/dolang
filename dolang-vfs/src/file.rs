//! File-specific values shared by VFS implementations.

use serde::{Deserialize, Serialize};

bitflags::bitflags! {
    /// Permissions checked by [`Vfs::access`](crate::Vfs::access).
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct AccessFlags: i32 {
        /// Checks execute permission.
        const X_OK = 1;
        /// Checks write permission.
        const W_OK = 2;
        /// Checks read permission.
        const R_OK = 4;
        /// Checks only whether the path exists.
        const F_OK = 0;
    }
}

/// Lock access requested for a byte range of a file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FileLockMode {
    /// Prevents other exclusive or shared locks from overlapping this range.
    Exclusive,
    /// Allows other shared locks but not exclusive locks to overlap this range.
    Shared,
}

/// Whether acquiring a file lock may wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FileLockBehavior {
    /// Waits until the lock can be acquired.
    Blocking,
    /// Returns without waiting when the lock cannot be acquired.
    Try,
}

/// A half-open byte range used for a file lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileLockRange {
    /// Inclusive byte offset at which the range starts.
    pub start: u64,
    /// Exclusive byte offset at which the range ends, or no end for EOF.
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
    /// Byte range to lock.
    pub range: FileLockRange,
    /// Access mode to acquire.
    pub mode: FileLockMode,
    /// Whether acquisition may block.
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
