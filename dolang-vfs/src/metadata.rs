//! File and filesystem metadata returned by a [`crate::Vfs`].

use dolang_winterop::security::Sid;
use serde::{Deserialize, Serialize};

use crate::security::OwnershipIdentity;

/// The kind of filesystem object described by [`Metadata`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    /// Regular file.
    File,
    /// Directory.
    Dir,
    /// Symbolic link.
    Symlink,
    /// FIFO or named pipe.
    Fifo,
    /// Character device.
    CharacterDevice,
    /// Block device.
    BlockDevice,
    /// Unix-domain socket.
    Socket,
    /// Unrecognized file type.
    Unknown,
}

/// Portable Unix-style permission bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    mode: u32,
}

impl Permissions {
    /// Creates permissions from Unix-style mode bits.
    pub fn from_mode(mode: u32) -> Self {
        Self { mode }
    }
    /// Returns the Unix-style mode bits.
    pub fn mode(&self) -> u32 {
        self.mode
    }
    /// Replaces the Unix-style mode bits.
    pub fn set_mode(&mut self, mode: u32) {
        self.mode = mode;
    }
    /// Returns whether no write permission bit is set.
    pub fn readonly(&self) -> bool {
        self.mode & 0o222 == 0
    }
    /// Sets or clears the read-only state.
    pub fn set_readonly(&mut self, readonly: bool) {
        if readonly {
            self.mode &= !0o222;
        } else {
            self.mode |= 0o200;
        }
    }
}

/// Metadata for one filesystem object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// File length in bytes.
    pub len: u64,
    /// File kind.
    pub file_type: FileType,
    /// Last-access time in seconds since the Unix epoch.
    pub atime: i64,
    /// Nanosecond component of [`atime`](Self::atime).
    pub atime_nsec: i64,
    /// Last-modification time in seconds since the Unix epoch.
    pub mtime: i64,
    /// Nanosecond component of [`mtime`](Self::mtime).
    pub mtime_nsec: i64,
    /// Metadata-change time in seconds since the Unix epoch.
    pub ctime: i64,
    /// Nanosecond component of [`ctime`](Self::ctime).
    pub ctime_nsec: i64,
    /// Platform-specific metadata.
    pub family: MetadataFamily,
}

/// Platform-specific metadata payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetadataFamily {
    /// Unix metadata.
    Unix(UnixMetadata),
    /// Windows metadata.
    Windows(WindowsMetadata),
}

/// Metadata specific to Unix targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnixMetadata {
    /// File mode bits.
    pub mode: u32,
    /// Device ID containing the file.
    pub dev: u64,
    /// File inode number.
    pub ino: u64,
    /// Number of hard links.
    pub nlink: u64,
    /// Owning user ID.
    pub uid: u32,
    /// Owning group ID.
    pub gid: u32,
    /// Device ID for special files.
    pub rdev: u64,
    /// Preferred I/O block size.
    pub blksize: u64,
    /// Number of allocated blocks.
    pub blocks: u64,
    pub platform: UnixMetadataPlatform,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnixMetadataPlatform {
    FreeBsd { attrs: u32 },
    Linux { attrs: Option<u32> },
    Macos { attrs: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsMetadata {
    pub mode: u32,
    pub attrs: u32,
    pub user: Option<Sid>,
    pub group: Option<Sid>,
}

impl Metadata {
    pub fn unix(&self) -> Option<&UnixMetadata> {
        if let MetadataFamily::Unix(metadata) = &self.family {
            Some(metadata)
        } else {
            None
        }
    }
    pub fn windows(&self) -> Option<&WindowsMetadata> {
        if let MetadataFamily::Windows(metadata) = &self.family {
            Some(metadata)
        } else {
            None
        }
    }
    pub fn permissions(&self) -> Permissions {
        Permissions::from_mode(match &self.family {
            MetadataFamily::Unix(metadata) => metadata.mode,
            MetadataFamily::Windows(metadata) => metadata.mode,
        })
    }
    pub const fn linux_attrs(&self) -> Option<u32> {
        match &self.family {
            MetadataFamily::Unix(UnixMetadata {
                platform: UnixMetadataPlatform::Linux { attrs },
                ..
            }) => *attrs,
            _ => None,
        }
    }
    pub const fn freebsd_attrs(&self) -> Option<u32> {
        match &self.family {
            MetadataFamily::Unix(UnixMetadata {
                platform: UnixMetadataPlatform::FreeBsd { attrs },
                ..
            }) => Some(*attrs),
            _ => None,
        }
    }
    pub const fn macos_attrs(&self) -> Option<u32> {
        match &self.family {
            MetadataFamily::Unix(UnixMetadata {
                platform: UnixMetadataPlatform::Macos { attrs },
                ..
            }) => Some(*attrs),
            _ => None,
        }
    }
    pub const fn win_attrs(&self) -> Option<u32> {
        match &self.family {
            MetadataFamily::Windows(metadata) => Some(metadata.attrs),
            _ => None,
        }
    }
}

/// Capacity and allocation information for a filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsMetadata {
    pub capacity: u64,
    pub free: u64,
    pub available: u64,
    pub block_size: u32,
    pub family: FsMetadataFamily,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsMetadataFamily {
    Unix(UnixFsMetadata),
    Windows(WindowsFsMetadata),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnixFsMetadata {
    pub blocks: u64,
    pub blocks_free: u64,
    pub blocks_available: u64,
    pub files: u64,
    pub files_free: u64,
    pub files_available: u64,
    pub fragment_size: u32,
    pub fsid: Option<u64>,
    pub name_max: u32,
    pub platform: UnixFsMetadataPlatform,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnixFsMetadataPlatform {
    Linux { flags: u64 },
    Macos { flags: u64 },
    FreeBsd { flags: u64 },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowsFsMetadata {
    pub flags: u32,
    pub volume_serial_number: u32,
    pub component_length_max: u32,
}

impl FsMetadata {
    pub fn unix(&self) -> Option<&UnixFsMetadata> {
        if let FsMetadataFamily::Unix(metadata) = &self.family {
            Some(metadata)
        } else {
            None
        }
    }
    pub fn windows(&self) -> Option<&WindowsFsMetadata> {
        if let FsMetadataFamily::Windows(metadata) = &self.family {
            Some(metadata)
        } else {
            None
        }
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn read_only(&self) -> bool {
        match &self.family {
            FsMetadataFamily::Unix(metadata) => metadata.platform.flags() & 1 != 0,
            FsMetadataFamily::Windows(metadata) => metadata.flags & 0x0008_0000 != 0,
        }
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn no_suid(&self) -> Option<bool> {
        match &self.family {
            FsMetadataFamily::Unix(metadata) => Some(metadata.platform.flags() & 2 != 0),
            FsMetadataFamily::Windows(_) => None,
        }
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn no_exec(&self) -> Option<bool> {
        self.linux_flag(8)
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn synchronous(&self) -> Option<bool> {
        self.linux_flag(16)
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn no_dev(&self) -> Option<bool> {
        self.linux_flag(4)
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn no_atime(&self) -> Option<bool> {
        self.linux_flag(1024)
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn no_dir_atime(&self) -> Option<bool> {
        self.linux_flag(2048)
    }
    #[allow(clippy::unnecessary_cast)]
    pub fn relatime(&self) -> Option<bool> {
        self.linux_flag(1 << 21)
    }
    fn linux_flag(&self, flag: u64) -> Option<bool> {
        match &self.family {
            FsMetadataFamily::Unix(UnixFsMetadata {
                platform: UnixFsMetadataPlatform::Linux { flags },
                ..
            }) => Some(flags & flag != 0),
            _ => None,
        }
    }
}
impl UnixFsMetadataPlatform {
    pub fn flags(&self) -> u64 {
        match self {
            Self::FreeBsd { flags } | Self::Linux { flags } | Self::Macos { flags } => *flags,
        }
    }
}

/// Portable filesystem attribute flags.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttrFlags(u64);
impl AttrFlags {
    pub const READONLY: Self = Self(1 << 0);
    pub const HIDDEN: Self = Self(1 << 1);
    pub const SYSTEM: Self = Self(1 << 2);
    pub const ARCHIVE: Self = Self(1 << 3);
    pub const COMPRESSED: Self = Self(1 << 4);
    pub const TEMPORARY: Self = Self(1 << 5);
    pub const OFFLINE: Self = Self(1 << 6);
    pub const NOT_CONTENT_INDEXED: Self = Self(1 << 7);
    pub const IMMUTABLE: Self = Self(1 << 8);
    pub const APPEND_ONLY: Self = Self(1 << 9);
    pub const NO_DUMP: Self = Self(1 << 10);
    pub const NO_ATIME: Self = Self(1 << 11);
    pub const NO_COPY_ON_WRITE: Self = Self(1 << 12);
    pub const DIR_SYNC: Self = Self(1 << 13);
    pub const CASEFOLD: Self = Self(1 << 14);
    pub const DATA_JOURNALING: Self = Self(1 << 15);
    pub const NO_COMPRESS: Self = Self(1 << 16);
    pub const PROJECT_INHERIT: Self = Self(1 << 17);
    pub const SECURE_DELETE: Self = Self(1 << 18);
    pub const SYNC: Self = Self(1 << 19);
    pub const NO_TAIL_MERGE: Self = Self(1 << 20);
    pub const TOP_DIR: Self = Self(1 << 21);
    pub const UNDELETE: Self = Self(1 << 22);
    pub const DIRECT_ACCESS: Self = Self(1 << 23);
    pub const EXTENT_FORMAT: Self = Self(1 << 24);
    pub const OPAQUE: Self = Self(1 << 25);
    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttrsPatch {
    pub set: AttrFlags,
    pub clear: AttrFlags,
}
impl AttrsPatch {
    pub fn update(&mut self, flag: AttrFlags, value: Option<bool>) {
        match value {
            Some(true) => {
                self.set = self.set.union(flag);
                self.clear = self.clear.difference(flag);
            }
            Some(false) => {
                self.clear = self.clear.union(flag);
                self.set = self.set.difference(flag);
            }
            None => {}
        }
    }
    pub const fn requested(self) -> AttrFlags {
        self.set.union(self.clear)
    }
    pub const fn is_empty(self) -> bool {
        self.set.is_empty() && self.clear.is_empty()
    }
}

/// Requested changes to a filesystem object's metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataPatch {
    pub mode: Option<u32>,
    pub user: Option<OwnershipIdentity>,
    pub group: Option<OwnershipIdentity>,
    pub accessed: Option<i128>,
    pub modified: Option<i128>,
    pub created: Option<i128>,
    pub attrs: AttrsPatch,
    pub follow: bool,
}
impl Default for MetadataPatch {
    fn default() -> Self {
        Self {
            mode: None,
            user: None,
            group: None,
            accessed: None,
            modified: None,
            created: None,
            attrs: AttrsPatch::default(),
            follow: true,
        }
    }
}
impl MetadataPatch {
    pub fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.user.is_none()
            && self.group.is_none()
            && self.accessed.is_none()
            && self.modified.is_none()
            && self.created.is_none()
            && self.attrs.is_empty()
    }
}

pub(crate) fn metadata_from_std(metadata: std::fs::Metadata) -> Metadata {
    #[cfg(unix)]
    {
        use nix::sys::stat::{SFlag, mode_t};
        #[cfg(target_os = "macos")]
        use std::os::darwin::fs::MetadataExt as DarwinMetadataExt;
        #[cfg(target_os = "freebsd")]
        use std::os::freebsd::fs::MetadataExt as FreeBsdMetadataExt;
        use std::os::unix::fs::MetadataExt;
        let mode = metadata.mode();
        let file_type = match SFlag::from_bits_truncate(mode as mode_t) & SFlag::S_IFMT {
            SFlag::S_IFREG => FileType::File,
            SFlag::S_IFDIR => FileType::Dir,
            SFlag::S_IFLNK => FileType::Symlink,
            SFlag::S_IFIFO => FileType::Fifo,
            SFlag::S_IFCHR => FileType::CharacterDevice,
            SFlag::S_IFBLK => FileType::BlockDevice,
            SFlag::S_IFSOCK => FileType::Socket,
            _ => FileType::Unknown,
        };
        Metadata {
            len: metadata.len(),
            file_type,
            atime: metadata.atime(),
            atime_nsec: metadata.atime_nsec(),
            mtime: metadata.mtime(),
            mtime_nsec: metadata.mtime_nsec(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
            family: MetadataFamily::Unix(UnixMetadata {
                mode,
                dev: metadata.dev(),
                ino: metadata.ino(),
                nlink: metadata.nlink(),
                uid: metadata.uid(),
                gid: metadata.gid(),
                rdev: metadata.rdev(),
                blksize: metadata.blksize(),
                blocks: metadata.blocks(),
                #[cfg(target_os = "linux")]
                platform: UnixMetadataPlatform::Linux { attrs: None },
                #[cfg(target_os = "freebsd")]
                platform: UnixMetadataPlatform::FreeBsd {
                    attrs: metadata.st_flags(),
                },
                #[cfg(target_os = "macos")]
                platform: UnixMetadataPlatform::Macos {
                    attrs: metadata.st_flags(),
                },
            }),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let file_type = if metadata.is_file() {
            FileType::File
        } else if metadata.is_dir() {
            FileType::Dir
        } else if metadata.file_type().is_symlink() {
            FileType::Symlink
        } else {
            FileType::Unknown
        };
        Metadata {
            len: metadata.len(),
            file_type,
            atime: system_time_to_parts(metadata.accessed().ok()).0,
            atime_nsec: i64::from(system_time_to_parts(metadata.accessed().ok()).1),
            mtime: system_time_to_parts(metadata.modified().ok()).0,
            mtime_nsec: i64::from(system_time_to_parts(metadata.modified().ok()).1),
            ctime: system_time_to_parts(metadata.created().ok()).0,
            ctime_nsec: i64::from(system_time_to_parts(metadata.created().ok()).1),
            family: MetadataFamily::Windows(WindowsMetadata {
                mode: if metadata.permissions().readonly() {
                    0o444
                } else {
                    0o666
                },
                attrs: metadata.file_attributes(),
                user: None,
                group: None,
            }),
        }
    }
}
#[cfg(windows)]
pub(crate) fn metadata_with_sids(
    mut metadata: Metadata,
    user: Option<Sid>,
    group: Option<Sid>,
) -> Metadata {
    let MetadataFamily::Windows(windows) = &mut metadata.family else {
        unreachable!()
    };
    windows.user = user;
    windows.group = group;
    metadata
}
#[cfg(windows)]
fn system_time_to_parts(time: Option<std::time::SystemTime>) -> (i64, u32) {
    use std::time::UNIX_EPOCH;
    let Some(time) = time else { return (0, 0) };
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
            duration.subsec_nanos(),
        ),
        Err(err) => {
            let duration = err.duration();
            let secs = i64::try_from(duration.as_secs()).unwrap_or(i64::MAX);
            if duration.subsec_nanos() == 0 {
                (-secs, 0)
            } else {
                (-secs - 1, 1_000_000_000 - duration.subsec_nanos())
            }
        }
    }
}
