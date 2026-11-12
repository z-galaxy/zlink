//! Tests for the metadata of a service, which the struct that derives `Service` gives.

use zlink::{
    Server,
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn with_metadata() -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Setup the server with a service that has metadata.
    let listener = bind(&socket_path).unwrap();
    let service = MetadataService(Metadata);
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(&socket_path).await?;

            // Test GetInfo - should return service metadata.
            let info = conn.get_info().await?.unwrap();
            assert_eq!(info.vendor, "Test Vendor");
            assert_eq!(info.product, "Test Product");
            assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
            assert_eq!(info.url, env!("CARGO_PKG_REPOSITORY"));
            let interfaces: Vec<&str> = info.interfaces.iter().map(|s| s.as_ref()).collect();
            assert_eq!(
                interfaces.as_slice(),
                ["org.example.metadata-hyphen", "org.varlink.service"],
                "Unexpected interfaces"
            );

            // Test GetInterfaceDescription - verify both methods are exposed.
            // This tests that the interface of the macro applies to all methods.
            let desc = conn.get_interface_description("org.example.metadata-hyphen".try_into().unwrap()).await?.unwrap();
            let interface = desc.parse()?;
            let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
            assert_eq!(
                method_names.as_slice(),
                ["Ping", "Pong"],
                "Expected both Ping and Pong methods of the interface"
            );

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A simple service with metadata attributes.
/// This is `pub` to test that the generated types work with public service structs (issue #216).
#[derive(zlink::Service)]
#[zlink(
    vendor = "Test Vendor",
    product = "Test Product",
    version = env!("CARGO_PKG_VERSION"),
    url = env!("CARGO_PKG_REPOSITORY")
)]
pub struct MetadataService(pub Metadata);

/// The interface of the service, `pub` for the same reason.
pub struct Metadata;

// The interface is given by key here, rather than as the only argument.
// The interface name also exercises a `-` character in a segment.
#[zlink::interface(interface = "org.example.metadata-hyphen")]
impl Metadata {
    async fn ping(&self) {}

    // Add another method to verify all methods get the interface.
    async fn pong(&self) {}
}
