//! What pipelining saves a client: making calls one at a time, each sent once the reply to the one
//! before it is in, and all at once, ahead of their replies, to a server that sleeps before it
//! replies to a call, or to a batch of calls received together. The server is a task on another
//! thread of a multi-threaded runtime, and its sleep is on a timer, which only the clock can
//! measure. What receiving calls costs a server, which waits on no timer or other thread, is in
//! `server_receiving.rs`.

use common::{BiPipeSocket, NUM_CALLS, TestMethod};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use futures_util::{StreamExt, pin_mut};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{runtime::Runtime, time::sleep};
use zlink_core::{Call, Reply, connection::Connection};

mod common;

criterion_group!(benches, client_sending);
criterion_main!(benches);

// Client-side benchmarks: Sequential vs Pipelined sending
fn client_sending(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("client_sending");
    group.measurement_time(Duration::from_secs(10));
    group.throughput(Throughput::Elements(NUM_CALLS as u64));

    group.bench_function("sequential", |b| {
        b.to_async(&rt).iter_batched(
            || {
                // Setup: Create sockets and spawn server.
                let (client_socket, server_socket) = BiPipeSocket::new_pair();

                // Spawn a simple echo server with context switching simulation.
                tokio::spawn(async move {
                    let mut server_conn = Connection::new(server_socket);
                    for _ in 0..NUM_CALLS {
                        #[cfg(feature = "std")]
                        let (_call, _fds): (Call<TestMethod>, _) =
                            server_conn.read_mut().receive_call().await.unwrap();
                        #[cfg(not(feature = "std"))]
                        let _call: Call<TestMethod> =
                            server_conn.read_mut().receive_call().await.unwrap();

                        // Simulate context switch overhead (100 microseconds).
                        sleep(Duration::from_micros(100)).await;

                        let reply = Reply::new(Some(PingReply {
                            id: 1,
                            timestamp: 12345,
                        }));
                        #[cfg(feature = "std")]
                        server_conn
                            .write_mut()
                            .send_reply(&reply, vec![])
                            .await
                            .unwrap();
                        #[cfg(not(feature = "std"))]
                        server_conn.write_mut().send_reply(&reply).await.unwrap();
                    }
                });

                Connection::new(client_socket)
            },
            |mut conn| async move {
                // Benchmark: Make sequential calls - each waits for reply before sending next.
                for i in 0..NUM_CALLS {
                    let call = Call::new(TestMethod::Ping { id: i as u32 });
                    #[cfg(feature = "std")]
                    conn.send_call(&call, vec![]).await.unwrap();
                    #[cfg(not(feature = "std"))]
                    conn.send_call(&call).await.unwrap();

                    #[derive(Debug, Deserialize)]
                    struct DummyError;
                    #[cfg(feature = "std")]
                    let (_reply, _fds): (
                        zlink_core::reply::Result<PingReply, DummyError>,
                        _,
                    ) = conn.receive_reply().await.unwrap();
                    #[cfg(not(feature = "std"))]
                    let _reply: zlink_core::reply::Result<
                        PingReply,
                        DummyError,
                    > = conn.receive_reply().await.unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("pipelined", |b| {
        b.to_async(&rt).iter_batched(
            || {
                // Setup: Create sockets and spawn server.
                let (client_socket, server_socket) = BiPipeSocket::new_pair();

                // Spawn a server that processes batch with single context switch.
                tokio::spawn(async move {
                    let mut server_conn = Connection::new(server_socket);

                    // Server receives all calls at once.
                    for _ in 0..NUM_CALLS {
                        #[cfg(feature = "std")]
                        let (_call, _fds): (Call<TestMethod>, _) =
                            server_conn.read_mut().receive_call().await.unwrap();
                        #[cfg(not(feature = "std"))]
                        let _call: Call<TestMethod> =
                            server_conn.read_mut().receive_call().await.unwrap();
                    }

                    // Single context switch for batch processing.
                    sleep(Duration::from_micros(100)).await;

                    // Send all replies.
                    for _ in 0..NUM_CALLS {
                        let reply = Reply::new(Some(PingReply {
                            id: 1,
                            timestamp: 12345,
                        }));
                        #[cfg(feature = "std")]
                        server_conn
                            .write_mut()
                            .send_reply(&reply, vec![])
                            .await
                            .unwrap();
                        #[cfg(not(feature = "std"))]
                        server_conn.write_mut().send_reply(&reply).await.unwrap();
                    }
                });

                Connection::new(client_socket)
            },
            |mut conn| async move {
                // Benchmark: Build and send pipelined calls.
                let call = Call::new(TestMethod::Ping { id: 0 });
                #[derive(Debug, Deserialize)]
                struct DummyError;
                #[cfg(feature = "std")]
                let chain_result = conn.chain_call(&call, vec![]);
                #[cfg(not(feature = "std"))]
                let chain_result = conn.chain_call(&call);

                let mut chain = chain_result.unwrap();

                for i in 1..NUM_CALLS {
                    let call = Call::new(TestMethod::Ping { id: i as u32 });
                    #[cfg(feature = "std")]
                    let new_chain = chain.append(&call, vec![]);
                    #[cfg(not(feature = "std"))]
                    let new_chain = chain.append(&call);

                    chain = new_chain.unwrap();
                }

                // Send all at once and collect replies.
                let replies = chain.send::<PingReply, DummyError>().await.unwrap();
                pin_mut!(replies);

                let mut count = 0;
                while let Some(result) = replies.next().await {
                    #[cfg(feature = "std")]
                    let reply = {
                        let (r, _fds) = result.unwrap();
                        r
                    };
                    #[cfg(not(feature = "std"))]
                    let reply = result.unwrap();

                    let _ = reply.unwrap();
                    count += 1;
                }
                assert_eq!(count, NUM_CALLS);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PingReply {
    id: u32,
    timestamp: u64,
}
