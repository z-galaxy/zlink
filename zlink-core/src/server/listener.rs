use core::future::Future;

use crate::{Connection, Result, connection::Socket};

/// A listener is a server that listens for incoming connections.
pub trait Listener: core::fmt::Debug {
    /// The type of the socket the connections this listener creates will use.
    type Socket: Socket;

    /// Accept a new connection.
    fn accept(&mut self) -> impl Future<Output = Result<Connection<Self::Socket>>>;

    /// Whether this is a one-shot listener whose [`accept`] resolves at most once.
    ///
    /// When this returns `true`, [`accept`] is contracted to resolve exactly once (handing over a
    /// single, already-connected socket) and to never resolve again afterwards.
    /// [`crate::Server::run`] relies on that contract: it stops polling [`accept`] after the
    /// first connection and returns cleanly once that connection closes and all in-flight work
    /// has drained, rather than blocking forever on an [`accept`] that can never resolve again.
    ///
    /// Infinite listeners (the default, `false`) keep [`accept`] live and the server running,
    /// waiting for more connections. This is the right behavior for the usual
    /// [`bind`](crate)-style listeners. One-shot listeners that hand over a socket received from a
    /// supervisor (systemd `Accept=yes`, `varlinkctl exec:`, such as [`ReadyListener`]) override
    /// this to `true`.
    ///
    /// [`accept`]: Listener::accept
    fn exit_when_done(&self) -> bool {
        false
    }
}

/// A listener that already has a socket.
///
/// This is useful for services that get spawned by their supervisor with a connected socket
/// already in hand: systemd `Accept=yes` activation, `varlinkctl exec:`, and similar one-shot
/// patterns.
///
/// A [`crate::Server`] built on a `ReadyListener` serves that single connection and then returns
/// from [`crate::Server::run`] once it closes, because [`Listener::exit_when_done`] is `true` for
/// this listener.
#[derive(Debug)]
pub struct ReadyListener<Sock: Socket> {
    socket: Option<Sock>,
}

impl<Sock> ReadyListener<Sock>
where
    Sock: Socket,
{
    /// Create a new listener from a socket.
    pub fn new(socket: Sock) -> Self {
        Self {
            socket: Some(socket),
        }
    }
}

impl<Sock> Listener for ReadyListener<Sock>
where
    Sock: Socket,
{
    type Socket = Sock;

    /// This implementation simply returns the contained socket.
    ///
    /// After the first call, it never returns on subsequent calls. [`crate::Server::run`] does
    /// not re-poll it: because [`Listener::exit_when_done`] is `true`, the server returns once
    /// the single connection has closed and any in-flight work has drained.
    async fn accept(&mut self) -> Result<Connection<Self::Socket>> {
        match self.socket.take() {
            Some(socket) => Ok(Connection::new(socket)),
            None => core::future::pending().await,
        }
    }

    fn exit_when_done(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use core::{future::poll_fn, task::Poll};

    use super::*;
    use crate::test_utils::mock_socket::MockSocket;

    #[tokio::test]
    async fn ready_listener() {
        let socket = MockSocket::with_responses(&["test"]);
        let mut listener = ReadyListener::new(socket);

        // First call returns a connection with properly split read/write halves.
        let conn = listener.accept().await.unwrap();
        let (read, write) = conn.split();
        assert_eq!(read.id(), write.id());

        // Second call should be pending forever.
        let accept_fut = listener.accept();
        futures_util::pin_mut!(accept_fut);
        let is_pending = poll_fn(|cx| Poll::Ready(accept_fut.as_mut().poll(cx).is_pending())).await;
        assert!(is_pending);
    }
}
