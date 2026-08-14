use std::{collections::VecDeque, io, path::Path};

use dolang_rpc::session::Gift;

use crate::FileType;
#[cfg(unix)]
use nix::{
    dir::{Dir as NixDir, OwningIter, Type},
    fcntl::OFlag,
    sys::stat::Mode,
};
use serde::{Deserialize, Serialize};

/// An entry returned by [`ReadDir`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    file_name: String,
    file_type: FileType,
    family: DirEntryFamily,
}

/// Platform-specific fields carried by a directory entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirEntryFamily {
    /// Unix-specific entry information.
    Unix { ino: u64 },
    /// Windows entry information.
    Windows,
}

impl DirEntry {
    /// Returns the entry name without its parent path.
    pub fn file_name(&self) -> &std::ffi::OsStr {
        std::ffi::OsStr::new(&self.file_name)
    }

    /// Returns the inode number when the target is Unix-like.
    pub fn ino(&self) -> Option<u64> {
        match self.family {
            DirEntryFamily::Unix { ino } => Some(ino),
            DirEntryFamily::Windows => None,
        }
    }

    /// Returns the entry's file type.
    pub fn file_type(&self) -> FileType {
        self.file_type
    }
}

/// An asynchronous directory iterator.
#[derive(Debug)]
pub struct ReadDir {
    inner: ReadDirInner,
}

#[derive(Debug)]
enum ReadDirInner {
    #[cfg(unix)]
    Unix(Option<OwningIter>),
    #[cfg(windows)]
    Windows(Box<tokio::fs::ReadDir>),
    Remote(RemoteReadDir),
}

struct RemoteReadDir {
    client: crate::Client,
    handle: Option<Gift<crate::session::ReadDirMarker>>,
    entries: VecDeque<DirEntry>,
}

impl std::fmt::Debug for RemoteReadDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteReadDir")
            .field("handle", &self.handle)
            .field("buffered", &self.entries.len())
            .finish()
    }
}

fn vfs_io(error: crate::Error) -> io::Error {
    io::Error::new(error.kind().into(), error.to_string())
}

impl ReadDir {
    #[cfg(unix)]
    pub(crate) async fn open(path: &Path) -> io::Result<Self> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let nix_dir =
                NixDir::open(&path, OFlag::O_DIRECTORY, Mode::empty()).map_err(io::Error::other)?;
            Ok(Self {
                inner: ReadDirInner::Unix(Some(nix_dir.into_iter())),
            })
        })
        .await
        .map_err(io::Error::other)?
    }

    #[cfg(windows)]
    pub(crate) async fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            inner: ReadDirInner::Windows(Box::new(tokio::fs::read_dir(path).await?)),
        })
    }

    pub(crate) fn from_remote(
        client: crate::Client,
        handle: Gift<crate::session::ReadDirMarker>,
    ) -> Self {
        Self {
            inner: ReadDirInner::Remote(RemoteReadDir {
                client,
                handle: Some(handle),
                entries: VecDeque::new(),
            }),
        }
    }

    /// Returns the next directory entry, or `None` after the iterator is exhausted.
    pub async fn next_entry(&mut self) -> io::Result<Option<DirEntry>> {
        match &mut self.inner {
            #[cfg(unix)]
            ReadDirInner::Unix(iter) => Self::next_unix(iter).await,
            #[cfg(windows)]
            ReadDirInner::Windows(inner) => Self::next_windows(inner).await,
            ReadDirInner::Remote(remote) => remote.next_entry().await,
        }
    }

    #[cfg(unix)]
    async fn next_unix(iter: &mut Option<OwningIter>) -> io::Result<Option<DirEntry>> {
        let mut owned_iter = match iter.take() {
            Some(iter) => iter,
            None => return Ok(None),
        };
        let (result, next_iter) = tokio::task::spawn_blocking(move || {
            loop {
                match owned_iter.next() {
                    Some(Ok(entry)) => {
                        let name = entry.file_name().to_bytes();
                        if name == b"." || name == b".." {
                            continue;
                        }
                        let file_name = match String::from_utf8(name.to_vec()) {
                            Ok(name) => name,
                            Err(error) => {
                                return (
                                    Err(io::Error::new(io::ErrorKind::InvalidData, error)),
                                    Some(owned_iter),
                                );
                            }
                        };
                        let file_type = entry
                            .file_type()
                            .map(|ty| match ty {
                                Type::File => FileType::File,
                                Type::Directory => FileType::Dir,
                                Type::Symlink => FileType::Symlink,
                                Type::Fifo => FileType::Fifo,
                                Type::CharacterDevice => FileType::CharacterDevice,
                                Type::BlockDevice => FileType::BlockDevice,
                                Type::Socket => FileType::Socket,
                            })
                            .unwrap_or(FileType::Unknown);
                        return (
                            Ok(Some(DirEntry {
                                file_name,
                                file_type,
                                family: DirEntryFamily::Unix { ino: entry.ino() },
                            })),
                            Some(owned_iter),
                        );
                    }
                    Some(Err(error)) => {
                        return (Err(io::Error::other(error)), Some(owned_iter));
                    }
                    None => return (Ok(None), None),
                }
            }
        })
        .await
        .map_err(io::Error::other)?;
        *iter = next_iter;
        result
    }

    #[cfg(windows)]
    async fn next_windows(inner: &mut tokio::fs::ReadDir) -> io::Result<Option<DirEntry>> {
        let Some(entry) = inner.next_entry().await? else {
            return Ok(None);
        };
        let file_type = entry.file_type().await?;
        let file_type = if file_type.is_file() {
            FileType::File
        } else if file_type.is_dir() {
            FileType::Dir
        } else if file_type.is_symlink() {
            FileType::Symlink
        } else {
            FileType::Unknown
        };
        let file_name = entry.file_name().into_string().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "directory entry is not valid UTF-8",
            )
        })?;
        Ok(Some(DirEntry {
            file_name,
            file_type,
            family: DirEntryFamily::Windows,
        }))
    }
}

impl RemoteReadDir {
    async fn next_entry(&mut self) -> io::Result<Option<DirEntry>> {
        if let Some(entry) = self.entries.pop_front() {
            return Ok(Some(entry));
        }
        let Some(handle) = self.handle.as_ref().map(Gift::cite) else {
            return Ok(None);
        };
        match self
            .client
            .request(crate::protocol::RequestKind::ReadDirNext { read_dir: handle })
            .await
            .map_err(vfs_io)?
        {
            crate::protocol::ResponseKind::ReadDirNext(result) => {
                let page = result.map_err(crate::Error::from).map_err(vfs_io)?;
                if page.done {
                    self.handle = None;
                }
                self.entries = page.entries.into();
                Ok(self.entries.pop_front())
            }
            _ => Err(io::Error::other("unexpected response for ReadDirNext")),
        }
    }
}

impl Drop for RemoteReadDir {
    fn drop(&mut self) {
        let Some(read_dir) = self.handle.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let client = self.client.clone();
        runtime.spawn(async move {
            let _ = client
                .request(crate::protocol::RequestKind::ReadDirClose {
                    read_dir: read_dir.cite(),
                })
                .await;
        });
    }
}
