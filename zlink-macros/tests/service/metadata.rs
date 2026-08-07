//! Tests for service with metadata attributes.

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
    let service = MetadataService;
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
                ["org.example.metadata", "org.varlink.service"],
                "Unexpected interfaces"
            );

            // Test GetInterfaceDescription - verify both methods are exposed.
            // This tests that the macro-level interface attribute applies to all methods.
            let desc = conn.get_interface_description("org.example.metadata").await?.unwrap();
            let interface = desc.parse()?;
            let method_names: Vec<_> = interface.methods().map(|m| m.name()).collect();
            assert_eq!(
                method_names.as_slice(),
                ["Ping", "Pong"],
                "Expected both Ping and Pong methods from macro-level interface attribute"
            );

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A simple service with metadata attributes.
/// This is `pub` to test that the generated types work with public service structs (issue #216).
pub struct MetadataService;

// Test the interface attribute at the macro level instead of on each method.
#[zlink::service(
    interface = "org.example.metadata",
    vendor = "Test Vendor",
    product = "Test Product",
    version = env!("CARGO_PKG_VERSION"),
    url = env!("CARGO_PKG_REPOSITORY")
)]
impl MetadataService {
    async fn ping(&self) {}

    // Add another method to verify all methods get the interface.
    async fn pong(&self) {}
}

#[allow(dead_code)]
struct HyphenService;

// Test if `service` macro can handle an interface with `-` in the name.
#[zlink::service(
    interface = "org.example.metadata-hyphen",
    vendor = "Test Vendor",
    product = "Test Product",
    version = env!("CARGO_PKG_VERSION"),
    url = env!("CARGO_PKG_REPOSITORY")
)]
impl HyphenService {
    #[allow(dead_code)]
    async fn hyphenhyphen(&self) {}
}
