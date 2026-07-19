//! Tests for raw identifiers (`r#type`) as method and parameter names.
//!
//! `r#` is Rust syntax for using a keyword as an identifier; it is never part of the name, and
//! Varlink cannot express it at all (`#` starts a comment there). serde strips it when
//! (de)serializing, so the IDL must strip it too — otherwise the interface description advertises
//! names the method can never accept.

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self, CustomType},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn raw_ident_method_and_param() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Set up the server and run it concurrently with the client.
    let listener = bind(&socket_path).unwrap();
    let service = Probe;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // A successful call proves the wire side: the proxy sends `Type` with a `type` parameter, and
    // dispatch accepts both.
    let reply = conn.probe_type("frobnicate").await?.unwrap();
    assert_eq!(reply.echoed, "frobnicate");

    // The introspection data must advertise exactly those names, or a client trusting our own IDL
    // would send `r#type` and get `missing field type`.
    {
        use zlink::varlink_service::Proxy as VarlinkProxy;

        let desc = conn
            .get_interface_description("org.example.probe")
            .await?
            .unwrap();
        let interface = desc.parse()?;
        let method = interface
            .methods()
            .find(|m| m.name() == "Type")
            .expect("raw method ident must be advertised unraw'd");
        let input_names: Vec<_> = method.inputs().map(|f| f.name()).collect();
        assert_eq!(input_names.as_slice(), &["type"]);
    }

    Ok(())
}

// Response type echoing back the raw-named parameter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Probed {
    echoed: String,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.probe")]
enum ProbeError {
    Failed,
}

struct Probe;

#[zlink::service(interface = "org.example.probe", types = [Probed])]
impl Probe {
    /// Echo back the raw-named parameter.
    async fn r#type(&mut self, r#type: String) -> Result<Probed, ProbeError> {
        Ok(Probed { echoed: r#type })
    }
}

// The proxy method is deliberately not itself raw-named: a raw ident on the proxy side is a
// separate concern, so this reaches the service's `Type` method by an explicit rename.
#[zlink::proxy("org.example.probe")]
trait ProbeProxy {
    #[zlink(rename = "Type")]
    async fn probe_type(&mut self, r#type: &str) -> zlink::Result<Result<Probed, ProbeError>>;
}
