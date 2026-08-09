use std::{
    collections::HashSet,
    io::{self, IoSlice},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::Arc,
    task::{Context, Poll},
};

use bytes::BufMut;
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE},
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        Threading::{GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE},
    },
};

use crate::handle::{ErasedHandle, PutHandle};

use super::{Receiver, RecvFrame, SendFrame, Sender};

enum Pipe {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}

impl Pipe {
    fn peer_pid(&self) -> io::Result<u32> {
        match self {
            Self::Server(pipe) => server_pipe_peer_pid(pipe),
            Self::Client(pipe) => client_pipe_peer_pid(pipe),
        }
    }

    fn poll_read_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self {
            Self::Server(pipe) => pipe.poll_read_ready(cx),
            Self::Client(pipe) => pipe.poll_read_ready(cx),
        }
    }

    fn poll_write_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self {
            Self::Server(pipe) => pipe.poll_write_ready(cx),
            Self::Client(pipe) => pipe.poll_write_ready(cx),
        }
    }

    fn try_read_buf<B: BufMut>(&self, buffer: &mut B) -> io::Result<usize> {
        match self {
            Self::Server(pipe) => pipe.try_read_buf(buffer),
            Self::Client(pipe) => pipe.try_read_buf(buffer),
        }
    }

    fn try_write_vectored(&self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        match self {
            Self::Server(pipe) => pipe.try_write_vectored(buffers),
            Self::Client(pipe) => pipe.try_write_vectored(buffers),
        }
    }
}

pub(crate) fn server_pipe_peer_pid(pipe: &NamedPipeServer) -> io::Result<u32> {
    let mut pid = 0;
    let ok = unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(pid)
    }
}

pub(crate) fn client_pipe_peer_pid(pipe: &NamedPipeClient) -> io::Result<u32> {
    let mut pid = 0;
    let ok = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(pid)
    }
}

struct Common {
    pipe: Pipe,
    is_server: bool,
    peer_process: Option<OwnedHandle>,
}

pub(crate) struct WindowsSender(Arc<Common>);
pub(crate) struct WindowsReceiver(Arc<Common>);

pub(crate) fn server_pipe(
    pipe: NamedPipeServer,
    is_server: bool,
) -> io::Result<(WindowsSender, WindowsReceiver)> {
    new(Pipe::Server(pipe), is_server)
}

pub(crate) fn client_pipe(
    pipe: NamedPipeClient,
    is_server: bool,
) -> io::Result<(WindowsSender, WindowsReceiver)> {
    new(Pipe::Client(pipe), is_server)
}

fn new(pipe: Pipe, is_server: bool) -> io::Result<(WindowsSender, WindowsReceiver)> {
    let peer_process = if is_server {
        let process = unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, pipe.peer_pid()?) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        Some(unsafe { OwnedHandle::from_raw_handle(process as _) })
    } else {
        None
    };
    let common = Arc::new(Common {
        pipe,
        is_server,
        peer_process,
    });
    Ok((WindowsSender(common.clone()), WindowsReceiver(common)))
}

pub(crate) struct WindowsSend<'a>(&'a mut WindowsSender);

pub(crate) struct WindowsPutHandle<'handle> {
    common: Arc<Common>,
    handles: Vec<&'handle dyn ErasedHandle>,
    duplicated: Vec<HANDLE>,
}

pub(crate) struct WindowsRecv<'a> {
    receiver: &'a mut WindowsReceiver,
    consumed: HashSet<usize>,
}

impl Sender for WindowsSender {
    type Send<'a> = WindowsSend<'a>;

    fn send(&mut self) -> Self::Send<'_> {
        WindowsSend(self)
    }
}

impl WindowsSender {
    pub(crate) fn put_handles<'handle>(&self) -> WindowsPutHandle<'handle> {
        WindowsPutHandle {
            common: self.0.clone(),
            handles: Vec::new(),
            duplicated: Vec::new(),
        }
    }
}

impl WindowsPutHandle<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// Steals the serialized source handles and relinquishes rollback of
    /// handles already duplicated into the peer process.
    pub(crate) fn finish(mut self) -> Vec<OwnedHandle> {
        let handles = self
            .handles
            .drain(..)
            .map(ErasedHandle::steal_handle)
            .collect();
        self.duplicated.clear();
        handles
    }
}

impl<'handle> PutHandle<'handle> for WindowsPutHandle<'handle> {
    fn put_handle(&mut self, handle: &'handle dyn ErasedHandle) -> io::Result<usize> {
        if self
            .handles
            .iter()
            .any(|existing| std::ptr::eq(*existing, handle))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the same operating-system handle was serialized more than once",
            ));
        }
        let raw = handle.raw_handle();
        let value = if self.common.is_server {
            // SAFETY: both process handles and the source handle are valid.
            // The result is an opaque value owned by the peer process.
            let duplicated = unsafe {
                duplicate_raw(
                    GetCurrentProcess(),
                    raw as HANDLE,
                    self.common.peer_process.as_ref().unwrap().as_raw_handle() as HANDLE,
                )?
            };
            self.duplicated.push(duplicated);
            duplicated as usize
        } else {
            raw as usize
        };
        self.handles.push(handle);
        Ok(value)
    }
}

impl Receiver for WindowsReceiver {
    type Recv<'a> = WindowsRecv<'a>;

    fn recv(&mut self) -> Self::Recv<'_> {
        WindowsRecv {
            receiver: self,
            consumed: HashSet::new(),
        }
    }
}

impl RecvFrame for WindowsRecv<'_> {
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle> {
        let common = &self.receiver.0;
        if !common.is_server && !self.consumed.insert(value) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "handle value was already consumed",
            ));
        }
        if common.is_server {
            // SAFETY: the trusted peer serialized a handle valid in its own
            // process. The duplicated result is valid in the current process.
            let raw = unsafe {
                duplicate_raw(
                    common.peer_process.as_ref().unwrap().as_raw_handle() as HANDLE,
                    value as HANDLE,
                    GetCurrentProcess(),
                )?
            };
            Ok(unsafe { OwnedHandle::from_raw_handle(raw as _) })
        } else {
            // SAFETY: the trusted server created this value in our process
            // with DuplicateHandle before transmitting the frame.
            Ok(unsafe { OwnedHandle::from_raw_handle(value as _) })
        }
    }

    fn poll_read_once<B: BufMut>(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut B,
    ) -> Poll<io::Result<usize>> {
        loop {
            // Use separate readable and try_read_buf operations to avoid
            // using `&mut self` methods on the named pipe, allowing sender
            // and receiver sides to share it via `Arc` without additional
            // synchronization.
            match self.receiver.0.pipe.poll_read_ready(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
            match self.receiver.0.pipe.try_read_buf(buffer) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                result => return Poll::Ready(result),
            }
        }
    }
}

impl SendFrame<'_> for WindowsSend<'_> {
    fn poll_write_once(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.poll_write_vectored_once(cx, &[IoSlice::new(buf)])
    }

    fn poll_write_vectored_once(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        loop {
            // Use the raw, non-exclusive `poll_write_ready`/`try_write` pair
            // (rather than `AsyncWrite::poll_write`, which needs `&mut` on
            // the pipe itself) so the sender and receiver sides can share
            // the pipe via `Arc` without additional synchronization.
            match self.0.0.pipe.poll_write_ready(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
            match self.0.0.pipe.try_write_vectored(bufs) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return Poll::Ready(result),
            }
        }
    }
}

impl Drop for WindowsPutHandle<'_> {
    fn drop(&mut self) {
        if !self.common.is_server {
            return;
        }
        let peer_process = self.common.peer_process.as_ref().unwrap().as_raw_handle() as HANDLE;
        for handle in self.duplicated.drain(..) {
            // SAFETY: `peer_process` remains owned by `common`, and every
            // recorded value was returned by DuplicateHandle for that process.
            unsafe { close_remote(peer_process, handle) };
        }
    }
}

/// Closes a handle in another process, discarding the duplicated local handle.
///
/// # Safety
///
/// `peer_process` must be a valid process handle with duplication rights, and
/// `handle` must be a valid handle in that process.
unsafe fn close_remote(peer_process: HANDLE, handle: HANDLE) {
    let mut local = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            peer_process,
            handle,
            GetCurrentProcess(),
            &mut local,
            0,
            0,
            DUPLICATE_SAME_ACCESS | DUPLICATE_CLOSE_SOURCE,
        )
    };
    if ok != 0 {
        drop(unsafe { OwnedHandle::from_raw_handle(local as _) });
    }
}

/// Duplicates a raw handle between process handle tables.
///
/// # Safety
///
/// `source_process`, `source`, and `target_process` must be valid handles in
/// the contexts required by `DuplicateHandle`. The returned value is valid
/// only in `target_process` and must not be treated as locally owned unless
/// that is the current process.
unsafe fn duplicate_raw(
    source_process: HANDLE,
    source: HANDLE,
    target_process: HANDLE,
) -> io::Result<HANDLE> {
    let mut target = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            source_process,
            source,
            target_process,
            &mut target,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::windows::io::AsRawHandle,
        sync::atomic::{AtomicU64, Ordering},
    };

    use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
    use windows_sys::Win32::Foundation::CompareObjectHandles;

    use super::*;
    use crate::handle::OsHandle;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(0);

    async fn pipe_pair() -> (NamedPipeServer, NamedPipeClient) {
        let id = NEXT_PIPE.fetch_add(1, Ordering::Relaxed);
        let name = format!(r"\\.\pipe\dolang-rpc-transport-{}-{id}", std::process::id());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let client = ClientOptions::new().open(&name).unwrap();
        server.connect().await.unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn discovers_peer_from_either_pipe_end() {
        let (pipe_server, pipe_client) = pipe_pair().await;
        let _ = server_pipe(pipe_server, true).unwrap();
        let _ = client_pipe(pipe_client, true).unwrap();
    }

    #[tokio::test]
    async fn rejects_duplicate_client_adoption() {
        let (pipe_server, pipe_client) = pipe_pair().await;
        let (_, mut client_receiver) = server_pipe(pipe_server, false).unwrap();
        let (server_sender, _) = client_pipe(pipe_client, true).unwrap();
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let handle = OsHandle::new(OwnedHandle::from(file));
        let mut handles = server_sender.put_handles();
        let value = handles.put_handle(&handle).unwrap();
        drop(handles.finish());

        let mut frame = client_receiver.recv();
        let received = frame.take_handle(value).unwrap();
        assert_eq!(
            frame.take_handle(value).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        drop(received);
    }

    #[tokio::test]
    async fn dropping_unfinished_handle_transaction_closes_duplicates() {
        let (pipe_server, pipe_client) = pipe_pair().await;
        let _client = server_pipe(pipe_server, false).unwrap();
        let (server_sender, _) = client_pipe(pipe_client, true).unwrap();
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let handle = OsHandle::new(OwnedHandle::from(file.try_clone().unwrap()));
        let mut handles = server_sender.put_handles();
        let value = handles.put_handle(&handle).unwrap();

        assert_ne!(
            unsafe { CompareObjectHandles(value as HANDLE, file.as_raw_handle() as HANDLE) },
            0
        );
        drop(handles);
        assert_eq!(
            unsafe { CompareObjectHandles(value as HANDLE, file.as_raw_handle() as HANDLE) },
            0
        );
    }

    #[tokio::test]
    async fn duplicates_client_handle_into_server() {
        let (pipe_server, pipe_client) = pipe_pair().await;
        let (client_sender, _) = server_pipe(pipe_server, false).unwrap();
        let (_, mut server_receiver) = client_pipe(pipe_client, true).unwrap();
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let handle = OsHandle::new(OwnedHandle::from(file));
        let mut handles = client_sender.put_handles();
        let value = handles.put_handle(&handle).unwrap();
        let handles = handles.finish();
        let received = server_receiver.recv().take_handle(value).unwrap();
        drop(received);
        drop(handles);
    }
}
