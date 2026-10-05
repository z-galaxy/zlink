//! What receiving calls costs a server: reading and deserializing calls and replying to each,
//! from a client that sends each call once the reply to the one before it is in, and from one that
//! sends all of them at once ahead of their replies. The client is a task of a single-threaded
//! runtime and the server the future it blocks on, talking through in-memory channels, so these
//! ids wait on no timer, socket or other thread; the client's share of the work runs on the same
//! thread and is measured along with the server's. What pipelining saves a client, which waits on
//! a timer, is in `benchmarks.rs`.

use common::{BiPipeSocket, NUM_CALLS, TestMethod};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};
use std::{hint::black_box, time::Duration};
use tokio::runtime::Builder;
use zlink_core::{Call, Reply, connection::Connection};

mod common;

criterion_group!(benches, server_receiving);
criterion_main!(benches);

// Server-side benchmarks: Sequential vs Batched receiving
fn server_receiving(c: &mut Criterion) {
    let rt = Builder::new_current_thread().build().unwrap();
    let mut group = c.benchmark_group("server_receiving");
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(NUM_CALLS as u64));

    group.bench_function("sequential", |b| {
        b.to_async(&rt).iter_batched(
            || {
                // Setup: Create sockets and spawn client.
                let (client_socket, server_socket) = BiPipeSocket::new_pair();

                // Spawn client that sends calls one by one.
                tokio::spawn(async move {
                    let mut client_conn = Connection::new(client_socket);
                    for i in 0..NUM_CALLS {
                        let call = Call::new(TestMethod::Compute {
                            values: vec![i as u32; 10],
                        });
                        #[cfg(feature = "std")]
                        client_conn.send_call(&call, vec![]).await.unwrap();
                        #[cfg(not(feature = "std"))]
                        client_conn.send_call(&call).await.unwrap();

                        // Wait for reply before sending next (sequential pattern).
                        #[derive(Debug, Deserialize)]
                        struct DummyError;
                        #[cfg(feature = "std")]
                        let (_reply, _fds): (
                            zlink_core::reply::Result<ComputeReply, DummyError>,
                            _,
                        ) = client_conn.receive_reply().await.unwrap();
                        #[cfg(not(feature = "std"))]
                        let _reply: zlink_core::reply::Result<
                            ComputeReply,
                            DummyError,
                        > = client_conn.receive_reply().await.unwrap();
                    }
                });

                Connection::new(server_socket)
            },
            |mut server_conn| async move {
                // Benchmark: Server receives calls one at a time.
                for _ in 0..NUM_CALLS {
                    // Measure the time to receive and deserialize each call.
                    #[cfg(feature = "std")]
                    let (call, _fds): (Call<TestMethod>, _) =
                        server_conn.read_mut().receive_call().await.unwrap();
                    #[cfg(not(feature = "std"))]
                    let call: Call<TestMethod> =
                        server_conn.read_mut().receive_call().await.unwrap();
                    black_box(call);

                    // Send reply so client can continue.
                    let reply = Reply::new(Some(ComputeReply { result: 42 }));
                    #[cfg(feature = "std")]
                    server_conn
                        .write_mut()
                        .send_reply(&reply, vec![])
                        .await
                        .unwrap();
                    #[cfg(not(feature = "std"))]
                    server_conn.write_mut().send_reply(&reply).await.unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("batched", |b| {
        b.to_async(&rt).iter_batched(
            || {
                // Setup: Create sockets and spawn client.
                let (client_socket, server_socket) = BiPipeSocket::new_pair();

                // Spawn client that pipelines all calls at once.
                tokio::spawn(async move {
                    let mut client_conn = Connection::new(client_socket);

                    // Send all calls in a batch using pipelining.
                    for i in 0..NUM_CALLS {
                        let call = Call::new(TestMethod::Compute {
                            values: vec![i as u32; 10],
                        });
                        #[cfg(feature = "std")]
                        client_conn.write_mut().enqueue_call(&call, vec![]).unwrap();
                        #[cfg(not(feature = "std"))]
                        client_conn.write_mut().enqueue_call(&call).unwrap();
                    }
                    // Flush all at once.
                    client_conn.write_mut().flush().await.unwrap();

                    // Collect replies.
                    for _ in 0..NUM_CALLS {
                        #[derive(Debug, Deserialize)]
                        struct DummyError;
                        #[cfg(feature = "std")]
                        let (_reply, _fds): (
                            zlink_core::reply::Result<ComputeReply, DummyError>,
                            _,
                        ) = client_conn.receive_reply().await.unwrap();
                        #[cfg(not(feature = "std"))]
                        let _reply: zlink_core::reply::Result<
                            ComputeReply,
                            DummyError,
                        > = client_conn.receive_reply().await.unwrap();
                    }
                });

                Connection::new(server_socket)
            },
            |mut server_conn| async move {
                // Benchmark: Server receives all calls from the batch.
                // This implicitly tests zero-byte detection as messages arrive together.
                let mut calls = Vec::new();

                // Receive all calls - they're already in the buffer.
                for _ in 0..NUM_CALLS {
                    #[cfg(feature = "std")]
                    let (call, _fds): (Call<TestMethod>, _) =
                        server_conn.read_mut().receive_call().await.unwrap();
                    #[cfg(not(feature = "std"))]
                    let call: Call<TestMethod> =
                        server_conn.read_mut().receive_call().await.unwrap();
                    calls.push(call);
                }
                black_box(&calls);

                // Send replies.
                for _ in 0..NUM_CALLS {
                    let reply = Reply::new(Some(ComputeReply { result: 42 }));
                    #[cfg(feature = "std")]
                    server_conn
                        .write_mut()
                        .send_reply(&reply, vec![])
                        .await
                        .unwrap();
                    #[cfg(not(feature = "std"))]
                    server_conn.write_mut().send_reply(&reply).await.unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComputeReply {
    result: u64,
}
