use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use futures_util::{pin_mut, StreamExt};
use serde::{Deserialize, Serialize};
use std::{hint::black_box, time::Duration};
use tokio::{runtime::Runtime, time::sleep};
use zlink_core::{
    connection::{
        socket::{ReadHalf, Socket, WriteHalf},
        Connection,
    },
    Call, Reply,
};

criterion_group!(benches, client_sending, server_receiving);
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
                        let _: Call<TestMethod> =
                            server_conn.read_mut().receive_call().await.unwrap();

                        // Simulate context switch overhead (100 microseconds).
                        sleep(Duration::from_micros(100)).await;

                        let reply = Reply::new(Some(PingReply {
                            id: 1,
                            timestamp: 12345,
                        }));
                        server_conn.write_mut().send_reply(&reply).await.unwrap();
                    }
                });

                Connection::new(client_socket)
            },
            |mut conn| async move {
                // Benchmark: Make sequential calls - each waits for reply before sending next.
                for i in 0..NUM_CALLS {
                    let call = Call::new(TestMethod::Ping { id: i as u32 });
                    conn.send_call(&call).await.unwrap();

                    #[derive(Debug, Deserialize)]
                    struct DummyError;
                    let _: zlink_core::reply::Result<PingReply, DummyError> =
                        conn.receive_reply().await.unwrap();
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
                        let _: Call<TestMethod> =
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
                let mut chain = conn.chain_call::<_, PingReply, DummyError>(&call).unwrap();

                for i in 1..NUM_CALLS {
                    let call = Call::new(TestMethod::Ping { id: i as u32 });
                    chain = chain.append(&call).unwrap();
                }

                // Send all at once and collect replies.
                let replies = chain.send().await.unwrap();
                pin_mut!(replies);

                let mut count = 0;
                while let Some(reply) = replies.next().await {
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

// Server-side benchmarks: Sequential vs Batched receiving
fn server_receiving(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
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
                        client_conn.send_call(&call).await.unwrap();

                        // Wait for reply before sending next (sequential pattern).
                        #[derive(Debug, Deserialize)]
                        struct DummyError;
                        let _: zlink_core::reply::Result<ComputeReply, DummyError> =
                            client_conn.receive_reply().await.unwrap();
                    }
                });

                Connection::new(server_socket)
            },
            |mut server_conn| async move {
                // Benchmark: Server receives calls one at a time.
                for _ in 0..NUM_CALLS {
                    // Measure the time to receive and deserialize each call.
                    let call: Call<TestMethod> =
                        server_conn.read_mut().receive_call().await.unwrap();
                    black_box(call);

                    // Send reply so client can continue.
                    let reply = Reply::new(Some(ComputeReply { result: 42 }));
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
                        client_conn.write_mut().enqueue_call(&call).unwrap();
                    }
                    // Flush all at once.
                    client_conn.write_mut().flush().await.unwrap();

                    // Collect replies.
                    for _ in 0..NUM_CALLS {
                        #[derive(Debug, Deserialize)]
                        struct DummyError;
                        let _: zlink_core::reply::Result<ComputeReply, DummyError> =
                            client_conn.receive_reply().await.unwrap();
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
                    let call: Call<TestMethod> =
                        server_conn.read_mut().receive_call().await.unwrap();
                    calls.push(call);
                }
                black_box(&calls);

                // Send replies.
                for _ in 0..NUM_CALLS {
                    let reply = Reply::new(Some(ComputeReply { result: 42 }));
                    server_conn.write_mut().send_reply(&reply).await.unwrap();
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

const NUM_CALLS: usize = 20;

// Method definitions for benchmarking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum TestMethod {
    Ping { id: u32 },
    Compute { values: Vec<u32> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PingReply {
    id: u32,
    timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComputeReply {
    result: u64,
}

// Bidirectional mock socket for in-memory communication.
#[derive(Debug)]
struct BiPipeSocket {
    client_to_server: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    server_to_client: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl BiPipeSocket {
    fn new_pair() -> (Self, Self) {
        let (c2s_tx, c2s_rx) = tokio::sync::mpsc::unbounded_channel();
        let (s2c_tx, s2c_rx) = tokio::sync::mpsc::unbounded_channel();

        let client = BiPipeSocket {
            client_to_server: c2s_tx,
            server_to_client: s2c_rx,
        };

        let server = BiPipeSocket {
            client_to_server: s2c_tx,
            server_to_client: c2s_rx,
        };

        (client, server)
    }
}

impl Socket for BiPipeSocket {
    type ReadHalf = BiPipeReadHalf;
    type WriteHalf = BiPipeWriteHalf;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (
            BiPipeReadHalf {
                receiver: self.server_to_client,
                buffer: Vec::new(),
                pos: 0,
            },
            BiPipeWriteHalf {
                sender: self.client_to_server,
            },
        )
    }
}

#[derive(Debug)]
struct BiPipeReadHalf {
    receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    buffer: Vec<u8>,
    pos: usize,
}

impl ReadHalf for BiPipeReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> zlink_core::Result<usize> {
        // If we have buffered data, return it.
        if self.pos < self.buffer.len() {
            let to_read = (self.buffer.len() - self.pos).min(buf.len());
            buf[..to_read].copy_from_slice(&self.buffer[self.pos..self.pos + to_read]);
            self.pos += to_read;
            return Ok(to_read);
        }

        // Otherwise, wait for new data.
        match self.receiver.recv().await {
            Some(data) => {
                self.buffer = data;
                self.pos = 0;
                let to_read = self.buffer.len().min(buf.len());
                buf[..to_read].copy_from_slice(&self.buffer[..to_read]);
                self.pos = to_read;
                Ok(to_read)
            }
            None => Ok(0), // Connection closed.
        }
    }
}

#[derive(Debug)]
struct BiPipeWriteHalf {
    sender: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl WriteHalf for BiPipeWriteHalf {
    async fn write(&mut self, buf: &[u8]) -> zlink_core::Result<()> {
        self.sender
            .send(buf.to_vec())
            .map_err(|_| zlink_core::Error::UnexpectedEof)?;
        Ok(())
    }
}
