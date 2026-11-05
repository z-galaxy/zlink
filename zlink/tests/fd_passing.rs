//! Test that file descriptors reach the receiver along with the message they were sent with.

#![cfg(feature = "server")]

use std::{fs::File, os::fd::OwnedFd};

use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use zlink::{Call, Listener};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix;
#[cfg(feature = "tokio")]
use zlink::tokio::unix;
#[cfg(all(feature = "zruntime", not(any(feature = "tokio", feature = "smol"))))]
use zlink::zruntime::unix;

#[tokio::test]
async fn fds_arrive_with_their_call() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fd-passing.sock");
    let mut listener = unix::bind(&path).unwrap();
    let mut client = unix::connect(&path).await.unwrap();
    let mut server = listener.accept().await.unwrap().unwrap();

    // Send all the calls before the server reads any of them, so the kernel is free to return
    // several of them in one read, including calls sent along with FDs, the way it does when the
    // receiver falls behind.
    client.send_call(&call(0), vec![]).await.unwrap();
    client.send_call(&call(1), vec![fd()]).await.unwrap();
    client.send_call(&call(2), vec![]).await.unwrap();
    client
        .chain_call(&call(3), vec![])
        .unwrap()
        .append(&call(4), vec![fd(), fd()])
        .unwrap()
        .append(&call(5), vec![])
        .unwrap()
        .append(&call(6), vec![fd()])
        .unwrap()
        .send_ignore_replies()
        .await
        .unwrap();

    for (n, fd_count) in [(0, 0), (1, 1), (2, 0), (3, 0), (4, 2), (5, 0), (6, 1)] {
        let (call, fds) = server.receive_call::<Methods>().await.unwrap();
        let Methods::Pass { n: received_n } = call.method();
        assert_eq!(*received_n, n);
        assert_eq!(fds.len(), fd_count, "FDs received with call {n}");
    }
}

fn call(n: u32) -> Call<Methods> {
    Call::new(Methods::Pass { n }).set_oneway(true)
}

fn fd() -> OwnedFd {
    File::open("/dev/null").unwrap().into()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum Methods {
    #[serde(rename = "org.example.fds.Pass")]
    Pass { n: u32 },
}
