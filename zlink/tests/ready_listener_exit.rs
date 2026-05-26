//! Tests that a `ReadyListener`-driven server exits cleanly when the handed-down connection
//! closes.
//!
//! This is the regression test for the systemd `Accept=yes` / `varlinkctl exec:` hang: before
//! the fix, `Server::run()` with a `ReadyListener` would block forever after the lone client
//! disconnected, because `accept()` pends after the first call. Now `ReadyListener` reports
//! `Listener::exit_when_done() == true`, so the server returns `Ok(())` once the connection is
//! gone and any in-flight work has drained.

#![cfg(all(feature = "service", feature = "proxy"))]

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::{net::UnixStream, time::timeout};
use zlink::{
    Connection, ReadyListener, Server,
    introspect::{self, CustomType},
    unix::Stream,
};

/// The whole test must complete in well under this; the hang behavior we're guarding against
/// would manifest as the server task never returning.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Server side returns `Ok(())` once the client side closes, even though `ReadyListener::accept`
/// continues to pend forever after the first call. The fix is `ReadyListener::exit_when_done()`
/// returning `true`, which `Server::run` honors.
#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn ready_listener_exits_after_client_disconnects() {
    let (server_sock, client_sock) = UnixStream::pair().unwrap();

    let server = Server::new(ReadyListener::new(Stream::from(server_sock)), PingService);
    let client = Connection::new(Stream::from(client_sock));

    // We cannot `tokio::spawn(server.run())` because the future is `!Send` (see the
    // rustc-100013 caveat on `Server::run`). Drive both halves on the same task with
    // `tokio::join!` instead — the client closes its side at the end, which is what lets
    // the server's `run()` return.
    let result = timeout(TIMEOUT, async {
        let (server_result, client_result) = tokio::join!(server.run(), run_client(client));
        client_result.expect("client side failed");
        server_result
    })
    .await;

    let server_result = result.unwrap_or_else(|_| {
        panic!(
            "ReadyListener-driven server did not exit within {TIMEOUT:?} after the client \
             disconnected, which is the hang bug exit_when_done is supposed to fix"
        )
    });

    server_result.expect("server returned an error instead of Ok(())");
}

/// Send one `Ping` call, await the reply, drop the connection. Dropping closes the client
/// side, which is what the server needs to observe to wind down.
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
    /// Replies once with `{ ok: true }`.
    async fn ping(&self) -> Pong {
        Pong { ok: true }
    }
}
