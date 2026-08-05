//! Comparison benchmark for `pipe_trailer`: the same native-pipe transport
//! and data sizes, but no `dolang-rpc` framing at all. The server reads a
//! ~16 byte dummy message from the client, then writes 2-4MiB back in 512KiB
//! chunks (matching `dolang-rpc`'s default fragment size); the client reads
//! it all back. Isolates raw pipe I/O throughput from RPC framing overhead.
//!
//! Unix and Windows only; a no-op stub keeps the bench target building
//! elsewhere. Run with `cargo bench -p dolang-rpc`.

mod support;

use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use support::{Recv, Send, native_pipe};

const DUMMY_LEN: usize = 16;
const CHUNK_SIZE: usize = 512 * 1024;

/// Wires up two native pipes (one per direction, like stdin/stdout) and
/// starts the server loop in the background. The returned sender/receiver
/// are used for repeated, back-to-back round trips.
async fn build_pipes(data: Arc<Vec<u8>>) -> (Send, Recv, tokio::task::JoinHandle<()>) {
    let (server_recv, client_send) = native_pipe();
    let (client_recv, server_send) = native_pipe();

    let server_task = tokio::spawn(async move {
        let mut server_recv = server_recv;
        let mut server_send = server_send;
        let mut dummy = [0u8; DUMMY_LEN];
        while server_recv.read_exact(&mut dummy).await.is_ok() {
            for chunk in data.chunks(CHUNK_SIZE) {
                if server_send.write_all(chunk).await.is_err() {
                    return;
                }
            }
        }
    });

    (client_send, client_recv, server_task)
}

async fn round_trip(client_send: &mut Send, client_recv: &mut Recv, data_len: usize) {
    client_send.write_all(&[0u8; DUMMY_LEN]).await.unwrap();
    let mut buf = vec![0u8; data_len];
    client_recv.read_exact(&mut buf).await.unwrap();
}

fn bench_raw_roundtrip(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut group = c.benchmark_group("pipe_raw_roundtrip");
    group.sample_size(10);
    for data_len in [2 * 1024 * 1024, 4 * 1024 * 1024] {
        group.throughput(Throughput::Bytes(data_len as u64));
        let (mut client_send, mut client_recv, _server) =
            rt.block_on(build_pipes(Arc::new(vec![0u8; data_len])));
        group.bench_with_input(
            BenchmarkId::from_parameter(data_len),
            &data_len,
            |b, &data_len| {
                // Plain (sync) `Bencher::iter`, driving the round trip via
                // `rt.block_on` inside the closure, rather than
                // `to_async(&rt).iter`: a sync `FnMut` closure can't soundly
                // return a future holding a reference derived from its own
                // per-call capture (the borrow checker can't rule out the
                // closure being called again, and thus the same `&mut`
                // handed out twice, while a previously returned future is
                // still alive). Driving the future to completion here avoids
                // that entirely — nothing borrowed escapes the closure call.
                b.iter(|| rt.block_on(round_trip(&mut client_send, &mut client_recv, data_len)));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_raw_roundtrip);
criterion_main!(benches);
