//! Regression test: a `ReadyListener`-driven server must exit cleanly once its handed-down
//! connection closes, instead of hanging on the next `accept()`.

#![cfg(all(feature = "server", feature = "proxy"))]

use std::{os::unix::net::UnixStream as StdUnixStream, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use zlink::{
    Connection, ReadyListener, Server,
    introspect::{self, CustomType},
};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix::Stream;
#[cfg(feature = "tokio")]
use zlink::tokio::unix::Stream;

const TIMEOUT: Duration = Duration::from_secs(5);

#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn ready_listener_exits_after_client_disconnects() {
    let (server_sock, client_sock) = StdUnixStream::pair().unwrap();
    let server_stream = Stream::try_from(server_sock).unwrap();
    let client_stream = Stream::try_from(client_sock).unwrap();

    let server = Server::new(ReadyListener::new(server_stream), PingService);
    let client = Connection::new(client_stream);

    // `Server::run` is `!Send` (rustc-100013), so drive both halves on the same task.
    let result = timeout(TIMEOUT, async {
        let (server_result, client_result) = tokio::join!(server.run(), run_client(client));
        client_result.expect("client side failed");
        server_result
    })
    .await;

    let server_result = result.unwrap_or_else(|_| {
        panic!(
            "ReadyListener-driven server did not exit within {TIMEOUT:?} after the client \
             disconnected"
        )
    });

    server_result.expect("server returned an error instead of Ok(())");
}

async fn run_client(mut conn: Connection<Stream>) -> Result<(), Box<dyn std::error::Error>> {
    use zlink::proxy;

    #[proxy(interface = "org.example.ping")]
    trait PingProxy {
        async fn ping(&mut self) -> zlink::Result<Result<Pong, PingError>>;
    }

    let reply = conn.ping().await?.expect("server returned an error");
    assert!(reply.ok);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Pong {
    ok: bool,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.ping")]
enum PingError {
    Boom,
}

struct PingService;

#[zlink::service(
    interface = "org.example.ping",
    vendor = "zlink tests",
    product = "ping",
    version = "1",
    url = "https://example.invalid/",
    types = [Pong]
)]
impl PingService {
    async fn ping(&self) -> Pong {
        Pong { ok: true }
    }
}
