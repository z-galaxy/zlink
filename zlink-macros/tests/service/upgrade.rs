//! Tests for protocol upgrades with services.

use serde::{Deserialize, Serialize};
use std::{error::Error, future::Future, num::ParseIntError, os::fd::OwnedFd, path::PathBuf};
use zlink::{
    Call, Server,
    connection::socket::{FetchPeerCredentials, ReadHalf, Socket, WriteHalf},
    introspect,
    service::{TakeOver, Upgraded},
    tokio::unix::{Stream, bind, connect},
    varlink_service::Proxy as _,
};

#[test_log::test(tokio::test)]
async fn regular_method_refuses_upgrade() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut conn = connect(&socket_path).await?;

        // A successful reply would confirm the upgrade, so it's refused without calling the
        // method.
        assert_upgrade_refused(conn.ping_with_upgrade().await.unwrap_err());
        // That includes the methods of the `org.varlink.service` interface.
        let call = Call::new(serde_json::json!({"method": "org.varlink.service.GetInfo"}))
            .set_upgrade(true);
        conn.send_call(&call, vec![]).await?;
        let err = conn
            .receive_reply::<serde_json::Value, UpgradeError>()
            .await
            .unwrap_err();
        assert_upgrade_refused(err);

        // The connection is still serving Varlink.
        conn.ping().await?.unwrap();

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_method_requires_upgrade() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut conn = connect(&socket_path).await?;

        // Nothing would be upgraded by the reply, so the method isn't called.
        let err = conn.echo_without_upgrade().await.unwrap_err();
        let zlink::Error::VarlinkService(err) = err else {
            panic!("unexpected error: {err}");
        };
        assert_eq!(err.inner(), &zlink::varlink_service::Error::ExpectedUpgrade);
        conn.ping().await?.unwrap();

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_method() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let (mut read, mut write) = connect(&socket_path).await?.split();

        let call =
            Call::new(serde_json::json!({"method": "org.example.upgrade.Echo"})).set_upgrade(true);
        write.send_call(&call, vec![]).await?;
        // The data of the new protocol is sent right away, before the reply is in. It starts with
        // a NUL byte, which the service must not mistake for the end of a message.
        write_all(&mut write.into_write_half(), b"\0hi").await;

        let (reply, _fds) = read.receive_reply::<(), UpgradeError>().await?;
        reply.unwrap();

        // The service echoes the data back, which it got from the read half of the socket it took
        // over.
        assert_eq!(read_at_least(&mut read.into_read_half(), 3).await, b"\0hi");

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_method_can_refuse() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut conn = connect(&socket_path).await?;

        // An early return and `?`, with the conversion of the error, refuse the upgrade like in
        // any other method.
        assert_eq!(
            conn.fill("", 7).await?.unwrap_err(),
            UpgradeError::MissingSize
        );
        assert_eq!(
            conn.fill("many", 7).await?.unwrap_err(),
            UpgradeError::InvalidSize
        );

        // The connection is still serving Varlink.
        conn.ping().await?.unwrap();

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_methods_take_over_differently() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        // `Fill` sends the bytes it was asked for and closes the connection.
        let mut conn = connect(&socket_path).await?;
        assert_eq!(conn.fill("3", 7).await?.unwrap().size, 3);
        let (read, _write) = conn.split();
        assert_eq!(read_to_end(&mut read.into_read_half()).await, [7, 7, 7]);

        // `Echo` echoes back what it's sent.
        let mut conn = connect(&socket_path).await?;
        conn.echo().await?.unwrap();
        let (read, write) = conn.split();
        let (mut read, mut write) = (read.into_read_half(), write.into_write_half());
        write_all(&mut write, b"hello").await;
        assert_eq!(read_at_least(&mut read, 5).await, b"hello");

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_method_with_connection() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut conn = connect(&socket_path).await?;

        // The method can check the connection before it confirms the upgrade.
        conn.echo_with_credentials().await?.unwrap();
        let (read, write) = conn.split();
        let (mut read, mut write) = (read.into_read_half(), write.into_write_half());
        write_all(&mut write, b"hi").await;
        assert_eq!(read_at_least(&mut read, 2).await, b"hi");

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_method_changes_service() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut first = connect(&socket_path).await?;
        assert_eq!(first.count().await?.unwrap().upgrades, 1);
        let (read, write) = first.split();
        let (mut read, mut write) = (read.into_read_half(), write.into_write_half());

        // The server keeps serving the other connections, which see the change, while it serves
        // the upgraded one.
        let mut second = connect(&socket_path).await?;
        assert_eq!(second.count().await?.unwrap().upgrades, 2);
        write_all(&mut write, b"hi").await;
        assert_eq!(read_at_least(&mut read, 2).await, b"hi");

        Ok(())
    })
    .await
}

#[test_log::test(tokio::test)]
async fn upgrade_methods_can_be_called_directly() {
    let mut upgrader = Upgrader { upgrades: 0 };

    // The state is returned along with the reply.
    let (filling, state) = upgrader.fill("3".to_owned(), 7).await.unwrap();
    assert_eq!(filling.size, 3);
    assert_eq!(state, Protocol::Fill { size: 3, byte: 7 });
    assert_eq!(
        upgrader.fill(String::new(), 7).await.unwrap_err(),
        UpgradeError::MissingSize
    );

    let (count, state) = upgrader.count().await;
    assert_eq!(count.upgrades, 1);
    assert_eq!(state, Protocol::Echo);
}

#[test_log::test(tokio::test)]
async fn upgrade_methods_are_described_by_their_replies() -> Result<(), Box<dyn Error>> {
    run(|socket_path| async move {
        let mut conn = connect(&socket_path).await?;

        let desc = conn
            .get_interface_description("org.example.upgrade".try_into()?)
            .await?
            .unwrap();
        let interface = desc.parse()?;

        // The states aren't part of the interface, only the replies are.
        let fill = interface.methods().find(|m| m.name() == "Fill").unwrap();
        let outputs: Vec<_> = fill.outputs().map(|f| f.name()).collect();
        assert_eq!(outputs, ["size"]);
        let echo = interface.methods().find(|m| m.name() == "Echo").unwrap();
        assert!(echo.has_no_outputs());

        // Like systemd, the service marks the methods requiring the `upgrade` flag in a comment
        // right before them.
        let raw = desc.as_raw().unwrap();
        assert!(raw.contains("# [Requires 'upgrade' flag]\nmethod Echo("));
        assert_eq!(raw.matches("[Requires 'upgrade' flag]").count(), 4);
        let ping = interface.methods().find(|m| m.name() == "Ping").unwrap();
        assert_eq!(ping.comments().count(), 0);

        Ok(())
    })
    .await
}

// Whether the future that `take_over` returns is `Send` is only known for a given socket type,
// since generic code can't require the futures of the socket to be. This is checked at compile
// time, for the socket type of the tokio runtime: the function is never called, it only has to
// compile.
#[test]
fn take_over_future_is_send() {
    fn take_over(state: Protocol, socket: Upgraded<Stream>) -> impl Future<Output = ()> + Send {
        state.take_over(socket)
    }

    let _ = take_over;
}

// Serves the service and runs `client` against it.
async fn run<Client, Output>(client: Client) -> Result<(), Box<dyn Error>>
where
    Client: FnOnce(PathBuf) -> Output,
    Output: Future<Output = Result<(), Box<dyn Error>>>,
{
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");
    let server = Server::new(bind(&socket_path)?, Upgrader { upgrades: 0 });

    tokio::select! {
        res = server.run() => res?,
        res = client(socket_path) => res?,
    }

    Ok(())
}

fn assert_upgrade_refused(err: zlink::Error) {
    let zlink::Error::VarlinkService(err) = err else {
        panic!("unexpected error: {err}");
    };
    assert_eq!(
        err.inner(),
        &zlink::varlink_service::Error::InvalidParameter {
            parameter: "upgrade".into(),
        },
    );
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

// Reads from `read` until the connection is closed.
async fn read_to_end<Read>(read: &mut Read) -> Vec<u8>
where
    Read: ReadHalf,
{
    let mut data = Vec::new();
    let mut buf = [0; 64];
    loop {
        let len = read.read(&mut buf).await.unwrap().bytes_read();
        if len == 0 {
            return data;
        }
        data.extend_from_slice(&buf[..len]);
    }
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

struct Upgrader {
    upgrades: u32,
}

#[zlink::service(interface = "org.example.upgrade")]
impl<Sock> Upgrader
where
    Sock::ReadHalf: FetchPeerCredentials,
{
    async fn ping(&self) {}

    /// Upgrades the connection to a protocol that echoes back everything it's sent.
    #[zlink(upgrade)]
    async fn echo(&self) -> ((), Protocol) {
        ((), Protocol::Echo)
    }

    /// `Echo`, for the peers whose credentials can be fetched.
    #[zlink(upgrade)]
    async fn echo_with_credentials(
        &self,
        #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
    ) -> Result<((), Protocol), UpgradeError> {
        conn.peer_credentials()
            .await
            .map_err(|_| UpgradeError::NoCredentials)?;

        Ok(((), Protocol::Echo))
    }

    /// `Echo`, counting the upgrades.
    #[zlink(upgrade)]
    async fn count(&mut self) -> (Count, CountState) {
        self.upgrades += 1;

        (
            Count {
                upgrades: self.upgrades,
            },
            Protocol::Echo,
        )
    }

    /// Upgrades the connection to a protocol that sends `size` bytes and then closes it.
    #[zlink(upgrade)]
    async fn fill(&self, size: String, byte: u8) -> Result<(Filling, Protocol), UpgradeError> {
        if size.is_empty() {
            return Err(UpgradeError::MissingSize);
        }
        let size: u32 = size.parse()?;

        Ok((Filling { size }, Protocol::Fill { size, byte }))
    }
}

// The state that `count` returns, under another name: the upgrade methods only have to return the
// same type of state.
type CountState = Protocol;

// The protocols that the connections are upgraded to, which the upgrade methods share, along with
// what the protocols need from the calls.
#[derive(Debug, PartialEq)]
enum Protocol {
    // Echoes back everything it's sent.
    Echo,
    // Sends `size` bytes of `byte` and then closes the connection.
    Fill { size: u32, byte: u8 },
}

impl<Sock> TakeOver<Sock> for Protocol
where
    Sock: Socket,
{
    async fn take_over(self, socket: Upgraded<Sock>) {
        match self {
            Self::Echo => serve_echo(socket).await,
            Self::Fill { size, byte } => {
                let (_read, mut write) = socket.split();
                write_all(&mut write, &vec![byte; size as usize]).await;
            }
        }
    }
}

// Echoes back everything that comes over `socket`, until the client closes it.
async fn serve_echo<Sock>(socket: Upgraded<Sock>)
where
    Sock: Socket,
{
    let (mut read, mut write) = socket.split();
    let mut buf = [0; 64];
    loop {
        let len = read.read(&mut buf).await.unwrap().bytes_read();
        if len == 0 {
            break;
        }
        write_all(&mut write, &buf[..len]).await;
    }
}

#[zlink::proxy("org.example.upgrade")]
trait UpgraderProxy {
    async fn ping(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    /// `Ping`, asking for an upgrade.
    #[zlink(rename = "Ping", upgrade)]
    async fn ping_with_upgrade(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    /// `Echo`, without asking for an upgrade.
    #[zlink(rename = "Echo")]
    async fn echo_without_upgrade(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    #[zlink(upgrade)]
    async fn echo(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    #[zlink(upgrade)]
    async fn echo_with_credentials(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    #[zlink(upgrade)]
    async fn count(&mut self) -> zlink::Result<Result<Count, UpgradeError>>;
    #[zlink(upgrade)]
    async fn fill(&mut self, size: &str, byte: u8) -> zlink::Result<Result<Filling, UpgradeError>>;
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Count {
    upgrades: u32,
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Filling {
    size: u32,
}

#[derive(Debug, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.upgrade")]
enum UpgradeError {
    Unexpected,
    NoCredentials,
    MissingSize,
    InvalidSize,
}

impl From<ParseIntError> for UpgradeError {
    fn from(_: ParseIntError) -> Self {
        Self::InvalidSize
    }
}
