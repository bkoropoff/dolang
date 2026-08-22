//! File-specific values shared by VFS implementations.

use std::{
    future::Future,
    io,
    mem::MaybeUninit,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use dolang_winterop::security::SecDesc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use typed_path::Utf8TypedPath;

use crate::{
    client, direct,
    error::{HandoffError, Result},
    metadata::{FsMetadata, Metadata},
    process::{StdioRecv, StdioSend},
    security::{Acl, AclKind},
    xattr::{XattrEntry, XattrNamespace},
};

/// Describes one alternate data stream associated with a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEntry {
    /// Stream name.
    pub name: String,
    /// Stream type reported by the target.
    pub r#type: String,
    /// Logical stream length in bytes.
    pub size: u64,
    /// Allocated stream size in bytes.
    pub alloc_size: u64,
}

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
    pub async fn release(&mut self) -> Result<()> {
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
/// An asynchronous file handle backed by either a remote [`Client`] or local [`Direct`].
///
/// File handles implement Tokio's asynchronous read, write, and seek traits.
#[derive(Debug)]
enum FileInner {
    Client(client::File),
    Direct(direct::File),
}

#[derive(Debug)]
pub struct File {
    inner: FileInner,
}

impl File {
    pub(crate) fn client(file: client::File) -> Self {
        Self {
            inner: FileInner::Client(file),
        }
    }

    pub(crate) fn direct(file: direct::File) -> Self {
        Self {
            inner: FileInner::Direct(file),
        }
    }
}

/// One of two futures, chosen at dispatch time.
///
/// The positional operations return `impl Future`, so a backend that dispatches
/// between two implementations has two distinct future types to reconcile and
/// cannot simply `match` inside an `async` block: the returned future captures
/// no lifetimes, so it cannot borrow the handle it came from. Boxing would
/// work; this avoids the allocation on what is meant to be the hot path.
pub(crate) enum EitherFuture<L, R> {
    Left(L),
    Right(R),
}

impl<T, L: Future<Output = T>, R: Future<Output = T>> Future for EitherFuture<L, R> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        // SAFETY: the projection is structural and neither variant is ever
        // moved out of, so the pinning guarantee carries through to whichever
        // future is inside.
        unsafe {
            match self.get_unchecked_mut() {
                Self::Left(future) => Pin::new_unchecked(future).poll(cx),
                Self::Right(future) => Pin::new_unchecked(future).poll(cx),
            }
        }
    }
}

macro_rules! dispatch_file_mut {
    ($self:expr, $method:ident($($arg:expr),* $(,)?)) => {{
        match &mut $self.inner {
            FileInner::Client(file) => Pin::new(file).$method($($arg),*),
            FileInner::Direct(file) => Pin::new(file).$method($($arg),*),
        }
    }};
}

macro_rules! match_file {
    ($self:expr, $file:ident => $body:expr) => {{
        match &$self.inner {
            FileInner::Client($file) => $body,
            FileInner::Direct($file) => $body,
        }
    }};
}

impl AsyncRead for File {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_read(cx, buf))
    }
}

impl AsyncWrite for File {
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

impl AsyncSeek for File {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        dispatch_file_mut!(self.as_mut().get_mut(), start_seek(position))
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        dispatch_file_mut!(self.as_mut().get_mut(), poll_complete(cx))
    }
}

/// Puts a handle recovered from a failed handoff back in the wrapper it was
/// dispatched out of.
pub(crate) fn rewrap<I, O>(error: HandoffError<I>, wrap: impl FnOnce(I) -> O) -> HandoffError<O> {
    let (handle, error) = error.into_parts();
    HandoffError::new(wrap(handle), error)
}

impl File {
    /// Consumes this handle, converting it into a standard-output or
    /// standard-error endpoint positioned at `offset`.
    ///
    /// The endpoint carries a position of its own and the cursor is kept on
    /// this side rather than in the kernel, so the position has to be stated
    /// explicitly: this handle's own is only the right answer when the caller
    /// is the one holding it, which is not the case for anything relaying on
    /// someone else's behalf.
    ///
    /// The handle is consumed because two live handles onto one description
    /// would each believe a cursor the other moves. Callers that want to keep
    /// reading or writing should open the file again instead.
    ///
    /// # Errors
    ///
    /// Returns [`HandoffError`], which carries the handle back. Nothing has
    /// been surrendered when it does — most importantly on the busy path,
    /// where operations are still in flight against the descriptor and the
    /// caller may simply retry once they finish.
    pub async fn into_stdio_send(
        self,
        offset: u64,
    ) -> std::result::Result<StdioSend, HandoffError<Self>> {
        match self.inner {
            FileInner::Client(file) => file
                .into_stdio_send(offset)
                .await
                .map_err(|error| rewrap(error, Self::client)),
            FileInner::Direct(file) => file
                .into_stdio_send(offset)
                .await
                .map_err(|error| rewrap(error, Self::direct)),
        }
    }

    /// Consumes this handle, converting it into a standard-input endpoint
    /// positioned at `offset`. See [`into_stdio_send`](Self::into_stdio_send).
    pub async fn into_stdio_recv(
        self,
        offset: u64,
    ) -> std::result::Result<StdioRecv, HandoffError<Self>> {
        match self.inner {
            FileInner::Client(file) => file
                .into_stdio_recv(offset)
                .await
                .map_err(|error| rewrap(error, Self::client)),
            FileInner::Direct(file) => file
                .into_stdio_recv(offset)
                .await
                .map_err(|error| rewrap(error, Self::direct)),
        }
    }

    /// Commits anything this handle is holding back from the target.
    ///
    /// The `&self` counterpart of [`AsyncWriteExt::flush`], which the
    /// positional operations need because they never take the handle
    /// exclusively. It is deliberately *not* called `flush`: a `&self` method
    /// of that name would win method resolution over `AsyncWriteExt`'s
    /// `&mut self` one at every existing call site, silently changing what
    /// `file.flush()` means. This does not settle work started through the
    /// poll surface; `poll_flush` owns that.
    ///
    /// [`AsyncWriteExt::flush`]: tokio::io::AsyncWriteExt::flush
    pub async fn commit(&self) -> Result<()> {
        match_file!(self, file => file.commit().await)
    }

    /// Closes this handle.
    pub async fn close(self) -> Result<()> {
        match self.inner {
            FileInner::Client(file) => file.close().await,
            FileInner::Direct(file) => file.close().await,
        }
    }

    /// Reads into `buf`'s spare capacity starting at `offset`, returning the
    /// transfer count.
    ///
    /// Cancelling the read may leave `buf` empty.
    ///
    /// Bytes are *appended* into the spare capacity rather than replacing the
    /// contents, so buffers recycle; pass a cleared buffer to read from the
    /// start.
    ///
    /// The transfer is capped by the spare capacity available, and may be
    /// shorter than that for reasons other than end of file — a partial page
    /// cache hit, a signal, a chunk boundary on the wire. A remote file caps
    /// every read at [`crate::MAX_FILE_READ`], because the peer must hold the whole
    /// reply before it can report a failure structurally. Zero means end of
    /// file; callers that need a specific count must loop.
    ///
    /// The returned future borrows the buffer but not the handle, so it may
    /// outlive the handle it came from and any number may be in flight on one
    /// handle at once.
    pub fn read_at<'b>(
        &self,
        buf: &'b mut BytesMut,
        offset: u64,
    ) -> impl Future<Output = Result<usize>> + Send + use<'b> {
        match &self.inner {
            FileInner::Client(file) => EitherFuture::Left(file.read_at(buf, offset)),
            FileInner::Direct(file) => EitherFuture::Right(file.read_at(buf, offset)),
        }
    }

    /// Writes `data` at `offset`, returning the byte count.
    ///
    /// May write less than all of `data`; callers that need it all must loop.
    /// Rejected on an append-mode handle, where the offset would be ignored
    /// and the data appended regardless; use [`append`](Self::append) there.
    pub fn write_at(
        &self,
        data: Bytes,
        offset: u64,
    ) -> impl Future<Output = Result<usize>> + Send + use<> {
        match &self.inner {
            FileInner::Client(file) => EitherFuture::Left(file.write_at(data, offset)),
            FileInner::Direct(file) => EitherFuture::Right(file.write_at(data, offset)),
        }
    }

    /// Appends `data`, returning the byte count and the position just past
    /// what was written.
    ///
    /// The resulting position is reported because the caller cannot compute
    /// it: an append lands wherever the end happened to be when it ran.
    pub fn append(&self, data: Bytes) -> impl Future<Output = Result<(usize, u64)>> + Send + use<> {
        match &self.inner {
            FileInner::Client(file) => EitherFuture::Left(file.append(data)),
            FileInner::Direct(file) => EitherFuture::Right(file.append(data)),
        }
    }

    // Forwarded explicitly rather than left to the trait's copying default,
    // which at a dispatch point would copy before reaching the backend that
    // wanted the borrow.
    /// Reads into the uninitialized `buf` starting at `offset`, returning how
    /// many bytes at its front were filled.
    ///
    /// The destination-borrowing counterpart of [`read_at`](Self::read_at),
    /// for callers whose buffer is not a [`BytesMut`].
    ///
    /// Only the returned count is initialized, and only on success. An error
    /// says nothing about how much of `buf` was written, but writing into
    /// uninitialized memory harms nothing: the caller has no count to act on,
    /// so the bytes are unreachable either way.
    ///
    /// Short transfers are contractual for the same reasons as `read_at`'s,
    /// with the copying route bounded additionally by the size of temporary it
    /// is willing to allocate. Zero means end of file.
    pub fn read_at_into<'b>(
        &self,
        buf: &'b mut [MaybeUninit<u8>],
        offset: u64,
    ) -> impl Future<Output = Result<usize>> + Send + use<'b> {
        match &self.inner {
            FileInner::Client(file) => EitherFuture::Left(file.read_at_into(buf, offset)),
            FileInner::Direct(file) => EitherFuture::Right(file.read_at_into(buf, offset)),
        }
    }

    /// Writes `data` at `offset` from borrowed storage, returning the byte
    /// count.
    ///
    /// The source-borrowing counterpart of [`write_at`](Self::write_at), for
    /// callers holding bytes they cannot cheaply turn into a [`Bytes`] —
    /// memory belonging to a garbage collector, say. A backend that can send
    /// from borrowed storage does; the default copies, which is exactly what
    /// such a caller would have done itself.
    ///
    /// Same short-write and append-mode rules as `write_at`.
    pub fn write_at_from<'b>(
        &self,
        data: &'b [u8],
        offset: u64,
    ) -> impl Future<Output = Result<usize>> + Send + use<'b> {
        match &self.inner {
            FileInner::Client(file) => EitherFuture::Left(file.write_at_from(data, offset)),
            FileInner::Direct(file) => EitherFuture::Right(file.write_at_from(data, offset)),
        }
    }

    /// Changes the file length to `size` bytes.
    pub async fn set_size(&self, size: u64) -> Result<()> {
        match_file!(self, file => file.set_size(size).await)
    }

    /// Returns metadata for the open file.
    pub async fn metadata(&self) -> Result<Metadata> {
        match_file!(self, file => file.metadata().await)
    }

    /// Returns metadata for the filesystem containing the open file.
    pub async fn fs_metadata(&self) -> Result<FsMetadata> {
        match_file!(self, file => file.fs_metadata().await)
    }

    /// Returns the ACL of the requested `kind`. For a POSIX ACL, `default`
    /// selects the directory's default ACL rather than its access ACL; it
    /// must be `false` for `AclKind::Nfs4`.
    pub async fn acl(&self, kind: AclKind, default: bool) -> Result<Option<Acl>> {
        match_file!(self, file => file.acl(kind, default).await)
    }

    /// Sets or removes the ACL of `kind`. `acl`, if present, must match
    /// `kind`. `default` selects the POSIX default ACL, as in
    /// [`acl`](Self::acl); it must be `false` for `AclKind::Nfs4`.
    pub async fn set_acl(&self, kind: AclKind, acl: Option<&Acl>, default: bool) -> Result<()> {
        match_file!(self, file => file.set_acl(kind, acl, default).await)
    }

    /// Returns the Windows security descriptor selected by `mask`.
    pub async fn sec_desc(&self, mask: dolang_winterop::security::SecInfo) -> Result<SecDesc> {
        match_file!(self, file => file.sec_desc(mask).await)
    }

    /// Replaces the Windows security descriptor.
    pub async fn set_sec_desc(&self, sec_desc: &SecDesc) -> Result<()> {
        match_file!(self, file => file.set_sec_desc(sec_desc).await)
    }

    /// Lists extended attributes in `namespace`.
    pub async fn xattrs(&self, namespace: XattrNamespace<'_>) -> Result<Vec<XattrEntry>> {
        match_file!(self, file => file.xattrs(namespace).await)
    }

    /// Reads one extended attribute.
    pub async fn xattr(&self, name: &str, namespace: Option<&str>) -> Result<Vec<u8>> {
        match_file!(self, file => file.xattr(name, namespace).await)
    }

    /// Lists alternate data streams.
    pub async fn streams(&self) -> Result<Vec<StreamEntry>> {
        match_file!(self, file => file.streams().await)
    }

    /// Creates or replaces an extended attribute.
    pub async fn set_xattr(&self, name: &str, namespace: Option<&str>, value: &[u8]) -> Result<()> {
        match_file!(self, file => file.set_xattr(name, namespace, value).await)
    }

    /// Removes an extended attribute.
    pub async fn remove_xattr(&self, name: &str, namespace: Option<&str>) -> Result<()> {
        match_file!(self, file => file.remove_xattr(name, namespace).await)
    }

    /// Acquires a byte-range lock according to `request`.
    pub async fn lock(&self, request: FileLockRequest) -> Result<Option<FileLock>> {
        match &self.inner {
            FileInner::Client(file) => file.lock(request).await,
            FileInner::Direct(file) => file.lock(request).await,
        }
    }

    /// Converts this handle into a local standard-library file when possible.
    pub async fn try_into_std(self) -> std::result::Result<std::fs::File, Self> {
        match self.inner {
            FileInner::Client(file) => file.try_into_std().await.map_err(Self::client),
            FileInner::Direct(file) => file.try_into_std().await.map_err(Self::direct),
        }
    }
}

/// Configures and opens a file on one [`Vfs`] backend.
enum OpenOptionsInner<'a> {
    Client(client::OpenOptions<'a>),
    Direct(direct::OpenOptions),
}

pub struct OpenOptions<'a> {
    inner: OpenOptionsInner<'a>,
}

impl<'a> OpenOptions<'a> {
    pub(crate) fn client(options: client::OpenOptions<'a>) -> Self {
        Self {
            inner: OpenOptionsInner::Client(options),
        }
    }

    pub(crate) fn direct(options: direct::OpenOptions) -> Self {
        Self {
            inner: OpenOptionsInner::Direct(options),
        }
    }
}

impl OpenOptions<'_> {
    /// Enables or disables read access.
    pub fn read(&mut self, read: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.read(read);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.read(read);
            }
        }
        self
    }

    /// Enables or disables write access.
    pub fn write(&mut self, write: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.write(write);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.write(write);
            }
        }
        self
    }

    /// Enables or disables append mode.
    pub fn append(&mut self, append: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.append(append);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.append(append);
            }
        }
        self
    }

    /// Enables or disables creation when the file is absent.
    pub fn create(&mut self, create: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.create(create);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.create(create);
            }
        }
        self
    }

    /// Enables or disables exclusive creation.
    pub fn create_new(&mut self, create_new: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.create_new(create_new);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.create_new(create_new);
            }
        }
        self
    }

    /// Enables or disables truncation when opening.
    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.truncate(truncate);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.truncate(truncate);
            }
        }
        self
    }

    /// Enables or disables following the final path component when it is a link.
    pub fn no_follow(&mut self, no_follow: bool) -> &mut Self {
        match &mut self.inner {
            OpenOptionsInner::Client(opts) => {
                opts.no_follow(no_follow);
            }
            OpenOptionsInner::Direct(opts) => {
                opts.no_follow(no_follow);
            }
        }
        self
    }

    /// Opens `path` using the configured options.
    pub async fn open(&self, path: Utf8TypedPath<'_>) -> Result<File> {
        match &self.inner {
            OpenOptionsInner::Client(opts) => client::OpenOptions::open(opts, path).await,
            OpenOptionsInner::Direct(opts) => direct::OpenOptions::open(opts, path)
                .await
                .map(File::direct),
        }
    }
}
