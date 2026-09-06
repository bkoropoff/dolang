//! Connection-scoped VFS state and RPC handle markers.

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    error::Result, extension::ExtensionSet, path, security::SecurityInfo, target::TargetInfo,
};

/// Snapshot of a VFS target's initial process context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
    /// Identifies the target this context was captured from.
    ///
    /// Values that name something only meaningful on one target — a process ID
    /// and its start time, for now — carry this so they cannot be silently
    /// interpreted against a different one. See
    /// [`crate::process::ProcessInfo`].
    ///
    /// Freshly generated per [`Query::current`], so it identifies a target
    /// *session* rather than a host: a PID validated against a rebooted
    /// machine, or against a second agent on the same machine, proves nothing
    /// either. A direct target generates one too, so the local path is not a
    /// special case that skips the check.
    pub session: Uuid,
    /// Process ID of the target process itself.
    ///
    /// The process that serves this VFS: the interpreter for a direct target,
    /// the remote agent for a client. Interpreted against
    /// [`session`](Self::session) like any other PID from this target.
    pub pid: u32,
    /// Environment variables from the target process.
    pub env: HashMap<String, String>,
    /// Target process's current working directory.
    pub cwd: path::PathBuf,
    /// Path to the target process's current executable.
    pub current_exe: path::PathBuf,
    /// Target operating system and processor information.
    pub target: TargetInfo,
    /// Target process security information.
    pub security: SecurityInfo,
    /// Registered extension protocol versions.
    pub extensions: ExtensionSet,
}

impl Query {
    /// Captures the current process context for a direct VFS target.
    pub fn current() -> Result<Self> {
        Ok(Self {
            session: Uuid::new_v4(),
            pid: std::process::id(),
            env: current_environment().collect(),
            cwd: path::PathBuf::from_native(std::env::current_dir()?)?,
            current_exe: path::PathBuf::from_native(std::env::current_exe()?)?,
            target: TargetInfo::current(),
            security: SecurityInfo::current()?,
            extensions: crate::extension::registered()?.clone(),
        })
    }
}

pub(crate) fn current_environment() -> impl Iterator<Item = (String, String)> {
    std::env::vars_os().filter_map(|(name, value)| {
        let name = name.into_string().ok()?;
        #[cfg(windows)]
        let name = name.to_uppercase();
        Some((name, value.into_string().ok()?))
    })
}

#[cfg(test)]
mod tests {
    use crate::extension::ExtensionSet;

    #[test]
    fn extension_versions_are_collated_and_negotiated() {
        let extensions = ExtensionSet::from_pairs([
            ("example".to_owned(), 3),
            ("other".to_owned(), 4),
            ("example".to_owned(), 1),
            ("example".to_owned(), 2),
        ])
        .unwrap();

        assert_eq!(extensions.versions("example"), Some([1, 2, 3].as_slice()));
        assert!(extensions.supports("example", 2));
        assert!(!extensions.supports("example", 4));
        assert_eq!(
            extensions.maximum_common_version("example", &[2, 7, 1]),
            Some(2)
        );
        assert_eq!(extensions.maximum_common_version("missing", &[1]), None);
    }

    #[test]
    fn duplicate_extension_version_is_rejected() {
        let error = ExtensionSet::from_pairs([
            ("example".to_owned(), 2),
            ("example".to_owned(), 1),
            ("example".to_owned(), 2),
        ])
        .unwrap_err();
        assert!(error.message().contains("example version 2"));
    }
}

/// Marker for a regular file retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct FileMarker;
/// Marker for a held file lock retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct FileLockMarker;
/// Marker for a directory enumeration retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct ReadDirMarker;
/// Marker for another VFS retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct VfsMarker;
#[derive(Debug)]
pub(crate) struct StdioSendMarker;
#[derive(Debug)]
pub(crate) struct StdioRecvMarker;
/// Marker for a child process retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct ChildMarker;
/// Marker for a foreign process handle retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct ProcessMarker;
/// Marker for a process-table enumeration retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct ProcessEnumMarker;
