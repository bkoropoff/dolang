#![deny(warnings)]
#![allow(async_fn_in_trait)]
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
//!     let vfs = Direct::default();
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
pub mod client;
pub mod direct;
pub mod directory;
pub mod error;
pub mod extension;
pub mod file;
pub mod metadata;
pub mod path;
mod posix_acl;
mod probe;
pub mod process;
mod protocol;
pub mod security;
pub mod server;
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
use session::{Query, VfsSession};
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
    async fn to_stdio_send(&self) -> Result<StdioSend>;
    async fn to_stdio_recv(&self) -> Result<StdioRecv>;
    async fn close(self) -> Result<()>;
    async fn set_size(&mut self, size: u64) -> Result<()>;
    async fn metadata(&mut self) -> Result<Metadata>;
    async fn fs_metadata(&mut self) -> Result<FsMetadata>;
    async fn acl(&mut self, default: bool) -> Result<Option<PosixAcl>>;
    async fn set_acl(&mut self, acl: Option<&PosixAcl>, default: bool) -> Result<()>;
    async fn sec_desc(&mut self, mask: u32) -> Result<SecDesc>;
    async fn set_sec_desc(&mut self, sec_desc: &SecDesc) -> Result<()>;
    async fn xattrs(&mut self, namespace: XattrNamespace<'_>) -> Result<Vec<XattrEntry>>;
    async fn xattr(&mut self, name: &str, namespace: Option<&str>) -> Result<Vec<u8>>;
    async fn streams(&mut self) -> Result<Vec<StreamEntry>>;
    async fn set_xattr(&mut self, name: &str, namespace: Option<&str>, value: &[u8]) -> Result<()>;
    async fn remove_xattr(&mut self, name: &str, namespace: Option<&str>) -> Result<()>;
    async fn lock(&self, request: FileLockRequest) -> Result<Option<FileLock>>;
    async fn try_into_std(self) -> std::result::Result<std::fs::File, Self>;
}

#[allow(async_fn_in_trait)]
/// A spawned process owned by a [`Command`] backend.
pub trait Child {
    async fn wait(&mut self) -> Result<ProcessStatus>;
    async fn terminate(self) -> Result<Option<ProcessStatus>>
    where
        Self: Sized;
}

#[allow(async_fn_in_trait)]
/// Configures and spawns a process on a [`Vfs`] backend.
pub trait Command {
    type Child: Child;
    type StdioSend: AsyncWrite + Unpin;
    type StdioRecv: AsyncRead + Unpin;

    fn arg(&mut self, arg: &str) -> &mut Self;
    fn env(&mut self, key: &str, val: &str) -> &mut Self;
    fn env_remove(&mut self, key: &str) -> &mut Self;
    fn current_dir(&mut self, dir: Utf8TypedPath<'_>) -> &mut Self;
    fn stdin(&mut self, stdio: Self::StdioRecv) -> io::Result<&mut Self>;
    fn stdout(&mut self, stdio: Self::StdioSend) -> io::Result<&mut Self>;
    /// Inherit the host process's standard input.
    ///
    /// Opaque remote clients treat terminal input as null because Tokio cannot
    /// cancel an outstanding terminal read. Redirected input is relayed to the
    /// remote process.
    fn stdin_inherit(&mut self) -> io::Result<&mut Self>;
    fn stdout_inherit(&mut self) -> io::Result<&mut Self>;
    fn stdin_null(&mut self) -> &mut Self;
    fn stdout_null(&mut self) -> &mut Self;
    fn stderr(&mut self, stdio: Self::StdioSend) -> io::Result<&mut Self>;
    fn stderr_inherit(&mut self) -> io::Result<&mut Self>;
    fn stderr_inherit_stdout(&mut self) -> io::Result<&mut Self>;
    fn stderr_null(&mut self) -> &mut Self;
    fn process_control(&mut self, control: ProcessControl) -> &mut Self;
    fn termination_policy(&mut self, policy: TerminationPolicy) -> &mut Self;
    async fn spawn(self) -> Result<Self::Child>;
}

#[allow(async_fn_in_trait)]
/// A filesystem and process-execution backend.
///
/// Implementations may be local, remote, or a dispatcher over either. A
/// value's path arguments always use the target's syntax; consult
/// [`Query::target`] when selecting one for a remote VFS.
pub trait Vfs {
    type File: FileHandle;
    type StdioSend: AsyncWrite + Unpin;
    type StdioRecv: AsyncRead + Unpin;
    type OpenOptions<'a>: OpenOptions<File = Self::File>
    where
        Self: 'a;
    type Command<'a>: Command<StdioSend = Self::StdioSend, StdioRecv = Self::StdioRecv>
    where
        Self: 'a;

    fn open_options(&self) -> Self::OpenOptions<'_>;
    fn command(&self, program: Utf8TypedPath<'_>) -> Self::Command<'_>;
    async fn unix_socket(&self, path: Utf8TypedPath<'_>) -> Result<AnyVfs>;
    async fn windows_admin(
        &self,
        cwd: Utf8TypedPath<'_>,
        env: HashMap<String, Option<String>>,
        elevate: bool,
    ) -> Result<VfsSession>;
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
    async fn query(&self) -> Result<Query>;
    async fn user_name(&self, uid: u32) -> Result<String>;
    async fn user_id(&self, name: &str) -> Result<u32>;
    async fn group_name(&self, gid: u32) -> Result<String>;
    async fn group_id(&self, name: &str) -> Result<u32>;
    async fn sid_name(&self, sid: &Sid) -> Result<SidName>;
    async fn account_name(&self, name: &str) -> Result<SidName>;
    async fn read_dir(&self, path: Utf8TypedPath<'_>) -> Result<ReadDir>;
    async fn which(
        &self,
        program: Utf8TypedPath<'_>,
        path: Option<&str>,
        cwd: Option<Utf8TypedPath<'_>>,
    ) -> Result<Option<Utf8TypedPathBuf>>;
    async fn well_known_path(
        &self,
        key: WellKnownPath,
        app: Option<&str>,
        env: &HashMap<String, Option<String>>,
    ) -> Result<Utf8TypedPathBuf>;
    async fn clear_cache(&self) -> Result<()>;
    async fn xattrs(
        &self,
        path: Utf8TypedPath<'_>,
        namespace: XattrNamespace<'_>,
        follow: bool,
    ) -> Result<Vec<XattrEntry>>;
    async fn xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> Result<Vec<u8>>;
    async fn set_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        value: &[u8],
        follow: bool,
    ) -> Result<()>;
    async fn remove_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> Result<()>;
    async fn streams(&self, path: Utf8TypedPath<'_>, follow: bool) -> Result<Vec<StreamEntry>>;

    async fn remove(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> Result<()>;
    async fn metadata(&self, path: Utf8TypedPath<'_>) -> Result<Metadata>;
    async fn fs_metadata(&self, path: Utf8TypedPath<'_>, follow: bool) -> Result<FsMetadata>;
    async fn acl(
        &self,
        path: Utf8TypedPath<'_>,
        default: bool,
        follow: bool,
    ) -> Result<Option<PosixAcl>>;
    async fn set_acl(
        &self,
        path: Utf8TypedPath<'_>,
        acl: Option<&PosixAcl>,
        default: bool,
        follow: bool,
    ) -> Result<()>;
    async fn sec_desc(&self, path: Utf8TypedPath<'_>, mask: u32, follow: bool) -> Result<SecDesc>;
    async fn set_sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        sec_desc: &SecDesc,
        follow: bool,
    ) -> Result<()>;
    async fn create_dir(&self, path: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    async fn remove_dir(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> Result<()>;
    async fn copy(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    async fn rename(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        replace: bool,
    ) -> Result<()>;
    async fn move_(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>, all: bool) -> Result<()>;
    async fn symlink(
        &self,
        cwd: Utf8TypedPath<'_>,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> Result<()>;
    async fn hard_link(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    async fn symlink_dir(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    async fn symlink_file(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> Result<()>;
    async fn symlink_metadata(&self, path: Utf8TypedPath<'_>) -> Result<Metadata>;
    async fn set_metadata(&self, paths: &[Utf8TypedPathBuf], patch: MetadataPatch) -> Result<()>;
    async fn canonicalize(&self, path: Utf8TypedPath<'_>) -> Result<Utf8TypedPathBuf>;
    async fn read_link(&self, path: Utf8TypedPath<'_>) -> Result<Utf8TypedPathBuf>;
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

impl Default for AnyVfs {
    fn default() -> Self {
        Self::Direct(Direct::default())
    }
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

    async fn unix_socket(&self, path: Utf8TypedPath<'_>) -> crate::Result<AnyVfs> {
        match self {
            Self::Client(client) => client.unix_socket(path).await,
            Self::Direct(direct) => direct.unix_socket(path).await,
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

    async fn query(&self) -> crate::Result<Query> {
        match self {
            Self::Client(client) => client.query().await,
            Self::Direct(direct) => direct.query().await,
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
