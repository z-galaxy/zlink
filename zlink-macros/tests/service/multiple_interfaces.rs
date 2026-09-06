//! Tests for service implementing multiple interfaces (each with its own error type).

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
        user_authenticated: false,
        items: vec!["apple".to_string(), "banana".to_string()],
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
struct MultiInterfaceService {
    user_authenticated: bool,
    items: Vec<String>,
}

#[zlink::service]
impl MultiInterfaceService {
    // ---- org.example.auth interface ----

    /// Authenticate with a password.
    #[zlink(interface = "org.example.auth")]
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

    // ---- org.example.storage interface ----

    /// Get the number of items (returns plain value, no Result).
    #[zlink(interface = "org.example.storage")]
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

// Regression test: interface names differing only in `.` vs `-` used to sanitize to the same
// generated identifier (both mapped to `_`), causing a duplicate const definition.
#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn colliding_interface_names() -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path).unwrap();
    let service = CollidingInterfaceService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(&socket_path).await?;

            let dot_desc = conn
                .get_interface_description("org.example.foo.bar")
                .await?
                .unwrap();
            let dot_interface = dot_desc.parse()?;
            let dot_methods: Vec<_> = dot_interface.methods().map(|m| m.name()).collect();
            assert_eq!(dot_methods.as_slice(), ["DotMethod"]);

            let hyphen_desc = conn
                .get_interface_description("org.example.foo-bar")
                .await?
                .unwrap();
            let hyphen_interface = hyphen_desc.parse()?;
            let hyphen_methods: Vec<_> = hyphen_interface.methods().map(|m| m.name()).collect();
            assert_eq!(hyphen_methods.as_slice(), ["HyphenMethod"]);

            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// A service exercising two interface names that would collide under a naive `.`/`-` -> `_`
/// identifier sanitization scheme.
struct CollidingInterfaceService;

#[zlink::service]
impl CollidingInterfaceService {
    #[zlink(interface = "org.example.foo.bar")]
    async fn dot_method(&self) {}

    #[zlink(interface = "org.example.foo-bar")]
    async fn hyphen_method(&self) {}
}
