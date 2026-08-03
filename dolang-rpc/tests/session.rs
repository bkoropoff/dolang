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

use dolang_rpc::{Client, Error, Limits, Protocol, Server};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

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
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Echo(value) => Response(value),
            Request::Delay(ms) => {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                Response(ms as u32)
            }
            Request::Shutdown | Request::Bulk(_) | Request::TrailerRoundTrip(_) => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::new(client_io);
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
    let server = Server::<Test>::new_split(server_reader, server_writer);
    let server = tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Echo(value) => Response(value),
            Request::Shutdown => {
                context.shutdown();
                Response(0)
            }
            Request::Delay(_) | Request::Bulk(_) | Request::TrailerRoundTrip(_) => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::new_split(client_reader, client_writer);
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
async fn split_transport_flushes_buffered_writers() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let (client_reader, client_writer) = tokio::io::split(client_io);
    let (server_reader, server_writer) = tokio::io::split(server_io);
    let server = Server::<Test>::new_split(server_reader, tokio::io::BufWriter::new(server_writer));
    let server = tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Echo(value) => Response(value),
            Request::Shutdown => {
                context.shutdown();
                Response(0)
            }
            Request::Delay(_) | Request::Bulk(_) | Request::TrailerRoundTrip(_) => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::new_split(client_reader, tokio::io::BufWriter::new(client_writer));
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
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async move |context, _| {
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
    }));
    let client = Client::<Test>::new(client_io);
    let mut call = client.call(Request::Delay(10_000));
    tokio::time::sleep(Duration::from_millis(10)).await;
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test]
async fn guarded_cancellation_returns_normal_response() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async |mut context, _| {
        let cancelled = context
            .cancel_guard(async |_| tokio::time::sleep(Duration::from_secs(10)).await)
            .await
            .is_err();
        context.respond(Response(u32::from(cancelled)));
    }));
    let client = Client::<Test>::new(client_io);
    let mut call = client.call(Request::Delay(10_000));
    tokio::time::sleep(Duration::from_millis(10)).await;
    call.cancel();
    assert_eq!(call.await.unwrap().into_response(), Response(1));
}

#[tokio::test]
async fn disconnect_fails_pending_calls() {
    let (client_io, server_io) = tokio::io::duplex(64);
    let client = Client::<Test>::new(client_io);
    let call = client.call(Request::Echo(1));
    drop(server_io);
    assert!(matches!(
        call.await,
        Err(Error::Io(_)) | Err(Error::ConnectionClosed)
    ));
}

#[tokio::test]
async fn close_stops_tasks_and_fails_pending_calls() {
    let (client_io, _peer_io) = tokio::io::duplex(64);
    let client = Client::<Test>::new(client_io);
    let call = client.call(Request::Echo(1));
    client.close().await;
    assert!(matches!(call.await, Err(Error::ConnectionClosed)));
}

#[tokio::test]
async fn server_shutdown_drains_outstanding_requests() {
    let (client_io, server_io) = tokio::io::duplex(4096);
    let delay_started = Arc::new(tokio::sync::Notify::new());
    let server_delay_started = delay_started.clone();
    let server = Server::<Test>::new(server_io);
    let server = tokio::spawn(server.serve(async move |context, request| {
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
    }));
    let client = Client::<Test>::new(client_io);
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
    let limits = Limits {
        max_fragment_size: 256,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Echo(value) => Response(value),
            Request::Bulk(data) => Response(data.len() as u32),
            Request::Delay(_) | Request::Shutdown | Request::TrailerRoundTrip(_) => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
    let bulk = client.call(Request::Bulk(vec![b'x'; 64 * 1024]));
    let echo = client.call(Request::Echo(7));
    assert_eq!(echo.await.unwrap().into_response(), Response(7));
    assert_eq!(bulk.await.unwrap().into_response(), Response(64 * 1024));
}

#[tokio::test]
async fn bounded_concurrency_limits_simultaneous_large_transfers() {
    let limits = Limits {
        max_fragment_size: 256,
        max_incomplete_messages: 2,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(8192);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Echo(value) => Response(value),
            Request::Bulk(data) => Response(data.len() as u32),
            Request::Delay(_) | Request::Shutdown | Request::TrailerRoundTrip(_) => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
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
async fn cancel_before_first_fragment_sent() {
    let dispatched = Arc::new(AtomicBool::new(false));
    let server_dispatched = dispatched.clone();
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async move |context, request| {
        server_dispatched.store(true, Ordering::Release);
        let response = match request {
            Request::Bulk(data) => Response(data.len() as u32),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::new(client_io);
    // `call` and `cancel` both enqueue onto the same channel without any
    // intervening `.await`, so on the single-threaded test runtime the
    // writer task cannot have run yet: it will see the request already
    // cancelled before ever admitting it into the scheduler.
    let mut call = client.call(Request::Bulk(vec![b'x'; 64 * 1024]));
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!dispatched.load(Ordering::Acquire));
}

#[tokio::test]
async fn cancel_during_fragment_transmission_completes_without_hanging() {
    let dispatched = Arc::new(AtomicBool::new(false));
    let server_dispatched = dispatched.clone();
    let limits = Limits {
        max_fragment_size: 32,
        ..Limits::default()
    };
    // A tiny duplex buffer forces many small read/write handoffs between
    // the client writer and server reader tasks, spreading a large
    // transfer out over many scheduling points so cancellation reliably
    // lands mid-transmission rather than before or after it entirely.
    let (client_io, server_io) = tokio::io::duplex(64);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async move |context, request| {
        server_dispatched.store(true, Ordering::Release);
        let response = match request {
            Request::Bulk(data) => Response(data.len() as u32),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
    let mut call = client.call(Request::Bulk(vec![b'x'; 256 * 1024]));
    tokio::time::sleep(Duration::from_micros(200)).await;
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
}

#[tokio::test]
async fn resource_limits_enforced_end_to_end() {
    let limits = Limits {
        max_payload_size: 16,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::Bulk(data) => Response(data.len() as u32),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
    let call = client.call(Request::Bulk(vec![b'x'; 1024]));
    assert!(matches!(
        call.await,
        Err(Error::Protocol(_)) | Err(Error::ConnectionClosed) | Err(Error::Io(_))
    ));
}

async fn trailer_echo_handler(mut context: dolang_rpc::CallContext<Test>, request: Request) {
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
    let limits = Limits {
        max_fragment_size: 8,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(65536);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(trailer_echo_handler));
    let client = Client::<Test>::with_limits(client_io, limits);

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
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(trailer_echo_handler));
    let client = Client::<Test>::new_split(
        client_reader,
        ShortWriter {
            inner: client_writer,
            // The wire header fits, then only this prefix of the payload
            // fits in the direct transport write.
            max_write: 16,
        },
    );

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
    let server = Server::<Test>::from_unix_stream(server_stream).unwrap();
    tokio::spawn(server.serve(trailer_echo_handler));
    let client = Client::<Test>::from_unix_stream(client_stream).unwrap();
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
async fn trailer_call_cancelled_before_any_fragment_sent() {
    let dispatched = Arc::new(AtomicBool::new(false));
    let server_dispatched = dispatched.clone();
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async move |context, request| {
        server_dispatched.store(true, Ordering::Release);
        let response = match request {
            Request::TrailerRoundTrip(value) => Response(value),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::new(client_io);
    // As in `cancel_before_first_fragment_sent`: no `.await` between `call`
    // and `cancel`, so on the single-threaded test runtime the writer task
    // cannot have run yet.
    let mut call = client
        .call_with_trailer(Request::TrailerRoundTrip(1))
        .finish();
    call.cancel();
    assert!(matches!(call.await, Err(Error::Cancelled)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!dispatched.load(Ordering::Acquire));
}

#[tokio::test]
async fn trailer_call_cancelled_mid_transmission_completes_without_hanging() {
    let limits = Limits {
        max_fragment_size: 32,
        ..Limits::default()
    };
    // A tiny duplex buffer forces many small read/write handoffs, spreading
    // the trailer transfer out over many scheduling points so cancellation
    // spreads trailer transmission across scheduling points.
    let (client_io, server_io) = tokio::io::duplex(64);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::TrailerRoundTrip(value) => Response(value),
            Request::Echo(value) => Response(value),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
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
    let server = Server::<Test>::new(server_io);
    tokio::spawn(server.serve(async move |mut context, _request| {
        let mut body = Vec::new();
        context
            .request_trailer()
            .unwrap()
            .read_to_end(&mut body)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(10)).await;
        context.respond(Response(0));
    }));
    let client = Client::<Test>::new(client_io);
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
    let limits = Limits {
        max_trailer_size: 16,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(4096);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async |context, request| {
        let response = match request {
            Request::TrailerRoundTrip(value) => Response(value),
            _ => unreachable!(),
        };
        context.respond(response);
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
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
    let limits = Limits {
        max_fragment_size: 8,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(64);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    tokio::spawn(server.serve(async move |mut context, request| {
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
        // Simulate hitting an error partway through consuming the request
        // trailer (e.g. a failed file write): stop wanting more of it, but
        // still answer normally through the ordinary response.
        context.request_trailer().unwrap().discard();
        context.respond(Response(value));
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
    let mut send = client.call_with_trailer(Request::TrailerRoundTrip(1));
    let big = vec![b'x'; 10_000];
    let error = send.write_all(&big).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    let response = send.finish().await.unwrap().into_response();
    assert_eq!(response, Response(1));
}

#[tokio::test]
async fn client_discarding_a_response_trailer_errors_the_servers_writer() {
    let limits = Limits {
        max_fragment_size: 8,
        ..Limits::default()
    };
    let (client_io, server_io) = tokio::io::duplex(64);
    let server = Server::<Test>::new(server_io).with_limits(limits);
    let write_error = Arc::new(Mutex::new(None));
    let server_write_error = write_error.clone();
    tokio::spawn(server.serve(async move |context, request| {
        let value = match request {
            Request::TrailerRoundTrip(value) => value,
            _ => unreachable!(),
        };
        let mut trailer = context.respond_with_trailer(Response(value));
        let big = vec![b'x'; 10_000];
        let result = trailer.write_all(&big).await;
        *server_write_error.lock().unwrap() = result.err();
    }));
    let client = Client::<Test>::with_limits(client_io, limits);
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
    use std::{io::Read, os::fd::AsFd};

    use dolang_rpc::OsHandle;
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
        handle: Option<OsHandle>,
    }

    #[tokio::test]
    async fn transfers_handles_in_requests_and_responses() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = Server::<HandlesProtocol>::from_unix_stream(server_stream).unwrap();
        tokio::spawn(server.serve(async |context, mut request| {
            context.respond(HandleResponse {
                handle: request.handles.pop(),
            });
        }));
        let client = Client::<HandlesProtocol>::from_unix_stream(client_stream).unwrap();
        let (read_fd, write_fd) = pipe().unwrap();
        let call = client.call(HandleRequest {
            handles: vec![OsHandle::new(read_fd)],
        });
        let response = call.await.unwrap().into_response();
        let received = response.handle.unwrap().into_inner();
        write(&write_fd, b"ok").unwrap();
        let mut file = std::fs::File::from(received);
        let mut bytes = [0; 2];
        file.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ok");
    }

    #[test]
    fn os_handle_keeps_its_descriptor_borrowable() {
        let (fd, _) = pipe().unwrap();
        let handle = OsHandle::new(fd);
        let _ = handle.as_inner().as_fd();
    }

    #[tokio::test]
    async fn attachments_with_trailer_fails_as_a_session_error() {
        let (client_stream, server_stream) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = Server::<HandlesProtocol>::from_unix_stream(server_stream).unwrap();
        tokio::spawn(server.serve(async |context, mut request| {
            context.respond(HandleResponse {
                handle: request.handles.pop(),
            });
        }));
        let client = Client::<HandlesProtocol>::from_unix_stream(client_stream).unwrap();
        let (read_fd, _write_fd) = pipe().unwrap();
        let send = client.call_with_trailer(HandleRequest {
            handles: vec![OsHandle::new(read_fd)],
        });
        let call = send.finish();
        assert!(matches!(
            call.await,
            Err(Error::Protocol(_)) | Err(Error::ConnectionClosed) | Err(Error::Io(_))
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

    use dolang_rpc::OsHandle;
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
        let server = Server::<HandlesProtocol>::from_named_pipe_client(server_pipe).unwrap();
        let server = tokio::spawn(server.serve(async |context, request| {
            context.respond(HandleResponse {
                handle: request.handle,
            });
        }));
        // SAFETY: this test owns and controls the connected server endpoint.
        let client = unsafe {
            Client::<HandlesProtocol>::from_named_pipe_server(client_pipe, current_process_handle())
                .unwrap()
        };

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
