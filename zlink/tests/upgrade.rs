//! Test protocol upgrades end to end: a service upgrading connections to a protocol of its own, a
//! client taking over its side of them and another one that keeps speaking Varlink to the same
//! server.

#![cfg(all(feature = "service", feature = "proxy"))]

use std::{error::Error, fs::File, future::Future, os::fd::OwnedFd, path::Path};

use serde::{Deserialize, Serialize};
use tokio::select;
use zlink::{
    Listener, Server,
    connection::socket::{ReadHalf, Socket, WriteHalf},
    introspect,
    service::{TakeOver, Upgraded},
};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix::{Stream, bind, connect};
#[cfg(feature = "tokio")]
use zlink::tokio::unix::{Stream, bind, connect};

#[test_log::test(tokio::test)]
async fn upgrade() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("upgrade.sock");

    let server = Server::new(bind(&socket_path)?, Echo::default());
    select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut conn = connect(socket_path).await?;

    // The upgrade is refused, which leaves a Varlink connection.
    assert_eq!(conn.echo("").await?.unwrap_err(), EchoError::EmptyGreeting);

    // This time it's confirmed so the connection is taken apart to talk the new protocol.
    conn.echo("hello").await?.unwrap();
    let (read, write) = conn.split();
    let (mut read, mut write) = (read.into_read_half(), write.into_write_half());
    // The greeting is sent right after the reply, so it can be read along with it.
    assert_eq!(read_at_least(&mut read, 5).await, b"hello");

    // Meanwhile, the server keeps serving other clients. It's the future serving the upgraded
    // connection that wrote the greeting above, and the server only gets that future when the
    // connection is handed over. That's why the server is known to have done so at this point.
    let mut other_conn = connect(socket_path).await?;
    assert_eq!(other_conn.upgrades().await?.unwrap().count, 1);

    // Anything goes in the new protocol, including NUL bytes.
    write_all(&mut write, b"\0echo\0\x01").await;
    assert_eq!(read_at_least(&mut read, 7).await, b"\0echo\0\x01");

    Ok(())
}

// The data of the new protocol can be sent right after the reply confirming the upgrade, so it can
// be read along with the reply, and so can the FDs sent with it. They must reach the new protocol
// with that data, whatever the data is: here, the single NUL byte commonly sent along with FDs.
#[test_log::test(tokio::test)]
async fn upgrade_with_fds() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("upgrade-fds.sock");
    let mut listener = bind(&socket_path)?;
    let mut conn = connect(&socket_path).await?;
    let (_read, write) = listener.accept().await?.unwrap().split();
    let mut write = write.into_write_half();

    // Both are sent before the client reads anything, so Linux returns them in a single read.
    write_all(&mut write, b"{}\0").await;
    write
        .write(
            b"\0",
            &[File::open("/dev/null")?],
            #[cfg(target_os = "linux")]
            None,
        )
        .await?;

    // The proxy method drops the FDs it receives along with the reply.
    conn.echo("hello").await?.unwrap();
    let (read, _write) = conn.split();
    let mut read = read.into_read_half();
    // The read half returns the data that was read along with the reply, and the FDs that came with
    // it. Other kernels return the data sent along with FDs in a read that doesn't return the data
    // sent before it, so they're read from the socket then.
    let (mut data, mut fds) = (Vec::new(), Vec::new());
    while data.is_empty() {
        let mut buf = [0; 64];
        let mut result = read.read(&mut buf).await?;
        assert_ne!(result.bytes_read(), 0, "the connection was closed");
        data.extend_from_slice(&buf[..result.bytes_read()]);
        fds.extend(result.take_fds());
    }
    assert_eq!(data, b"\0");
    assert_eq!(fds.len(), 1);

    Ok(())
}

// Whether the future that `take_over` returns is `Send` is only known for a given socket type,
// since generic code can't require the futures of the socket to be. This is checked at compile
// time, for the socket type of the runtime in use: the function is never called, it only has to
// compile.
#[test]
fn take_over_future_is_send() {
    fn take_over(state: Greeting, socket: Upgraded<Stream>) -> impl Future<Output = ()> + Send {
        state.take_over(socket)
    }

    let _ = take_over;
}

// Reads from `read` until it has at least `len` bytes.
async fn read_at_least<Read>(read: &mut Read, len: usize) -> Vec<u8>
where
    Read: ReadHalf,
{
    let mut data = Vec::new();
    let mut buf = [0; 64];
    while data.len() < len {
        let len = read.read(&mut buf).await.unwrap().bytes_read();
        assert_ne!(len, 0, "the connection was closed");
        data.extend_from_slice(&buf[..len]);
    }

    data
}

async fn write_all<Write>(write: &mut Write, data: &[u8])
where
    Write: WriteHalf,
{
    write
        .write(
            data,
            &[] as &[OwnedFd],
            #[cfg(target_os = "linux")]
            None,
        )
        .await
        .unwrap();
}

#[derive(Default)]
struct Echo {
    upgrades: u32,
}

#[zlink::service(interface = "org.example.echo")]
impl Echo {
    /// Upgrades the connection to a protocol that greets and then echoes back everything it's
    /// sent.
    #[zlink(upgrade)]
    async fn echo(&mut self, greeting: String) -> Result<((), Greeting), EchoError> {
        if greeting.is_empty() {
            return Err(EchoError::EmptyGreeting);
        }
        self.upgrades += 1;

        // The greeting is what the protocol needs from the call.
        Ok(((), Greeting(greeting)))
    }

    /// The number of connections that were upgraded.
    async fn upgrades(&self) -> Upgrades {
        Upgrades {
            count: self.upgrades,
        }
    }
}

// Greets and then echoes back everything that comes over the socket, until the client closes it.
struct Greeting(String);

impl<Sock> TakeOver<Sock> for Greeting
where
    Sock: Socket,
{
    async fn take_over(self, socket: Upgraded<Sock>) {
        let (mut read, mut write) = socket.split();

        write_all(&mut write, self.0.as_bytes()).await;
        // The read half returns the data that was read along with the upgrade call before the rest.
        let mut buf = [0; 64];
        loop {
            let len = read.read(&mut buf).await.unwrap().bytes_read();
            if len == 0 {
                break;
            }
            write_all(&mut write, &buf[..len]).await;
        }
    }
}

#[zlink::proxy("org.example.echo")]
trait EchoProxy {
    #[zlink(upgrade)]
    async fn echo(&mut self, greeting: &str) -> zlink::Result<Result<(), EchoError>>;
    async fn upgrades(&mut self) -> zlink::Result<Result<Upgrades, EchoError>>;
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Upgrades {
    count: u32,
}

#[derive(Debug, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.echo")]
enum EchoError {
    EmptyGreeting,
}
