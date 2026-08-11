use std::{
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use dolang_rpc::{Builder, Error, Protocol, client::Client, server::CallContext};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// `Server<P>`/`Client<P>` are only ever reachable via `UnboundServer`/
/// `UnboundClient`, which mandate an application-protocol descriptor. Tests
/// don't care about application-protocol negotiation itself, so they all
/// share this one dummy descriptor.
const APP_PROTOCOL: (&str, &[u16]) = ("test", &[1]);

fn builder() -> Builder {
    Builder::new(APP_PROTOCOL.0, APP_PROTOCOL.1)
}

async fn unbound_client<T, P: Protocol>(stream: T) -> Client<P>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    builder().client(stream).await.unwrap().bind()
}

async fn unbound_client_with_builder<T, P: Protocol>(b: Builder, stream: T) -> Client<P>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    b.client(stream).await.unwrap().bind()
}

async fn unbound_client_split<R, W, P: Protocol>(reader: R, writer: W) -> Client<P>
where
    R: AsyncRead + Send + 'static,
    W: AsyncWrite + Send + 'static,
{
    builder().client_split(reader, writer).await.unwrap().bind()
}

#[cfg(unix)]
async fn unbound_client_unix<P: Protocol>(stream: std::os::unix::net::UnixStream) -> Client<P> {
    builder().client_unix(stream).await.unwrap().bind()
}

struct ShortWriter<W> {
    inner: W,
    max_write: usize,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ShortWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let len = buf.len().min(self.max_write);
        Pin::new(&mut self.inner).poll_write(cx, &buf[..len])
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

struct Test;
impl Protocol for Test {
    type Request = Request;
    type Response = Response;
}

#[derive(Serialize, Deserialize)]
enum Request {
    Echo(u32),
    Delay(u64),
    Shutdown,
    /// A large payload, used to force multi-fragment messages.
    Bulk(Vec<u8>),
    /// Echoes `u32` back in the response, and — if the request carried a
    /// raw trailer — echoes that trailer back as the response's trailer.
    TrailerRoundTrip(u32),
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Response(u32);

#[tokio::test]
async fn multiplexes_out_of_order_calls() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    // `UnboundServer` construction performs a real handshake, so it must run
    // concurrently with the client's own construction below (one spawned,
    // one awaited directly) rather than sequentially.
    tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::Echo(value) => Response(value),
                    Request::Delay(ms) => {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        Response(ms as u32)
                    }
                    Request::Shutdown | Request::Bulk(_) | Request::TrailerRoundTrip(_) => {
                        unreachable!()
                    }
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let slow = client.call(Request::Delay(30));
    let fast = client.call(Request::Echo(7));
    assert_eq!(fast.await.unwrap().into_response(), Response(7));
    assert_eq!(slow.await.unwrap().into_response(), Response(30));
}

#[tokio::test]
async fn split_transport_round_trip() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = tokio::io::split(client_io);
    let (server_reader, server_writer) = tokio::io::split(server_io);
    let server = tokio::spawn(async move {
        builder()
            .server_split(server_reader, server_writer)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |mut context, request| {
                let response = match request {
                    Request::Echo(value) => Response(value),
                    Request::Shutdown => {
                        context.shutdown();
                        Response(0)
                    }
                    Request::Delay(_) | Request::Bulk(_) | Request::TrailerRoundTrip(_) => {
                        unreachable!()
                    }
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_split::<_, _, Test>(client_reader, client_writer).await;
    assert_eq!(
        client.call(Request::Echo(7)).await.unwrap().into_response(),
        Response(7)
    );
    assert_eq!(
        client
            .call(Request::Shutdown)
            .await
            .unwrap()
            .into_response(),
        Response(0)
    );
    assert!(server.await.unwrap().is_ok());
}

#[tokio::test]
async fn unguarded_cancellation_aborts_handler() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let dropped = Arc::new(AtomicBool::new(false));
    let server_dropped = dropped.clone();
    tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |context, _| {
                struct SetOnDrop(Arc<AtomicBool>);
                impl Drop for SetOnDrop {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Release);
                    }
                }
                let guard = SetOnDrop(server_dropped.clone());
                tokio::time::sleep(Duration::from_secs(10)).await;
                drop(guard);
                context.respond(Response(0));
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let mut call = client.call(Request::Delay(10_000));
    tokio::time::sleep(Duration::from_millis(10)).await;
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test]
async fn guarded_cancellation_returns_normal_response() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |mut context, _| {
                let cancelled = context
                    .cancel_guard(async |_| tokio::time::sleep(Duration::from_secs(10)).await)
                    .await
                    .is_err();
                context.respond(Response(u32::from(cancelled)));
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let mut call = client.call(Request::Delay(10_000));
    tokio::time::sleep(Duration::from_millis(10)).await;
    call.cancel();
    assert_eq!(call.await.unwrap().into_response(), Response(1));
}

#[tokio::test]
async fn disconnect_fails_pending_calls() {
    let (client_io, server_io) = tokio::io::duplex(64);
    // A real server is needed so the client's construction handshake has a
    // peer to negotiate with; its handler never responds, so the pending
    // call is still outstanding when the connection drops.
    let server = tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |_context, _request| {
                std::future::pending::<()>().await;
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let call = client.call(Request::Echo(1));
    server.abort();
    assert!(matches!(
        call.await,
        Err(Error::Io(_)) | Err(Error::ConnectionClosed)
    ));
}

#[tokio::test]
async fn close_stops_tasks_and_fails_pending_calls() {
    let (client_io, peer_io) = tokio::io::duplex(64);
    // Kept running (not aborted) for the whole test: this test exercises
    // `Client::close`, not peer disconnection.
    let _server = tokio::spawn(async move {
        builder()
            .server(peer_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |_context, _request| {
                std::future::pending::<()>().await;
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let call = client.call(Request::Echo(1));
    client.close().await;
    assert!(matches!(call.await, Err(Error::ConnectionClosed)));
}

#[tokio::test]
async fn server_shutdown_drains_outstanding_requests() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let delay_started = Arc::new(tokio::sync::Notify::new());
    let server_delay_started = delay_started.clone();
    let server = tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |mut context, request| {
                let response = match request {
                    Request::Echo(value) => Response(value),
                    Request::Delay(ms) => {
                        server_delay_started.notify_one();
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        Response(ms as u32)
                    }
                    Request::Shutdown => {
                        context.shutdown();
                        Response(99)
                    }
                    Request::Bulk(_) | Request::TrailerRoundTrip(_) => unreachable!(),
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let slow = client.call(Request::Delay(20));
    delay_started.notified().await;
    let shutdown = client.call(Request::Shutdown);
    assert_eq!(shutdown.await.unwrap().into_response(), Response(99));
    assert_eq!(slow.await.unwrap().into_response(), Response(20));
    assert!(server.await.unwrap().is_ok());
    client.close().await;
}

#[tokio::test]
async fn interleaves_large_and_small_messages_round_robin() {
    let make = || builder().max_fragment_size(256);
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::Echo(value) => Response(value),
                    Request::Bulk(data) => Response(data.len() as u32),
                    Request::Delay(_) | Request::Shutdown | Request::TrailerRoundTrip(_) => {
                        unreachable!()
                    }
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let bulk = client.call(Request::Bulk(vec![b'x'; 64 * 1024]));
    let echo = client.call(Request::Echo(7));
    assert_eq!(echo.await.unwrap().into_response(), Response(7));
    assert_eq!(bulk.await.unwrap().into_response(), Response(64 * 1024));
}

#[tokio::test]
async fn bounded_concurrency_limits_simultaneous_large_transfers() {
    let make = || builder().max_fragment_size(256).max_incomplete_messages(2);
    let (client_io, server_io) = tokio::io::duplex(8192);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::Echo(value) => Response(value),
                    Request::Bulk(data) => Response(data.len() as u32),
                    Request::Delay(_) | Request::Shutdown | Request::TrailerRoundTrip(_) => {
                        unreachable!()
                    }
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    // More concurrent large transfers than `max_incomplete_messages`, so at
    // least one must sit in the scheduler's `waiting` queue.
    let bulk_a = client.call(Request::Bulk(vec![b'a'; 16 * 1024]));
    let bulk_b = client.call(Request::Bulk(vec![b'b'; 16 * 1024]));
    let bulk_c = client.call(Request::Bulk(vec![b'c'; 16 * 1024]));
    let echo = client.call(Request::Echo(7));
    assert_eq!(echo.await.unwrap().into_response(), Response(7));
    assert_eq!(bulk_a.await.unwrap().into_response(), Response(16 * 1024));
    assert_eq!(bulk_b.await.unwrap().into_response(), Response(16 * 1024));
    assert_eq!(bulk_c.await.unwrap().into_response(), Response(16 * 1024));
}

#[tokio::test]
async fn cancel_during_fragment_transmission_completes_without_hanging() {
    let dispatched = Arc::new(AtomicBool::new(false));
    let server_dispatched = dispatched.clone();
    let make = || builder().max_fragment_size(32);
    // A tiny duplex buffer forces many small read/write handoffs between
    // the client writer and server reader tasks, spreading a large
    // transfer out over many scheduling points so cancellation reliably
    // lands mid-transmission rather than before or after it entirely.
    let (client_io, server_io) = tokio::io::duplex(64);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |context, request| {
                server_dispatched.store(true, Ordering::Release);
                let response = match request {
                    Request::Bulk(data) => Response(data.len() as u32),
                    _ => unreachable!(),
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let mut call = client.call(Request::Bulk(vec![b'x'; 256 * 1024]));
    tokio::time::sleep(Duration::from_micros(200)).await;
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
}

#[tokio::test]
async fn resource_limits_enforced_end_to_end() {
    let make = || builder().max_payload_size(16);
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::Bulk(data) => Response(data.len() as u32),
                    _ => unreachable!(),
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let call = client.call(Request::Bulk(vec![b'x'; 1024]));
    assert!(matches!(
        call.await,
        Err(Error::Protocol(_)) | Err(Error::ConnectionClosed) | Err(Error::Io(_))
    ));
}

async fn trailer_echo_handler(mut context: CallContext<Test>, request: Request) {
    match request {
        Request::TrailerRoundTrip(value) => {
            let mut data = None;
            if let Some(trailer) = context.request_trailer() {
                let mut bytes = Vec::new();
                trailer.read_to_end(&mut bytes).await.unwrap();
                data = Some(bytes);
            }
            if let Some(data) = data {
                let mut trailer = context.respond_with_trailer(Response(value));
                trailer.write_all(&data).await.unwrap();
                trailer.finish();
            } else {
                context.respond(Response(value));
            }
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn request_and_response_trailers_round_trip_absent_empty_single_and_multi_fragment() {
    let make = || builder().max_fragment_size(8);
    let (client_io, server_io) = tokio::io::duplex(65536);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(trailer_echo_handler)
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;

    // Absent: an ordinary call sends and receives no trailer at all.
    let (response, trailer) = client
        .call(Request::TrailerRoundTrip(1))
        .await
        .unwrap()
        .into_response_trailer();
    assert_eq!(response, Response(1));
    assert!(trailer.is_none());

    // Present but empty, distinguishable from absent.
    let send = client.call_with_trailer(Request::TrailerRoundTrip(2));
    let (response, mut trailer) = send.finish().await.unwrap().into_response_trailer();
    assert_eq!(response, Response(2));
    let mut received = Vec::new();
    trailer
        .as_mut()
        .unwrap()
        .read_to_end(&mut received)
        .await
        .unwrap();
    assert!(received.is_empty());

    // Single-fragment: fits within max_fragment_size.
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(3));
    send.write_all(b"abcd").await.unwrap();
    let (response, mut trailer) = send.finish().await.unwrap().into_response_trailer();
    assert_eq!(response, Response(3));
    let mut received = Vec::new();
    trailer
        .as_mut()
        .unwrap()
        .read_to_end(&mut received)
        .await
        .unwrap();
    assert_eq!(received, b"abcd");

    // Multi-fragment: exceeds max_fragment_size, both directions.
    let big = vec![b'x'; 100];
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(4));
    send.write_all(&big).await.unwrap();
    let (response, mut trailer) = send.finish().await.unwrap().into_response_trailer();
    assert_eq!(response, Response(4));
    let mut received = Vec::new();
    trailer
        .as_mut()
        .unwrap()
        .read_to_end(&mut received)
        .await
        .unwrap();
    assert_eq!(received, big);
}

#[tokio::test]
async fn short_transport_write_stages_and_flushes_the_fragment_suffix() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = tokio::io::split(client_io);
    tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(trailer_echo_handler)
            .await
    });
    let client = unbound_client_split::<_, _, Test>(
        client_reader,
        ShortWriter {
            inner: client_writer,
            // The wire header fits, then only this prefix of the payload
            // fits in the direct transport write.
            max_write: 16,
        },
    )
    .await;

    let data = (0..100).map(|value| value as u8).collect::<Vec<_>>();
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(9));
    send.write_all(&data).await.unwrap();
    let (response, mut trailer) = send.finish().await.unwrap().into_response_trailer();
    assert_eq!(response, Response(9));
    let mut received = Vec::new();
    trailer
        .as_mut()
        .unwrap()
        .read_to_end(&mut received)
        .await
        .unwrap();
    assert_eq!(received, data);
}

#[cfg(unix)]
#[tokio::test]
async fn request_trailer_round_trips_over_unix_transport() {
    let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    tokio::spawn(async move {
        builder()
            .server_unix(server_stream)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(trailer_echo_handler)
            .await
    });
    let client = unbound_client_unix::<Test>(client_stream).await;
    let data = vec![b'x'; 4096];
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    send.write_all(&data).await.unwrap();
    let (response, mut trailer) = send.finish().await.unwrap().into_response_trailer();
    assert_eq!(response, Response(1));
    let mut received = Vec::new();
    trailer
        .as_mut()
        .unwrap()
        .read_to_end(&mut received)
        .await
        .unwrap();
    assert_eq!(received, data);
}

#[tokio::test]
async fn trailer_call_cancelled_mid_transmission_completes_without_hanging() {
    let make = || builder().max_fragment_size(32);
    // A tiny duplex buffer forces many small read/write handoffs, spreading
    // the trailer transfer out over many scheduling points so cancellation
    // spreads trailer transmission across scheduling points.
    let (client_io, server_io) = tokio::io::duplex(64);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::TrailerRoundTrip(value) => Response(value),
                    Request::Echo(value) => Response(value),
                    _ => unreachable!(),
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    send.write_all(&[b'x'; 17]).await.unwrap();
    drop(send);
    // Dropping a producer after committing a fragment finishes its staged
    // bytes and aborts that call without poisoning the following message.
    assert_eq!(
        client.call(Request::Echo(7)).await.unwrap().into_response(),
        Response(7)
    );
}

#[tokio::test]
async fn trailer_call_cancelled_after_full_transmission_falls_back_to_ordinary_cancel() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        builder()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |mut context, _request| {
                let mut body = Vec::new();
                context
                    .request_trailer()
                    .unwrap()
                    .read_to_end(&mut body)
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_secs(10)).await;
                context.respond(Response(0));
            })
            .await
    });
    let client = unbound_client::<_, Test>(client_io).await;
    let data = b"a small trailer that finishes sending almost immediately";
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    send.write_all(data).await.unwrap();
    let mut call = send.finish();
    tokio::time::sleep(Duration::from_millis(10)).await;
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
}

#[tokio::test]
async fn trailer_resource_limits_enforced_end_to_end() {
    let make = || builder().max_trailer_size(16);
    let (client_io, server_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async |context, request| {
                let response = match request {
                    Request::TrailerRoundTrip(value) => Response(value),
                    _ => unreachable!(),
                };
                context.respond(response);
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let data = vec![b'x'; 1024];
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    let result = send.write_all(&data).await;
    if result.is_ok() {
        assert!(matches!(
            send.finish().await,
            Err(Error::Protocol(_)) | Err(Error::ConnectionClosed) | Err(Error::Io(_))
        ));
    }
}

#[tokio::test]
async fn server_discarding_a_request_trailer_errors_the_writer_but_response_still_completes() {
    let make = || builder().max_fragment_size(8);
    let (client_io, server_io) = tokio::io::duplex(64);
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |mut context, request| {
                let value = match request {
                    Request::TrailerRoundTrip(value) => value,
                    _ => unreachable!(),
                };
                let mut prefix = [0u8; 4];
                context
                    .request_trailer()
                    .unwrap()
                    .read_exact(&mut prefix)
                    .await
                    .unwrap();
                // Simulate hitting an error partway through consuming the
                // request trailer (e.g. a failed file write): stop wanting
                // more of it, but still answer normally through the ordinary
                // response.
                context.request_trailer().unwrap().discard();
                context.respond(Response(value));
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    let big = vec![b'x'; 10_000];
    let error = send.write_all(&big).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let response = send.finish().await.unwrap().into_response();
    assert_eq!(response, Response(1));
}

#[tokio::test]
async fn client_discarding_a_response_trailer_errors_the_servers_writer() {
    let make = || builder().max_fragment_size(8);
    let (client_io, server_io) = tokio::io::duplex(64);
    let write_error = Arc::new(Mutex::new(None));
    let server_write_error = write_error.clone();
    tokio::spawn(async move {
        make()
            .server(server_io)
            .await
            .unwrap()
            .bind::<Test>()
            .serve(async move |context, request| {
                let value = match request {
                    Request::TrailerRoundTrip(value) => value,
                    _ => unreachable!(),
                };
                let mut trailer = context.respond_with_trailer(Response(value));
                let big = vec![b'x'; 10_000];
                let result = trailer.write_all(&big).await;
                *server_write_error.lock().unwrap() = result.err();
            })
            .await
    });
    let client = unbound_client_with_builder::<_, Test>(make(), client_io).await;
    let (response, trailer) = client
        .call(Request::TrailerRoundTrip(1))
        .await
        .unwrap()
        .into_response_trailer();
    assert_eq!(response, Response(1));
    let mut trailer = trailer.unwrap();
    let mut prefix = [0u8; 4];
    trailer.read_exact(&mut prefix).await.unwrap();
    // Stop wanting the rest of a still-streaming response trailer.
    trailer.discard();
    drop(trailer);
    // Give the server's writer time to observe the discard and fail.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        write_error.lock().unwrap().as_ref().map(|e| e.kind()),
        Some(io::ErrorKind::BrokenPipe)
    );
}

#[cfg(unix)]
mod unix_handles {
    use std::io::Read;

    use dolang_rpc::handle::OsHandle;
    use nix::unistd::{pipe, write};
    use serde::{Deserialize, Serialize};

    use super::*;

    struct HandlesProtocol;
    impl Protocol for HandlesProtocol {
        type Request = HandleRequest;
        type Response = HandleResponse;
    }

    #[derive(Serialize, Deserialize)]
    struct HandleRequest {
        handles: Vec<OsHandle>,
    }

    #[derive(Serialize, Deserialize)]
    struct HandleResponse {
        handles: Vec<OsHandle>,
    }

    #[tokio::test]
    async fn transfers_handles_in_requests_and_responses() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            builder()
                .server_unix(server_stream)
                .await
                .unwrap()
                .bind::<HandlesProtocol>()
                .serve(async |context, request| {
                    context.respond(HandleResponse {
                        handles: request.handles,
                    });
                })
                .await
        });
        let client = unbound_client_unix::<HandlesProtocol>(client_stream).await;
        let (read_fd, write_fd) = pipe().unwrap();
        let call = client.call(HandleRequest {
            handles: vec![OsHandle::new(read_fd)],
        });
        let response = call.await.unwrap().into_response();
        let received = response.handles.into_iter().next().unwrap().into_inner();
        write(&write_fd, b"ok").unwrap();
        let mut file = std::fs::File::from(received);
        let mut bytes = [0; 2];
        file.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ok");
    }

    #[tokio::test]
    async fn attachments_can_be_combined_with_a_trailer() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            builder()
                .server_unix(server_stream)
                .await
                .unwrap()
                .bind::<HandlesProtocol>()
                .serve(async |context, request| {
                    context.respond(HandleResponse {
                        handles: request.handles,
                    });
                })
                .await
        });
        let client = unbound_client_unix::<HandlesProtocol>(client_stream).await;
        let (read_fd, write_fd) = pipe().unwrap();
        let mut send = client.call_with_trailer(HandleRequest {
            handles: vec![OsHandle::new(read_fd)],
        });
        send.write_all(b"trailer").await.unwrap();
        let call = send.finish();
        let response = call.await.unwrap().into_response();
        let received = response.handles.into_iter().next().unwrap().into_inner();
        write(&write_fd, b"ok").unwrap();
        let mut file = std::fs::File::from(received);
        let mut bytes = [0; 2];
        file.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ok");
    }

    #[tokio::test]
    async fn transfers_handles_across_multiple_attachment_fragments() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            builder()
                .server_unix(server_stream)
                .await
                .unwrap()
                .bind::<HandlesProtocol>()
                .serve(async |context, request| {
                    context.respond(HandleResponse {
                        handles: request.handles,
                    });
                })
                .await
        });
        let client = unbound_client_unix::<HandlesProtocol>(client_stream).await;
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        for _ in 0..65 {
            let (read, write) = pipe().unwrap();
            reads.push(OsHandle::new(read));
            writes.push(write);
        }
        let response = client
            .call(HandleRequest { handles: reads })
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.handles.len(), 65);
        for (index, (handle, write_fd)) in response
            .handles
            .into_iter()
            .zip(writes.into_iter())
            .enumerate()
        {
            write(&write_fd, &[index as u8]).unwrap();
            let mut file = std::fs::File::from(handle.into_inner());
            let mut byte = [0];
            file.read_exact(&mut byte).unwrap();
            assert_eq!(byte[0], index as u8);
        }
    }

    #[tokio::test]
    async fn negotiates_handle_fragment_and_message_limits() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        tokio::spawn(async move {
            builder()
                .max_handles_per_fragment(1)
                .max_handles_per_message(2)
                .server_unix(server_stream)
                .await
                .unwrap()
                .bind::<HandlesProtocol>()
                .serve(async |context, request| {
                    context.respond(HandleResponse {
                        handles: request.handles,
                    });
                })
                .await
        });
        let client = unbound_client_unix::<HandlesProtocol>(client_stream).await;
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        for _ in 0..2 {
            let (read, write) = pipe().unwrap();
            reads.push(OsHandle::new(read));
            writes.push(write);
        }
        let response = client
            .call(HandleRequest { handles: reads })
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.handles.len(), 2);

        let mut excess = Vec::new();
        for _ in 0..3 {
            let (read, _write) = pipe().unwrap();
            excess.push(OsHandle::new(read));
        }
        assert!(matches!(
            client.call(HandleRequest { handles: excess }).await,
            Err(Error::Serialize(_))
        ));
    }
}

#[cfg(windows)]
mod windows_handles {
    use std::{
        io::Read,
        os::windows::io::{AsHandle, FromRawHandle, OwnedHandle},
        sync::atomic::{AtomicU64, Ordering},
    };

    use dolang_rpc::handle::OsHandle;
    use serde::{Deserialize, Serialize};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    use super::*;

    static NEXT_PIPE: AtomicU64 = AtomicU64::new(0);

    struct HandlesProtocol;
    impl Protocol for HandlesProtocol {
        type Request = HandleRequest;
        type Response = HandleResponse;
    }

    #[derive(Serialize, Deserialize)]
    struct HandleRequest {
        handle: OsHandle,
    }

    #[derive(Serialize, Deserialize)]
    struct HandleResponse {
        handle: OsHandle,
    }

    async fn pipe_pair() -> (NamedPipeServer, NamedPipeClient) {
        let id = NEXT_PIPE.fetch_add(1, Ordering::Relaxed);
        let name = format!(r"\\.\pipe\dolang-rpc-{}-{id}", std::process::id());
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let client = ClientOptions::new().open(&name).unwrap();
        server.connect().await.unwrap();
        (server, client)
    }

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

    #[tokio::test]
    async fn transfers_handles_in_requests_and_responses() {
        // Use the pipe-server end for the less-privileged RPC client, matching
        // the parent/helper deployment that motivates this transport.
        let (client_pipe, server_pipe) = pipe_pair().await;
        // `UnboundServer`/`UnboundClient` construction both perform a real
        // handshake, so they must run concurrently (one spawned, one
        // awaited directly) rather than sequentially.
        let server = tokio::spawn(async move {
            builder()
                .server_named_pipe_client(server_pipe)
                .await
                .unwrap()
                .bind::<HandlesProtocol>()
                .serve(async |context, request| {
                    context.respond(HandleResponse {
                        handle: request.handle,
                    });
                })
                .await
        });
        // SAFETY: this test owns and controls the connected server endpoint.
        let client =
            unsafe { builder().client_named_pipe_server(client_pipe, current_process_handle()) }
                .await
                .unwrap()
                .bind::<HandlesProtocol>();

        let file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
        let _ = file.as_handle();
        let response = client
            .call(HandleRequest {
                handle: OsHandle::new(OwnedHandle::from(file)),
            })
            .await;
        let response = match response {
            Ok(response) => response.into_response(),
            Err(error) => panic!(
                "client failed with {error}; server returned {:?}",
                server.await
            ),
        };
        let mut received = std::fs::File::from(response.handle.into_inner());
        let mut byte = [0];
        received.read_exact(&mut byte).unwrap();
    }
}
