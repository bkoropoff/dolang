//! Connection-scoped VFS state and RPC handle markers.

use std::collections::HashMap;

use typed_path::Utf8TypedPathBuf;

use crate::{Client, Result, path::typed_path, security::SecurityInfo, target::TargetInfo};

/// Snapshot of a VFS target's initial process context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Environment variables from the target process.
    pub env: HashMap<String, String>,
    /// Target process's current working directory.
    pub cwd: Utf8TypedPathBuf,
    /// Path to the target process's current executable.
    pub current_exe: Utf8TypedPathBuf,
    /// Target operating system and processor information.
    pub target: TargetInfo,
    /// Target process security information.
    pub security: SecurityInfo,
}

impl Query {
    /// Captures the current process context for a direct VFS target.
    pub fn current() -> Result<Self> {
        Ok(Self {
            env: std::env::vars_os()
                .filter_map(|(name, value)| {
                    let name = name.into_string().ok()?;
                    // Windows environment variable names are case-insensitive
                    // and the OS preserves whatever casing a variable
                    // happened to be created with, which can vary depending
                    // on how it was inherited or last set (e.g. `Path` vs.
                    // `PATH`). Normalize to uppercase so lookups against a
                    // captured `Query::env` don't depend on that incidental
                    // casing.
                    #[cfg(windows)]
                    let name = name.to_uppercase();
                    Some((name, value.into_string().ok()?))
                })
                .collect(),
            cwd: typed_path(std::env::current_dir()?)?,
            current_exe: typed_path(std::env::current_exe()?)?,
            target: TargetInfo::current(),
            security: SecurityInfo::current()?,
        })
    }
}

/// Marker for a regular file retained by a VFS RPC session.
#[derive(Debug)]
pub(crate) struct FileMarker;
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

/// An owned connection to a VFS process whose lifetime is tied to the connection.
pub struct VfsSession {
    client: Client,
    #[cfg(windows)]
    windows: Option<crate::windows::AdminSession>,
}

impl VfsSession {
    pub(crate) fn from_client(client: Client) -> Self {
        Self {
            client,
            #[cfg(windows)]
            windows: None,
        }
    }
    #[cfg(windows)]
    pub(crate) fn from_windows(session: crate::windows::AdminSession) -> Self {
        Self {
            client: session.client().clone(),
            windows: Some(session),
        }
    }
    /// Returns the client for the owned VFS connection.
    pub fn client(&self) -> &Client {
        &self.client
    }
    /// Stops the VFS server and waits for an owned process to exit.
    pub async fn stop(&self) -> Result<()> {
        #[cfg(windows)]
        if let Some(session) = &self.windows {
            return session.stop().await.map_err(Into::into);
        }
        self.client.stop().await
    }
}
