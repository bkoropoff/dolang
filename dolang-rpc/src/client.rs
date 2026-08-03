use std::{
    collections::HashMap,
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
};

#[cfg(unix)]
use std::os::unix::net::UnixStream;

use bytes::Buf;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot},
};

#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, OwnedHandle};

#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};

#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetProcessId;

use crate::{
    Error, Kind, Limits, Protocol, decode, encode_payload, fragment,
    transport::{self, Receiver, SendFrame, Sender},
};

type Pending<R> = HashMap<u64, oneshot::Sender<Result<R, Error>>>;

enum Message<Q> {
    Request { id: u64, value: Q },
    Cancel { id: u64 },
}

struct Inner<P: Protocol> {
    outgoing: mpsc::UnboundedSender<Message<P::Request>>,
    pending: Mutex<Pending<P::Response>>,
    next_id: Mutex<u64>,
    tasks: Mutex<Option<Tasks>>,
    request_keepalive: Mutex<HashMap<u64, P::Request>>,
    #[cfg(windows)]
    _peer_process: Option<OwnedHandle>,
}

struct Writer<P: Protocol> {
    transport: transport::AnySender,
    outgoing: mpsc::UnboundedReceiver<Message<P::Request>>,
    inner: Weak<Inner<P>>,
    keep_requests_alive: bool,
    limits: Limits,
}

struct Reader<P: Protocol> {
    transport: transport::AnyReceiver,
    inner: Weak<Inner<P>>,
    limits: Limits,
}

struct Tasks {
    writer_shutdown: Option<oneshot::Sender<()>>,
    reader_shutdown: Option<oneshot::Sender<()>>,
    writer: tokio::task::JoinHandle<()>,
    reader: tokio::task::JoinHandle<()>,
}

impl Tasks {
    fn shutdown(&mut self) {
        if let Some(shutdown) = self.writer_shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(shutdown) = self.reader_shutdown.take() {
            let _ = shutdown.send(());
        }
    }

    async fn join(mut self) {
        self.shutdown();
        let _ = tokio::join!(self.writer, self.reader);
    }
}

impl<P: Protocol> Drop for Inner<P> {
    fn drop(&mut self) {
        if let Some(tasks) = self.tasks.get_mut().unwrap().as_mut() {
            tasks.shutdown();
        }
        self.fail(Error::ConnectionClosed);
    }
}

impl<P: Protocol> Inner<P> {
    fn complete(&self, id: u64, result: Result<P::Response, Error>) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(result);
        }
    }

    fn fail(&self, error: Error) {
        for (_, tx) in std::mem::take(&mut *self.pending.lock().unwrap()) {
            let _ = tx.send(Err(error.copy()));
        }
    }
}

/// A cloneable request endpoint.
pub struct Client<P: Protocol> {
    inner: Arc<Inner<P>>,
}

impl<P: Protocol> Clone for Client<P> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<P: Protocol> Client<P> {
    /// Returns whether both clients refer to the same RPC session.
    pub fn is_same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Starts a client session on a bidirectional byte stream.
    pub fn new<T>(stream: T) -> Self
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Self::with_limits(stream, Limits::default())
    }

    /// Starts a client session on separate byte-stream reader and writer halves.
    pub fn new_split<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + 'static,
        W: AsyncWrite + Send + 'static,
    {
        let (sender, receiver) = transport::generic(reader, writer);
        Self::from_transport(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            Limits::default(),
            false,
            #[cfg(windows)]
            None,
        )
    }

    /// Starts a client session with explicit size and concurrency limits.
    pub fn with_limits<T>(stream: T, limits: Limits) -> Self
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (sender, receiver) = transport::generic_duplex(stream);
        Self::from_transport(
            transport::AnySender::Generic(sender),
            transport::AnyReceiver::Generic(receiver),
            limits,
            false,
            #[cfg(windows)]
            None,
        )
    }

    #[cfg(unix)]
    pub fn from_unix_stream(stream: UnixStream) -> io::Result<Self> {
        let (sender, receiver) = transport::unix::unix(stream)?;
        Ok(Self::from_transport(
            transport::AnySender::Unix(sender),
            transport::AnyReceiver::Unix(receiver),
            Limits::default(),
            false,
            #[cfg(windows)]
            None,
        ))
    }

    #[cfg(windows)]
    /// Starts a client session on the server end of a Windows named pipe.
    ///
    /// `peer_process` is retained for the lifetime of the session and must
    /// grant process-query and synchronization access. Construction fails if
    /// it does not identify the named-pipe peer.
    ///
    /// # Safety
    ///
    /// The identified peer must be trusted to send only handle values that it
    /// created in this process with `DuplicateHandle`. A malicious peer can
    /// otherwise cause this process to close arbitrary handles.
    pub unsafe fn from_named_pipe_server(
        pipe: NamedPipeServer,
        peer_process: OwnedHandle,
    ) -> io::Result<Self> {
        validate_peer_process(
            &peer_process,
            transport::windows::server_pipe_peer_pid(&pipe)?,
        )?;
        let (sender, receiver) = transport::windows::server_pipe(pipe, false)?;
        Ok(Self::from_transport(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            Limits::default(),
            true,
            Some(peer_process),
        ))
    }

    #[cfg(windows)]
    /// Starts a client session on the client end of a Windows named pipe.
    ///
    /// `peer_process` is retained for the lifetime of the session and must
    /// grant process-query and synchronization access. Construction fails if
    /// it does not identify the named-pipe peer.
    ///
    /// # Safety
    ///
    /// The identified peer must be trusted to send only handle values that it
    /// created in this process with `DuplicateHandle`. A malicious peer can
    /// otherwise cause this process to close arbitrary handles.
    pub unsafe fn from_named_pipe_client(
        pipe: NamedPipeClient,
        peer_process: OwnedHandle,
    ) -> io::Result<Self> {
        validate_peer_process(
            &peer_process,
            transport::windows::client_pipe_peer_pid(&pipe)?,
        )?;
        let (sender, receiver) = transport::windows::client_pipe(pipe, false)?;
        Ok(Self::from_transport(
            transport::AnySender::Windows(sender),
            transport::AnyReceiver::Windows(receiver),
            Limits::default(),
            true,
            Some(peer_process),
        ))
    }

    fn from_transport(
        sender: transport::AnySender,
        receiver: transport::AnyReceiver,
        limits: Limits,
        keep_requests_alive: bool,
        #[cfg(windows)] peer_process: Option<OwnedHandle>,
    ) -> Self {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            outgoing,
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(0),
            tasks: Mutex::new(None),
            request_keepalive: Mutex::new(HashMap::new()),
            #[cfg(windows)]
            _peer_process: peer_process,
        });
        let (writer_shutdown, writer_stop) = oneshot::channel();
        let (reader_shutdown, reader_stop) = oneshot::channel();
        let writer = tokio::spawn(
            Writer {
                transport: sender,
                outgoing: outgoing_rx,
                inner: Arc::downgrade(&inner),
                keep_requests_alive,
                limits,
            }
            .run(writer_stop),
        );
        let reader = tokio::spawn(
            Reader {
                transport: receiver,
                inner: Arc::downgrade(&inner),
                limits,
            }
            .run(reader_stop),
        );
        *inner.tasks.lock().unwrap() = Some(Tasks {
            writer_shutdown: Some(writer_shutdown),
            reader_shutdown: Some(reader_shutdown),
            writer,
            reader,
        });
        Self { inner }
    }

    /// Stops the session and waits for its background tasks to exit.
    pub async fn close(self) {
        let tasks = self.inner.tasks.lock().unwrap().take();
        self.inner.fail(Error::ConnectionClosed);
        if let Some(tasks) = tasks {
            tasks.join().await;
        }
    }

    /// Begins one request.
    pub fn call(&self, request: P::Request) -> Call<P> {
        let (tx, rx) = oneshot::channel();
        let tasks = self.inner.tasks.lock().unwrap();
        let id = {
            let mut next = self.inner.next_id.lock().unwrap();
            let id = *next;
            *next = id.checked_add(1).expect("request identifiers exhausted");
            id
        };
        if tasks.is_none() {
            let _ = tx.send(Err(Error::ConnectionClosed));
            return Call {
                id,
                rx,
                inner: self.inner.clone(),
                cancel_sent: true,
            };
        }
        self.inner.pending.lock().unwrap().insert(id, tx);
        let queued = self
            .inner
            .outgoing
            .send(Message::Request { id, value: request })
            .is_ok();
        drop(tasks);
        if !queued {
            self.inner.complete(id, Err(Error::ConnectionClosed));
        }
        Call {
            id,
            rx,
            inner: self.inner.clone(),
            cancel_sent: !queued,
        }
    }
}

#[cfg(windows)]
fn validate_peer_process(peer_process: &OwnedHandle, pipe_peer_pid: u32) -> io::Result<()> {
    let process_pid = unsafe { GetProcessId(peer_process.as_raw_handle() as _) };
    if process_pid == 0 {
        return Err(io::Error::last_os_error());
    }
    if process_pid != pipe_peer_pid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "named-pipe peer does not match the expected process",
        ));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::os::windows::io::FromRawHandle;

    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    use super::*;

    fn current_process_handle() -> OwnedHandle {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                GetCurrentProcessId(),
            )
        };
        assert!(!handle.is_null());
        unsafe { OwnedHandle::from_raw_handle(handle as _) }
    }

    #[test]
    fn validates_named_pipe_peer_process() {
        let process = current_process_handle();
        let pid = unsafe { GetCurrentProcessId() };
        validate_peer_process(&process, pid).unwrap();
        assert_eq!(
            validate_peer_process(&process, !pid).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}

/// An in-progress RPC request.
pub struct Call<P: Protocol> {
    id: u64,
    rx: oneshot::Receiver<Result<P::Response, Error>>,
    inner: Arc<Inner<P>>,
    cancel_sent: bool,
}

impl<P: Protocol> Call<P> {
    /// Requests cancellation. The call remains awaitable.
    pub fn cancel(&mut self) {
        if !self.cancel_sent {
            self.cancel_sent = true;
            let _ = self.inner.outgoing.send(Message::Cancel { id: self.id });
        }
    }
}

impl<P: Protocol> Future for Call<P> {
    type Output = Result<P::Response, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.rx).poll(cx) {
            Poll::Ready(Ok(Ok(response))) => Poll::Ready(Ok(response)),
            Poll::Ready(Ok(Err(e))) => Poll::Ready(Err(e)),
            Poll::Ready(Err(_)) => Poll::Ready(Err(Error::ConnectionClosed)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<P: Protocol> Drop for Call<P> {
    fn drop(&mut self) {
        if self
            .inner
            .pending
            .lock()
            .unwrap()
            .remove(&self.id)
            .is_some()
        {
            self.cancel();
        }
    }
}

impl<P: Protocol> Writer<P> {
    /// Completes a pending call with an error. Returns `true` if the
    /// session is already gone and the writer should stop.
    fn complete_err(&self, id: u64, error: Error) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return true;
        };
        inner.complete(id, Err(error));
        false
    }

    /// Fails every pending call. Used for transport-level (I/O) failures,
    /// which are fatal for the whole session rather than one call.
    fn fail_all(&self, error: Error) {
        if let Some(inner) = self.inner.upgrade() {
            inner.fail(error);
        }
    }

    /// Admits one queued item into the scheduler (or sends it immediately,
    /// for the native-handle atomic path). Returns `true` if the writer
    /// should stop.
    async fn admit(
        &mut self,
        message: Message<P::Request>,
        scheduler: &mut fragment::Scheduler,
    ) -> bool {
        match message {
            Message::Request { id, value } => self.admit_request(id, value, scheduler).await,
            Message::Cancel { id } => {
                self.admit_cancel(id, scheduler);
                false
            }
        }
    }

    async fn admit_request(
        &mut self,
        id: u64,
        value: P::Request,
        scheduler: &mut fragment::Scheduler,
    ) -> bool {
        let mut probe = self.transport.send();
        let payload = match encode_payload(&value, &mut probe) {
            Ok(payload) => payload,
            Err(err) => {
                drop(probe);
                return self.complete_err(id, err);
            }
        };
        if probe.has_attachments() {
            let header = fragment::FragmentHeader {
                flags: fragment::Flags::FIRST | fragment::Flags::LAST,
                kind: Kind::Request,
                id,
                payload_len: payload.len(),
            };
            let mut buffer = header.encode().chain(payload);
            if let Err(err) = probe.finish(&mut buffer).await {
                self.fail_all(err.into());
                return true;
            }
        } else {
            drop(probe);
            scheduler.admit_message(Kind::Request, id, payload);
        }
        if self.keep_requests_alive {
            let Some(inner) = self.inner.upgrade() else {
                return true;
            };
            inner.request_keepalive.lock().unwrap().insert(id, value);
        }
        false
    }

    fn admit_cancel(&mut self, id: u64, scheduler: &mut fragment::Scheduler) {
        match scheduler.try_cancel_active(id) {
            fragment::AbortOutcome::NotActive => scheduler.admit_empty(Kind::Cancel, id),
            fragment::AbortOutcome::Discarded { started } => {
                if started {
                    scheduler.admit_abort(id);
                }
                let _ = self.complete_err(id, Error::Cancelled);
            }
        }
    }

    async fn run(mut self, mut shutdown: oneshot::Receiver<()>) {
        let mut scheduler = fragment::Scheduler::new(&self.limits);
        loop {
            while let Ok(message) = self.outgoing.try_recv() {
                if self.admit(message, &mut scheduler).await {
                    return;
                }
            }
            if !scheduler.has_work() {
                let message = tokio::select! {
                    message = self.outgoing.recv() => message,
                    _ = &mut shutdown => return,
                };
                let Some(message) = message else {
                    return;
                };
                if self.admit(message, &mut scheduler).await {
                    return;
                }
                continue;
            }
            let result = tokio::select! {
                result = scheduler.advance(&mut self.transport) => result,
                _ = &mut shutdown => return,
            };
            if let Err(err) = result {
                self.fail_all(err);
                return;
            }
        }
    }
}

impl<P: Protocol> Reader<P> {
    async fn run(mut self, mut shutdown: oneshot::Receiver<()>) {
        let mut reassembler = fragment::Reassembler::new(self.limits);
        loop {
            let mut frame = self.transport.recv();
            let header = tokio::select! {
                header = fragment::read_fragment_header(&mut frame) => header,
                _ = &mut shutdown => return,
            };
            let header = match header {
                Ok(header) => header,
                Err(error) => {
                    fail(&self.inner, error);
                    return;
                }
            };
            let accepted = tokio::select! {
                accepted = reassembler.accept_fragment(header, &mut frame) => accepted,
                _ = &mut shutdown => return,
            };
            let complete = match accepted {
                Ok(complete) => complete,
                Err(error) => {
                    fail(&self.inner, error);
                    return;
                }
            };
            let Some(fragment::CompleteMessage { kind, id, payload }) = complete else {
                continue;
            };
            let Some(inner) = self.inner.upgrade() else {
                return;
            };
            match kind {
                Kind::Response => match decode(&payload, &mut frame) {
                    Ok(response) => {
                        inner.request_keepalive.lock().unwrap().remove(&id);
                        inner.complete(id, Ok(response));
                    }
                    Err(error) => {
                        inner.fail(error);
                        return;
                    }
                },
                Kind::Error => {
                    inner.request_keepalive.lock().unwrap().remove(&id);
                    inner.complete(id, Err(Error::Cancelled));
                }
                kind => {
                    inner.fail(Error::Protocol(format!("unexpected {kind:?} frame")));
                    return;
                }
            }
        }
    }
}

/// Fails every pending call. Takes `inner` by reference (rather than a
/// `Reader` method borrowing `&self`) so it can be called while another
/// field (e.g. a `RecvFrame` token borrowing `self.transport`) is still
/// mutably borrowed.
fn fail<P: Protocol>(inner: &Weak<Inner<P>>, error: Error) {
    if let Some(inner) = inner.upgrade() {
        inner.fail(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Test;

    impl Protocol for Test {
        type Request = u8;
        type Response = u8;
    }

    fn pending_call() -> (Call<Test>, mpsc::UnboundedReceiver<Message<u8>>) {
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(Inner {
            outgoing,
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            tasks: Mutex::new(None),
            request_keepalive: Mutex::new(HashMap::new()),
            #[cfg(windows)]
            _peer_process: None,
        });
        let (tx, rx) = oneshot::channel();
        inner.pending.lock().unwrap().insert(0, tx);
        (
            Call {
                id: 0,
                rx,
                inner,
                cancel_sent: false,
            },
            outgoing_rx,
        )
    }

    #[tokio::test]
    async fn completed_call_does_not_send_cancel_when_dropped() {
        let (call, mut outgoing) = pending_call();
        let inner = call.inner.clone();
        call.inner.complete(call.id, Ok(7));
        assert_eq!(call.await.unwrap(), 7);
        assert!(matches!(
            outgoing.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        drop(inner);
    }

    #[test]
    fn dropped_pending_call_sends_cancel() {
        let (call, mut outgoing) = pending_call();
        drop(call);
        assert!(matches!(outgoing.try_recv(), Ok(Message::Cancel { id: 0 })));
    }
}
