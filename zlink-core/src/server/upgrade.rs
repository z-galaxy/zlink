//! Contains the API to take over the connections upgraded to another protocol.

use core::future::Future;

use crate::{
    Connection,
    connection::{Rewind, Socket},
};

/// Taking over the connections upgraded to another protocol.
///
/// A method call can ask for its connection to be upgraded to another protocol (see
/// [`Call::upgrade`]). A service confirms the upgrade with a reply that comes with a value of this
/// trait (see [`MethodReply::Upgrade`]), which carries what the new protocol needs from the call.
/// The server sends the reply and stops serving the connection, handing its socket over to that
/// value instead. An error reply refuses the upgrade and the connection keeps being served as
/// usual. If the reply can't be sent, the connection and the value are dropped instead.
///
/// The server polls the future that [`TakeOver::take_over`] returns until it completes, along with
/// its other connections and while handling method calls, which can thus wait for it, so it doesn't
/// need a task of its own. The future can't borrow the service, which the server keeps calling
/// into. It's `Send` whenever what it holds and awaits is, e.g. the futures of the socket.
///
/// `()` closes the connection: a service that doesn't support upgrades has no other protocol to
/// speak. Such a service should refuse the calls asking for an upgrade with an error reply instead.
///
/// [`Call::upgrade`]: crate::Call::upgrade
/// [`MethodReply::Upgrade`]: super::service::MethodReply::Upgrade
pub trait TakeOver<Sock>
where
    Sock: Socket,
{
    /// Take over `socket`, the socket of the upgraded connection.
    ///
    /// The returned future speaks the new protocol over the socket.
    fn take_over(self, socket: Upgraded<Sock>) -> impl Future<Output = ()>;
}

impl<Sock> TakeOver<Sock> for ()
where
    Sock: Socket,
{
    fn take_over(self, socket: Upgraded<Sock>) -> impl Future<Output = ()> {
        warn!(
            "A client upgraded but the service doesn't take over upgraded connections, closing it"
        );
        drop(socket);

        core::future::ready(())
    }
}

/// The socket of a connection that was upgraded to another protocol.
///
/// Once the server has sent the reply confirming the upgrade, it hands the socket over to the state
/// that came with the reply (see [`TakeOver`]). The client doesn't have to wait for that reply
/// before it starts sending the data of the new protocol, so the server may have read some of that
/// data along with the method call that asked for the upgrade. The read half of this socket returns
/// that data first (see [`Rewind`]).
#[derive(Debug)]
pub struct Upgraded<Sock>
where
    Sock: Socket,
{
    read: Rewind<Sock::ReadHalf>,
    write: Sock::WriteHalf,
}

impl<Sock> Upgraded<Sock>
where
    Sock: Socket,
{
    /// The socket of `conn`, whose upgrade was confirmed.
    pub(super) fn new(conn: Connection<Sock>) -> Self {
        let (read, write) = conn.split();

        Self {
            read: read.into_read_half(),
            write: write.into_write_half(),
        }
    }
}

impl<Sock> Socket for Upgraded<Sock>
where
    Sock: Socket,
{
    type ReadHalf = Rewind<Sock::ReadHalf>;
    type WriteHalf = Sock::WriteHalf;

    const CAN_TRANSFER_FDS: bool = Sock::CAN_TRANSFER_FDS;

    fn split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        (self.read, self.write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        connection::socket::{ReadHalf, WriteHalf},
        test_utils::mock_socket::MockSocket,
    };

    // A takeover's future is `Send` whenever the futures of the socket are, which only a given
    // socket type tells: generic code can't require them to be.
    #[test]
    fn take_over_future_can_be_send() {
        fn take_over(socket: Upgraded<MockSocket>) -> impl Future<Output = ()> + Send {
            Echo.take_over(socket)
        }

        let _ = take_over;
    }

    // Echoes back the first data it reads.
    struct Echo;

    impl<Sock> TakeOver<Sock> for Echo
    where
        Sock: Socket,
    {
        async fn take_over(self, socket: Upgraded<Sock>) {
            let (mut read, mut write) = socket.split();
            let mut buf = [0; 16];
            let Ok(result) = read.read(&mut buf).await else {
                return;
            };
            #[cfg(feature = "std")]
            let fds: &[std::os::fd::OwnedFd] = &[];
            let _ = write
                .write(
                    &buf[..result.bytes_read()],
                    #[cfg(feature = "std")]
                    fds,
                    #[cfg(all(feature = "std", target_os = "linux"))]
                    None,
                )
                .await;
        }
    }
}
