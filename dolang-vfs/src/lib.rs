#![deny(warnings)]
#![allow(async_fn_in_trait)]
#![cfg_attr(docsrs, feature(doc_cfg))]
//! Filesystem and process operations over either a local or remote target.
//!
//! [`Direct`] performs operations in the current process's environment.
//! [`Client`] performs the same broad class of operations through a
//! `dolang-vfs` agent. [`AnyVfs`] lets an application carry either backend
//! behind one value, while the [`Vfs`], [`OpenOptions`], [`FileHandle`], and
//! [`Command`] traits abstract over a chosen backend.
//!
//! Paths passed through [`Vfs`] are [`Utf8TypedPath`] values. Their syntax
//! belongs to the target VFS rather than necessarily to the host running this
//! code, which lets a Unix host describe Windows paths and vice versa.
//!
//! ```no_run
//! use dolang_vfs::{OpenOptions, Vfs, direct::Direct};
//! use typed_path::{Utf8TypedPath, Utf8UnixPath, Utf8WindowsPath};
//!
//! async fn read_a_file() -> dolang_vfs::error::Result<()> {
//!     let vfs = Direct::new()?;
//!     let path = if cfg!(windows) {
//!         Utf8TypedPath::Windows(Utf8WindowsPath::new(r"C:\\example.txt"))
//!     } else {
//!         Utf8TypedPath::Unix(Utf8UnixPath::new("/tmp/example.txt"))
//!     };
//!     let mut options = vfs.open_options();
//!     options.read(true);
//!     let _file = options.open(path).await?;
//!     Ok(())
//! }
//! ```

use direct::{Direct, DirectFile, DirectOpenOptions};
use dolang_winterop::security::{SecDesc, Sid};
use extension::VfsExtension;
use std::{
    collections::HashMap,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf},
    task::JoinHandle,
};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf, Utf8UnixPath, Utf8WindowsPath};
/// Remote VFS client implementation.
pub mod client;
/// Local-process VFS implementation.
pub mod direct;
/// Directory iteration types.
pub mod directory;
/// Error types returned by VFS operations.
pub mod error;
pub mod extension;
pub mod file;
pub mod metadata;
pub mod path;
mod posix_acl;
mod probe;
/// Process status, control, and standard-I/O types.
pub mod process;
mod protocol;
pub mod security;
/// RPC server implementation.
pub mod server;
/// VFS service executable support.
pub mod service;
pub mod session;
pub mod stream;
pub mod target;
#[cfg(windows)]
mod windows;
pub mod xattr;

use directory::DirEntry;
pub(crate) use error::{Error, ErrorKind, Result};
use file::{FileLock, FileLockRequest};
use metadata::{AttrFlags, AttrsPatch, FileType, FsMetadata, Metadata, MetadataPatch};
use path::WellKnownPath;
use process::{ProcessControl, ProcessStatus, TerminationPolicy};
use security::{OwnershipIdentity, PosixAcl, SecurityInfo, SidName};
use session::{ExtensionSet, VfsSession};
use stream::StreamEntry;
use target::TargetInfo;
use xattr::{XattrEntry, XattrNamespace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionMode {
    Native,
    Remote,
}

pub(crate) use directory::ReadDir;

#[allow(async_fn_in_trait)]
/// Configures and opens a file on one [`Vfs`] backend.
pub trait OpenOptions {
    /// The file handle created by [`open`](Self::open).
    type File: FileHandle;

    /// Enables or disables read access.
    fn read(&mut self, read: bool) -> &mut Self;
    /// Enables or disables write access.
    fn write(&mut self, write: bool) -> &mut Self;
    /// Enables or disables append mode.
    fn append(&mut self, append: bool) -> &mut Self;
    /// Enables or disables creation when the file is absent.
    fn create(&mut self, create: bool) -> &mut Self;
    /// Enables or disables exclusive creation.
    fn create_new(&mut self, create_new: bool) -> &mut Self;
    /// Enables or disables truncation when opening.
    fn truncate(&mut self, truncate: bool) -> &mut Self;
    /// Enables or disables following the final path component when it is a link.
    fn no_follow(&mut self, no_follow: bool) -> &mut Self;
    /// Opens `path` using the configured options.
    async fn open(&self, path: Utf8TypedPath<'_>) -> Result<Self::File>;
}

/// An asynchronous file handle produced by a [`Vfs`].
///
/// File handles implement Tokio's asynchronous read, write, and seek traits.
pub trait FileHandle: AsyncRead + AsyncWrite + AsyncSeek + Unpin + Sized {
    /// Converts this handle into a standard-output or standard-error endpoint.
    async fn to_stdio_send(&self) -> Result<StdioSend>;
    /// Converts this handle into a standard-input endpoint.
    async fn to_stdio_recv(&self) -> Result<StdioRecv>;
    /// Closes this handle.
    async fn close(self) -> Result<()>;
    /// Changes the file length to `size` bytes.
    async fn set_size(&mut self, size: u64) -> Result<()>;
    /// Returns metadata for the open file.
    async fn metadata(&mut self) -> Result<Metadata>;
    /// Returns metadata for the filesystem containing the open file.
    async fn fs_metadata(&mut self) -> Result<FsMetadata>;
    /// Returns the POSIX ACL, optionally its default ACL when this is a directory.
    async fn acl(&mut self, default: bool) -> Result<Option<PosixAcl>>;
    /// Sets or removes the POSIX ACL, optionally its default ACL.
    async fn set_acl(&mut self, acl: Option<&PosixAcl>, default: bool) -> Result<()>;
    /// Returns the Windows security descriptor selected by `mask`.
    async fn sec_desc(&mut self, mask: u32) -> Result<SecDesc>;
    /// Replaces the Windows security descriptor.
    async fn set_sec_desc(&mut self, sec_desc: &SecDesc) -> Result<()>;
    /// Lists extended attributes in `namespace`.
    async fn xattrs(&mut self, namespace: XattrNamespace<'_>) -> Result<Vec<XattrEntry>>;
    /// Reads one extended attribute.
    async fn xattr(&mut self, name: &str, namespace: Option<&str>) -> Result<Vec<u8>>;
    /// Lists alternate data streams.
    async fn streams(&mut self) -> Result<Vec<StreamEntry>>;
    /// Creates or replaces an extended attribute.
    async fn set_xattr(&mut self, name: &str, namespace: Option<&str>, value: &[u8]) -> Result<()>;
    /// Removes an extended attribute.
    async fn remove_xattr(&mut self, name: &str, namespace: Option<&str>) -> Result<()>;
    /// Acquires a byte-range lock according to `request`.
    async fn lock(&self, request: FileLockRequest) -> Result<Option<FileLock>>;
    /// Converts this handle into a local standard-library file when possible.
    async fn try_into_std(self) -> std::result::Result<std::fs::File, Self>;
}

#[allow(async_fn_in_trait)]
/// A spawned process owned by a [`Command`] backend.
pub trait Child {
    /// Waits for the process to exit.
    async fn wait(&mut self) -> Result<ProcessStatus>;
    /// Terminates the process and returns its status when it has exited.
    async fn terminate(self) -> Result<Option<ProcessStatus>>
    where
        Self: Sized;
}

#[allow(async_fn_in_trait)]
/// Configures and spawns a process on a [`Vfs`] backend.
pub trait Command {
    /// Child process returned by [`spawn`](Self::spawn).
    type Child: Child;
    /// Writable endpoint accepted for standard output and error.
    type StdioSend: AsyncWrite + Unpin;
    /// Readable endpoint accepted for standard input.
    type StdioRecv: AsyncRead + Unpin;

    /// Appends an argument to the program invocation.
    fn arg(&mut self, arg: &str) -> &mut Self;
    /// Sets an environment variable for the child.
    fn env(&mut self, key: &str, val: &str) -> &mut Self;
    /// Removes an environment variable from the child.
    fn env_remove(&mut self, key: &str) -> &mut Self;
    /// Sets the child's working directory.
    fn current_dir(&mut self, dir: Utf8TypedPath<'_>) -> &mut Self;
    /// Sets the child's standard input.
    fn stdin(&mut self, stdio: Self::StdioRecv) -> io::Result<&mut Self>;
    /// Sets the child's standard output.
    fn stdout(&mut self, stdio: Self::StdioSend) -> io::Result<&mut Self>;
    /// Inherit the host process's standard input.
    ///
    /// Opaque remote clients treat terminal input as null because Tokio cannot
    /// cancel an outstanding terminal read. Redirected input is relayed to the
    /// remote process.
    fn stdin_inherit(&mut self) -> io::Result<&mut Self>;
    /// Inherits the host process's standard output.
    fn stdout_inherit(&mut self) -> io::Result<&mut Self>;
    /// Connects the child's standard output to the parent process's standard
    /// error.
    fn stdout_inherit_stderr(&mut self) -> io::Result<&mut Self>;
    /// Connects the child's standard input to the null device.
    fn stdin_null(&mut self) -> &mut Self;
    /// Connects the child's standard output to the null device.
    fn stdout_null(&mut self) -> &mut Self;
    /// Sets the child's standard error.
    fn stderr(&mut self, stdio: Self::StdioSend) -> io::Result<&mut Self>;
    /// Inherits the host process's standard error.
    fn stderr_inherit(&mut self) -> io::Result<&mut Self>;
    /// Connects the child's standard error to the same destination as its
    /// configured standard output.
    fn stderr_to_stdout(&mut self) -> io::Result<&mut Self>;
    /// Connects the child's standard error to the parent process's standard
    /// output.
    fn stderr_inherit_stdout(&mut self) -> io::Result<&mut Self>;
    /// Connects the child's standard error to the null device.
    fn stderr_null(&mut self) -> &mut Self;
    /// Sets foreground or background process control behavior.
    fn process_control(&mut self, control: ProcessControl) -> &mut Self;
    /// Sets the policy used to terminate the child.
    fn termination_policy(&mut self, policy: TerminationPolicy) -> &mut Self;
    /// Spawns the configured process.
    async fn spawn(self) -> Result<Self::Child>;
}

#[allow(async_fn_in_trait)]
/// A filesystem and process-execution backend.
///
/// Implementations may be local, remote, or a dispatcher over either. A
/// value's path arguments always use the target's syntax; consult
/// [`Vfs::target`] when selecting one for a remote VFS.
pub trait Vfs {
    /// File handle returned by this backend.
    type File: FileHandle;
    /// Writable standard-I/O endpoint produced by this backend.
    type StdioSend: AsyncWrite + Unpin;
    /// Readable standard-I/O endpoint produced by this backend.
    type StdioRecv: AsyncRead + Unpin;
    /// File-open options builder for this backend.
    type OpenOptions<'a>: OpenOptions<File = Self::File>
    where
        Self: 'a;
    /// Command builder for this backend.
    type Command<'a>: Command<StdioSend = Self::StdioSend, StdioRecv = Self::StdioRecv>
    where
        Self: 'a;

    /// Iterates the target's initial process environment.
    fn env(&self) -> Box<dyn Iterator<Item = (String, String)> + '_>;
    /// Returns the target's initial working directory.
    fn cwd(&self) -> Utf8TypedPath<'_>;
    /// Returns the target process executable.
    fn current_exe(&self) -> Utf8TypedPath<'_>;
    /// Returns target platform information.
    fn target(&self) -> &TargetInfo;
    /// Returns the target's initial security context.
    fn security(&self) -> &SecurityInfo;
    /// Returns supported VFS extension protocol versions.
    fn extensions(&self) -> &ExtensionSet;

    /// Creates a file-open options builder.
    fn open_options(&self) -> Self::OpenOptions<'_>;
    /// Creates a command builder for `program`.
    fn command(&self, program: Utf8TypedPath<'_>) -> Self::Command<'_>;
    /// Connects to a VFS agent over a Unix-domain socket.
    ///
    /// `key` is an optional pre-shared key that both ends must prove knowledge
    /// of during negotiation. It is what identifies the intended agent when
    /// the socket's permissions cannot; the concrete client accepts the same
    /// key when connecting.
    async fn unix_socket(&self, path: Utf8TypedPath<'_>, key: Option<&[u8]>) -> Result<AnyVfs>;
    /// Starts a Windows administrative VFS session.
    async fn windows_admin(
        &self,
        cwd: Utf8TypedPath<'_>,
        env: HashMap<String, Option<String>>,
        elevate: bool,
    ) -> Result<VfsSession>;
    /// Creates a connected writable and readable pipe endpoint.
    async fn pipe(&self) -> Result<(Self::StdioSend, Self::StdioRecv)>;
    /// Like [`pipe`](Self::pipe), with a best-effort kernel buffer size
    /// hint. Backends that can't honor the hint (e.g. a pipe created on a
    /// remote peer) fall back to the default via this provided impl.
    async fn pipe_sized(
        &self,
        _buf_size: Option<usize>,
    ) -> Result<(Self::StdioSend, Self::StdioRecv)> {
        self.pipe().await
    }
    /// Resolves a Unix user ID to a name.
    async fn user_name(&self, uid: u32) -> Result<String>;
    /// Resolves a Unix user name to an ID.
    async fn user_id(&self, name: &str) -> Result<u32>;
    /// Resolves a Unix group ID to a name.
    async fn group_name(&self, gid: u32) -> Result<String>;
    /// Resolves a Unix group name to an ID.
    async fn group_id(&self, name: &str) -> Result<u32>;
    /// Resolves a Windows SID to its account name.
    async fn sid_name(&self, sid: &Sid) -> Result<SidName>;
    /// Resolves a Windows account name to its SID.
    async fn account_name(&self, name: &str) -> Result<SidName>;
    /// Opens a directory iterator.
    async fn read_dir(&self, path: Utf8TypedPath<'_>) -> Result<ReadDir>;
    /// Finds an executable using a target search path.
    async fn which(
        &self,
        program: Utf8TypedPath<'_>,
        path: Option<&str>,
        cwd: Option<Utf8TypedPath<'_>>,
    ) -> Result<Option<Utf8TypedPathBuf>>;
    /// Resolves a target-specific well-known path.
    async fn well_known_path(
        &self,
        key: WellKnownPath,
        app: Option<&str>,
        env: &HashMap<String, Option<String>>,
    ) -> Result<Utf8TypedPathBuf>;
    /// Clears target-side cached state.
    async fn clear_cache(&self) -> Result<()>;
    /// Lists extended attributes for a path.
    async fn xattrs(
        &self,
        path: Utf8TypedPath<'_>,
        namespace: XattrNamespace<'_>,
        follow: bool,
    ) -> Result<Vec<XattrEntry>>;
    /// Reads an extended attribute for a path.
    async fn xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> Result<Vec<u8>>;
    /// Creates or replaces an extended attribute for a path.
    async fn set_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        value: &[u8],
        follow: bool,
    ) -> Result<()>;
    /// Removes an extended attribute from a path.
    async fn remove_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> Result<()>;
    /// Lists alternate data streams for a path.
    async fn streams(&self, path: Utf8TypedPath<'_>, follow: bool) -> Result<Vec<StreamEntry>>;

    /// Removes a file or symlink.
    async fn remove(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> Result<()>;
    /// Returns metadata without following the final symlink.
    async fn metadata(&self, path: Utf8TypedPath<'_>) -> Result<Metadata>;
    /// Returns filesystem metadata for a path.
    async fn fs_metadata(&self, path: Utf8TypedPath<'_>, follow: bool) -> Result<FsMetadata>;
    /// Returns the POSIX ACL for a path.
    async fn acl(
        &self,
        path: Utf8TypedPath<'_>,
        default: bool,
        follow: bool,
    ) -> Result<Option<PosixAcl>>;
    /// Sets or removes the POSIX ACL for a path.
    async fn set_acl(
        &self,
        path: Utf8TypedPath<'_>,
        acl: Option<&PosixAcl>,
        default: bool,
        follow: bool,
    ) -> Result<()>;
    /// Returns the Windows security descriptor for a path.
    async fn sec_desc(&self, path: Utf8TypedPath<'_>, mask: u32, follow: bool) -> Result<SecDesc>;
    /// Replaces the Windows security descriptor for a path.
    async fn set_sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        sec_desc: &SecDesc,
        follow: bool,
    ) -> Result<()>;
    /// Creates a directory, optionally including missing parents.
    async fn create_dir(&self, path: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    /// Removes a directory.
    async fn remove_dir(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> Result<()>;
    /// Copies a path, optionally including directory contents.
    async fn copy(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    /// Renames a path.
    async fn rename(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        replace: bool,
    ) -> Result<()>;
    /// Moves a path, optionally including directory contents.
    async fn move_(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    /// Creates a symbolic link using `cwd` to interpret relative source paths.
    async fn symlink(
        &self,
        cwd: Utf8TypedPath<'_>,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> Result<()>;
    /// Creates a hard link.
    async fn hard_link(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    /// Creates a symbolic link to a directory.
    async fn symlink_dir(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    /// Creates a symbolic link to a file.
    async fn symlink_file(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    /// Returns metadata without following the final symlink.
    async fn symlink_metadata(&self, path: Utf8TypedPath<'_>) -> Result<Metadata>;
    /// Applies a metadata patch to every path.
    async fn set_metadata(&self, paths: &[Utf8TypedPathBuf], patch: MetadataPatch) -> Result<()>;
    /// Resolves a path to its canonical absolute form.
    async fn canonicalize(&self, path: Utf8TypedPath<'_>) -> Result<Utf8TypedPathBuf>;
    /// Returns the destination of a symbolic link.
    async fn read_link(&self, path: Utf8TypedPath<'_>) -> Result<Utf8TypedPathBuf>;
    /// Expands a glob pattern beneath `root`.
    async fn glob(
        &self,
        pattern: impl Into<String>,
        root: Utf8TypedPath<'_>,
        follow_symlinks: bool,
        max_depth: Option<usize>,
    ) -> Result<Vec<Utf8TypedPathBuf>>;
}

pub(crate) use process::{StdioRecv, StdioSend};

/// A file handle backed by either a remote [`Client`] or local [`Direct`].
#[derive(Debug)]
pub enum AnyFile {
    Client(client::ClientFile),
    Direct(DirectFile),
}

macro_rules! dispatch_file_mut {
    ($self:expr, $method:ident($($arg:expr),* $(,)?)) => {{
        match $self {
            AnyFile::Client(file) => Pin::new(file).$method($($arg),*),
            AnyFile::Direct(file) => Pin::new(file).$method($($arg),*),
        }
    }};
}

macro_rules! match_file {
    ($self:expr, $file:ident => $body:expr) => {{
        match $self {
            AnyFile::Client($file) => $body,
            AnyFile::Direct($file) => $body,
        }
    }};
}

impl AsyncRead for AnyFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_read(cx, buf))
    }
}

impl AsyncWrite for AnyFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_write(cx, buf))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_flush(cx))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_shutdown(cx))
    }
}

impl AsyncSeek for AnyFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        dispatch_file_mut!(self.as_mut().get_mut(), start_seek(position))
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_complete(cx))
    }
}

impl FileHandle for AnyFile {
    async fn to_stdio_send(&self) -> crate::Result<StdioSend> {
        match self {
            Self::Client(file) => file.to_stdio_send().await,
            Self::Direct(file) => file.to_stdio_send().await,
        }
    }

    async fn to_stdio_recv(&self) -> crate::Result<StdioRecv> {
        match self {
            Self::Client(file) => file.to_stdio_recv().await,
            Self::Direct(file) => file.to_stdio_recv().await,
        }
    }

    async fn close(self) -> crate::Result<()> {
        match self {
            Self::Client(file) => file.close().await,
            Self::Direct(file) => file.close().await,
        }
    }

    async fn set_size(&mut self, size: u64) -> crate::Result<()> {
        match_file!(self, file => file.set_size(size).await)
    }

    async fn metadata(&mut self) -> crate::Result<Metadata> {
        match_file!(self, file => file.metadata().await)
    }

    async fn fs_metadata(&mut self) -> crate::Result<FsMetadata> {
        match_file!(self, file => file.fs_metadata().await)
    }

    async fn acl(&mut self, default: bool) -> crate::Result<Option<PosixAcl>> {
        match_file!(self, file => file.acl(default).await)
    }

    async fn set_acl(&mut self, acl: Option<&PosixAcl>, default: bool) -> crate::Result<()> {
        match_file!(self, file => file.set_acl(acl, default).await)
    }

    async fn sec_desc(&mut self, mask: u32) -> crate::Result<SecDesc> {
        match_file!(self, file => file.sec_desc(mask).await)
    }

    async fn set_sec_desc(&mut self, sec_desc: &SecDesc) -> crate::Result<()> {
        match_file!(self, file => file.set_sec_desc(sec_desc).await)
    }

    async fn xattrs(&mut self, namespace: XattrNamespace<'_>) -> crate::Result<Vec<XattrEntry>> {
        match_file!(self, file => file.xattrs(namespace).await)
    }

    async fn xattr(&mut self, name: &str, namespace: Option<&str>) -> crate::Result<Vec<u8>> {
        match_file!(self, file => file.xattr(name, namespace).await)
    }

    async fn streams(&mut self) -> crate::Result<Vec<StreamEntry>> {
        match_file!(self, file => file.streams().await)
    }

    async fn set_xattr(
        &mut self,
        name: &str,
        namespace: Option<&str>,
        value: &[u8],
    ) -> crate::Result<()> {
        match_file!(self, file => file.set_xattr(name, namespace, value).await)
    }

    async fn remove_xattr(&mut self, name: &str, namespace: Option<&str>) -> crate::Result<()> {
        match_file!(self, file => file.remove_xattr(name, namespace).await)
    }

    async fn lock(&self, request: FileLockRequest) -> crate::Result<Option<FileLock>> {
        match self {
            Self::Client(file) => file.lock(request).await,
            Self::Direct(file) => file.lock(request).await,
        }
    }

    async fn try_into_std(self) -> std::result::Result<std::fs::File, Self> {
        match self {
            Self::Client(file) => file.try_into_std().await.map_err(Self::Client),
            Self::Direct(file) => file.try_into_std().await.map_err(Self::Direct),
        }
    }
}

pub enum AnyOpenOptions<'a> {
    Client(client::OpenOptions<'a>),
    Direct(DirectOpenOptions),
}

impl OpenOptions for AnyOpenOptions<'_> {
    type File = AnyFile;

    fn read(&mut self, read: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.read(read);
            }
            Self::Direct(opts) => {
                opts.read(read);
            }
        }
        self
    }

    fn write(&mut self, write: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.write(write);
            }
            Self::Direct(opts) => {
                opts.write(write);
            }
        }
        self
    }

    fn append(&mut self, append: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.append(append);
            }
            Self::Direct(opts) => {
                opts.append(append);
            }
        }
        self
    }

    fn create(&mut self, create: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.create(create);
            }
            Self::Direct(opts) => {
                opts.create(create);
            }
        }
        self
    }

    fn create_new(&mut self, create_new: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.create_new(create_new);
            }
            Self::Direct(opts) => {
                opts.create_new(create_new);
            }
        }
        self
    }

    fn truncate(&mut self, truncate: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.truncate(truncate);
            }
            Self::Direct(opts) => {
                opts.truncate(truncate);
            }
        }
        self
    }

    fn no_follow(&mut self, no_follow: bool) -> &mut Self {
        match self {
            Self::Client(opts) => {
                opts.no_follow(no_follow);
            }
            Self::Direct(opts) => {
                opts.no_follow(no_follow);
            }
        }
        self
    }

    async fn open(&self, path: Utf8TypedPath<'_>) -> crate::Result<AnyFile> {
        match self {
            Self::Client(opts) => OpenOptions::open(opts, path).await.map(AnyFile::Client),
            Self::Direct(opts) => OpenOptions::open(opts, path).await.map(AnyFile::Direct),
        }
    }
}

enum AnyCommandInner<'a> {
    Client(client::CommandBuilder<'a>),
    Direct(direct::DirectCommand<'a>),
}

/// Builds a process to spawn, relaying stdio across VFS domains when a
/// given endpoint cannot be consumed directly by the spawn target.
pub struct AnyCommand<'a> {
    inner: AnyCommandInner<'a>,
    vfs: AnyVfs,
    stdin: Option<StdioRecv>,
    stdout: Option<StdioSend>,
    stderr: Option<StdioSend>,
}

enum AnyChildInner {
    Client(client::ClientChild),
    Direct(Box<direct::DirectChild>),
}

/// Tasks pumping bytes between a foreign-domain stdio endpoint and a pipe
/// created in the spawn target's own domain, not yet started.
///
/// Relay tasks are only started once the underlying process has actually
/// been spawned, so a failure setting up the spawn itself just drops the
/// held endpoints (triggering their own best-effort cleanup) instead of
/// leaking a running task.
#[derive(Default)]
struct PendingRelays {
    inputs: Vec<(StdioRecv, StdioSend)>,
    outputs: Vec<(StdioRecv, StdioSend)>,
}

impl PendingRelays {
    fn start(self) -> ActiveRelays {
        let spawn_all = |pairs: Vec<(StdioRecv, StdioSend)>| {
            pairs
                .into_iter()
                .map(|(src, dst)| tokio::spawn(process::relay(src, dst)))
                .collect()
        };
        ActiveRelays {
            inputs: spawn_all(self.inputs),
            outputs: spawn_all(self.outputs),
        }
    }
}

#[derive(Default)]
struct ActiveRelays {
    inputs: Vec<JoinHandle<()>>,
    outputs: Vec<JoinHandle<()>>,
}

impl ActiveRelays {
    fn abort_inputs(&mut self) {
        for handle in self.inputs.drain(..) {
            handle.abort();
        }
    }

    /// Waits for output relays to finish flushing everything the child
    /// already produced before it exited. Input relays are aborted instead
    /// of awaited, since no further input can matter once the child is gone.
    ///
    /// Once the child has exited, its end of each output pipe is closed, so
    /// the relay's copy loop reaches EOF and returns on its own; this just
    /// ensures the caller can't observe a partially-relayed destination.
    async fn finish(&mut self) {
        self.abort_inputs();
        for handle in self.outputs.drain(..) {
            let _ = handle.await;
        }
    }

    /// Aborts every relay task without waiting, for use when giving up on
    /// the child without having confirmed it actually exited (e.g. `Drop`).
    fn abandon(&mut self) {
        self.abort_inputs();
        for handle in self.outputs.drain(..) {
            handle.abort();
        }
    }
}

impl Drop for ActiveRelays {
    fn drop(&mut self) {
        self.abandon();
    }
}

/// A spawned process, possibly relaying stdio across VFS domains.
pub struct AnyChild {
    inner: AnyChildInner,
    relays: ActiveRelays,
}

impl Child for AnyChild {
    async fn wait(&mut self) -> crate::Result<ProcessStatus> {
        let status = match &mut self.inner {
            AnyChildInner::Client(child) => child.wait().await,
            AnyChildInner::Direct(child) => child.wait().await,
        }?;
        self.relays.finish().await;
        Ok(status)
    }

    async fn terminate(mut self) -> crate::Result<Option<ProcessStatus>> {
        self.relays.abort_inputs();
        let result = match self.inner {
            AnyChildInner::Client(child) => child.terminate().await,
            AnyChildInner::Direct(child) => (*child).terminate().await,
        };
        if result.as_ref().is_ok_and(Option::is_some) {
            self.relays.finish().await;
        } else {
            self.relays.abandon();
        }
        result
    }
}

/// Returns whether `stdio` can be handed directly to a process spawned in
/// `target`'s domain without relaying.
fn is_direct_recv(target: &AnyVfs, stdio: &StdioRecv) -> bool {
    match (target, stdio) {
        (AnyVfs::Direct(_), StdioRecv::Native(_)) => true,
        (AnyVfs::Client(client), StdioRecv::Remote(remote)) => client.is_same_vfs(remote.client()),
        (AnyVfs::Client(client), StdioRecv::Native(_)) => client.mode() == SessionMode::Native,
        _ => false,
    }
}

/// Returns whether `stdio` can be handed directly to a process spawned in
/// `target`'s domain without relaying.
fn is_direct_send(target: &AnyVfs, stdio: &StdioSend) -> bool {
    match (target, stdio) {
        (AnyVfs::Direct(_), StdioSend::Native(_)) => true,
        (AnyVfs::Client(client), StdioSend::Remote(remote)) => client.is_same_vfs(remote.client()),
        (AnyVfs::Client(client), StdioSend::Native(_)) => client.mode() == SessionMode::Native,
        _ => false,
    }
}

/// Classifies a stdin endpoint for a process about to be spawned in
/// `target`'s domain, creating a relay pipe and queuing a pump task in
/// `relays` if the endpoint cannot be consumed directly.
async fn classify_recv(
    target: &AnyVfs,
    stdio: StdioRecv,
    relays: &mut PendingRelays,
) -> crate::Result<StdioRecv> {
    if is_direct_recv(target, &stdio) {
        return Ok(stdio);
    }
    let (send, recv) = target.pipe().await?;
    relays.inputs.push((stdio, send));
    Ok(recv)
}

/// Classifies a stdout/stderr endpoint for a process about to be spawned in
/// `target`'s domain, creating a relay pipe and queuing a pump task in
/// `relays` if the endpoint cannot be consumed directly.
async fn classify_send(
    target: &AnyVfs,
    stdio: StdioSend,
    relays: &mut PendingRelays,
) -> crate::Result<StdioSend> {
    if is_direct_send(target, &stdio) {
        return Ok(stdio);
    }
    let (send, recv) = target.pipe().await?;
    relays.outputs.push((recv, stdio));
    Ok(send)
}

impl<'a> Command for AnyCommand<'a> {
    type Child = AnyChild;
    type StdioSend = StdioSend;
    type StdioRecv = StdioRecv;

    fn arg(&mut self, arg: &str) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.arg(arg);
            }
            AnyCommandInner::Direct(builder) => {
                builder.arg(arg);
            }
        }
        self
    }

    fn env(&mut self, key: &str, val: &str) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.env(key, val);
            }
            AnyCommandInner::Direct(builder) => {
                builder.env(key, val);
            }
        }
        self
    }

    fn env_remove(&mut self, key: &str) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.env_remove(key);
            }
            AnyCommandInner::Direct(builder) => {
                builder.env_remove(key);
            }
        }
        self
    }

    fn current_dir(&mut self, dir: Utf8TypedPath<'_>) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.current_dir(dir);
            }
            AnyCommandInner::Direct(builder) => {
                builder.current_dir(dir);
            }
        }
        self
    }

    fn stdin(&mut self, stdio: StdioRecv) -> io::Result<&mut Self> {
        self.stdin = Some(stdio);
        Ok(self)
    }

    fn stdout(&mut self, stdio: StdioSend) -> io::Result<&mut Self> {
        self.stdout = Some(stdio);
        Ok(self)
    }

    fn stdin_inherit(&mut self) -> io::Result<&mut Self> {
        self.stdin = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stdin_inherit()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stdin_inherit()?;
            }
        }
        Ok(self)
    }

    fn stdout_inherit(&mut self) -> io::Result<&mut Self> {
        self.stdout = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stdout_inherit()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stdout_inherit()?;
            }
        }
        Ok(self)
    }

    fn stdout_inherit_stderr(&mut self) -> io::Result<&mut Self> {
        self.stdout = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stdout_inherit_stderr()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stdout_inherit_stderr()?;
            }
        }
        Ok(self)
    }

    fn stdin_null(&mut self) -> &mut Self {
        self.stdin = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stdin_null();
            }
            AnyCommandInner::Direct(builder) => {
                builder.stdin_null();
            }
        }
        self
    }

    fn stdout_null(&mut self) -> &mut Self {
        self.stdout = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stdout_null();
            }
            AnyCommandInner::Direct(builder) => {
                builder.stdout_null();
            }
        }
        self
    }

    fn stderr(&mut self, stdio: StdioSend) -> io::Result<&mut Self> {
        self.stderr = Some(stdio);
        Ok(self)
    }

    fn stderr_inherit(&mut self) -> io::Result<&mut Self> {
        self.stderr = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stderr_inherit()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stderr_inherit()?;
            }
        }
        Ok(self)
    }

    fn stderr_to_stdout(&mut self) -> io::Result<&mut Self> {
        self.stderr = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stderr_to_stdout()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stderr_to_stdout()?;
            }
        }
        Ok(self)
    }

    fn stderr_inherit_stdout(&mut self) -> io::Result<&mut Self> {
        self.stderr = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stderr_inherit_stdout()?;
            }
            AnyCommandInner::Direct(builder) => {
                builder.stderr_inherit_stdout()?;
            }
        }
        Ok(self)
    }

    fn stderr_null(&mut self) -> &mut Self {
        self.stderr = None;
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.stderr_null();
            }
            AnyCommandInner::Direct(builder) => {
                builder.stderr_null();
            }
        }
        self
    }

    fn process_control(&mut self, control: ProcessControl) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.process_control(control);
            }
            AnyCommandInner::Direct(builder) => {
                builder.process_control(control);
            }
        }
        self
    }

    fn termination_policy(&mut self, policy: TerminationPolicy) -> &mut Self {
        match &mut self.inner {
            AnyCommandInner::Client(builder) => {
                builder.termination_policy(policy);
            }
            AnyCommandInner::Direct(builder) => {
                builder.termination_policy(policy);
            }
        }
        self
    }

    async fn spawn(mut self) -> crate::Result<Self::Child> {
        let mut relays = PendingRelays::default();
        if let Some(stdio) = self.stdin.take() {
            let stdio = classify_recv(&self.vfs, stdio, &mut relays).await?;
            match &mut self.inner {
                AnyCommandInner::Client(builder) => {
                    builder.stdin(stdio)?;
                }
                AnyCommandInner::Direct(builder) => {
                    builder.stdin(stdio)?;
                }
            }
        }
        if let Some(stdio) = self.stdout.take() {
            let stdio = classify_send(&self.vfs, stdio, &mut relays).await?;
            match &mut self.inner {
                AnyCommandInner::Client(builder) => {
                    builder.stdout(stdio)?;
                }
                AnyCommandInner::Direct(builder) => {
                    builder.stdout(stdio)?;
                }
            }
        }
        if let Some(stdio) = self.stderr.take() {
            let stdio = classify_send(&self.vfs, stdio, &mut relays).await?;
            match &mut self.inner {
                AnyCommandInner::Client(builder) => {
                    builder.stderr(stdio)?;
                }
                AnyCommandInner::Direct(builder) => {
                    builder.stderr(stdio)?;
                }
            }
        }
        let inner = match self.inner {
            AnyCommandInner::Client(builder) => builder.spawn().await.map(AnyChildInner::Client),
            AnyCommandInner::Direct(builder) => builder
                .spawn()
                .await
                .map(Box::new)
                .map(AnyChildInner::Direct),
        }?;
        Ok(AnyChild {
            inner,
            relays: relays.start(),
        })
    }
}

/// A VFS backed by either a remote client or the local process.
#[derive(Clone)]
pub enum AnyVfs {
    Client(client::Client),
    Direct(Direct),
}

impl From<client::Client> for AnyVfs {
    fn from(value: client::Client) -> Self {
        Self::Client(value)
    }
}

impl From<Direct> for AnyVfs {
    fn from(value: Direct) -> Self {
        Self::Direct(value)
    }
}

impl AnyVfs {
    /// Returns the remote client when this is the remote variant.
    pub fn as_client(&self) -> Option<&client::Client> {
        match self {
            Self::Client(client) => Some(client),
            Self::Direct(_) => None,
        }
    }

    /// Returns the remote client when this is the remote variant.
    pub fn into_client(self) -> Option<client::Client> {
        match self {
            Self::Client(client) => Some(client),
            Self::Direct(_) => None,
        }
    }

    /// Calls a registered VFS extension, dispatching directly in-process or
    /// over RPC depending on which backend this `AnyVfs` wraps.
    pub async fn call_extension<T: VfsExtension>(
        &self,
        request: T::Request,
    ) -> Result<T::Response> {
        match self {
            Self::Client(client) => client.call_extension::<T>(request).await,
            Self::Direct(direct) => direct.call_extension::<T>(request).await,
        }
    }
}

impl Vfs for AnyVfs {
    type File = AnyFile;
    type StdioSend = StdioSend;
    type StdioRecv = StdioRecv;
    type OpenOptions<'a>
        = AnyOpenOptions<'a>
    where
        Self: 'a;
    type Command<'a>
        = AnyCommand<'a>
    where
        Self: 'a;

    fn env(&self) -> Box<dyn Iterator<Item = (String, String)> + '_> {
        match self {
            Self::Client(client) => client.env(),
            Self::Direct(direct) => direct.env(),
        }
    }

    fn cwd(&self) -> Utf8TypedPath<'_> {
        match self {
            Self::Client(vfs) => vfs.cwd(),
            Self::Direct(vfs) => vfs.cwd(),
        }
    }

    fn current_exe(&self) -> Utf8TypedPath<'_> {
        match self {
            Self::Client(vfs) => vfs.current_exe(),
            Self::Direct(vfs) => vfs.current_exe(),
        }
    }

    fn target(&self) -> &TargetInfo {
        match self {
            Self::Client(vfs) => vfs.target(),
            Self::Direct(vfs) => vfs.target(),
        }
    }

    fn security(&self) -> &SecurityInfo {
        match self {
            Self::Client(vfs) => vfs.security(),
            Self::Direct(vfs) => vfs.security(),
        }
    }

    fn extensions(&self) -> &ExtensionSet {
        match self {
            Self::Client(vfs) => vfs.extensions(),
            Self::Direct(vfs) => vfs.extensions(),
        }
    }

    fn open_options(&self) -> Self::OpenOptions<'_> {
        match self {
            Self::Client(client) => AnyOpenOptions::Client(client.open_options()),
            Self::Direct(direct) => AnyOpenOptions::Direct(direct.open_options()),
        }
    }

    fn command(&self, program: Utf8TypedPath<'_>) -> Self::Command<'_> {
        let inner = match self {
            Self::Client(client) => AnyCommandInner::Client(client.command(program)),
            Self::Direct(direct) => AnyCommandInner::Direct(direct.command(program)),
        };
        AnyCommand {
            inner,
            vfs: self.clone(),
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    async fn unix_socket(
        &self,
        path: Utf8TypedPath<'_>,
        key: Option<&[u8]>,
    ) -> crate::Result<AnyVfs> {
        match self {
            Self::Client(client) => client.unix_socket(path, key).await,
            Self::Direct(direct) => direct.unix_socket(path, key).await,
        }
    }

    async fn windows_admin(
        &self,
        cwd: Utf8TypedPath<'_>,
        env: HashMap<String, Option<String>>,
        elevate: bool,
    ) -> crate::Result<VfsSession> {
        match self {
            Self::Client(client) => client.windows_admin(cwd, env, elevate).await,
            Self::Direct(direct) => direct.windows_admin(cwd, env, elevate).await,
        }
    }

    async fn pipe(&self) -> crate::Result<(StdioSend, StdioRecv)> {
        match self {
            Self::Client(client) => client.pipe().await,
            Self::Direct(direct) => direct.pipe().await,
        }
    }

    async fn pipe_sized(&self, buf_size: Option<usize>) -> crate::Result<(StdioSend, StdioRecv)> {
        match self {
            Self::Client(client) => client.pipe_sized(buf_size).await,
            Self::Direct(direct) => direct.pipe_sized(buf_size).await,
        }
    }

    async fn user_name(&self, uid: u32) -> crate::Result<String> {
        match self {
            Self::Client(client) => client.user_name(uid).await,
            Self::Direct(direct) => direct.user_name(uid).await,
        }
    }

    async fn user_id(&self, name: &str) -> crate::Result<u32> {
        match self {
            Self::Client(client) => client.user_id(name).await,
            Self::Direct(direct) => direct.user_id(name).await,
        }
    }

    async fn group_name(&self, gid: u32) -> crate::Result<String> {
        match self {
            Self::Client(client) => client.group_name(gid).await,
            Self::Direct(direct) => direct.group_name(gid).await,
        }
    }

    async fn group_id(&self, name: &str) -> crate::Result<u32> {
        match self {
            Self::Client(client) => client.group_id(name).await,
            Self::Direct(direct) => direct.group_id(name).await,
        }
    }

    async fn sid_name(&self, sid: &Sid) -> crate::Result<SidName> {
        match self {
            Self::Client(client) => client.sid_name(sid).await,
            Self::Direct(direct) => direct.sid_name(sid).await,
        }
    }

    async fn account_name(&self, name: &str) -> crate::Result<SidName> {
        match self {
            Self::Client(client) => client.account_name(name).await,
            Self::Direct(direct) => direct.account_name(name).await,
        }
    }

    async fn read_dir(&self, path: Utf8TypedPath<'_>) -> crate::Result<ReadDir> {
        match self {
            Self::Client(client) => client.read_dir(path).await,
            Self::Direct(direct) => direct.read_dir(path).await,
        }
    }

    async fn which(
        &self,
        program: Utf8TypedPath<'_>,
        path: Option<&str>,
        cwd: Option<Utf8TypedPath<'_>>,
    ) -> crate::Result<Option<Utf8TypedPathBuf>> {
        match self {
            Self::Client(client) => Vfs::which(client, program, path, cwd).await,
            Self::Direct(direct) => Vfs::which(direct, program, path, cwd).await,
        }
    }

    async fn well_known_path(
        &self,
        key: WellKnownPath,
        app: Option<&str>,
        env: &HashMap<String, Option<String>>,
    ) -> crate::Result<Utf8TypedPathBuf> {
        match self {
            Self::Client(client) => Vfs::well_known_path(client, key, app, env).await,
            Self::Direct(direct) => Vfs::well_known_path(direct, key, app, env).await,
        }
    }

    async fn clear_cache(&self) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.clear_cache().await,
            Self::Direct(direct) => direct.clear_cache().await,
        }
    }

    async fn xattrs(
        &self,
        path: Utf8TypedPath<'_>,
        namespace: XattrNamespace<'_>,
        follow: bool,
    ) -> crate::Result<Vec<XattrEntry>> {
        match self {
            Self::Client(client) => client.xattrs(path, namespace, follow).await,
            Self::Direct(direct) => direct.xattrs(path, namespace, follow).await,
        }
    }

    async fn streams(
        &self,
        path: Utf8TypedPath<'_>,
        follow: bool,
    ) -> crate::Result<Vec<StreamEntry>> {
        match self {
            Self::Client(client) => client.streams(path, follow).await,
            Self::Direct(direct) => direct.streams(path, follow).await,
        }
    }

    async fn xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> crate::Result<Vec<u8>> {
        match self {
            Self::Client(client) => client.xattr(path, name, namespace, follow).await,
            Self::Direct(direct) => direct.xattr(path, name, namespace, follow).await,
        }
    }

    async fn set_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        value: &[u8],
        follow: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.set_xattr(path, name, namespace, value, follow).await,
            Self::Direct(direct) => direct.set_xattr(path, name, namespace, value, follow).await,
        }
    }

    async fn remove_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.remove_xattr(path, name, namespace, follow).await,
            Self::Direct(direct) => direct.remove_xattr(path, name, namespace, follow).await,
        }
    }

    async fn remove(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.remove(path, all, ignore).await,
            Self::Direct(direct) => direct.remove(path, all, ignore).await,
        }
    }

    async fn metadata(&self, path: Utf8TypedPath<'_>) -> crate::Result<Metadata> {
        match self {
            Self::Client(client) => client.metadata(path).await,
            Self::Direct(direct) => direct.metadata(path).await,
        }
    }

    async fn fs_metadata(
        &self,
        path: Utf8TypedPath<'_>,
        follow: bool,
    ) -> crate::Result<FsMetadata> {
        match self {
            Self::Client(client) => client.fs_metadata(path, follow).await,
            Self::Direct(direct) => direct.fs_metadata(path, follow).await,
        }
    }

    async fn acl(
        &self,
        path: Utf8TypedPath<'_>,
        default: bool,
        follow: bool,
    ) -> crate::Result<Option<PosixAcl>> {
        match self {
            Self::Client(client) => client.acl(path, default, follow).await,
            Self::Direct(direct) => direct.acl(path, default, follow).await,
        }
    }

    async fn set_acl(
        &self,
        path: Utf8TypedPath<'_>,
        acl: Option<&PosixAcl>,
        default: bool,
        follow: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.set_acl(path, acl, default, follow).await,
            Self::Direct(direct) => direct.set_acl(path, acl, default, follow).await,
        }
    }

    async fn sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        mask: u32,
        follow: bool,
    ) -> crate::Result<SecDesc> {
        match self {
            Self::Client(client) => client.sec_desc(path, mask, follow).await,
            Self::Direct(direct) => direct.sec_desc(path, mask, follow).await,
        }
    }

    async fn set_sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        sec_desc: &SecDesc,
        follow: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.set_sec_desc(path, sec_desc, follow).await,
            Self::Direct(direct) => direct.set_sec_desc(path, sec_desc, follow).await,
        }
    }

    async fn create_dir(&self, path: Utf8TypedPath<'_>, all: bool) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.create_dir(path, all).await,
            Self::Direct(direct) => direct.create_dir(path, all).await,
        }
    }

    async fn remove_dir(
        &self,
        path: Utf8TypedPath<'_>,
        all: bool,
        ignore: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.remove_dir(path, all, ignore).await,
            Self::Direct(direct) => direct.remove_dir(path, all, ignore).await,
        }
    }

    async fn copy(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        all: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.copy(from, to, all).await,
            Self::Direct(direct) => direct.copy(from, to, all).await,
        }
    }

    async fn rename(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        replace: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.rename(from, to, replace).await,
            Self::Direct(direct) => direct.rename(from, to, replace).await,
        }
    }

    async fn move_(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        all: bool,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.move_(from, to, all).await,
            Self::Direct(direct) => direct.move_(from, to, all).await,
        }
    }

    async fn symlink(
        &self,
        cwd: Utf8TypedPath<'_>,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.symlink(cwd, src, dst).await,
            Self::Direct(direct) => direct.symlink(cwd, src, dst).await,
        }
    }

    async fn hard_link(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.hard_link(src, dst).await,
            Self::Direct(direct) => direct.hard_link(src, dst).await,
        }
    }

    async fn symlink_dir(
        &self,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.symlink_dir(src, dst).await,
            Self::Direct(direct) => direct.symlink_dir(src, dst).await,
        }
    }

    async fn symlink_file(
        &self,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.symlink_file(src, dst).await,
            Self::Direct(direct) => direct.symlink_file(src, dst).await,
        }
    }

    async fn symlink_metadata(&self, path: Utf8TypedPath<'_>) -> crate::Result<Metadata> {
        match self {
            Self::Client(client) => client.symlink_metadata(path).await,
            Self::Direct(direct) => direct.symlink_metadata(path).await,
        }
    }

    async fn set_metadata(
        &self,
        paths: &[Utf8TypedPathBuf],
        patch: MetadataPatch,
    ) -> crate::Result<()> {
        match self {
            Self::Client(client) => client.set_metadata(paths, patch).await,
            Self::Direct(direct) => direct.set_metadata(paths, patch).await,
        }
    }

    async fn canonicalize(&self, path: Utf8TypedPath<'_>) -> crate::Result<Utf8TypedPathBuf> {
        match self {
            Self::Client(client) => client.canonicalize(path).await,
            Self::Direct(direct) => direct.canonicalize(path).await,
        }
    }

    async fn read_link(&self, path: Utf8TypedPath<'_>) -> crate::Result<Utf8TypedPathBuf> {
        match self {
            Self::Client(client) => client.read_link(path).await,
            Self::Direct(direct) => direct.read_link(path).await,
        }
    }

    async fn glob(
        &self,
        pattern: impl Into<String>,
        root: Utf8TypedPath<'_>,
        follow_symlinks: bool,
        max_depth: Option<usize>,
    ) -> crate::Result<Vec<Utf8TypedPathBuf>> {
        let pattern = pattern.into();

        match self {
            Self::Client(client) => client.glob(pattern, root, follow_symlinks, max_depth).await,
            Self::Direct(direct) => direct.glob(pattern, root, follow_symlinks, max_depth).await,
        }
    }
}

/// Client for connecting to the agent daemon and spawning processes.
pub(crate) use client::Client;
/// Builder for constructing spawn requests.
/// Agent server for VFS RPC connections.
pub(crate) use server::Server;
