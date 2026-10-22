//! A call's `parameters` may be left out or be an empty object when there are none to pass: a
//! service takes both, for methods without parameters and for those whose parameters are all
//! optional.

#![cfg(all(feature = "server", feature = "tokio"))]

use std::{os::unix::net::UnixStream as StdUnixStream, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use zlink::{ReadyListener, Server, introspect::CustomType, tokio::unix::Stream};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn empty_or_missing_parameters() {
    let (server_sock, client_sock) = StdUnixStream::pair().unwrap();
    let server = Server::new(
        ReadyListener::new(Stream::try_from(server_sock).unwrap()),
        CounterService,
    );
    client_sock.set_nonblocking(true).unwrap();
    let mut client = UnixStream::from_std(client_sock).unwrap();

    let calls = async {
        for (call, reply) in [
            (
                r#"{"method":"org.varlink.service.GetInfo"}"#,
                r#"{"vendor":"#,
            ),
            (
                r#"{"method":"org.varlink.service.GetInfo","parameters":{}}"#,
                r#"{"vendor":"#,
            ),
            (r#"{"method":"org.example.counter.Get"}"#, r#"{"count":0}"#),
            (
                r#"{"method":"org.example.counter.Get","parameters":{}}"#,
                r#"{"count":0}"#,
            ),
            (r#"{"method":"org.example.counter.Add"}"#, r#"{"count":1}"#),
            (
                r#"{"method":"org.example.counter.Add","parameters":{}}"#,
                r#"{"count":1}"#,
            ),
            (
                r#"{"method":"org.example.counter.Add","parameters":null}"#,
                r#"{"count":1}"#,
            ),
            (
                r#"{"method":"org.example.counter.Add","parameters":{"step":2}}"#,
                r#"{"count":2}"#,
            ),
        ] {
            let got = call_raw(&mut client, call).await;
            let want = format!(r#"{{"parameters":{reply}"#);
            assert!(got.starts_with(&want), "for {call}: {got}");
        }
        drop(client);
    };
    let (server_result, ()) = timeout(TIMEOUT, async { tokio::join!(server.run(), calls) })
        .await
        .expect("timed out");
    server_result.unwrap();
}

async fn call_raw(client: &mut UnixStream, call: &str) -> String {
    client.write_all(call.as_bytes()).await.unwrap();
    client.write_all(b"\0").await.unwrap();
    let mut reply = vec![];
    loop {
        let b = client.read_u8().await.unwrap_or_else(|e| {
            panic!(
                "no reply to {call} (the connection closed): {e}; got {:?}",
                String::from_utf8_lossy(&reply)
            )
        });
        if b == 0 {
            break;
        }
        reply.push(b);
    }
    String::from_utf8(reply).unwrap()
}

#[derive(Debug, Serialize, Deserialize, CustomType)]
struct Count {
    count: i64,
}

struct CounterService;

#[zlink::service(
    interface = "org.example.counter",
    vendor = "zlink tests",
    product = "counter",
    version = "1",
    url = "https://example.invalid/",
    types = [Count]
)]
impl CounterService {
    /// A method without parameters.
    async fn get(&self) -> Count {
        Count { count: 0 }
    }

    /// A method whose parameters are all optional.
    async fn add(&self, step: Option<i64>) -> Count {
        Count {
            count: step.unwrap_or(1),
        }
    }
}
