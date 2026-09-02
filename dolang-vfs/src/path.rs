//! Target-path conversion and target-relative well-known locations.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use typed_path::{PathType, Utf8TypedPath, Utf8TypedPathBuf};

use crate::error::{Error, ErrorKind, Result};

#[doc(hidden)]
pub use crate::protocol::WirePath;

/// A standard location resolved by a VFS target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WellKnownPath {
    /// User's home directory.
    HomeDir,
    /// Per-user cache directory.
    CacheDir,
    /// Directory for temporary files.
    TempDir,
}

/// Converts a path in this target's syntax into a native host path.
pub fn native_path(path: Utf8TypedPath<'_>) -> Result<PathBuf> {
    let matches_target = if cfg!(windows) {
        path.is_windows()
    } else {
        path.is_unix()
    };
    if !matches_target {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "path style does not match VFS target",
        ));
    }
    Ok(PathBuf::from(path.as_str()))
}

/// Converts a native host path into a UTF-8 path tagged with host syntax.
pub fn typed_path(path: PathBuf) -> Result<Utf8TypedPathBuf> {
    let path = path
        .into_os_string()
        .into_string()
        .map_err(|_| Error::new(ErrorKind::InvalidData, "path is not valid UTF-8"))?;
    Ok(if cfg!(windows) {
        Utf8TypedPathBuf::from_windows(path)
    } else {
        Utf8TypedPathBuf::from_unix(path)
    })
}

/// Returns the native host's path syntax.
pub const fn target_path_type() -> PathType {
    if cfg!(windows) {
        PathType::Windows
    } else {
        PathType::Unix
    }
}
