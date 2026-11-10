//! Unit tests for WriteConnection file descriptor passing.

#![cfg(test)]

use crate::{
    Call, Reply,
    connection::write_connection::WriteConnection,
    test_utils::mock_socket::{MockWriteHalf, TestWriteHalf},
};
use core::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use serde::{Deserialize, Serialize};
use std::os::unix::net::UnixStream;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum TestMethod {
    #[serde(rename = "org.example.Test")]
    Test { value: u32 },
}

#[derive(Debug, Serialize, Deserialize)]
struct TestReply {
    result: String,
}

#[tokio::test]
async fn send_call_with_single_fd() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn
        .send_call(&call, vec![r_fd.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
    assert_eq!(write_conn.write_half().fds_written().len(), 1);
}

#[tokio::test]
async fn send_call_with_multiple_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();
    let (r3, _w3) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn
        .send_call(&call, vec![r1.into(), r2.into(), r3.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
    let fds_written = write_conn.write_half().fds_written();
    assert_eq!(fds_written.len(), 1);
    assert_eq!(fds_written[0].len(), 3);
}

#[tokio::test]
async fn send_call_with_no_fds() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn.send_call(&call, vec![]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn send_call_with_empty_fd_vec() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn.send_call(&call, vec![]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn send_reply() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let reply = Reply::new(Some(TestReply {
        result: "success".to_string(),
    }));

    write_conn
        .send_reply(&reply, vec![r_fd.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
}

#[tokio::test]
async fn enqueue_call_with_fds_succeeds() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn.enqueue_call(&call, vec![r_fd.into()]).unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn enqueue_call_without_fds_succeeds() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn.enqueue_call(&call, vec![]).unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn multiple_sequential_fd_sends() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();
    let (r3, _w3) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call1 = Call::new(TestMethod::Test { value: 1 });
    write_conn.send_call(&call1, vec![r1.into()]).await.unwrap();

    let call2 = Call::new(TestMethod::Test { value: 2 });
    write_conn.send_call(&call2, vec![r2.into()]).await.unwrap();

    let call3 = Call::new(TestMethod::Test { value: 3 });
    write_conn.send_call(&call3, vec![r3.into()]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 3);
}

#[tokio::test]
async fn send_call_validates_data_length() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let call = Call::new(TestMethod::Test { value: 42 });
    let serialized_len = serde_json::to_vec(&call).unwrap().len() + 1; // +1 for null terminator.

    let mut write_conn = WriteConnection::new(TestWriteHalf::new_with_fds(serialized_len, 1), 0);

    write_conn
        .send_call(&call, vec![r_fd.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().write_count(), 1);
}

#[tokio::test]
async fn send_call_preserves_call_flags() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 })
        .set_oneway(true)
        .set_more(true);

    write_conn
        .send_call(&call, vec![r_fd.into()])
        .await
        .unwrap();

    let written = write_conn.write_half().written_data();
    let written_str = core::str::from_utf8(written).unwrap();

    assert!(written_str.contains("\"oneway\":true"));
    assert!(written_str.contains("\"more\":true"));
}

#[tokio::test]
async fn backward_compat_send_call_works() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn.send_call(&call, vec![]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn mixed_send_call_and_send_call() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call1 = Call::new(TestMethod::Test { value: 1 });
    write_conn.send_call(&call1, vec![]).await.unwrap();

    let call2 = Call::new(TestMethod::Test { value: 2 });
    write_conn
        .send_call(&call2, vec![r_fd.into()])
        .await
        .unwrap();

    let call3 = Call::new(TestMethod::Test { value: 3 });
    write_conn.send_call(&call3, vec![]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
}

#[tokio::test]
async fn send_call_buffer_growth() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let _large_data = vec![0u8; 10000];
    let call = Call::new(TestMethod::Test { value: 42 });

    write_conn
        .send_call(&call, vec![r_fd.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
}

#[tokio::test]
async fn send_reply_with_multiple_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let reply = Reply::new(Some(TestReply {
        result: "success".to_string(),
    }));

    write_conn
        .send_reply(&reply, vec![r1.into(), r2.into()])
        .await
        .unwrap();

    let fds_written = write_conn.write_half().fds_written();
    assert_eq!(fds_written.len(), 1);
    assert_eq!(fds_written[0].len(), 2);
}

#[tokio::test]
async fn send_reply_with_no_fds() {
    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let reply = Reply::new(Some(TestReply {
        result: "success".to_string(),
    }));

    write_conn.send_reply(&reply, vec![]).await.unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 0);
}

#[tokio::test]
async fn send_error() {
    use crate::ReplyError;

    #[derive(Debug, ReplyError)]
    #[zlink(interface = "org.example")]
    enum TestError {
        NotFound { code: i32 },
    }

    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let error = TestError::NotFound { code: 404 };

    write_conn
        .send_error(&error, vec![r_fd.into()])
        .await
        .unwrap();

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
}

#[tokio::test]
async fn enqueue_normal_then_send_with_fds_clears_queue() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call1 = Call::new(TestMethod::Test { value: 1 });
    write_conn.enqueue_call(&call1, vec![]).unwrap();

    let call2 = Call::new(TestMethod::Test { value: 2 });

    let result = write_conn.send_call(&call2, vec![r_fd.into()]).await;

    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn fd_ownership_with_borrowed_fd() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let mut write_conn = WriteConnection::new(MockWriteHalf::new(), 0);

    let call = Call::new(TestMethod::Test { value: 42 });

    {
        write_conn
            .send_call(&call, vec![r_fd.into()])
            .await
            .unwrap();
    }

    assert_eq!(write_conn.write_half().fd_write_count(), 1);
}

/// The file descriptors of a message go with its first bytes alone, however many writes it takes.
#[tokio::test]
async fn fds_go_with_the_first_bytes_of_their_message() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();
    let (r3, _w3) = UnixStream::pair().unwrap();
    let mut write_conn = WriteConnection::new(MockWriteHalf::new().writing_in_chunks(5), 0);
    let call = Call::new(TestMethod::Test { value: 42 });
    write_conn.enqueue_call(&call, vec![]).unwrap();
    let second = write_conn.pos;
    write_conn.enqueue_call(&call, vec![r1.into()]).unwrap();
    let third = write_conn.pos;
    write_conn
        .enqueue_call(&call, vec![r2.into(), r3.into()])
        .unwrap();
    let enqueued = write_conn.buffer[..write_conn.pos].to_vec();

    write_conn.flush().await.unwrap();
    let write = write_conn.write_half();
    assert_eq!(write.written_data(), enqueued);
    assert_eq!(write.fd_positions(), [second, third]);
    let counts: Vec<_> = write.fds_written().iter().map(Vec::len).collect();
    assert_eq!(counts, [1, 2]);
}

/// The file descriptors of a message go once, even if the flush that sent them with the first bytes
/// of the message is dropped before the rest of it is written.
#[tokio::test]
async fn dropped_flush_sends_fds_once() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let mut write_conn = WriteConnection::new(MockWriteHalf::new().writing_in_chunks(5), 0);
    let call = Call::new(TestMethod::Test { value: 42 });
    write_conn.enqueue_call(&call, vec![r1.into()]).unwrap();
    let enqueued = write_conn.buffer[..write_conn.pos].to_vec();

    let mut cx = Context::from_waker(Waker::noop());
    assert!(pin!(write_conn.flush()).poll(&mut cx).is_pending());
    assert_eq!(write_conn.write_half().fd_write_count(), 1);
    write_conn.flush().await.unwrap();
    let write = write_conn.write_half();
    assert_eq!(write.written_data(), enqueued);
    assert_eq!(write.fd_positions(), [0]);
}
