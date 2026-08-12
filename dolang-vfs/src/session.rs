//! Connection-scoped VFS state and RPC handle markers.

use std::collections::HashMap;

use typed_path::Utf8TypedPathBuf;

use crate::{Client, Result, path::typed_path, security::SecurityInfo, target::TargetInfo};

/// VFS extension protocol versions supported by a backend.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExtensionSet {
    versions: HashMap<String, Vec<u16>>,
}

impl ExtensionSet {
    pub(crate) fn from_pairs(pairs: impl IntoIterator<Item = (String, u16)>) -> Result<Self> {
        let mut versions: HashMap<String, Vec<u16>> = HashMap::new();
        for (name, version) in pairs {
            versions.entry(name).or_default().push(version);
        }
        for (name, versions) in &mut versions {
            versions.sort_unstable();
            if let Some(version) = versions
                .windows(2)
                .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
            {
                return Err(crate::Error::new(
                    crate::ErrorKind::AlreadyExists,
                    format!("duplicate VFS extension registration: {name} version {version}"),
                ));
            }
        }
        Ok(Self { versions })
    }

    /// Returns all supported versions for `name`, in ascending order.
    pub fn versions(&self, name: &str) -> Option<&[u16]> {
        self.versions.get(name).map(Vec::as_slice)
    }

    /// Returns whether the exact extension version is supported.
    pub fn supports(&self, name: &str, version: u16) -> bool {
        self.versions(name)
            .is_some_and(|versions| versions.binary_search(&version).is_ok())
    }

    /// Returns the highest version supported by both the backend and caller.
    pub fn maximum_common_version(&self, name: &str, supported: &[u16]) -> Option<u16> {
        let versions = self.versions(name)?;
        supported
            .iter()
            .copied()
            .filter(|version| versions.binary_search(version).is_ok())
            .max()
    }
}

/// Snapshot of a VFS target's initial process context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Query {
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
    /// Registered extension protocol versions.
    pub extensions: ExtensionSet,
}

impl Query {
    /// Captures the current process context for a direct VFS target.
    pub fn current() -> Result<Self> {
        Ok(Self {
            env: current_environment().collect(),
            cwd: typed_path(std::env::current_dir()?)?,
            current_exe: typed_path(std::env::current_exe()?)?,
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
    use super::ExtensionSet;

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
