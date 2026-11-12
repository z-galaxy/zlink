//! Tests for services made up of multiple interfaces (each with its own error type).

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self, Type},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn multiple_interfaces() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Setup the server with the multi-interface service.
    let listener = bind(&socket_path).unwrap();
    let service = MultiInterfaceService {
        auth: Auth {
            user_authenticated: false,
        },
        storage: Storage {
            items: vec!["apple".to_string(), "banana".to_string()],
        },
    };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // Test org.example.auth interface.

    // Test AuthError: not authenticated.
    let err = conn.get_user_info().await?.unwrap_err();
    assert_eq!(err, AuthError::NotAuthenticated);

    // Test successful authentication.
    conn.authenticate("secret".to_string()).await?.unwrap();

    // Test AuthError: invalid credentials.
    let err = conn.authenticate("wrong".to_string()).await?.unwrap_err();
    assert_eq!(
        err,
        AuthError::InvalidCredentials {
            reason: "wrong password".to_string()
        }
    );

    // After successful auth, get_user_info should work.
    let info = conn.get_user_info().await?.unwrap();
    assert_eq!(info.name, "TestUser");

    // Test org.example.storage interface.

    // Test method returning plain value (no error).
    let count = conn.item_count().await?.unwrap();
    assert_eq!(count.count, 2);

    // Test StorageError: item not found.
    let err = conn.get_item(10).await?.unwrap_err();
    assert_eq!(err, StorageError::NotFound);

    // Test successful item retrieval.
    let item = conn.get_item(0).await?.unwrap();
    assert_eq!(item.value, "apple");

    // Test StorageError: quota exceeded.
    let err = conn.add_item("cherry".to_string()).await?.unwrap_err();
    assert_eq!(err, StorageError::QuotaExceeded { limit: 2 });

    Ok(())
}

/// Response type for item count.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct ItemCount {
    count: usize,
}

/// Response type for user info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct UserInfo {
    name: String,
}

/// Response type for item retrieval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
struct Item {
    value: String,
}

/// Authentication-related errors (for org.example.auth interface).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.auth")]
enum AuthError {
    NotAuthenticated,
    InvalidCredentials { reason: String },
}

/// Storage-related errors (for org.example.storage interface).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.storage")]
enum StorageError {
    NotFound,
    QuotaExceeded { limit: usize },
}

/// A service that implements multiple interfaces.
#[derive(zlink::Service)]
struct MultiInterfaceService {
    auth: Auth,
    storage: Storage,
}

/// The org.example.auth interface.
struct Auth {
    user_authenticated: bool,
}

#[zlink::interface("org.example.auth")]
impl Auth {
    /// Authenticate with a password.
    async fn authenticate(&mut self, password: String) -> Result<(), AuthError> {
        if password == "secret" {
            self.user_authenticated = true;
            Ok(())
        } else {
            Err(AuthError::InvalidCredentials {
                reason: "wrong password".to_string(),
            })
        }
    }

    /// Get user info (requires authentication).
    async fn get_user_info(&self) -> Result<UserInfo, AuthError> {
        if self.user_authenticated {
            Ok(UserInfo {
                name: "TestUser".to_string(),
            })
        } else {
            Err(AuthError::NotAuthenticated)
        }
    }
}

/// The org.example.storage interface.
struct Storage {
    items: Vec<String>,
}

#[zlink::interface("org.example.storage")]
impl Storage {
    /// Get the number of items (returns plain value, no Result).
    async fn item_count(&self) -> ItemCount {
        ItemCount {
            count: self.items.len(),
        }
    }

    /// Get an item by index.
    async fn get_item(&self, index: usize) -> Result<Item, StorageError> {
        self.items
            .get(index)
            .map(|v| Item { value: v.clone() })
            .ok_or(StorageError::NotFound)
    }

    /// Add a new item.
    async fn add_item(&mut self, item: String) -> Result<(), StorageError> {
        if self.items.len() >= 2 {
            Err(StorageError::QuotaExceeded { limit: 2 })
        } else {
            self.items.push(item);
            Ok(())
        }
    }
}

/// Proxy for org.example.auth interface.
#[zlink::proxy("org.example.auth")]
trait AuthProxy {
    async fn authenticate(&mut self, password: String) -> zlink::Result<Result<(), AuthError>>;
    async fn get_user_info(&mut self) -> zlink::Result<Result<UserInfo, AuthError>>;
}

/// Proxy for org.example.storage interface.
#[zlink::proxy("org.example.storage")]
trait StorageProxy {
    async fn item_count(&mut self) -> zlink::Result<Result<ItemCount, StorageError>>;
    async fn get_item(&mut self, index: usize) -> zlink::Result<Result<Item, StorageError>>;
    async fn add_item(&mut self, item: String) -> zlink::Result<Result<(), StorageError>>;
}

// Regression test (https://github.com/z-galaxy/zlink/issues/315): interface names differing
// only in case used to fold to the same generated const identifier. Here they are the interfaces
// of one service.
#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn case_distinct_interface_names() -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("case-distinct.sock");

    let listener = bind(&socket_path).unwrap();
    let service = CaseDistinctService(LowerFoo, UpperFoo);
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(&socket_path).await?;

            // Verify GetInterfaceDescription resolves each interface to its own description.
            let lower_desc = conn
                .get_interface_description("org.example.foo".try_into().unwrap())
                .await?
                .unwrap();
            let lower_interface = lower_desc.parse()?;
            assert_eq!(lower_interface.name(), "org.example.foo");
            let lower_methods: Vec<_> = lower_interface.methods().map(|m| m.name()).collect();
            assert_eq!(lower_methods.as_slice(), ["LowerMethod"]);

            let upper_desc = conn
                .get_interface_description("org.example.Foo".try_into().unwrap())
                .await?
                .unwrap();
            let upper_interface = upper_desc.parse()?;
            assert_eq!(upper_interface.name(), "org.example.Foo");
            let upper_methods: Vec<_> = upper_interface.methods().map(|m| m.name()).collect();
            assert_eq!(upper_methods.as_slice(), ["UpperMethod"]);

            // Verify method calls are also routed to the correct interface, not just
            // GetInterfaceDescription.
            conn.lower_method().await?.unwrap();
            conn.upper_method().await?.unwrap();

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A service exercising two interface names differing only in case.
#[derive(zlink::Service)]
struct CaseDistinctService(LowerFoo, UpperFoo);

struct LowerFoo;

#[zlink::interface("org.example.foo")]
impl LowerFoo {
    async fn lower_method(&self) {}
}

struct UpperFoo;

#[zlink::interface("org.example.Foo")]
impl UpperFoo {
    async fn upper_method(&self) {}
}

/// Error type for org.example.foo (never actually returned; needed for the proxy signature).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.foo")]
enum LowerError {}

/// Error type for org.example.Foo (never actually returned; needed for the proxy signature).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.Foo")]
enum UpperError {}

/// Proxy for org.example.foo interface.
#[zlink::proxy("org.example.foo")]
trait LowerProxy {
    async fn lower_method(&mut self) -> zlink::Result<Result<(), LowerError>>;
}

/// Proxy for org.example.Foo interface.
#[zlink::proxy("org.example.Foo")]
trait UpperProxy {
    async fn upper_method(&mut self) -> zlink::Result<Result<(), UpperError>>;
}

// Regression test (https://github.com/z-galaxy/zlink/issues/315): interface type names differing
// only in case used to collapse to the same generated interface constant identifier. The names of
// their services differ only in case too.
#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn case_distinct_service_type_names() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path_a = dir.path().join("case-distinct-a.sock");
    let socket_path_b = dir.path().join("case-distinct-b.sock");

    let listener_a = bind(&socket_path_a).unwrap();
    let server_a = Server::new(listener_a, FooBarService(FooBar));
    let listener_b = bind(&socket_path_b).unwrap();
    let server_b = Server::new(listener_b, FoobarService(Foobar));

    tokio::select! {
        res = server_a.run() => res?,
        res = server_b.run() => res?,
        res = async {
            let mut conn_a = connect(&socket_path_a).await?;
            conn_a.method_a().await?.unwrap();

            let mut conn_b = connect(&socket_path_b).await?;
            conn_b.method_b().await?.unwrap();

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A service type whose name differs from `FoobarService` only in case.
#[derive(zlink::Service)]
struct FooBarService(FooBar);

/// An interface type whose name differs from `Foobar` only in case.
struct FooBar;

#[zlink::interface("org.example.a")]
impl FooBar {
    async fn method_a(&self) {}
}

/// A service type whose name differs from `FooBarService` only in case.
#[derive(zlink::Service)]
struct FoobarService(Foobar);

/// An interface type whose name differs from `FooBar` only in case.
struct Foobar;

#[zlink::interface("org.example.b")]
impl Foobar {
    async fn method_b(&self) {}
}

/// Error type for org.example.a (never actually returned; needed for the proxy signature).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.a")]
enum AError {}

/// Error type for org.example.b (never actually returned; needed for the proxy signature).
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.b")]
enum BError {}

/// Proxy for org.example.a interface.
#[zlink::proxy("org.example.a")]
trait AProxy {
    async fn method_a(&mut self) -> zlink::Result<Result<(), AError>>;
}

/// Proxy for org.example.b interface.
#[zlink::proxy("org.example.b")]
trait BProxy {
    async fn method_b(&mut self) -> zlink::Result<Result<(), BError>>;
}
