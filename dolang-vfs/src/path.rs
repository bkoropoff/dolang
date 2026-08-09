//! Target-path conversion and target-relative well-known locations.

use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
use typed_path::{PathType, Utf8TypedPath, Utf8TypedPathBuf};

/// A standard location resolved by a VFS target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WellKnownPath {
    HomeDir,
    CacheDir,
    TempDir,
}

/// Converts a path in this target's syntax into a native host path.
pub fn native_path(path: Utf8TypedPath<'_>) -> io::Result<PathBuf> {
    let matches_target = if cfg!(windows) {
        path.is_windows()
    } else {
        path.is_unix()
    };
    if !matches_target {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path style does not match VFS target",
        ));
    }
    Ok(PathBuf::from(path.as_str()))
}

/// Converts a native host path into a UTF-8 path tagged with host syntax.
pub fn typed_path(path: PathBuf) -> io::Result<Utf8TypedPathBuf> {
    let path = path
        .into_os_string()
        .into_string()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"))?;
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
