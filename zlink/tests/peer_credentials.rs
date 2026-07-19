//! Integration test for peer credentials functionality.

#![cfg(feature = "server")]

#[path = "creds-utils.rs"]
mod creds_utils;

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use tempfile::TempDir;
use zlink::Listener;

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix;
#[cfg(feature = "tokio")]
use zlink::tokio::unix;

#[tokio::test]
async fn peer_credentials_unix_socket() {
    // Create a temporary directory for the socket.
    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("test_creds.sock");

    // Create a listener.
    let mut listener = unix::bind(&socket_path).unwrap();

    // Connect from a client.
    let socket_path_clone = socket_path.clone();
    let connect_task = tokio::spawn(async move {
        tokio::net::UnixStream::connect(&socket_path_clone)
            .await
            .unwrap()
    });

    let mut connection = listener.accept().await.unwrap().unwrap();

    // Get peer credentials.
    let creds = connection.peer_credentials().await.unwrap();

    // Verify all credentials match current process using shared utilities.
    creds_utils::verify_credentials(creds).expect("Credentials should match current process");

    // Save values for comparison.
    let uid1 = creds.unix_user_id();
    let pid1 = creds.process_id();
    let gid1 = creds.unix_primary_group_id();
    #[cfg(target_os = "linux")]
    let pidfd1 = creds.process_fd().map(|fd| fd.as_raw_fd());
    #[cfg(target_os = "linux")]
    let gids1 = creds.unix_supplementary_group_ids().to_owned();

    // Verify caching works - calling again should return the same values.
    let creds2 = connection.peer_credentials().await.unwrap();
    assert_eq!(uid1, creds2.unix_user_id(), "Cached UID should match");
    assert_eq!(pid1, creds2.process_id(), "Cached PID should match");
    assert_eq!(
        gid1,
        creds2.unix_primary_group_id(),
        "Cached GID should match"
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        pidfd1,
        creds2.process_fd().map(|fd| fd.as_raw_fd()),
        "Cached pidfd should match"
    );
    #[cfg(target_os = "linux")]
    assert_eq!(
        gids1,
        creds2.unix_supplementary_group_ids(),
        "Cached supplementary GIDs should match"
    );

    let _stream = connect_task.await.unwrap();
}

/// Verify that explicitly-set credentials are sent via `SCM_CREDENTIALS` and received on the peer.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn passed_credentials_over_unix_socket() {
    use serde::{Deserialize, Serialize};
    use serde_prefix_all::prefix_all;
    use zlink::{Call, connection::PassedCredentials};

    #[prefix_all("org.example.")]
    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "method", content = "parameters")]
    enum Method {
        Ping,
    }

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("passed_creds.sock");

    let mut listener = unix::bind(&socket_path).unwrap();

    let socket_path_clone = socket_path.clone();
    let client_task = tokio::spawn(async move {
        let mut client = unix::connect(&socket_path_clone).await.unwrap();

        let creds = PassedCredentials::new(
            rustix::process::getuid(),
            rustix::process::getgid(),
            rustix::process::getpid(),
        );
        client.set_credentials(Some(creds));

        client
            .send_call(&Call::new(Method::Ping), vec![])
            .await
            .unwrap();
    });

    let mut server = listener.accept().await.unwrap().unwrap();
    let _ = server.receive_call::<Method>().await.unwrap();

    let received = server
        .received_credentials()
        .expect("credentials should be received");
    assert_eq!(received.unix_user_id(), rustix::process::getuid());
    assert_eq!(received.unix_primary_group_id(), rustix::process::getgid());
    assert_eq!(received.process_id(), rustix::process::getpid());

    client_task.await.unwrap();
}
