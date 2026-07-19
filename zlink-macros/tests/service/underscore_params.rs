//! Tests for underscore-prefixed (unused) method parameters.
//!
//! Underscore-prefixed names (the Rust convention for unused parameters) are not valid Varlink
//! field names, so the service macro requires an explicit `#[zlink(rename = "...")]` wire name
//! for them (rejecting them with a compile error otherwise). This test verifies that renamed
//! underscore-prefixed parameters work end to end: dispatch accepts the wire name and the
//! introspection data advertises it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self, CustomType},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn underscore_prefixed_params() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Set up the server and run it concurrently with the client.
    let listener = bind(&socket_path).unwrap();
    let service = Launcher;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // Call the methods using the renamed parameter names, exactly as advertised by the
    // introspection data.
    let reply = conn
        .uninstall("org.example.Foo.desktop", HashMap::new())
        .await?
        .unwrap();
    assert_eq!(reply.desktop_file_id, "org.example.Foo.desktop");

    // Also exercise the connection-param code path (the method body is inlined by the macro).
    let reply = conn
        .launch("org.example.Bar.desktop", HashMap::new())
        .await?
        .unwrap();
    assert_eq!(reply.desktop_file_id, "org.example.Bar.desktop");

    // The introspection data must advertise the renamed names.
    {
        use zlink::varlink_service::Proxy as VarlinkProxy;

        let desc = conn
            .get_interface_description("org.example.launcher")
            .await?
            .unwrap();
        let interface = desc.parse()?;
        for name in ["Uninstall", "Launch"] {
            let method = interface
                .methods()
                .find(|m| m.name() == name)
                .expect("method not found in introspection data");
            let input_names: Vec<_> = method.inputs().map(|f| f.name()).collect();
            assert_eq!(input_names.as_slice(), &["desktop_file_id", "options"]);
        }
    }

    Ok(())
}

// Response type echoing back the desktop file ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Launched {
    desktop_file_id: String,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.launcher")]
enum LauncherError {
    NotFound,
}

struct Launcher;

#[zlink::service(types = [Launched])]
impl<Sock> Launcher {
    /// Uninstall a launcher, ignoring the options.
    #[zlink(interface = "org.example.launcher")]
    async fn uninstall(
        &mut self,
        desktop_file_id: String,
        #[zlink(rename = "options")] _options: HashMap<String, String>,
    ) -> Result<Launched, LauncherError> {
        Ok(Launched { desktop_file_id })
    }

    /// Launch an app, ignoring the options.
    async fn launch(
        &mut self,
        desktop_file_id: String,
        #[zlink(rename = "options")] _options: HashMap<String, String>,
        #[zlink(connection)] _conn: &mut zlink::Connection<Sock>,
    ) -> Result<Launched, LauncherError> {
        Ok(Launched { desktop_file_id })
    }
}

#[zlink::proxy("org.example.launcher")]
trait LauncherProxy {
    async fn uninstall(
        &mut self,
        desktop_file_id: &str,
        options: HashMap<String, String>,
    ) -> zlink::Result<Result<Launched, LauncherError>>;
    async fn launch(
        &mut self,
        desktop_file_id: &str,
        options: HashMap<String, String>,
    ) -> zlink::Result<Result<Launched, LauncherError>>;
}
