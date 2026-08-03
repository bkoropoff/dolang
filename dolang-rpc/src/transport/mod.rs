#[cfg(unix)]
use std::os::fd::{BorrowedFd, OwnedFd};
#[cfg(windows)]
use std::os::windows::io::{BorrowedHandle, OwnedHandle};
use std::{
    future::poll_fn,
    io::{self, IoSlice},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, BufMut};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Bound on the number of `IoSlice`s gathered from a chained `Buf` for one
/// vectored write attempt.
pub(crate) const MAX_VECTORED_SLICES: usize = 8;

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub(crate) trait Sender: Send + 'static {
    type Send<'a>: SendFrame<'a>
    where
        Self: 'a;

    fn send(&mut self) -> Self::Send<'_>;
}

pub(crate) trait Receiver: Send + 'static {
    type Recv<'a>: RecvFrame
    where
        Self: 'a;

    fn recv(&mut self) -> Self::Recv<'_>;
}

pub(crate) trait RecvFrame: Send {
    #[cfg(unix)]
    fn take_fd(&mut self, index: u32) -> io::Result<OwnedFd>;
    #[cfg(windows)]
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle>;
    fn poll_read_once<B: BufMut>(
        &mut self,
        _cx: &mut Context<'_>,
        _buffer: &mut B,
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "frame does not support direct polling",
        )))
    }

    async fn recv<B: BufMut>(&mut self, buffer: &mut B) -> io::Result<usize> {
        poll_fn(|cx| self.poll_read_once(cx, buffer)).await
    }
}

pub(crate) trait SendFrame<'frame>: Send {
    #[cfg(unix)]
    fn attach_fd(&mut self, fd: BorrowedFd<'frame>) -> io::Result<u32>;
    #[cfg(windows)]
    fn attach_handle(&mut self, handle: BorrowedHandle<'_>) -> io::Result<usize>;

    /// Whether serialization has staged any native-handle attachments on
    /// this token. If true, the message must be sent as a single atomic
    /// fragment via this same token rather than split across the
    /// round-robin fragment scheduler.
    fn has_attachments(&self) -> bool;

    /// Attempts one nonblocking write of `buf`, following the same
    /// readiness/registration contract as `AsyncWrite::poll_write`. The one
    /// low-level primitive each transport must implement; `finish` (below)
    /// is provided in terms of it, mirroring how `AsyncWriteExt` methods are
    /// built on `AsyncWrite::poll_write`.
    ///
    /// Also used directly by streaming trailer leases, which hold their
    /// shared-state lock only for one synchronous poll.
    fn poll_write_once(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>>;

    /// Vectored variant. Default mirrors `AsyncWrite`'s default
    /// `poll_write_vectored`: write the first non-empty slice via
    /// `poll_write_once`. Transports with vectored-write support override this
    /// so a frame header and payload can be submitted as one write operation.
    fn poll_write_vectored_once(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let buf = bufs
            .iter()
            .find(|slice| !slice.is_empty())
            .map_or(&[][..], |slice| &**slice);
        self.poll_write_once(cx, buf)
    }

    /// Flushes any internally buffered bytes. Default no-op (raw sockets
    /// need none); `GenericSend` overrides this to flush its wrapped
    /// `AsyncWrite`.
    fn poll_flush_once(&mut self, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// Writes all of `buffer`'s remaining bytes, looping
    /// `poll_write_vectored_once` via `Buf::chunks_vectored`, then flushes.
    /// Provided — no transport implements this directly.
    ///
    /// Returns whether the whole buffer was drained by a single successful
    /// `poll_write_vectored_once` call (`true`), as opposed to needing more
    /// than one write to fully drain (`false`) — a "short write" signal
    /// callers use to adapt future fragment sizing to the transport's
    /// actual atomic write capacity.
    async fn finish<B: Buf>(mut self, buffer: &mut B) -> io::Result<bool>
    where
        Self: Sized,
    {
        let mut atomic = true;
        let mut first = true;
        while buffer.has_remaining() {
            if !first {
                atomic = false;
            }
            first = false;
            let mut slices = [IoSlice::new(&[]); MAX_VECTORED_SLICES];
            let filled = buffer.chunks_vectored(&mut slices);
            let sent = poll_fn(|cx| self.poll_write_vectored_once(cx, &slices[..filled])).await?;
            if sent == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write frame",
                ));
            }
            buffer.advance(sent);
        }
        poll_fn(|cx| self.poll_flush_once(cx)).await?;
        Ok(atomic)
    }
}

pub(crate) enum AnySender {
    Generic(GenericSender),
    #[cfg(unix)]
    Unix(unix::UnixSender),
    #[cfg(windows)]
    Windows(windows::WindowsSender),
}

pub(crate) enum AnyReceiver {
    Generic(GenericReceiver),
    #[cfg(unix)]
    Unix(unix::UnixReceiver),
    #[cfg(windows)]
    Windows(windows::WindowsReceiver),
}

pub(crate) enum AnySend<'a> {
    Generic(GenericSend<'a>),
    #[cfg(unix)]
    Unix(unix::UnixSend<'a>),
    #[cfg(windows)]
    Windows(windows::WindowsSend<'a>),
}

pub(crate) enum AnyRecv<'a> {
    Generic(GenericRecv<'a>),
    #[cfg(unix)]
    Unix(unix::UnixRecv<'a>),
    #[cfg(windows)]
    Windows(windows::WindowsRecv<'a>),
}

impl Sender for AnySender {
    type Send<'a> = AnySend<'a>;
    fn send(&mut self) -> Self::Send<'_> {
        match self {
            Self::Generic(sender) => AnySend::Generic(sender.send()),
            #[cfg(unix)]
            Self::Unix(sender) => AnySend::Unix(sender.send()),
            #[cfg(windows)]
            Self::Windows(sender) => AnySend::Windows(sender.send()),
        }
    }
}

impl<'frame> SendFrame<'frame> for AnySend<'frame> {
    #[cfg(unix)]
    fn attach_fd(&mut self, fd: BorrowedFd<'frame>) -> io::Result<u32> {
        match self {
            Self::Generic(frame) => frame.attach_fd(fd),
            Self::Unix(frame) => frame.attach_fd(fd),
        }
    }
    #[cfg(windows)]
    fn attach_handle(&mut self, handle: BorrowedHandle<'_>) -> io::Result<usize> {
        match self {
            Self::Generic(frame) => frame.attach_handle(handle),
            Self::Windows(frame) => frame.attach_handle(handle),
        }
    }
    fn has_attachments(&self) -> bool {
        match self {
            Self::Generic(frame) => frame.has_attachments(),
            #[cfg(unix)]
            Self::Unix(frame) => frame.has_attachments(),
            #[cfg(windows)]
            Self::Windows(frame) => frame.has_attachments(),
        }
    }
    fn poll_write_once(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match self {
            Self::Generic(frame) => frame.poll_write_once(cx, buf),
            #[cfg(unix)]
            Self::Unix(frame) => frame.poll_write_once(cx, buf),
            #[cfg(windows)]
            Self::Windows(frame) => frame.poll_write_once(cx, buf),
        }
    }
    fn poll_write_vectored_once(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self {
            Self::Generic(frame) => frame.poll_write_vectored_once(cx, bufs),
            #[cfg(unix)]
            Self::Unix(frame) => frame.poll_write_vectored_once(cx, bufs),
            #[cfg(windows)]
            Self::Windows(frame) => frame.poll_write_vectored_once(cx, bufs),
        }
    }
    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self {
            Self::Generic(frame) => frame.poll_flush_once(cx),
            #[cfg(unix)]
            Self::Unix(frame) => frame.poll_flush_once(cx),
            #[cfg(windows)]
            Self::Windows(frame) => frame.poll_flush_once(cx),
        }
    }
}

impl Receiver for AnyReceiver {
    type Recv<'a> = AnyRecv<'a>;

    fn recv(&mut self) -> Self::Recv<'_> {
        match self {
            Self::Generic(receiver) => AnyRecv::Generic(receiver.recv()),
            #[cfg(unix)]
            Self::Unix(receiver) => AnyRecv::Unix(receiver.recv()),
            #[cfg(windows)]
            Self::Windows(receiver) => AnyRecv::Windows(receiver.recv()),
        }
    }
}

impl RecvFrame for AnyRecv<'_> {
    #[cfg(unix)]
    fn take_fd(&mut self, index: u32) -> io::Result<OwnedFd> {
        match self {
            Self::Generic(frame) => frame.take_fd(index),
            Self::Unix(frame) => frame.take_fd(index),
        }
    }
    #[cfg(windows)]
    fn take_handle(&mut self, value: usize) -> io::Result<OwnedHandle> {
        match self {
            Self::Generic(frame) => frame.take_handle(value),
            Self::Windows(frame) => frame.take_handle(value),
        }
    }
    fn poll_read_once<B: BufMut>(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut B,
    ) -> Poll<io::Result<usize>> {
        match self {
            Self::Generic(frame) => frame.poll_read_once(cx, buffer),
            #[cfg(unix)]
            Self::Unix(frame) => frame.poll_read_once(cx, buffer),
            #[cfg(windows)]
            Self::Windows(frame) => frame.poll_read_once(cx, buffer),
        }
    }
}

pub(crate) struct GenericSender(Pin<Box<dyn AsyncWrite + Send>>);
pub(crate) struct GenericReceiver(Pin<Box<dyn AsyncRead + Send>>);

pub(crate) fn generic<R, W>(reader: R, writer: W) -> (GenericSender, GenericReceiver)
where
    R: AsyncRead + Send + 'static,
    W: AsyncWrite + Send + 'static,
{
    (
        GenericSender(Box::pin(writer)),
        GenericReceiver(Box::pin(reader)),
    )
}

pub(crate) fn generic_duplex<T>(stream: T) -> (GenericSender, GenericReceiver)
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    generic(reader, writer)
}

pub(crate) struct GenericSend<'a>(&'a mut GenericSender);
pub(crate) struct GenericRecv<'a>(&'a mut GenericReceiver);
impl Sender for GenericSender {
    type Send<'a> = GenericSend<'a>;

    fn send(&mut self) -> Self::Send<'_> {
        GenericSend(self)
    }
}

impl Receiver for GenericReceiver {
    type Recv<'a> = GenericRecv<'a>;

    fn recv(&mut self) -> Self::Recv<'_> {
        GenericRecv(self)
    }
}

impl RecvFrame for GenericRecv<'_> {
    #[cfg(unix)]
    fn take_fd(&mut self, _index: u32) -> io::Result<OwnedFd> {
        panic!("generic byte-stream transport does not support file descriptors")
    }
    #[cfg(windows)]
    fn take_handle(&mut self, _value: usize) -> io::Result<OwnedHandle> {
        panic!("generic byte-stream transport does not support handles")
    }

    fn poll_read_once<B: BufMut>(
        &mut self,
        cx: &mut Context<'_>,
        buffer: &mut B,
    ) -> Poll<io::Result<usize>> {
        let chunk = buffer.chunk_mut();
        let mut read_buf = ReadBuf::uninit(unsafe {
            std::slice::from_raw_parts_mut(chunk.as_mut_ptr().cast(), chunk.len())
        });
        match self.0.0.as_mut().poll_read(cx, &mut read_buf) {
            Poll::Ready(Ok(())) => {
                let n = read_buf.filled().len();
                // SAFETY: `poll_read` initialized the reported filled bytes.
                unsafe { buffer.advance_mut(n) };
                Poll::Ready(Ok(n))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<'frame> SendFrame<'frame> for GenericSend<'frame> {
    #[cfg(unix)]
    fn attach_fd(&mut self, _fd: BorrowedFd<'frame>) -> io::Result<u32> {
        panic!("generic byte-stream transport does not support file descriptors")
    }
    #[cfg(windows)]
    fn attach_handle(&mut self, _handle: BorrowedHandle<'_>) -> io::Result<usize> {
        panic!("generic byte-stream transport does not support handles")
    }

    fn has_attachments(&self) -> bool {
        false
    }

    fn poll_write_once(&mut self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.0.0.as_mut().poll_write(cx, buf)
    }

    fn poll_write_vectored_once(
        &mut self,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.0.0.as_mut().poll_write_vectored(cx, bufs)
    }

    fn poll_flush_once(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.0.0.as_mut().poll_flush(cx)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::fd::AsFd;

    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    #[should_panic(expected = "generic byte-stream transport does not support file descriptors")]
    async fn generic_send_rejects_file_descriptors() {
        let (stream, _) = tokio::io::duplex(64);
        let (mut sender, _) = generic_duplex(stream);
        let (fd, _) = std::os::unix::net::UnixStream::pair().unwrap();
        sender.send().attach_fd(fd.as_fd()).unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "generic byte-stream transport does not support file descriptors")]
    async fn generic_receiver_rejects_file_descriptors() {
        let (stream, _) = tokio::io::duplex(64);
        let (_, mut receiver) = generic_duplex(stream);
        receiver.recv().take_fd(0).unwrap();
    }

    #[tokio::test]
    async fn generic_poll_write_once_writes_directly() {
        let (stream, mut other) = tokio::io::duplex(64);
        let (mut sender, _) = generic_duplex(stream);
        let mut send = sender.send();
        poll_fn(|cx| send.poll_write_once(cx, b"direct"))
            .await
            .unwrap();
        let mut buf = [0u8; 6];
        other.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"direct");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::os::windows::io::AsHandle;

    use super::*;

    #[tokio::test]
    #[should_panic(expected = "generic byte-stream transport does not support handles")]
    async fn generic_send_rejects_handles() {
        let (stream, _) = tokio::io::duplex(64);
        let (mut sender, _) = generic_duplex(stream);
        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        sender.send().attach_handle(file.as_handle()).unwrap();
    }

    #[tokio::test]
    #[should_panic(expected = "generic byte-stream transport does not support handles")]
    async fn generic_receiver_rejects_handles() {
        let (stream, _) = tokio::io::duplex(64);
        let (_, mut receiver) = generic_duplex(stream);
        receiver.recv().take_handle(1).unwrap();
    }
}
