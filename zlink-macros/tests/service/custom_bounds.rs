//! Tests for custom socket bounds via user-provided generics.

use super::basic::Balance;
use zlink::{
    Server,
    connection::socket::FetchPeerCredentials,
    introspect::{self},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn with_custom_socket_bounds() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Setup the server with the credential-checking service.
    let listener = bind(&socket_path).unwrap();
    let service = CredentialCheckingService { balance: 1000 };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = async {
            let mut conn = connect(&socket_path).await?;
            // Test that the service works and can check credentials.
            // The multiplier parameter is used AFTER an await point in the service method,
            // which tests the fix for issue #216 (parameters with #[zlink(connection)]).
            let reply = conn.get_balance_with_creds(2).await?.unwrap();
            assert_eq!(reply.amount, 2000); // 1000 * 2
            Ok::<(), Box<dyn std::error::Error>>(())
        } => res?,
    }

    Ok(())
}

/// Error type for credential-checking service.
#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.creds")]
enum CredsError {
    CredentialCheckFailed,
}

/// A service that uses custom socket bounds to check peer credentials.
struct CredentialCheckingService {
    balance: i64,
}

// Service implementation with custom socket bounds using user-provided generics.
// The first type parameter (Sock) is used as the socket type. The Socket bound is added
// automatically, so we only specify additional bounds.
#[zlink::service(types = [Balance])]
impl<Sock> CredentialCheckingService
where
    Sock::ReadHalf: FetchPeerCredentials,
{
    #[zlink(interface = "org.example.creds")]
    async fn get_balance_with_creds(
        &self,
        multiplier: i64,
        #[zlink(connection)] conn: &mut zlink::Connection<Sock>,
    ) -> Result<Balance, CredsError> {
        // Actually check credentials using the connection parameter.
        let creds = conn.peer_credentials().await.unwrap();
        // Verify we got valid credentials (check that unix_user_id is returned).
        let _ = creds.unix_user_id();
        // Use multiplier AFTER the await point - this tests the fix for issue #216.
        // Without `async move`, the multiplier would be captured by reference and not live long
        // enough.
        Ok(Balance {
            amount: self.balance * multiplier,
        })
    }
}

#[zlink::proxy("org.example.creds")]
trait CredsProxy {
    async fn get_balance_with_creds(
        &mut self,
        multiplier: i64,
    ) -> zlink::Result<Result<Balance, CredsError>>;
}
