use std::{
    collections::HashMap,
    future::Future,
    io,
    io::IsTerminal,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

#[cfg(unix)]
use std::os::unix::{
    io::{AsFd, OwnedFd},
    net::UnixStream as StdUnixStream,
};
#[cfg(windows)]
use std::os::windows::io::{AsHandle, OwnedHandle};
#[cfg(all(docsrs, not(windows)))]
struct OwnedHandle;

#[cfg(unix)]
use dolang_rpc::AuthKey;
use dolang_rpc::{
    client::Call,
    handle::{DefaultHandle, OsHandle},
    session::{Cite, Gift},
    trailer::{TrailerRecv, TrailerSend},
};
use dolang_winterop::security::{SecDesc, Sid};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(all(docsrs, not(windows)))]
struct NamedPipeServer;
use tokio::{
    io::{AsyncRead, AsyncSeek, AsyncWrite, AsyncWriteExt, ReadBuf},
    task::JoinHandle,
};

use crate::extension::VfsExtension;
#[cfg(unix)]
use crate::protocol::AccessRequest;
use crate::session::Query;
use crate::{
    Child, Command, FileHandle, FsMetadata, Metadata, MetadataPatch, PosixAcl, ProcessStatus,
    ReadDir, SessionMode, SidName, StdioRecv, StdioSend, StreamEntry, Utf8TypedPath,
    Utf8TypedPathBuf, Vfs, XattrEntry,
    direct::DirectFile,
    path::WellKnownPath,
    protocol::{
        AclRequest, CanonicalizeRequest, CopyRequest, CreateDirRequest, ExtensionRequest,
        ExtensionResponse, FsMetadataRequest, GlobRequest, HardLinkRequest, MetadataRequest,
        MoveRequest, OpenHandle, OpenHandlePreference, OpenRequest, OpenVfsHandle, QueryResponse,
        ReadLinkRequest, RemoveDirRequest, RemoveRequest, RenameRequest, Request, RequestKind,
        ResponseKind, SecDescRequest, SetAclRequest, SetMetadataRequest, SetSecDescRequest,
        SetXattrRequest, SpawnRequest, StdioRecvTarget, StdioSendTarget, StreamsRequest,
        SymlinkKind, SymlinkRequest, UnixVfsRequest, VfsProtocol, WellKnownPathRequest,
        WindowsAdminRequest, WirePath, XattrNamespaceRequest, XattrRequest, XattrsRequest,
        rpc_builder,
    },
};

/// Client for a VFS agent session.
///
/// Clones share one RPC connection. Generic-stream constructors create an
/// opaque-only session; Unix-socket and Windows named-pipe constructors can
/// use native handles when the peer supports them. Prefer the [`Vfs`]
/// trait when code should work with local and remote backends alike.
#[derive(Clone)]
pub struct Client {
    shared: Arc<ClientShared>,
    vfs: Option<Gift<crate::session::VfsMarker>>,
}

struct ClientShared {
    rpc: dolang_rpc::client::Client<VfsProtocol>,
    mode: SessionMode,
    query: Query,
}

/// A file handle returned by a [`Client`] operation.
///
/// Depending on the transport and the server's choice, this may hold a native
/// local file descriptor/handle or an opaque remote file reference. It
/// implements [`FileHandle`] in either case.
pub struct ClientFile(ClientFileInner);

enum ClientFileInner {
    Direct(DirectFile),
    Remote(RemoteFile),
}

struct RemoteFile {
    client: Client,
    file: Gift<crate::session::FileMarker>,
    pending: Option<PendingFileOperation>,
    read_body: Option<PendingTrailerRead>,
    write_body: Option<PendingTrailerWrite>,
}

pub(crate) struct RemoteFileLock {
    client: Client,
    file: Gift<crate::session::FileMarker>,
    lock: Option<u64>,
}

impl RemoteFileLock {
    pub(crate) async fn release(&mut self) -> crate::Result<()> {
        let Some(lock) = self.lock else {
            return Ok(());
        };
        match self
            .client
            .request(RequestKind::FileUnlock {
                file: self.file.cite(),
                lock,
            })
            .await?
        {
            ResponseKind::FileUnlock(result) => {
                result.map_err(crate::Error::from)?;
                self.lock = None;
                Ok(())
            }
            response => Err(unexpected(response).into()),
        }
    }
}

impl Drop for RemoteFileLock {
    fn drop(&mut self) {
        let Some(lock) = self.lock.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let client = self.client.clone();
        let file = self.file.cite();
        runtime.spawn(async move {
            let _ = client.request(RequestKind::FileUnlock { file, lock }).await;
        });
    }
}

struct PendingFileOperation {
    kind: FileOperationKind,
    call: Call<VfsProtocol>,
}

struct PendingTrailerWrite {
    send: Option<TrailerSend<Call<VfsProtocol>>>,
    call: Option<Call<VfsProtocol>>,
    target: usize,
    sent: usize,
    unreported: usize,
}

struct PendingTrailerRead {
    recv: TrailerRecv,
    remaining: usize,
    read: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileOperationKind {
    Read,
    Flush,
    Seek,
}

impl std::fmt::Debug for ClientFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ClientFile").field(&self.0).finish()
    }
}

impl std::fmt::Debug for ClientFileInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct(file) => file.fmt(f),
            Self::Remote(file) => file.fmt(f),
        }
    }
}

impl std::fmt::Debug for RemoteFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteFile")
            .field("file", &self.file)
            .field(
                "pending",
                &self.pending.as_ref().map(|pending| pending.kind),
            )
            .finish_non_exhaustive()
    }
}

impl ClientFile {
    fn from_std(file: std::fs::File, read: bool, write: bool, append: bool) -> Self {
        Self(ClientFileInner::Direct(DirectFile::from_std(
            file, read, write, append,
        )))
    }

    fn from_remote(client: Client, file: Gift<crate::session::FileMarker>) -> Self {
        Self(ClientFileInner::Remote(RemoteFile {
            client,
            file,
            pending: None,
            read_body: None,
            write_body: None,
        }))
    }
}

impl RemoteFile {
    fn cite(&self) -> Cite<crate::session::FileMarker> {
        self.file.cite()
    }

    fn poll_request(
        &mut self,
        cx: &mut Context<'_>,
        kind: FileOperationKind,
        request: impl FnOnce(Cite<crate::session::FileMarker>) -> (RequestKind, Option<Vec<u8>>),
    ) -> Poll<io::Result<(ResponseKind, Option<TrailerRecv>)>> {
        if self.pending.is_none() {
            let (request, trailer) = request(self.cite());
            self.pending = Some(PendingFileOperation {
                kind,
                call: {
                    assert!(trailer.is_none());
                    self.client.call(request)
                },
            });
        }
        let pending = self.pending.as_mut().unwrap();
        if pending.kind != kind {
            return Poll::Ready(Err(io::Error::other(format!(
                "file operation {:?} polled while {:?} is pending",
                kind, pending.kind
            ))));
        }
        match Pin::new(&mut pending.call).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.pending = None;
                let (response, trailer) = result.map_err(rpc_error)?.into_response_trailer();
                Poll::Ready(match response {
                    ResponseKind::Error(error) => Err(wire_io(error)),
                    response => Ok((response, trailer)),
                })
            }
        }
    }

    fn idle(&self) -> crate::Result<()> {
        if self
            .read_body
            .as_ref()
            .is_some_and(|body| body.remaining != 0)
            || self.write_body.is_some()
        {
            Err(io::Error::other("file trailer operation is still pending").into())
        } else if let Some(pending) = &self.pending {
            Err(io::Error::other(format!(
                "file operation {:?} is still pending",
                pending.kind
            ))
            .into())
        } else {
            Ok(())
        }
    }

    async fn cancel_pending(&mut self) {
        self.read_body.take();
        if let Some(mut pending) = self.write_body.take()
            && let Some(mut call) = pending.call.take()
        {
            call.cancel();
            let _ = call.await;
        }
        if let Some(mut pending) = self.pending.take() {
            pending.call.cancel();
            let _ = pending.call.await;
        }
    }
}

impl AsyncRead for ClientFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).poll_read(cx, buf),
            ClientFileInner::Remote(file) => loop {
                if buf.remaining() == 0 {
                    return Poll::Ready(Ok(()));
                }
                if let Some(body) = file.read_body.as_mut() {
                    let before = buf.filled().len();
                    match Pin::new(&mut body.recv).poll_read(cx, buf) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Err(error)) => {
                            file.read_body = None;
                            return Poll::Ready(Err(error));
                        }
                        Poll::Ready(Ok(())) => {
                            let read = buf.filled().len() - before;
                            if read > body.remaining {
                                file.read_body = None;
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "file read response exceeds requested length",
                                )));
                            }
                            body.remaining -= read;
                            body.read += read;
                            if read > 0 {
                                return Poll::Ready(Ok(()));
                            }
                            let empty = body.read == 0;
                            file.read_body = None;
                            if empty {
                                return Poll::Ready(Ok(()));
                            }
                            continue;
                        }
                    }
                }
                let requested = buf.remaining();
                match file.poll_request(cx, FileOperationKind::Read, |file| {
                    (
                        RequestKind::FileRead {
                            file,
                            len: requested,
                        },
                        None,
                    )
                }) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok((ResponseKind::FileRead(result), trailer))) => {
                        if let Err(error) = result.map_err(wire_io) {
                            return Poll::Ready(Err(error));
                        }
                        let Some(trailer) = trailer else {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "file read response is missing its data trailer",
                            )));
                        };
                        file.read_body = Some(PendingTrailerRead {
                            recv: trailer,
                            remaining: requested,
                            read: 0,
                        });
                    }
                    Poll::Ready(Ok((response, _))) => {
                        return Poll::Ready(Err(unexpected(response)));
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                }
            },
        }
    }
}

impl AsyncWrite for ClientFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).poll_write(cx, buf),
            ClientFileInner::Remote(file) => {
                if buf.is_empty() {
                    return Poll::Ready(Ok(0));
                }
                if file.write_body.is_none() {
                    file.write_body = Some(PendingTrailerWrite {
                        send: Some(
                            file.client
                                .call_with_trailer(RequestKind::FileWrite { file: file.cite() }),
                        ),
                        call: None,
                        target: buf.len(),
                        sent: 0,
                        unreported: 0,
                    });
                }
                let pending = file.write_body.as_mut().unwrap();
                if let Some(call) = pending.call.as_mut() {
                    match Pin::new(call).poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(result) => {
                            let target = pending.target;
                            let unreported = pending.unreported;
                            file.write_body = None;
                            let response = result.map_err(rpc_error)?.into_response();
                            match response {
                                ResponseKind::Error(error) => {
                                    return Poll::Ready(Err(wire_io(error)));
                                }
                                ResponseKind::FileWrite(result) => {
                                    let written = result.map_err(wire_io)?;
                                    if written != target {
                                        return Poll::Ready(Err(io::Error::new(
                                            io::ErrorKind::InvalidData,
                                            "file write response does not acknowledge the submitted trailer",
                                        )));
                                    }
                                    return Poll::Ready(Ok(unreported));
                                }
                                response => return Poll::Ready(Err(unexpected(response))),
                            }
                        }
                    }
                }
                let remaining = pending.target - pending.sent;
                let send = pending.send.as_mut().unwrap();
                match Pin::new(send).poll_write(cx, &buf[..buf.len().min(remaining)]) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                    Poll::Ready(Ok(n)) => {
                        pending.sent += n;
                        if pending.sent == pending.target {
                            let send = pending.send.take().unwrap();
                            pending.call = Some(send.finish());
                            pending.unreported = n;
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(n))
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).poll_flush(cx),
            ClientFileInner::Remote(file) => {
                if let Some(pending) = file.write_body.as_mut() {
                    if pending.call.is_none() {
                        let send = pending.send.take().unwrap();
                        pending.call = Some(send.finish());
                    }
                    let call = pending.call.as_mut().unwrap();
                    return match Pin::new(call).poll(cx) {
                        Poll::Pending => Poll::Pending,
                        Poll::Ready(result) => {
                            file.write_body = None;
                            Poll::Ready(result.map_err(rpc_error).and_then(|result| {
                                match result.into_response() {
                                    ResponseKind::FileWrite(result) => {
                                        result.map(|_| ()).map_err(wire_io)
                                    }
                                    ResponseKind::Error(error) => Err(wire_io(error)),
                                    response => Err(unexpected(response)),
                                }
                            }))
                        }
                    };
                }
                match file.poll_request(cx, FileOperationKind::Flush, |file| {
                    (RequestKind::FileFlush { file }, None)
                }) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok((ResponseKind::FileFlush(result), _))) => {
                        Poll::Ready(result.map_err(wire_io))
                    }
                    Poll::Ready(Ok((response, _))) => Poll::Ready(Err(unexpected(response))),
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                }
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).poll_shutdown(cx),
            ClientFileInner::Remote(_) => self.as_mut().poll_flush(cx),
        }
    }
}

impl AsyncSeek for ClientFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).start_seek(position),
            ClientFileInner::Remote(file) => {
                if file
                    .read_body
                    .as_ref()
                    .is_some_and(|body| body.remaining != 0)
                {
                    file.read_body.take();
                }
                file.idle().map_err(crate::Error::into_io_error)?;
                file.pending = Some(PendingFileOperation {
                    kind: FileOperationKind::Seek,
                    call: file.client.call(RequestKind::FileSeek {
                        file: file.cite(),
                        position: position.into(),
                    }),
                });
                Ok(())
            }
        }
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => Pin::new(file).poll_complete(cx),
            ClientFileInner::Remote(file) => {
                match file.poll_request(cx, FileOperationKind::Seek, |file| {
                    (
                        RequestKind::FileSeek {
                            file,
                            position: io::SeekFrom::Current(0).into(),
                        },
                        None,
                    )
                }) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Ok((ResponseKind::FileSeek(result), _))) => {
                        Poll::Ready(result.map_err(wire_io))
                    }
                    Poll::Ready(Ok((response, _))) => Poll::Ready(Err(unexpected(response))),
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                }
            }
        }
    }
}

impl FileHandle for ClientFile {
    async fn to_stdio_send(&self) -> crate::Result<StdioSend> {
        match &self.0 {
            ClientFileInner::Direct(file) => file.to_stdio_send().await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileToStdioSend { file: file.cite() })
                    .await?
                {
                    ResponseKind::FileToStdioSend(result) => result
                        .map(|stdio| {
                            StdioSend::Remote(RemoteStdioSend {
                                client: file.client.clone(),
                                stdio: Some(stdio),
                                pending: None,
                                write_body: None,
                            })
                        })
                        .map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn to_stdio_recv(&self) -> crate::Result<StdioRecv> {
        match &self.0 {
            ClientFileInner::Direct(file) => file.to_stdio_recv().await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileToStdioRecv { file: file.cite() })
                    .await?
                {
                    ResponseKind::FileToStdioRecv(result) => result
                        .map(|stdio| {
                            StdioRecv::Remote(RemoteStdioRecv {
                                client: file.client.clone(),
                                stdio: Some(stdio),
                                pending: None,
                                read_body: None,
                            })
                        })
                        .map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn close(self) -> crate::Result<()> {
        match self.0 {
            ClientFileInner::Direct(file) => file.close().await,
            ClientFileInner::Remote(mut file) => {
                file.cancel_pending().await;
                match file
                    .client
                    .request(RequestKind::FileClose {
                        file: file.file.cite(),
                    })
                    .await?
                {
                    ResponseKind::FileClose(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn set_size(&mut self, size: u64) -> crate::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.set_size(size).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileSetSize {
                        file: file.cite(),
                        size,
                    })
                    .await?
                {
                    ResponseKind::FileSetSize(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn metadata(&mut self) -> crate::Result<Metadata> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.metadata().await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileMetadata { file: file.cite() })
                    .await?
                {
                    ResponseKind::FileMetadata(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn fs_metadata(&mut self) -> crate::Result<FsMetadata> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.fs_metadata().await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileFsMetadata { file: file.cite() })
                    .await?
                {
                    ResponseKind::FileFsMetadata(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn acl(&mut self, default: bool) -> crate::Result<Option<PosixAcl>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.acl(default).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileAcl {
                        file: file.cite(),
                        default,
                    })
                    .await?
                {
                    ResponseKind::FileAcl(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn set_acl(&mut self, acl: Option<&PosixAcl>, default: bool) -> crate::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.set_acl(acl, default).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileSetAcl {
                        file: file.cite(),
                        acl: acl.cloned(),
                        default,
                    })
                    .await?
                {
                    ResponseKind::FileSetAcl(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn sec_desc(
        &mut self,
        mask: dolang_winterop::security::SecInfo,
    ) -> crate::Result<SecDesc> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.sec_desc(mask).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileSecDesc {
                        file: file.cite(),
                        mask,
                    })
                    .await?
                {
                    ResponseKind::FileSecDesc(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn set_sec_desc(&mut self, sec_desc: &SecDesc) -> crate::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.set_sec_desc(sec_desc).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileSetSecDesc {
                        file: file.cite(),
                        sec_desc: sec_desc.clone(),
                    })
                    .await?
                {
                    ResponseKind::FileSetSecDesc(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn xattrs(
        &mut self,
        namespace: crate::XattrNamespace<'_>,
    ) -> crate::Result<Vec<XattrEntry>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.xattrs(namespace).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileXattrs {
                        file: file.cite(),
                        namespace: XattrNamespaceRequest::from(namespace),
                    })
                    .await?
                {
                    ResponseKind::FileXattrs(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn xattr(&mut self, name: &str, namespace: Option<&str>) -> crate::Result<Vec<u8>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.xattr(name, namespace).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileXattr {
                        file: file.cite(),
                        name: name.to_owned(),
                        namespace: namespace.map(str::to_owned),
                    })
                    .await?
                {
                    ResponseKind::FileXattr(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn streams(&mut self) -> crate::Result<Vec<StreamEntry>> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.streams().await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileStreams { file: file.cite() })
                    .await?
                {
                    ResponseKind::FileStreams(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn set_xattr(
        &mut self,
        name: &str,
        namespace: Option<&str>,
        value: &[u8],
    ) -> crate::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.set_xattr(name, namespace, value).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileSetXattr {
                        file: file.cite(),
                        name: name.to_owned(),
                        namespace: namespace.map(str::to_owned),
                        value: value.to_vec(),
                    })
                    .await?
                {
                    ResponseKind::FileSetXattr(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn remove_xattr(&mut self, name: &str, namespace: Option<&str>) -> crate::Result<()> {
        match &mut self.0 {
            ClientFileInner::Direct(file) => file.remove_xattr(name, namespace).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileRemoveXattr {
                        file: file.cite(),
                        name: name.to_owned(),
                        namespace: namespace.map(str::to_owned),
                    })
                    .await?
                {
                    ResponseKind::FileRemoveXattr(result) => result.map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn lock(
        &self,
        request: crate::file::FileLockRequest,
    ) -> crate::Result<Option<crate::file::FileLock>> {
        match &self.0 {
            ClientFileInner::Direct(file) => file.lock(request).await,
            ClientFileInner::Remote(file) => {
                file.idle()?;
                match file
                    .client
                    .request(RequestKind::FileLock {
                        file: file.cite(),
                        request,
                    })
                    .await?
                {
                    ResponseKind::FileLock(result) => result
                        .map(|lock| {
                            lock.map(|lock| {
                                crate::file::FileLock::remote(RemoteFileLock {
                                    client: file.client.clone(),
                                    file: file.file.clone(),
                                    lock: Some(lock),
                                })
                            })
                        })
                        .map_err(Into::into),
                    response => Err(unexpected(response).into()),
                }
            }
        }
    }

    async fn try_into_std(self) -> std::result::Result<std::fs::File, Self> {
        match self.0 {
            ClientFileInner::Direct(file) => file
                .try_into_std()
                .await
                .map_err(|file| Self(ClientFileInner::Direct(file))),
            ClientFileInner::Remote(file) => Err(Self(ClientFileInner::Remote(file))),
        }
    }
}

fn wire_io(error: crate::protocol::WireError) -> io::Error {
    crate::Error::from(error).into_io_error()
}

fn query_from_wire(response: QueryResponse) -> Query {
    let QueryResponse {
        env,
        cwd,
        current_exe,
        target,
        security,
        extensions,
    } = response;
    Query {
        env,
        cwd: cwd.into(),
        current_exe: current_exe.into(),
        target,
        security,
        extensions,
    }
}

impl Client {
    async fn initialize(
        rpc: dolang_rpc::client::Client<VfsProtocol>,
        mode: SessionMode,
        vfs: Option<Gift<crate::session::VfsMarker>>,
    ) -> crate::Result<Self> {
        let response = rpc
            .call(Request {
                vfs: vfs.as_ref().map(Gift::cite),
                kind: RequestKind::Query,
            })
            .await
            .map_err(rpc_error)?
            .into_response();
        let ResponseKind::Query(result) = response else {
            return Err(unexpected(response).into());
        };
        let query = result.map_err(crate::Error::from).map(query_from_wire)?;
        Ok(Self {
            shared: Arc::new(ClientShared { rpc, mode, query }),
            vfs,
        })
    }

    pub(crate) fn is_same_vfs(&self, other: &Self) -> bool {
        self.shared.rpc.is_same_session(&other.shared.rpc) && self.vfs == other.vfs
    }

    pub(crate) fn mode(&self) -> SessionMode {
        self.shared.mode
    }

    /// Starts an opaque-only VFS client on a bidirectional byte stream.
    ///
    /// This transport cannot transfer native handles, so files, subprocesses,
    /// and stdio endpoints are represented by remote references and relays.
    pub async fn new<T>(stream: T) -> crate::Result<Self>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let rpc = rpc_builder(None)
            .client(stream)
            .await
            .map_err(rpc_error)?
            .bind();
        Self::initialize(rpc, SessionMode::Remote, None).await
    }

    /// Starts an opaque-only VFS client on separate reader and writer streams.
    ///
    /// This has the same opaque-only behavior as [`new`](Self::new).
    pub async fn new_split<R, W>(reader: R, writer: W) -> crate::Result<Self>
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        let rpc = rpc_builder(None)
            .client_split(reader, writer)
            .await
            .map_err(rpc_error)?
            .bind();
        Self::initialize(rpc, SessionMode::Remote, None).await
    }

    /// Closes this client's RPC session and releases its transport handles.
    ///
    /// Closing any clone closes the shared session, so remaining clones can no
    /// longer issue requests.
    pub async fn close(self) {
        self.shared.rpc.clone().close().await;
    }

    /// Connects to an agent daemon at a Unix-domain socket path.
    ///
    /// This transport supports native file-descriptor transfer.
    #[cfg(unix)]
    pub async fn connect(path: impl AsRef<Path>) -> crate::Result<Self> {
        Self::connect_with_key(path, None).await
    }

    /// Connects to an agent daemon at a Unix-domain socket path, proving
    /// knowledge of a pre-shared key.
    ///
    /// A socket that must be world-connectable cannot identify its peer from
    /// credentials alone, so `key` is what distinguishes the intended agent
    /// from anything else listening at that path — and, in the other
    /// direction, this client from anything else that reached the socket
    /// first. Both ends must agree: a key here requires a keyed agent, and an
    /// agent expecting one refuses an unkeyed client. See [`dolang_rpc::auth`].
    #[cfg(unix)]
    pub async fn connect_with_key(
        path: impl AsRef<Path>,
        key: Option<AuthKey>,
    ) -> crate::Result<Self> {
        Self::from_std_stream(UnixStream::connect(path).await?.into_std()?, key).await
    }

    /// Connects using an existing Unix-domain stream.
    ///
    /// This transport supports native file-descriptor transfer.
    #[cfg(unix)]
    pub async fn from_stream(stream: UnixStream) -> crate::Result<Self> {
        Self::from_std_stream(stream.into_std()?, None).await
    }

    #[cfg(unix)]
    async fn from_std_stream(stream: StdUnixStream, key: Option<AuthKey>) -> crate::Result<Self> {
        let rpc = rpc_builder(key)
            .client_unix(stream)
            .await
            .map_err(rpc_error)?
            .bind();
        Self::initialize(rpc, SessionMode::Native, None).await
    }

    /// Starts a VFS client on an already-connected Unix-domain socket file
    /// descriptor.
    ///
    /// This transport supports native file-descriptor transfer.
    #[cfg(unix)]
    pub async fn from_owned_fd(value: OwnedFd) -> crate::Result<Self> {
        Self::from_owned_fd_with_key(value, None).await
    }

    /// Starts a VFS client on an already-connected Unix-domain socket file
    /// descriptor, proving knowledge of a pre-shared key.
    #[cfg(unix)]
    pub async fn from_owned_fd_with_key(
        value: OwnedFd,
        key: Option<AuthKey>,
    ) -> crate::Result<Self> {
        let stream = StdUnixStream::from(value);
        stream.set_nonblocking(true)?;
        Self::from_std_stream(stream, key).await
    }

    /// Starts a VFS client on the server end of a connected Windows named pipe.
    ///
    /// # Safety
    ///
    /// `server_process` must identify the trusted process at the other end of
    /// the pipe. That process can transfer handles which this process adopts.
    #[cfg(any(windows, docsrs))]
    #[cfg_attr(docsrs, doc(cfg(windows)))]
    #[cfg_attr(all(docsrs, not(windows)), allow(private_interfaces))]
    pub async unsafe fn from_named_pipe_server(
        pipe: NamedPipeServer,
        server_process: OwnedHandle,
    ) -> crate::Result<Self> {
        #[cfg(windows)]
        {
            let rpc = unsafe { rpc_builder(None).client_named_pipe_server(pipe, server_process) }
                .await
                .map_err(rpc_error)?
                .bind();
            Self::initialize(rpc, SessionMode::Native, None).await
        }
        #[cfg(all(docsrs, not(windows)))]
        {
            let _ = (pipe, server_process);
            unreachable!()
        }
    }

    fn unsupported<T>(&self, operation: &str) -> crate::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{operation} is not supported by a remote VFS session"),
        )
        .into())
    }

    fn call(&self, request: RequestKind) -> Call<VfsProtocol> {
        self.shared.rpc.call(Request {
            vfs: self.vfs.as_ref().map(Gift::cite),
            kind: request,
        })
    }

    fn call_with_trailer(&self, request: RequestKind) -> TrailerSend<Call<VfsProtocol>> {
        self.shared.rpc.call_with_trailer(Request {
            vfs: self.vfs.as_ref().map(Gift::cite),
            kind: request,
        })
    }

    pub(crate) async fn request(&self, request: RequestKind) -> crate::Result<ResponseKind> {
        let response = self
            .call(request)
            .await
            .map_err(rpc_error)
            .map_err(crate::Error::from)?
            .into_response();
        match response {
            ResponseKind::Error(error) => Err(crate::Error::from(error)),
            response => Ok(response),
        }
    }

    async fn unix_vfs(
        &self,
        path: Utf8TypedPath<'_>,
        key: Option<&[u8]>,
    ) -> crate::Result<crate::AnyVfs> {
        // The key is sent because the peer may have to establish the nested
        // connection itself (the `Opaque` arm below), and which arm it takes
        // is its decision, not ours. When it returns a descriptor instead, we
        // authenticate locally and its copy goes unused.
        let request = UnixVfsRequest {
            path: path.into(),
            key: key.map(<[u8]>::to_vec),
        };
        match self.request(RequestKind::UnixVfs(request)).await? {
            ResponseKind::UnixVfs(result) => match result.map_err(crate::Error::from)? {
                OpenVfsHandle::Native(handle) => {
                    #[cfg(unix)]
                    {
                        let key = key.map(AuthKey::new).transpose().map_err(rpc_error)?;
                        Ok(Self::from_owned_fd_with_key(handle.into_inner(), key)
                            .await?
                            .into())
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = handle;
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "received a native Unix VFS connection on a non-Unix host",
                        )
                        .into())
                    }
                }
                OpenVfsHandle::Opaque(vfs) => {
                    Ok(
                        Self::initialize(self.shared.rpc.clone(), self.shared.mode, Some(vfs))
                            .await?
                            .into(),
                    )
                }
            },
            response => Err(unexpected(response).into()),
        }
    }

    async fn windows_admin_vfs(
        &self,
        cwd: Utf8TypedPath<'_>,
        env: HashMap<String, Option<String>>,
        elevate: bool,
    ) -> crate::Result<crate::session::VfsSession> {
        let request = WindowsAdminRequest {
            cwd: cwd.into(),
            env,
            elevate,
        };
        match self.request(RequestKind::WindowsAdmin(request)).await? {
            ResponseKind::WindowsAdmin(result) => {
                let vfs = result.map_err(crate::Error::from)?;
                Ok(crate::session::VfsSession::from_client(
                    Self::initialize(self.shared.rpc.clone(), self.shared.mode, Some(vfs)).await?,
                ))
            }
            response => Err(unexpected(response).into()),
        }
    }

    /// Check file accessibility.
    ///
    /// Mode is a bitmask of accessibility flags from [`AccessFlags`](crate::file::AccessFlags):
    /// - `AccessFlags::F_OK`: Test for existence
    /// - `AccessFlags::R_OK`: Test for read permission
    /// - `AccessFlags::W_OK`: Test for write permission
    /// - `AccessFlags::X_OK`: Test for execute permission
    #[cfg(unix)]
    pub async fn access(
        &self,
        path: impl AsRef<Path>,
        mode: crate::file::AccessFlags,
    ) -> crate::Result<()> {
        let request = AccessRequest {
            path: path.as_ref().to_path_buf().try_into()?,
            mode: mode.bits(),
        };
        match self.request(RequestKind::Access(request)).await? {
            ResponseKind::Access(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    /// Calls a registered VFS extension.
    ///
    /// The extension must be linked into both this process and the peer
    /// serving the connection (whether that peer is a remote `dolang-vfs`
    /// process or, when `mode == SessionMode::Native`, this same process's
    /// direct backend).
    pub async fn call_extension<T: VfsExtension>(
        &self,
        request: T::Request,
    ) -> crate::Result<T::Response> {
        let wire = RequestKind::Extension(ExtensionRequest {
            name: T::NAME.to_string(),
            version: T::VERSION,
            payload: Box::new(request),
        });
        match self.request(wire).await? {
            ResponseKind::Extension(Ok(ExtensionResponse { payload, .. })) => Ok(*payload
                .downcast::<T::Response>()
                .expect("response type matches the extension that produced it")),
            ResponseKind::Extension(Err(error)) => Err(error.into()),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Unix user ID on the target.
    pub async fn user_name(&self, uid: u32) -> crate::Result<String> {
        match self.request(RequestKind::UserName { uid }).await? {
            ResponseKind::UserName(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Unix user name on the target.
    pub async fn user_id(&self, name: &str) -> crate::Result<u32> {
        match self
            .request(RequestKind::UserId {
                name: name.to_owned(),
            })
            .await?
        {
            ResponseKind::UserId(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Unix group ID on the target.
    pub async fn group_name(&self, gid: u32) -> crate::Result<String> {
        match self.request(RequestKind::GroupName { gid }).await? {
            ResponseKind::GroupName(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Unix group name on the target.
    pub async fn group_id(&self, name: &str) -> crate::Result<u32> {
        match self
            .request(RequestKind::GroupId {
                name: name.to_owned(),
            })
            .await?
        {
            ResponseKind::GroupId(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Windows SID on the target.
    pub async fn sid_name(&self, sid: &Sid) -> crate::Result<SidName> {
        match self
            .request(RequestKind::SidName { sid: sid.clone() })
            .await?
        {
            ResponseKind::SidName(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a Windows account name on the target.
    pub async fn account_name(&self, name: &str) -> crate::Result<SidName> {
        match self
            .request(RequestKind::AccountName {
                name: name.to_owned(),
            })
            .await?
        {
            ResponseKind::AccountName(result) => result.map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolve a program path using the daemon's PATH resolution.
    pub async fn which(
        &self,
        program: impl AsRef<Path>,
        path: Option<&str>,
        cwd: Option<&Path>,
    ) -> crate::Result<Option<PathBuf>> {
        let request = RequestKind::Which {
            program: program.as_ref().to_path_buf().try_into()?,
            path: path.map(str::to_owned),
            cwd: cwd
                .map(|path| WirePath::try_from(path.to_path_buf()))
                .transpose()?,
        };
        match self.request(request).await? {
            ResponseKind::Which(result) => result
                .map_err(crate::Error::from)?
                .map(TryInto::try_into)
                .transpose(),
            response => Err(unexpected(response).into()),
        }
    }

    /// Resolves a target-specific well-known path.
    pub async fn well_known_path(
        &self,
        key: WellKnownPath,
        app: Option<&str>,
        env: &HashMap<String, Option<String>>,
    ) -> crate::Result<PathBuf> {
        let request = WellKnownPathRequest {
            key,
            app: app.map(str::to_owned),
            env: env.clone(),
        };
        match self.request(RequestKind::WellKnownPath(request)).await? {
            ResponseKind::WellKnownPath(result) => result.map_err(crate::Error::from)?.try_into(),
            response => Err(unexpected(response).into()),
        }
    }

    /// Signal the daemon to stop accepting new connections.
    pub async fn stop(&self) -> crate::Result<()> {
        match self.request(RequestKind::Stop).await? {
            ResponseKind::Stop => Ok(()),
            response => Err(unexpected(response).into()),
        }
    }

    /// Clear the server's path resolution cache.
    pub async fn clear_cache(&self) -> crate::Result<()> {
        match self.request(RequestKind::ClearCache).await? {
            ResponseKind::ClearCache(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }
}

pub(crate) fn rpc_error(error: dolang_rpc::Error) -> io::Error {
    match error {
        dolang_rpc::Error::Io(error) => error,
        dolang_rpc::Error::ConnectionClosed => {
            io::Error::new(io::ErrorKind::ConnectionReset, error.to_string())
        }
        dolang_rpc::Error::Cancelled => {
            io::Error::new(io::ErrorKind::Interrupted, error.to_string())
        }
        error => io::Error::other(error),
    }
}

fn unexpected(response: ResponseKind) -> io::Error {
    io::Error::other(format!("unexpected RPC response: {response:?}"))
}

fn clone_stdin_handle() -> io::Result<DefaultHandle> {
    #[cfg(unix)]
    {
        std::io::stdin().as_fd().try_clone_to_owned()
    }
    #[cfg(windows)]
    {
        std::io::stdin().as_handle().try_clone_to_owned()
    }
}

fn clone_stdout_handle() -> io::Result<DefaultHandle> {
    #[cfg(unix)]
    {
        std::io::stdout().as_fd().try_clone_to_owned()
    }
    #[cfg(windows)]
    {
        std::io::stdout().as_handle().try_clone_to_owned()
    }
}

fn clone_stderr_handle() -> io::Result<DefaultHandle> {
    #[cfg(unix)]
    {
        std::io::stderr().as_fd().try_clone_to_owned()
    }
    #[cfg(windows)]
    {
        std::io::stderr().as_handle().try_clone_to_owned()
    }
}

/// Builder for constructing a process-spawn request on a remote VFS.
///
/// Configure arguments, environment, working directory, and standard streams,
/// then call [`spawn`](crate::Command::spawn). This concrete API accepts host
/// [`Path`] values; use [`Vfs::command`]
/// when the target's path syntax may differ from the host's.
///
/// # Example
///
/// ```ignore
/// let child = client
///     .command("ls")
///     .arg("-l")
///     .arg("/tmp")
///     .env("RUST_LOG", "info")
///     .env_remove("DEBUG")
///     .current_dir("/home")
///     .stdin(fd)
///     .spawn()
///     .await?;
/// ```
pub struct CommandBuilder<'a> {
    client: &'a Client,
    program: WirePath,
    args: Vec<String>,
    env: HashMap<String, Option<String>>,
    cwd: Option<WirePath>,
    stdin: ClientRecv,
    stdout: ClientSend,
    stderr: ClientSend,
    process_control: crate::ProcessControl,
    termination_policy: crate::TerminationPolicy,
}

/// A process spawned by a [`Client`].
///
/// It implements [`Child`]; any relay tasks for cross-domain
/// standard streams are owned by this value.
pub struct ClientChild {
    client: Client,
    state: ClientChildState,
    relays: ClientRelays,
}

#[derive(Default)]
struct ClientRelays {
    stdin: Option<JoinHandle<()>>,
    outputs: Vec<JoinHandle<()>>,
}

#[derive(Clone, Copy)]
enum HostOutput {
    Stdout,
    Stderr,
}

#[derive(Default)]
struct PreparedRelays {
    stdin: Option<StdioSend>,
    outputs: Vec<(StdioRecv, HostOutput)>,
}

enum ClientChildState {
    Live(Gift<crate::session::ChildMarker>),
    Exited(ProcessStatus),
    Lost(crate::protocol::WireError),
}

/// A writable standard-stream endpoint owned by a remote VFS session.
///
/// This implements [`AsyncWrite`]. Shutting it down
/// closes the corresponding remote endpoint.
pub struct RemoteStdioSend {
    client: Client,
    stdio: Option<Gift<crate::session::StdioSendMarker>>,
    pending: Option<(StdioSendOperation, Call<VfsProtocol>)>,
    write_body: Option<PendingTrailerWrite>,
}

/// A readable standard-stream endpoint owned by a remote VFS session.
///
/// This implements [`AsyncRead`].
pub struct RemoteStdioRecv {
    client: Client,
    stdio: Option<Gift<crate::session::StdioRecvMarker>>,
    pending: Option<Call<VfsProtocol>>,
    read_body: Option<PendingTrailerRead>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StdioSendOperation {
    Close,
}

impl std::fmt::Debug for RemoteStdioSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteStdioSend")
            .field("stdio", &self.stdio)
            .field("pending", &self.pending.as_ref().map(|p| p.0))
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for RemoteStdioRecv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteStdioRecv")
            .field("stdio", &self.stdio)
            .field("pending", &self.pending.is_some())
            .finish_non_exhaustive()
    }
}

impl AsyncWrite for RemoteStdioSend {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if self.pending.is_some() {
            return Poll::Ready(Err(io::Error::other(
                "write polled while stdio close is pending",
            )));
        }
        if self.write_body.is_none() {
            let Some(stdio) = self.stdio.as_ref().map(Gift::cite) else {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "stdio send resource is closed",
                )));
            };
            self.write_body = Some(PendingTrailerWrite {
                send: Some(
                    self.client
                        .call_with_trailer(RequestKind::StdioSendWrite { stdio }),
                ),
                call: None,
                target: buf.len(),
                sent: 0,
                unreported: 0,
            });
        }
        let pending = self.write_body.as_mut().unwrap();
        if let Some(call) = pending.call.as_mut() {
            match Pin::new(call).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => {
                    let target = pending.target;
                    let unreported = pending.unreported;
                    self.write_body = None;
                    match result.map_err(rpc_error)?.into_response() {
                        ResponseKind::StdioSendWrite(result) => {
                            if result.map_err(wire_io)? != target {
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "stdio write response does not acknowledge the submitted trailer",
                                )));
                            }
                            return Poll::Ready(Ok(unreported));
                        }
                        ResponseKind::Error(error) => return Poll::Ready(Err(wire_io(error))),
                        response => return Poll::Ready(Err(unexpected(response))),
                    }
                }
            }
        }
        let remaining = pending.target - pending.sent;
        match Pin::new(pending.send.as_mut().unwrap())
            .poll_write(cx, &buf[..buf.len().min(remaining)])
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Ready(Ok(n)) => {
                pending.sent += n;
                if pending.sent == pending.target {
                    pending.call = Some(pending.send.take().unwrap().finish());
                    pending.unreported = n;
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(Ok(n))
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(pending) = self.write_body.as_mut() {
            if pending.call.is_none() {
                pending.call = Some(pending.send.take().unwrap().finish());
            }
            return match Pin::new(pending.call.as_mut().unwrap()).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(result) => {
                    self.write_body = None;
                    Poll::Ready(result.map_err(rpc_error).and_then(|result| {
                        match result.into_response() {
                            ResponseKind::StdioSendWrite(result) => {
                                result.map(|_| ()).map_err(wire_io)
                            }
                            ResponseKind::Error(error) => Err(wire_io(error)),
                            response => Err(unexpected(response)),
                        }
                    }))
                }
            };
        }
        let Some((_operation, _call)) = self.pending.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        Poll::Ready(Err(io::Error::other(
            "flush polled while stdio send close is pending",
        )))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending.is_none() {
            match self.as_mut().poll_flush(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => {}
            }
        }
        if self.stdio.is_none() {
            return Poll::Ready(Ok(()));
        }
        if self.pending.is_none() {
            let stdio = self.stdio.as_ref().unwrap().cite();
            self.pending = Some((
                StdioSendOperation::Close,
                self.client.call(RequestKind::StdioSendClose { stdio }),
            ));
        }
        let (operation, call) = self.pending.as_mut().unwrap();
        debug_assert_eq!(*operation, StdioSendOperation::Close);
        match Pin::new(call).poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.pending = None;
                match result.map_err(rpc_error)?.into_response() {
                    ResponseKind::Error(error) => Poll::Ready(Err(wire_io(error))),
                    ResponseKind::StdioSendClose(result) => match result.map_err(wire_io) {
                        Ok(()) => {
                            self.stdio.take();
                            Poll::Ready(Ok(()))
                        }
                        Err(error) => Poll::Ready(Err(error)),
                    },
                    response => Poll::Ready(Err(unexpected(response))),
                }
            }
        }
    }
}

impl AsyncRead for RemoteStdioRecv {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if let Some(body) = self.read_body.as_mut() {
                let before = buf.filled().len();
                match Pin::new(&mut body.recv).poll_read(cx, buf) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(error)) => {
                        self.read_body = None;
                        return Poll::Ready(Err(error));
                    }
                    Poll::Ready(Ok(())) => {
                        let read = buf.filled().len() - before;
                        if read > body.remaining {
                            self.read_body = None;
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "stdio read response exceeds requested length",
                            )));
                        }
                        body.remaining -= read;
                        body.read += read;
                        if read > 0 {
                            return Poll::Ready(Ok(()));
                        }
                        let empty = body.read == 0;
                        self.read_body = None;
                        if empty {
                            return Poll::Ready(Ok(()));
                        }
                        continue;
                    }
                }
            }
            if self.pending.is_none() {
                if buf.remaining() == 0 {
                    return Poll::Ready(Ok(()));
                }
                let Some(stdio) = &self.stdio else {
                    return Poll::Ready(Ok(()));
                };
                self.pending = Some(self.client.call(RequestKind::StdioRecvRead {
                    stdio: stdio.cite(),
                    len: buf.remaining(),
                }));
            }
            match Pin::new(self.pending.as_mut().unwrap()).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => {
                    self.pending = None;
                    let (response, trailer) = result.map_err(rpc_error)?.into_response_trailer();
                    match response {
                        ResponseKind::Error(error) => return Poll::Ready(Err(wire_io(error))),
                        ResponseKind::StdioRecvRead(result) => match result.map_err(wire_io) {
                            Ok(()) => {
                                let Some(data) = trailer else {
                                    return Poll::Ready(Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "stdio read response is missing its data trailer",
                                    )));
                                };
                                let requested = buf.remaining();
                                self.read_body = Some(PendingTrailerRead {
                                    recv: data,
                                    remaining: requested,
                                    read: 0,
                                });
                                continue;
                            }
                            Err(error) => return Poll::Ready(Err(error)),
                        },
                        response => return Poll::Ready(Err(unexpected(response))),
                    }
                }
            }
        }
    }
}

impl RemoteStdioSend {
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) async fn try_clone(&self) -> io::Result<Self> {
        if self.pending.is_some() {
            return Err(io::Error::other(
                "cannot clone stdio send while an operation is pending",
            ));
        }
        let stdio = self.stdio.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "stdio send resource is closed")
        })?;
        match self
            .client
            .request(RequestKind::StdioSendClone {
                stdio: stdio.cite(),
            })
            .await
            .map_err(crate::Error::into_io_error)?
        {
            ResponseKind::StdioSendClone(result) => result
                .map(|stdio| Self {
                    client: self.client.clone(),
                    stdio: Some(stdio),
                    pending: None,
                    write_body: None,
                })
                .map_err(wire_io),
            response => Err(unexpected(response)),
        }
    }
}

impl RemoteStdioRecv {
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) async fn try_clone(&self) -> io::Result<Self> {
        if self.pending.is_some() {
            return Err(io::Error::other(
                "cannot clone stdio receive while an operation is pending",
            ));
        }
        let stdio = self.stdio.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stdio receive resource is closed",
            )
        })?;
        match self
            .client
            .request(RequestKind::StdioRecvClone {
                stdio: stdio.cite(),
            })
            .await
            .map_err(crate::Error::into_io_error)?
        {
            ResponseKind::StdioRecvClone(result) => result
                .map(|stdio| Self {
                    client: self.client.clone(),
                    stdio: Some(stdio),
                    pending: None,
                    read_body: None,
                })
                .map_err(wire_io),
            response => Err(unexpected(response)),
        }
    }
}

enum ClientRecv {
    Null,
    Inherit,
    Native(DefaultHandle),
    Resource(StdioRecv),
}

enum ClientSend {
    Null,
    Stdout,
    Inherit(HostOutput),
    Native(DefaultHandle),
    Resource(StdioSend),
}

impl<'a> CommandBuilder<'a> {
    fn new(client: &'a Client, program: Utf8TypedPath<'_>) -> Self {
        Self {
            client,
            program: program.into(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            stdin: ClientRecv::Null,
            stdout: ClientSend::Null,
            stderr: ClientSend::Null,
            process_control: crate::ProcessControl::Foreground,
            termination_policy: crate::TerminationPolicy::default(),
        }
    }

    async fn prepare_recv(
        client: &Client,
        stdio: ClientRecv,
        relays: &mut PreparedRelays,
    ) -> crate::Result<StdioRecvTarget> {
        match stdio {
            ClientRecv::Null => Ok(StdioRecvTarget::Null),
            ClientRecv::Inherit => {
                let (send, recv) = client.pipe().await?;
                relays.stdin = Some(send);
                let StdioRecv::Remote(remote) = recv else {
                    return Err(io::Error::other(
                        "remote pipe unexpectedly returned a native receive endpoint",
                    )
                    .into());
                };
                Self::prepare_remote_recv(client, remote)
            }
            ClientRecv::Native(handle) => {
                if client.mode() == SessionMode::Remote {
                    return client.unsupported("native process stdio");
                }
                Ok(StdioRecvTarget::Native(OsHandle::new(handle)))
            }
            ClientRecv::Resource(stdio) => match stdio {
                StdioRecv::Native(_) => {
                    if client.mode() == SessionMode::Remote {
                        return client.unsupported("native process stdio");
                    }
                    let handle = stdio.into_blocking_handle().await?;
                    Ok(StdioRecvTarget::Native(OsHandle::new(handle)))
                }
                StdioRecv::Remote(remote) => Self::prepare_remote_recv(client, remote),
            },
        }
    }

    fn prepare_remote_recv(
        client: &Client,
        remote: RemoteStdioRecv,
    ) -> crate::Result<StdioRecvTarget> {
        if !client.is_same_vfs(&remote.client) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdio receive belongs to a different VFS session",
            )
            .into());
        }
        Ok(StdioRecvTarget::Opaque(
            remote.stdio.as_ref().unwrap().cite(),
        ))
    }

    async fn prepare_send(
        client: &Client,
        stdio: ClientSend,
        relays: &mut PreparedRelays,
    ) -> crate::Result<StdioSendTarget> {
        match stdio {
            ClientSend::Null => Ok(StdioSendTarget::Null),
            ClientSend::Stdout => Ok(StdioSendTarget::Stdout),
            ClientSend::Inherit(output) => {
                let (send, recv) = client.pipe().await?;
                relays.outputs.push((recv, output));
                let StdioSend::Remote(remote) = send else {
                    return Err(io::Error::other(
                        "remote pipe unexpectedly returned a native send endpoint",
                    )
                    .into());
                };
                Self::prepare_remote_send(client, remote)
            }
            ClientSend::Native(handle) => {
                if client.mode() == SessionMode::Remote {
                    return client.unsupported("native process stdio");
                }
                Ok(StdioSendTarget::Native(OsHandle::new(handle)))
            }
            ClientSend::Resource(stdio) => match stdio {
                StdioSend::Native(_) => {
                    if client.mode() == SessionMode::Remote {
                        return client.unsupported("native process stdio");
                    }
                    let handle = stdio.into_blocking_handle().await?;
                    Ok(StdioSendTarget::Native(OsHandle::new(handle)))
                }
                StdioSend::Remote(remote) => Self::prepare_remote_send(client, remote),
            },
        }
    }

    fn prepare_remote_send(
        client: &Client,
        remote: RemoteStdioSend,
    ) -> crate::Result<StdioSendTarget> {
        if !client.is_same_vfs(&remote.client) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdio send belongs to a different VFS session",
            )
            .into());
        }
        Ok(StdioSendTarget::Opaque(
            remote.stdio.as_ref().unwrap().cite(),
        ))
    }

    async fn prepare_outputs(
        client: &Client,
        stdout: ClientSend,
        stderr: ClientSend,
        relays: &mut PreparedRelays,
    ) -> crate::Result<(StdioSendTarget, StdioSendTarget)> {
        let stdout = Self::prepare_send(client, stdout, relays).await?;
        let stderr = Self::prepare_send(client, stderr, relays).await?;
        Ok((stdout, stderr))
    }
}

async fn relay_stdin(mut send: StdioSend) {
    let mut stdin = tokio::io::stdin();
    let _ = tokio::io::copy(&mut stdin, &mut send).await;
    let _ = send.shutdown().await;
}

async fn relay_output<W>(mut recv: StdioRecv, mut output: W)
where
    W: AsyncWrite + Unpin,
{
    let _ = tokio::io::copy(&mut recv, &mut output).await;
    let _ = output.flush().await;
}

impl PreparedRelays {
    fn start(self) -> ClientRelays {
        let stdin = self.stdin.map(|send| tokio::spawn(relay_stdin(send)));
        let outputs = self
            .outputs
            .into_iter()
            .map(|(recv, output)| match output {
                HostOutput::Stdout => tokio::spawn(relay_output(recv, tokio::io::stdout())),
                HostOutput::Stderr => tokio::spawn(relay_output(recv, tokio::io::stderr())),
            })
            .collect();
        ClientRelays { stdin, outputs }
    }
}

impl ClientRelays {
    fn abort_stdin(&mut self) {
        if let Some(stdin) = self.stdin.take() {
            stdin.abort();
        }
    }

    fn finish(&mut self) {
        self.abort_stdin();
        self.outputs.clear();
    }
}

impl ClientChild {
    fn result(&self) -> Option<crate::Result<ProcessStatus>> {
        match &self.state {
            ClientChildState::Live(_) => None,
            ClientChildState::Exited(status) => Some(Ok(*status)),
            ClientChildState::Lost(error) => Some(Err(error.clone().into())),
        }
    }

    fn store_result(
        &mut self,
        result: &std::result::Result<ProcessStatus, crate::protocol::WireError>,
    ) {
        self.state = match result {
            Ok(status) => ClientChildState::Exited(*status),
            Err(error) => ClientChildState::Lost(error.clone()),
        };
    }
}

impl Drop for ClientChild {
    fn drop(&mut self) {
        // Reaping the child is the opaque's own business: dropping the last
        // handle on it releases the registration, which drops the retained
        // child on the server. Only the relays need winding down here.
        self.relays.finish();
    }
}

impl Child for ClientChild {
    async fn wait(&mut self) -> crate::Result<ProcessStatus> {
        if let Some(result) = self.result() {
            return result;
        }
        let ClientChildState::Live(child) = &self.state else {
            unreachable!();
        };
        match self
            .client
            .request(RequestKind::ChildWait {
                child: child.cite(),
            })
            .await?
        {
            ResponseKind::ChildWait(result) => {
                self.relays.finish();
                self.store_result(&result);
                self.result().unwrap()
            }
            response => Err(unexpected(response).into()),
        }
    }

    async fn terminate(mut self) -> crate::Result<Option<ProcessStatus>> {
        self.relays.abort_stdin();
        if let Some(result) = self.result() {
            return result.map(Some);
        }
        let ClientChildState::Live(child) = &self.state else {
            unreachable!();
        };
        match self
            .client
            .request(RequestKind::ChildTerminate {
                child: child.cite(),
            })
            .await?
        {
            ResponseKind::ChildTerminate(result) => {
                self.relays.finish();
                if let Ok(Some(status)) = result {
                    self.state = ClientChildState::Exited(status);
                }
                result.map_err(Into::into)
            }
            response => Err(unexpected(response).into()),
        }
    }
}

impl<'a> Command for CommandBuilder<'a> {
    type Child = ClientChild;
    type StdioSend = StdioSend;
    type StdioRecv = StdioRecv;

    fn arg(&mut self, arg: &str) -> &mut Self {
        self.args.push(arg.to_owned());
        self
    }

    fn env(&mut self, key: &str, val: &str) -> &mut Self {
        self.env.insert(key.to_owned(), Some(val.to_owned()));
        self
    }

    fn env_remove(&mut self, key: &str) -> &mut Self {
        self.env.insert(key.to_owned(), None);
        self
    }

    fn current_dir(&mut self, dir: Utf8TypedPath<'_>) -> &mut Self {
        self.cwd = Some(dir.into());
        self
    }

    fn stdin(&mut self, stdio: StdioRecv) -> io::Result<&mut Self> {
        if let StdioRecv::Remote(remote) = &stdio
            && !self.client.is_same_vfs(&remote.client)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdio receive belongs to a different VFS session",
            ));
        }
        self.stdin = ClientRecv::Resource(stdio);
        Ok(self)
    }

    fn stdout(&mut self, stdio: StdioSend) -> io::Result<&mut Self> {
        if let StdioSend::Remote(remote) = &stdio
            && !self.client.is_same_vfs(&remote.client)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdio send belongs to a different VFS session",
            ));
        }
        self.stdout = ClientSend::Resource(stdio);
        Ok(self)
    }

    fn stdin_inherit(&mut self) -> io::Result<&mut Self> {
        self.stdin = if self.client.mode() == SessionMode::Remote {
            if std::io::stdin().is_terminal() {
                ClientRecv::Null
            } else {
                ClientRecv::Inherit
            }
        } else {
            ClientRecv::Native(clone_stdin_handle()?)
        };
        Ok(self)
    }

    fn stdout_inherit(&mut self) -> io::Result<&mut Self> {
        self.stdout = if self.client.mode() == SessionMode::Remote {
            ClientSend::Inherit(HostOutput::Stdout)
        } else {
            ClientSend::Native(clone_stdout_handle()?)
        };
        Ok(self)
    }

    fn stdout_inherit_stderr(&mut self) -> io::Result<&mut Self> {
        self.stdout = if self.client.mode() == SessionMode::Remote {
            ClientSend::Inherit(HostOutput::Stderr)
        } else {
            ClientSend::Native(clone_stderr_handle()?)
        };
        Ok(self)
    }

    fn stdin_null(&mut self) -> &mut Self {
        self.stdin = ClientRecv::Null;
        self
    }

    fn stdout_null(&mut self) -> &mut Self {
        self.stdout = ClientSend::Null;
        self
    }

    fn stderr(&mut self, stdio: StdioSend) -> io::Result<&mut Self> {
        if let StdioSend::Remote(remote) = &stdio
            && !self.client.is_same_vfs(&remote.client)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stdio send belongs to a different VFS session",
            ));
        }
        self.stderr = ClientSend::Resource(stdio);
        Ok(self)
    }

    fn stderr_inherit(&mut self) -> io::Result<&mut Self> {
        self.stderr = if self.client.mode() == SessionMode::Remote {
            ClientSend::Inherit(HostOutput::Stderr)
        } else {
            ClientSend::Native(clone_stderr_handle()?)
        };
        Ok(self)
    }

    fn stderr_to_stdout(&mut self) -> io::Result<&mut Self> {
        self.stderr = ClientSend::Stdout;
        Ok(self)
    }

    fn stderr_inherit_stdout(&mut self) -> io::Result<&mut Self> {
        self.stderr = if self.client.mode() == SessionMode::Remote {
            ClientSend::Inherit(HostOutput::Stdout)
        } else {
            ClientSend::Native(clone_stdout_handle()?)
        };
        Ok(self)
    }

    fn stderr_null(&mut self) -> &mut Self {
        self.stderr = ClientSend::Null;
        self
    }

    fn process_control(&mut self, control: crate::ProcessControl) -> &mut Self {
        self.process_control = control;
        self
    }

    fn termination_policy(&mut self, policy: crate::TerminationPolicy) -> &mut Self {
        self.termination_policy = policy;
        self
    }

    async fn spawn(self) -> crate::Result<Self::Child> {
        let Self {
            client,
            program,
            args,
            env,
            cwd,
            stdin,
            stdout,
            stderr,
            process_control,
            termination_policy,
        } = self;
        let mut relays = PreparedRelays::default();
        let stdin = Self::prepare_recv(client, stdin, &mut relays).await?;
        let (stdout, stderr) = Self::prepare_outputs(client, stdout, stderr, &mut relays).await?;
        let req = SpawnRequest {
            program,
            args,
            env,
            cwd,
            stdin,
            stdout,
            stderr,
            process_control,
            termination_policy,
        };
        match client.request(RequestKind::Spawn(req)).await? {
            ResponseKind::Spawn(result) => result
                .map(|child| ClientChild {
                    client: client.clone(),
                    state: ClientChildState::Live(child),
                    relays: relays.start(),
                })
                .map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }
}

/// Builder for opening files through a [`Client`].
///
/// Configure access and creation modes, then call
/// [`OpenOptions::open`](crate::OpenOptions::open). This concrete API accepts
/// host [`Path`] values; use
/// [`Vfs::open_options`] when the target's path
/// syntax may differ from the host's.
///
/// # Example
///
/// ```ignore
/// let file = client
///     .open_options()
///     .read(true)
///     .write(true)
///     .create(true)
///     .open("/tmp/myfile.txt")
///     .await?;
/// ```
pub struct OpenOptions<'a> {
    client: &'a Client,
    read: bool,
    write: bool,
    append: bool,
    create: bool,
    create_new: bool,
    truncate: bool,
    no_follow: bool,
}

impl<'a> OpenOptions<'a> {
    fn new(client: &'a Client) -> Self {
        Self {
            client,
            read: false,
            write: false,
            append: false,
            create: false,
            create_new: false,
            truncate: false,
            no_follow: false,
        }
    }
}

impl crate::OpenOptions for OpenOptions<'_> {
    type File = ClientFile;

    fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }

    fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.create_new = create_new;
        self
    }

    fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    fn no_follow(&mut self, no_follow: bool) -> &mut Self {
        self.no_follow = no_follow;
        self
    }

    async fn open(&self, path: Utf8TypedPath<'_>) -> crate::Result<ClientFile> {
        let req = OpenRequest {
            path: path.into(),
            read: self.read,
            write: self.write,
            append: self.append,
            create: self.create,
            create_new: self.create_new,
            truncate: self.truncate,
            no_follow: self.no_follow,
            handle_preference: if self.client.mode() == SessionMode::Remote {
                OpenHandlePreference::Opaque
            } else {
                OpenHandlePreference::NativePreferred
            },
        };
        match self.client.request(RequestKind::Open(req)).await? {
            ResponseKind::Open(result) => match result.map_err(crate::Error::from)? {
                OpenHandle::Native(handle) => Ok(ClientFile::from_std(
                    handle.into_inner().into(),
                    self.read,
                    self.write,
                    self.append,
                )),
                OpenHandle::Opaque(file) => Ok(ClientFile::from_remote(self.client.clone(), file)),
            },
            response => Err(unexpected(response).into()),
        }
    }
}

impl Vfs for Client {
    type File = ClientFile;
    type StdioSend = StdioSend;
    type StdioRecv = StdioRecv;
    type OpenOptions<'a>
        = OpenOptions<'a>
    where
        Self: 'a;
    type Command<'a>
        = CommandBuilder<'a>
    where
        Self: 'a;

    fn env(&self) -> Box<dyn Iterator<Item = (String, String)> + '_> {
        Box::new(
            self.shared
                .query
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
    }

    fn cwd(&self) -> Utf8TypedPath<'_> {
        self.shared.query.cwd.to_path()
    }

    fn current_exe(&self) -> Utf8TypedPath<'_> {
        self.shared.query.current_exe.to_path()
    }

    fn target(&self) -> &crate::TargetInfo {
        &self.shared.query.target
    }

    fn security(&self) -> &crate::SecurityInfo {
        &self.shared.query.security
    }

    fn extensions(&self) -> &crate::session::ExtensionSet {
        &self.shared.query.extensions
    }

    fn open_options(&self) -> Self::OpenOptions<'_> {
        OpenOptions::new(self)
    }

    fn command(&self, program: Utf8TypedPath<'_>) -> Self::Command<'_> {
        CommandBuilder::new(self, program)
    }

    async fn unix_socket(
        &self,
        path: Utf8TypedPath<'_>,
        key: Option<&[u8]>,
    ) -> crate::Result<crate::AnyVfs> {
        self.unix_vfs(path, key).await
    }

    async fn windows_admin(
        &self,
        cwd: Utf8TypedPath<'_>,
        env: HashMap<String, Option<String>>,
        elevate: bool,
    ) -> crate::Result<crate::session::VfsSession> {
        self.windows_admin_vfs(cwd, env, elevate).await
    }

    async fn pipe(&self) -> crate::Result<(StdioSend, StdioRecv)> {
        if self.mode() == SessionMode::Native {
            return crate::process::pipe(None).map_err(Into::into);
        }
        match self.request(RequestKind::Pipe).await? {
            ResponseKind::Pipe(result) => result
                .map(|pipe| {
                    (
                        StdioSend::Remote(RemoteStdioSend {
                            client: self.clone(),
                            stdio: Some(pipe.send),
                            pending: None,
                            write_body: None,
                        }),
                        StdioRecv::Remote(RemoteStdioRecv {
                            client: self.clone(),
                            stdio: Some(pipe.recv),
                            pending: None,
                            read_body: None,
                        }),
                    )
                })
                .map_err(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    async fn user_name(&self, uid: u32) -> crate::Result<String> {
        Client::user_name(self, uid).await
    }

    async fn user_id(&self, name: &str) -> crate::Result<u32> {
        Client::user_id(self, name).await
    }

    async fn group_name(&self, gid: u32) -> crate::Result<String> {
        Client::group_name(self, gid).await
    }

    async fn group_id(&self, name: &str) -> crate::Result<u32> {
        Client::group_id(self, name).await
    }

    async fn sid_name(&self, sid: &Sid) -> crate::Result<SidName> {
        Client::sid_name(self, sid).await
    }

    async fn account_name(&self, name: &str) -> crate::Result<SidName> {
        Client::account_name(self, name).await
    }

    async fn read_dir(&self, path: Utf8TypedPath<'_>) -> crate::Result<ReadDir> {
        match self
            .request(RequestKind::ReadDir { path: path.into() })
            .await?
        {
            ResponseKind::ReadDir(result) => result
                .map(|read_dir| ReadDir::from_remote(self.clone(), read_dir))
                .map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn which(
        &self,
        program: Utf8TypedPath<'_>,
        path: Option<&str>,
        cwd: Option<Utf8TypedPath<'_>>,
    ) -> crate::Result<Option<Utf8TypedPathBuf>> {
        let request = RequestKind::Which {
            program: program.into(),
            path: path.map(str::to_owned),
            cwd: cwd.map(Into::into),
        };
        match self.request(request).await? {
            ResponseKind::Which(result) => result
                .map(|path| path.map(Into::into))
                .map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn well_known_path(
        &self,
        key: WellKnownPath,
        app: Option<&str>,
        env: &HashMap<String, Option<String>>,
    ) -> crate::Result<Utf8TypedPathBuf> {
        let request = WellKnownPathRequest {
            key,
            app: app.map(str::to_owned),
            env: env.clone(),
        };
        match self.request(RequestKind::WellKnownPath(request)).await? {
            ResponseKind::WellKnownPath(result) => {
                result.map(Into::into).map_err(crate::Error::from)
            }
            response => Err(unexpected(response).into()),
        }
    }

    async fn clear_cache(&self) -> crate::Result<()> {
        Client::clear_cache(self).await
    }

    async fn xattrs(
        &self,
        path: Utf8TypedPath<'_>,
        namespace: crate::XattrNamespace<'_>,
        follow: bool,
    ) -> crate::Result<Vec<XattrEntry>> {
        let request = XattrsRequest {
            path: path.into(),
            namespace: namespace.into(),
            follow,
        };
        match self.request(RequestKind::Xattrs(request)).await? {
            ResponseKind::Xattrs(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn streams(
        &self,
        path: Utf8TypedPath<'_>,
        follow: bool,
    ) -> crate::Result<Vec<StreamEntry>> {
        let request = StreamsRequest {
            path: path.into(),
            follow,
        };
        match self.request(RequestKind::Streams(request)).await? {
            ResponseKind::Streams(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> crate::Result<Vec<u8>> {
        let request = XattrRequest {
            path: path.into(),
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            follow,
        };
        match self.request(RequestKind::Xattr(request)).await? {
            ResponseKind::Xattr(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
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
        let request = SetXattrRequest {
            path: path.into(),
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            value: value.to_vec(),
            follow,
        };
        match self.request(RequestKind::SetXattr(request)).await? {
            ResponseKind::SetXattr(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn remove_xattr(
        &self,
        path: Utf8TypedPath<'_>,
        name: &str,
        namespace: Option<&str>,
        follow: bool,
    ) -> crate::Result<()> {
        let request = XattrRequest {
            path: path.into(),
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            follow,
        };
        match self.request(RequestKind::RemoveXattr(request)).await? {
            ResponseKind::RemoveXattr(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn remove(&self, path: Utf8TypedPath<'_>, all: bool, ignore: bool) -> crate::Result<()> {
        let request = RemoveRequest {
            path: path.into(),
            all,
            ignore,
        };
        match self.request(RequestKind::Remove(request)).await? {
            ResponseKind::Remove(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn metadata(&self, path: Utf8TypedPath<'_>) -> crate::Result<Metadata> {
        let request = MetadataRequest { path: path.into() };
        match self.request(RequestKind::Metadata(request)).await? {
            ResponseKind::Metadata(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn fs_metadata(
        &self,
        path: Utf8TypedPath<'_>,
        follow: bool,
    ) -> crate::Result<FsMetadata> {
        let request = FsMetadataRequest {
            path: path.into(),
            follow,
        };
        match self.request(RequestKind::FsMetadata(request)).await? {
            ResponseKind::FsMetadata(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn acl(
        &self,
        path: Utf8TypedPath<'_>,
        default: bool,
        follow: bool,
    ) -> crate::Result<Option<PosixAcl>> {
        let request = AclRequest {
            path: path.into(),
            default,
            follow,
        };
        match self.request(RequestKind::Acl(request)).await? {
            ResponseKind::Acl(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn set_acl(
        &self,
        path: Utf8TypedPath<'_>,
        acl: Option<&PosixAcl>,
        default: bool,
        follow: bool,
    ) -> crate::Result<()> {
        let request = SetAclRequest {
            path: path.into(),
            acl: acl.cloned(),
            default,
            follow,
        };
        match self.request(RequestKind::SetAcl(request)).await? {
            ResponseKind::SetAcl(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        mask: dolang_winterop::security::SecInfo,
        follow: bool,
    ) -> crate::Result<SecDesc> {
        let request = SecDescRequest {
            path: path.into(),
            mask,
            follow,
        };
        match self.request(RequestKind::SecDesc(request)).await? {
            ResponseKind::SecDesc(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn set_sec_desc(
        &self,
        path: Utf8TypedPath<'_>,
        sec_desc: &SecDesc,
        follow: bool,
    ) -> crate::Result<()> {
        let request = SetSecDescRequest {
            path: path.into(),
            sec_desc: sec_desc.clone(),
            follow,
        };
        match self.request(RequestKind::SetSecDesc(request)).await? {
            ResponseKind::SetSecDesc(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn create_dir(&self, path: Utf8TypedPath<'_>, all: bool) -> crate::Result<()> {
        let request = CreateDirRequest {
            path: path.into(),
            all,
        };
        match self.request(RequestKind::CreateDir(request)).await? {
            ResponseKind::CreateDir(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn remove_dir(
        &self,
        path: Utf8TypedPath<'_>,
        all: bool,
        ignore: bool,
    ) -> crate::Result<()> {
        let request = RemoveDirRequest {
            path: path.into(),
            ignore,
            all,
        };
        match self.request(RequestKind::RemoveDir(request)).await? {
            ResponseKind::RemoveDir(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn copy(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        all: bool,
    ) -> crate::Result<()> {
        let request = CopyRequest {
            from: from.into(),
            to: to.into(),
            all,
        };
        match self.request(RequestKind::Copy(request)).await? {
            ResponseKind::Copy(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn rename(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        replace: bool,
    ) -> crate::Result<()> {
        let request = RenameRequest {
            from: from.into(),
            to: to.into(),
            replace,
        };
        match self.request(RequestKind::Rename(request)).await? {
            ResponseKind::Rename(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn move_(
        &self,
        from: Utf8TypedPath<'_>,
        to: Utf8TypedPath<'_>,
        all: bool,
    ) -> crate::Result<()> {
        let request = MoveRequest {
            from: from.into(),
            to: to.into(),
            all,
        };
        match self.request(RequestKind::Move(request)).await? {
            ResponseKind::Move(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn symlink(
        &self,
        cwd: Utf8TypedPath<'_>,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        let request = SymlinkRequest {
            cwd: cwd.into(),
            src: src.into(),
            dst: dst.into(),
            kind: SymlinkKind::Infer,
        };
        match self.request(RequestKind::Symlink(request)).await? {
            ResponseKind::Symlink(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn hard_link(&self, src: Utf8TypedPath<'_>, dst: Utf8TypedPath<'_>) -> crate::Result<()> {
        let request = HardLinkRequest {
            src: src.into(),
            dst: dst.into(),
        };
        match self.request(RequestKind::HardLink(request)).await? {
            ResponseKind::HardLink(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn symlink_dir(
        &self,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        let request = SymlinkRequest {
            cwd: WirePath::empty_like(src),
            src: src.into(),
            dst: dst.into(),
            kind: SymlinkKind::Dir,
        };
        match self.request(RequestKind::Symlink(request)).await? {
            ResponseKind::Symlink(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn symlink_file(
        &self,
        src: Utf8TypedPath<'_>,
        dst: Utf8TypedPath<'_>,
    ) -> crate::Result<()> {
        let request = SymlinkRequest {
            cwd: WirePath::empty_like(src),
            src: src.into(),
            dst: dst.into(),
            kind: SymlinkKind::File,
        };
        match self.request(RequestKind::Symlink(request)).await? {
            ResponseKind::Symlink(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn symlink_metadata(&self, path: Utf8TypedPath<'_>) -> crate::Result<Metadata> {
        let request = MetadataRequest { path: path.into() };
        match self.request(RequestKind::SymlinkMetadata(request)).await? {
            ResponseKind::SymlinkMetadata(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn set_metadata(
        &self,
        paths: &[Utf8TypedPathBuf],
        patch: MetadataPatch,
    ) -> crate::Result<()> {
        let request = SetMetadataRequest {
            paths: paths.iter().map(|path| path.to_path().into()).collect(),
            patch,
        };
        match self.request(RequestKind::SetMetadata(request)).await? {
            ResponseKind::SetMetadata(result) => result.map_err(crate::Error::from),
            response => Err(unexpected(response).into()),
        }
    }

    async fn canonicalize(&self, path: Utf8TypedPath<'_>) -> crate::Result<Utf8TypedPathBuf> {
        let request = CanonicalizeRequest { path: path.into() };
        match self.request(RequestKind::Canonicalize(request)).await? {
            ResponseKind::Canonicalize(result) => {
                result.map_err(crate::Error::from).map(Into::into)
            }
            response => Err(unexpected(response).into()),
        }
    }

    async fn read_link(&self, path: Utf8TypedPath<'_>) -> crate::Result<Utf8TypedPathBuf> {
        let request = ReadLinkRequest { path: path.into() };
        match self.request(RequestKind::ReadLink(request)).await? {
            ResponseKind::ReadLink(result) => result.map_err(crate::Error::from).map(Into::into),
            response => Err(unexpected(response).into()),
        }
    }

    async fn glob(
        &self,
        pattern: impl Into<String>,
        root: Utf8TypedPath<'_>,
        follow_symlinks: bool,
        max_depth: Option<usize>,
    ) -> crate::Result<Vec<Utf8TypedPathBuf>> {
        let request = GlobRequest {
            pattern: pattern.into(),
            root: root.into(),
            follow_symlinks,
            max_depth,
        };
        match self.request(RequestKind::Glob(request)).await? {
            ResponseKind::Glob(result) => Ok(result
                .map_err(crate::Error::from)?
                .into_iter()
                .map(Utf8TypedPathBuf::from)
                .collect()),
            response => Err(unexpected(response).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{Client, ClientChildState};
    use crate::{Child as _, Command as _, Server, Vfs as _, protocol::RequestKind};

    #[cfg(unix)]
    fn successful_command(client: &Client) -> super::CommandBuilder<'_> {
        let mut command =
            client.command(crate::Utf8TypedPath::Unix(crate::Utf8UnixPath::new("sh")));
        command.arg("-c").arg("exit 0");
        command
    }

    #[cfg(windows)]
    fn successful_command(client: &Client) -> super::CommandBuilder<'_> {
        let mut command = client.command(crate::Utf8TypedPath::Windows(
            crate::Utf8WindowsPath::new("cmd"),
        ));
        command.arg("/C").arg("exit 0");
        command
    }

    #[tokio::test]
    async fn child_wait_caches_wire_error() {
        let (client_stream, server_stream) = tokio::io::duplex(1024 * 1024);
        let server =
            tokio::spawn(async move { Server::new(server_stream).await.unwrap().serve().await });
        let client = Client::new(client_stream).await.unwrap();
        let mut child = successful_command(&client).spawn().await.unwrap();
        let ClientChildState::Live(opaque) = &child.state else {
            panic!("new child is not live");
        };
        let response = client
            .request(RequestKind::ChildClose {
                child: opaque.cite(),
            })
            .await
            .unwrap();
        let crate::protocol::ResponseKind::ChildClose(Ok(())) = response else {
            panic!("child close returned the wrong response");
        };

        let first = child.wait().await.unwrap_err();
        let second = child.wait().await.unwrap_err();
        assert_eq!(first.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(second.kind(), first.kind());
        assert_eq!(second.to_string(), first.to_string());

        client.stop().await.unwrap();
        server.await.unwrap().unwrap();
    }
}
