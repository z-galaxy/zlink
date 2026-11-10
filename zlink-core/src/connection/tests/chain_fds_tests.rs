//! Unit tests for Chain file descriptor passing.

#![cfg(test)]

use crate::{
    Call, Result,
    connection::{
        Connection,
        socket::{ReadHalf, Socket, WriteHalf},
    },
    test_utils::mock_socket::{MockSocket, MockWriteHalf},
};
use alloc::vec::Vec;
use futures_util::{pin_mut, stream::StreamExt};
use rustix::{fd::AsFd, io::write};
use serde::{Deserialize, Serialize};
use std::os::unix::net::UnixStream;

#[tokio::test]
async fn chain_replies_with_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();

    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;
    let reply2 = r#"{"parameters":{"id":2,"name":"Bob"}}"#;

    // Both replies fit in one read, so all FDs are returned with the first message.
    let fds = vec![vec![r1.into(), r2.into()]];

    let socket = MockSocket::new(&[reply1, reply2], fds);
    let mut conn = Connection::new(socket);

    let call1 = Call::new(GetUser { id: 1 });
    let call2 = Call::new(GetUser { id: 2 });

    let replies = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .append(&call2, vec![])
        .unwrap()
        .send::<User, ApiError>()
        .await
        .unwrap();

    pin_mut!(replies);

    let (reply1, fds1) = replies.next().await.unwrap().unwrap();
    let reply1 = reply1.unwrap();
    assert_eq!(reply1.parameters().unwrap().id, 1);
    assert_eq!(fds1.len(), 2); // All FDs from the read operation

    let (reply2, fds2) = replies.next().await.unwrap().unwrap();
    let reply2 = reply2.unwrap();
    assert_eq!(reply2.parameters().unwrap().id, 2);
    assert_eq!(fds2.len(), 0); // No new read, no new FDs
}

#[tokio::test]
async fn chain_replies_with_no_fds() {
    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;
    let reply2 = r#"{"parameters":{"id":2,"name":"Bob"}}"#;

    let fds = vec![vec![]];

    let socket = MockSocket::new(&[reply1, reply2], fds);
    let mut conn = Connection::new(socket);

    let call1 = Call::new(GetUser { id: 1 });
    let call2 = Call::new(GetUser { id: 2 });

    let replies = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .append(&call2, vec![])
        .unwrap()
        .send::<User, ApiError>()
        .await
        .unwrap();

    pin_mut!(replies);

    let (reply1, fds1) = replies.next().await.unwrap().unwrap();
    let reply1 = reply1.unwrap();
    assert_eq!(reply1.parameters().unwrap().id, 1);
    assert!(fds1.is_empty());

    let (reply2, fds2) = replies.next().await.unwrap().unwrap();
    let reply2 = reply2.unwrap();
    assert_eq!(reply2.parameters().unwrap().id, 2);
    assert!(fds2.is_empty());
}

#[tokio::test]
async fn chain_send_with_fds() {
    let (r1, w1) = UnixStream::pair().unwrap();
    let (r2, w2) = UnixStream::pair().unwrap();

    // Write test data to write ends so we can verify FDs were transmitted.
    write(w1.as_fd(), b"data1").unwrap();
    write(w2.as_fd(), b"data2").unwrap();

    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;
    let reply2 = r#"{"parameters":{"id":2,"name":"Bob"}}"#;

    let socket = MockSocket::new(&[reply1, reply2], vec![vec![]]);
    let (read_half, write_half) = socket.split();
    let tracking_write = TrackingWriteHalf { mock: write_half };
    let mut conn = Connection::new(TrackingSocket {
        read: read_half,
        write: tracking_write,
    });

    let call1 = Call::new(GetUser { id: 1 });
    let call2 = Call::new(GetUser { id: 2 });

    // Send FDs with the calls.
    let chain = conn
        .chain_call::<GetUser>(&call1, vec![r1.into()])
        .unwrap()
        .append(&call2, vec![r2.into()])
        .unwrap();

    let replies = chain.send::<User, ApiError>().await.unwrap();

    // Collect replies first to release the borrow on conn.
    let reply_results: Vec<_> = {
        pin_mut!(replies);
        replies.collect().await
    }; // `replies` dropped here, releasing the borrow.

    // Now we can access the FDs captured by the mock.
    let fds_written = conn.write_mut().socket.mock.fds_written();
    assert_eq!(fds_written.len(), 2, "Should have written FDs twice");
    assert_eq!(fds_written[0].len(), 1, "First call should send 1 FD");
    assert_eq!(fds_written[1].len(), 1, "Second call should send 1 FD");

    // Verify we can read data from the received FDs.
    let mut buf1 = [0u8; 5];
    rustix::io::read(fds_written[0][0].as_fd(), &mut buf1).unwrap();
    assert_eq!(&buf1, b"data1");

    let mut buf2 = [0u8; 5];
    rustix::io::read(fds_written[1][0].as_fd(), &mut buf2).unwrap();
    assert_eq!(&buf2, b"data2");

    // Verify the replies.
    assert_eq!(reply_results.len(), 2);

    let (reply1, _fds) = reply_results[0].as_ref().unwrap();
    let reply1 = reply1.as_ref().unwrap();
    assert_eq!(reply1.parameters().unwrap().id, 1);

    let (reply2, _fds) = reply_results[1].as_ref().unwrap();
    let reply2 = reply2.as_ref().unwrap();
    assert_eq!(reply2.parameters().unwrap().id, 2);
}

#[tokio::test]
async fn chain_oneway_call_with_fds() {
    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;

    let socket = MockSocket::new(&[reply1], vec![vec![]]);
    let mut conn = Connection::new(socket);

    let call1 = Call::new(GetUser { id: 1 });
    let oneway_call = Call::new(GetUser { id: 2 }).set_oneway(true);

    let replies = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .append(&oneway_call, vec![])
        .unwrap()
        .send::<User, ApiError>()
        .await
        .unwrap();

    pin_mut!(replies);

    // Only expect one reply since the second call is oneway.
    let (reply1, _fds) = replies.next().await.unwrap().unwrap();
    let reply1 = reply1.unwrap();
    assert_eq!(reply1.parameters().unwrap().id, 1);

    // No more replies.
    assert!(replies.next().await.is_none());
}

#[tokio::test]
async fn chain_error_reply_with_fds() {
    use crate::ReplyError;

    #[derive(Debug, ReplyError)]
    #[zlink(interface = "org.example")]
    enum TestError {
        NotFound { code: i32 },
    }

    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let error_reply = r#"{"error":"org.example.NotFound","parameters":{"code":404}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[error_reply], fds);
    let mut conn = Connection::new(socket);

    let call1 = Call::new(GetUser { id: 1 });

    let replies = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .send::<User, TestError>()
        .await
        .unwrap();

    pin_mut!(replies);

    let (reply, fds) = replies.next().await.unwrap().unwrap();
    assert!(reply.is_err());
    assert_eq!(fds.len(), 1);
}

/// Test FD distribution when server sends FDs back.
///
/// This tests the receiving side: when the server sends FDs with replies,
/// they arrive with the first read operation. MockSocket simulates all messages
/// arriving in one read, so all FDs come with the first message.
#[tokio::test]
async fn chain_receive_fds_from_server() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();
    let (r3, _w3) = UnixStream::pair().unwrap();

    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;
    let reply2 = r#"{"parameters":{"id":2,"name":"Bob"}}"#;
    let reply3 = r#"{"parameters":{"id":3,"name":"Charlie"}}"#;

    // Server sends FDs with replies. MockSocket delivers them all in one read,
    // so all FDs arrive with the first message's first byte.
    let fds = vec![vec![r1.into(), r2.into(), r3.into()]];

    let socket = MockSocket::new(&[reply1, reply2, reply3], fds);
    let mut conn = Connection::new(socket);

    let call1 = Call::new(GetUser { id: 1 });
    let call2 = Call::new(GetUser { id: 2 });
    let call3 = Call::new(GetUser { id: 3 });

    let replies = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .append(&call2, vec![])
        .unwrap()
        .append(&call3, vec![])
        .unwrap()
        .send::<User, ApiError>()
        .await
        .unwrap();

    pin_mut!(replies);
    let results: Vec<_> = replies.collect().await;

    assert_eq!(results.len(), 3);

    // First result gets all the FDs from the single read operation.
    let (reply, fds) = results[0].as_ref().unwrap();
    let user = reply.as_ref().unwrap();
    assert_eq!(user.parameters().unwrap().id, 1);
    assert_eq!(fds.len(), 3);

    // Remaining results have no FDs (no new read operations).
    for (i, result) in results.iter().enumerate().skip(1) {
        let (reply, fds) = result.as_ref().unwrap();
        let user = reply.as_ref().unwrap();
        assert_eq!(user.parameters().unwrap().id, (i + 1) as u32);
        assert_eq!(fds.len(), 0);
    }
}

/// Test that FDs are sent only with their specific message bytes.
///
/// This tests the critical flush behavior: when a message has FDs, we must flush pending messages
/// first, then write the FD message separately. This ensures that:
/// 1. FDs are sent in a separate write operation
/// 2. The FD write contains ONLY the bytes of the message with FDs
/// 3. Previous and subsequent messages are sent in their own writes
#[tokio::test]
async fn chain_fds_sent_only_with_their_message() {
    let (r_fd, w_fd) = UnixStream::pair().unwrap();
    write(w_fd.as_fd(), b"test").unwrap();

    let reply1 = r#"{"parameters":{"id":1,"name":"Alice"}}"#;
    let reply2 = r#"{"parameters":{"id":2,"name":"Bob"}}"#;
    let reply3 = r#"{"parameters":{"id":3,"name":"Charlie"}}"#;

    let socket = MockSocket::new(&[reply1, reply2, reply3], vec![vec![]]);
    let (read_half, write_half) = socket.split();

    // Wrap the write half to track each write operation.
    let tracking_write = WriteOperationTracker {
        mock: write_half,
        operations: Vec::new(),
    };

    let mut conn = Connection::new(TrackingSocket {
        read: read_half,
        write: tracking_write,
    });

    let call1 = Call::new(GetUser { id: 1 });
    let call2 = Call::new(GetUser { id: 2 });
    let call3 = Call::new(GetUser { id: 3 });

    // Build chain: call1 (no FDs), call2 (with FD), call3 (no FDs).
    let chain = conn
        .chain_call::<GetUser>(&call1, vec![])
        .unwrap()
        .append(&call2, vec![r_fd.into()])
        .unwrap()
        .append(&call3, vec![])
        .unwrap();

    // Flush to send all messages.
    chain.connection.write_mut().flush().await.unwrap();

    // Verify write operations.
    let ops = &mut conn.write_mut().socket.operations;

    // Should have exactly 3 write operations.
    assert_eq!(ops.len(), 3, "Expected 3 write operations");

    // First write: call1 without FDs.
    assert_eq!(ops[0].fd_count, 0, "call1 should have no FDs");
    let call1_str = core::str::from_utf8(&ops[0].data[..ops[0].data.len() - 1]).unwrap();
    assert!(
        call1_str.contains("\"id\":1"),
        "call1 write should contain id:1"
    );
    assert!(
        ops[0].data.ends_with(b"\0"),
        "call1 should be null-terminated"
    );

    // Second write: call2 with FD - critically, this should ONLY contain call2's bytes.
    assert_eq!(ops[1].fd_count, 1, "call2 should have 1 FD");
    let call2_str = core::str::from_utf8(&ops[1].data[..ops[1].data.len() - 1]).unwrap();
    assert!(
        call2_str.contains("\"id\":2"),
        "call2 write should contain id:2"
    );
    assert!(
        ops[1].data.ends_with(b"\0"),
        "call2 should be null-terminated"
    );

    // CRITICAL: Verify call2 write doesn't contain call1 or call3 data.
    // This ensures FDs are sent ONLY with their specific message bytes.
    assert!(
        !call2_str.contains("\"id\":1"),
        "call2 write should NOT contain id:1 from call1"
    );
    assert!(
        !call2_str.contains("\"id\":3"),
        "call2 write should NOT contain id:3 from call3"
    );

    // Third write: call3 without FDs.
    assert_eq!(ops[2].fd_count, 0, "call3 should have no FDs");
    let call3_str = core::str::from_utf8(&ops[2].data[..ops[2].data.len() - 1]).unwrap();
    assert!(
        call3_str.contains("\"id\":3"),
        "call3 write should contain id:3"
    );
    assert!(
        ops[2].data.ends_with(b"\0"),
        "call3 should be null-terminated"
    );
}

#[derive(Debug, Serialize, Deserialize)]
struct GetUser {
    id: u32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User {
    id: u32,
    name: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ApiError {
    code: i32,
}

/// Socket wrapper that provides access to the write half after use.
#[derive(Debug)]
struct TrackingSocket<R, W> {
    read: R,
    write: W,
}

impl<R: ReadHalf, W: WriteHalf> Socket for TrackingSocket<R, W> {
    type ReadHalf = R;
    type WriteHalf = W;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (self.read, self.write)
    }

    fn join(read: Self::ReadHalf, write: Self::WriteHalf) -> crate::Result<Self> {
        Ok(Self { read, write })
    }
}

/// Write half wrapper for testing that provides access to MockWriteHalf.
#[derive(Debug)]
struct TrackingWriteHalf {
    mock: MockWriteHalf,
}

impl WriteHalf for TrackingWriteHalf {
    async fn write(
        &mut self,
        buf: &[u8],
        fds: &[impl AsFd],
        #[cfg(target_os = "linux")] credentials: Option<&crate::connection::PassedCredentials>,
    ) -> Result<usize> {
        self.mock
            .write(
                buf,
                fds,
                #[cfg(target_os = "linux")]
                credentials,
            )
            .await
    }
}

/// Tracks individual write operations with their data and FD counts.
#[derive(Debug)]
struct WriteOperation {
    data: Vec<u8>,
    fd_count: usize,
}

/// Write half wrapper that tracks each write operation separately.
#[derive(Debug)]
struct WriteOperationTracker {
    mock: MockWriteHalf,
    operations: Vec<WriteOperation>,
}

impl WriteHalf for WriteOperationTracker {
    async fn write(
        &mut self,
        buf: &[u8],
        fds: &[impl AsFd],
        #[cfg(target_os = "linux")] credentials: Option<&crate::connection::PassedCredentials>,
    ) -> Result<usize> {
        // Record this write operation.
        self.operations.push(WriteOperation {
            data: buf.to_vec(),
            fd_count: fds.len(),
        });

        // Also write to the mock for actual functionality.
        self.mock
            .write(
                buf,
                fds,
                #[cfg(target_os = "linux")]
                credentials,
            )
            .await
    }
}
