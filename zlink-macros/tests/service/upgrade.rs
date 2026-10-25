//! Tests for protocol upgrades with services.

use zlink::{
    Call, Server,
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test)]
async fn regular_method_refuses_upgrade() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path)?;
    let server = Server::new(listener, Upgrader);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // A successful reply would confirm the upgrade, so it's refused without calling the method.
    assert_upgrade_refused(conn.ping_with_upgrade().await.unwrap_err());
    // That includes the methods of the `org.varlink.service` interface.
    let call =
        Call::new(serde_json::json!({"method": "org.varlink.service.GetInfo"})).set_upgrade(true);
    conn.send_call(&call, vec![]).await?;
    let err = conn
        .receive_reply::<serde_json::Value, UpgradeError>()
        .await
        .unwrap_err();
    assert_upgrade_refused(err);

    // The connection is still serving Varlink.
    conn.ping().await?.unwrap();

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

struct Upgrader;

#[zlink::service(interface = "org.example.upgrade")]
impl Upgrader {
    async fn ping(&self) {}
}

#[zlink::proxy("org.example.upgrade")]
trait UpgraderProxy {
    async fn ping(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
    /// `Ping`, asking for an upgrade.
    #[zlink(rename = "Ping", upgrade)]
    async fn ping_with_upgrade(&mut self) -> zlink::Result<Result<(), UpgradeError>>;
}

#[derive(Debug, zlink::ReplyError)]
#[zlink(interface = "org.example.upgrade")]
enum UpgradeError {
    Unexpected,
}
