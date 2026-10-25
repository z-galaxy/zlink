//! Unit tests for ReadConnection file descriptor passing.

#![cfg(test)]

use crate::{
    connection::{
        read_connection::ReadConnection,
        socket::{ReadHalf, ReadResult, Socket},
    },
    test_utils::mock_socket::MockSocket,
};
use alloc::{collections::VecDeque, vec::Vec};
use rustix::fd::AsFd;
use serde::{Deserialize, Serialize};
use std::os::{fd::OwnedFd, unix::net::UnixStream};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "method", content = "parameters")]
enum TestMethod {
    #[serde(rename = "org.example.Test")]
    Test { value: u32 },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct TestReply {
    result: String,
}

#[tokio::test]
async fn receive_call_with_single_fd() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();

    assert_eq!(call.method(), &TestMethod::Test { value: 42 });
    assert_eq!(received_fds.len(), 1);
}

#[tokio::test]
async fn receive_call_with_multiple_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();
    let (r3, _w3) = UnixStream::pair().unwrap();

    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![vec![r1.into(), r2.into(), r3.into()]];

    let socket = MockSocket::new(&[call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();

    assert_eq!(call.method(), &TestMethod::Test { value: 42 });
    assert_eq!(received_fds.len(), 3);
}

#[tokio::test]
async fn receive_call_with_no_fds() {
    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![vec![]];

    let socket = MockSocket::new(&[call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();

    assert_eq!(call.method(), &TestMethod::Test { value: 42 });
    assert!(received_fds.is_empty());
}

#[tokio::test]
async fn receive_multiple_calls_with_mixed_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();

    let call1 = r#"{"method":"org.example.Test","parameters":{"value":1}}"#;
    let call2 = r#"{"method":"org.example.Test","parameters":{"value":2}}"#;
    let call3 = r#"{"method":"org.example.Test","parameters":{"value":3}}"#;

    let fds = vec![vec![r1.into(), r2.into()]];

    let socket = MockSocket::new(&[call1, call2, call3], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(call.method(), &TestMethod::Test { value: 1 });
    assert_eq!(fds.len(), 2); // The FDs came with this call.

    let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(call.method(), &TestMethod::Test { value: 2 });
    assert!(fds.is_empty());

    let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(call.method(), &TestMethod::Test { value: 3 });
    assert!(fds.is_empty());
}

// The FDs of a call are sent along with its first byte, but the kernel may return them in a read
// that also returns other calls.
#[tokio::test]
async fn receive_calls_with_fds_among_other_calls() {
    let call4 = call(4);
    // Linux can return the data written before the FDs in the read that returns them, which ends
    // with the data written along with them.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let reads = [
        ([call(1), call(2)].concat(), vec![fd()]),
        ([&call(3), &call4[..10]].concat(), vec![fd(), fd()]),
        (call4[10..].to_vec(), vec![]),
    ];
    // BSD-derived kernels start the read that returns the FDs with the data written along with them
    // and can return the data written after them in the same read.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let reads = [
        (call(1), vec![]),
        ([call(2), call(3)].concat(), vec![fd()]),
        (call4[..10].to_vec(), vec![fd(), fd()]),
        (call4[10..].to_vec(), vec![]),
    ];
    let mut conn = ReadConnection::new(ScriptedReadHalf(reads.into()), 0);

    for (value, fd_count) in [(1, 0), (2, 1), (3, 0), (4, 2)] {
        let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
        assert_eq!(call.method(), &TestMethod::Test { value });
        assert_eq!(fds.len(), fd_count, "FDs received with call {value}");
    }
}

// The FDs of a call that's only partially read when the call before it is received are returned
// with it once the rest of it is read.
#[tokio::test]
async fn receive_partially_read_call_with_fds() {
    let call2 = call(2);
    // Linux returns the FDs in the read that ends with the data written along with them, so the
    // buffer is moved to make room for the rest of the second call while the FDs are pending.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let reads = [
        ([call(1), call2[..10].to_vec()].concat(), vec![fd(), fd()]),
        (call2[10..].to_vec(), vec![]),
    ];
    // BSD-derived kernels start the read that returns the FDs with the data written along with
    // them.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let reads = [
        (call(1), vec![]),
        (call2[..10].to_vec(), vec![fd(), fd()]),
        (call2[10..].to_vec(), vec![]),
    ];
    let mut conn = ReadConnection::new(ScriptedReadHalf(reads.into()), 0);

    for (value, fd_count) in [(1, 0), (2, 2)] {
        let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
        assert_eq!(call.method(), &TestMethod::Test { value });
        assert_eq!(fds.len(), fd_count, "FDs received with call {value}");
    }
}

// What follows a message isn't necessarily another message, e.g. after a protocol upgrade. Linux
// returns the FDs in the read that ends with the data written along with them, so they're received
// with the reply here, but they're not the reply's: they're read from `into_read_half` along with
// that data. That holds even when the data is only NUL bytes, like the single byte commonly sent
// along with FDs.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[tokio::test]
async fn receive_reply_followed_by_nul_with_fds() {
    let reads = [(
        [&br#"{"parameters":{"result":"ok"}}"#[..], b"\0\0"].concat(),
        vec![fd()],
    )];
    let mut conn = ReadConnection::new(ScriptedReadHalf(reads.into()), 0);

    let (reply, fds) = conn.receive_reply::<TestReply, TestReply>().await.unwrap();
    assert_eq!(reply.unwrap().parameters().unwrap().result, "ok");
    assert!(fds.is_empty());

    let mut read = conn.into_read_half();
    assert_eq!(read_all(&mut read).await, [(b"\0".to_vec(), 1)]);
}

#[tokio::test]
async fn receive_reply_with_single_fd() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let reply_json = r#"{"parameters":{"result":"success"}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[reply_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (reply, received_fds) = conn.receive_reply::<TestReply, TestReply>().await.unwrap();

    let reply = reply.unwrap();
    assert_eq!(reply.parameters().unwrap().result, "success");

    assert_eq!(received_fds.len(), 1);
}

#[tokio::test]
async fn receive_reply_with_multiple_fds() {
    let (r1, _w1) = UnixStream::pair().unwrap();
    let (r2, _w2) = UnixStream::pair().unwrap();

    let reply_json = r#"{"parameters":{"result":"success"}}"#;
    let fds = vec![vec![r1.into(), r2.into()]];

    let socket = MockSocket::new(&[reply_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (reply, received_fds) = conn.receive_reply::<TestReply, TestReply>().await.unwrap();

    let reply = reply.unwrap();
    assert_eq!(reply.parameters().unwrap().result, "success");
    assert_eq!(received_fds.len(), 2);
}

#[tokio::test]
async fn receive_reply_with_no_fds() {
    let reply_json = r#"{"parameters":{"result":"success"}}"#;
    let fds = vec![vec![]];

    let socket = MockSocket::new(&[reply_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (reply, received_fds) = conn.receive_reply::<TestReply, TestReply>().await.unwrap();

    let reply = reply.unwrap();
    assert_eq!(reply.parameters().unwrap().result, "success");
    assert!(received_fds.is_empty());
}

#[tokio::test]
async fn receive_call_malformed_json() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let malformed = r#"{"method":"org.example.Test","parameters":"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[malformed], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let result = conn.receive_call::<TestMethod>().await;
    assert!(matches!(result, Err(crate::Error::Json(_))));
}

#[tokio::test]
async fn receive_call_large_message() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let large_value = 1000000u32;
    let call_json = format!(
        r#"{{"method":"org.example.Test","parameters":{{"value":{}}}}}"#,
        large_value
    );

    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[&call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();

    assert_eq!(call.method(), &TestMethod::Test { value: large_value });
    assert_eq!(received_fds.len(), 1);
}

#[tokio::test]
async fn fd_validity_after_receive() {
    use rustix::io::{read, write};

    let (r_fd, w_fd) = UnixStream::pair().unwrap();

    write(w_fd.as_fd(), b"test data").unwrap();

    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[call_json], fds);
    let (read_half, _write) = socket.split();
    let mut conn = ReadConnection::new(read_half, 0);

    let (_call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();

    let mut buf = [0u8; 9];
    let n = read(received_fds[0].as_fd(), &mut buf).unwrap();
    assert_eq!(n, 9);
    assert_eq!(&buf[..n], b"test data");
}

#[tokio::test]
async fn receive_call_with_max_fds() {
    let mut fds_vec = Vec::new();
    for _ in 0..10 {
        let (r, _w) = UnixStream::pair().unwrap();
        fds_vec.push(r.into());
    }

    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![fds_vec];

    let socket = MockSocket::new(&[call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (_call, received_fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(received_fds.len(), 10);
}

#[tokio::test]
async fn receive_call_backward_compat_regular_receive() {
    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let call_json = r#"{"method":"org.example.Test","parameters":{"value":42}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[call_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, _fds) = conn.receive_call::<TestMethod>().await.unwrap();

    assert_eq!(call.method(), &TestMethod::Test { value: 42 });
}

#[tokio::test]
async fn mixed_receive_call_and_receive_call() {
    let (r1, _w1) = UnixStream::pair().unwrap();

    let call1 = r#"{"method":"org.example.Test","parameters":{"value":1}}"#;
    let call2 = r#"{"method":"org.example.Test","parameters":{"value":2}}"#;

    let fds = vec![vec![r1.into()], vec![]];

    let socket = MockSocket::new(&[call1, call2], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (call, fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(call.method(), &TestMethod::Test { value: 1 });
    assert_eq!(fds.len(), 1);

    let (call, _fds) = conn.receive_call::<TestMethod>().await.unwrap();
    assert_eq!(call.method(), &TestMethod::Test { value: 2 });
}

#[tokio::test]
async fn receive_reply_error_with_fds() {
    use crate::ReplyError;

    #[derive(Debug, ReplyError)]
    #[zlink(interface = "org.example")]
    enum TestError {
        NotFound { code: i32 },
    }

    let (r_fd, _w_fd) = UnixStream::pair().unwrap();

    let error_json = r#"{"error":"org.example.NotFound","parameters":{"code":404}}"#;
    let fds = vec![vec![r_fd.into()]];

    let socket = MockSocket::new(&[error_json], fds);
    let (read, _write) = socket.split();
    let mut conn = ReadConnection::new(read, 0);

    let (reply, received_fds) = conn.receive_reply::<TestReply, TestError>().await.unwrap();

    assert!(reply.is_err());
    assert!(matches!(
        reply.unwrap_err(),
        TestError::NotFound { code: 404 }
    ));
    assert_eq!(received_fds.len(), 1);
}

// Linux returns the FDs in the read that ends with the data written along with them. When that's
// the data of another protocol following a reply, the FDs are received before the reply is
// returned. They're then read from `into_read_half` along with the byte that came with them, alone,
// so that it's the last byte of the read as on Linux, as well as the first one as elsewhere.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[tokio::test]
async fn into_read_half_returns_unreceived_fds() {
    let reads = [
        (br#"{"parameters":"#.to_vec(), vec![fd()]),
        (
            [&br#"{"result":"ok"}}"#[..], b"\0raw"].concat(),
            vec![fd(), fd()],
        ),
    ];
    let mut conn = ReadConnection::new(ScriptedReadHalf(reads.into()), 0);

    let (reply, fds) = conn.receive_reply::<TestReply, TestReply>().await.unwrap();
    assert_eq!(reply.unwrap().parameters().unwrap().result, "ok");
    assert_eq!(fds.len(), 1);

    let mut read = conn.into_read_half();
    assert_eq!(
        read_all(&mut read).await,
        [(b"ra".to_vec(), 0), (b"w".to_vec(), 2)]
    );
}

/// A call to `org.example.Test` with `value`, as it's sent over the socket.
fn call(value: u32) -> Vec<u8> {
    let mut call =
        format!(r#"{{"method":"org.example.Test","parameters":{{"value":{value}}}}}"#).into_bytes();
    call.push(b'\0');

    call
}

/// The data and the number of FDs of each read from `read`, until the end of the data.
#[cfg(any(target_os = "linux", target_os = "android"))]
async fn read_all<R>(read: &mut R) -> Vec<(Vec<u8>, usize)>
where
    R: ReadHalf,
{
    let mut buf = [0; 64];
    let mut reads = Vec::new();
    loop {
        let result = read.read(&mut buf).await.unwrap();
        let bytes_read = result.bytes_read();
        if bytes_read == 0 {
            return reads;
        }
        reads.push((buf[..bytes_read].to_vec(), result.fds().len()));
    }
}

fn fd() -> OwnedFd {
    let (fd, _) = UnixStream::pair().unwrap();

    fd.into()
}

/// A read half returning the given data and FDs, in the given chunks.
///
/// This allows emulating how the kernel splits the data written by the peer into reads.
#[derive(Debug)]
struct ScriptedReadHalf(VecDeque<(Vec<u8>, Vec<OwnedFd>)>);

impl ReadHalf for ScriptedReadHalf {
    async fn read(&mut self, buf: &mut [u8]) -> crate::Result<ReadResult> {
        let Some((data, fds)) = self.0.pop_front() else {
            return Ok(ReadResult::new(0));
        };
        buf[..data.len()].copy_from_slice(&data);

        Ok(ReadResult::new(data.len()).set_fds(fds))
    }
}
