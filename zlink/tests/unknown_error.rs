//! Test that an error that the client doesn't know is not taken for a successful reply: a service
//! newer than its client can return errors that the error type of the client doesn't have.

#![cfg(all(feature = "service", feature = "proxy"))]

use std::{error::Error, path::Path};

use tokio::select;
use zlink::{Server, introspect};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix::{bind, connect};
#[cfg(feature = "tokio")]
use zlink::tokio::unix::{bind, connect};

#[test_log::test(tokio::test)]
async fn unknown_error() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("unknown-error.sock");

    let server = Server::new(bind(&socket_path)?, Versions);
    select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut conn = connect(socket_path).await?;

    // The service replies with an error that `ProxyError` doesn't have.
    let error = conn.check(true).await.unwrap_err();
    let zlink::Error::UnknownErrorReply(name) = error else {
        panic!("unexpected error: {error:?}");
    };
    assert_eq!(name, "org.example.versions.Newer");

    // The connection can still be used for the next call.
    assert_eq!(conn.check(false).await?, Ok(()));

    Ok(())
}

struct Versions;

#[zlink::service(interface = "org.example.versions")]
impl Versions {
    async fn check(&mut self, newer: bool) -> Result<(), ServiceError> {
        if newer {
            Err(ServiceError::Newer)
        } else {
            Ok(())
        }
    }
}

#[zlink::proxy("org.example.versions")]
trait VersionsProxy {
    async fn check(&mut self, newer: bool) -> zlink::Result<Result<(), ProxyError>>;
}

#[derive(Debug, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.versions")]
enum ServiceError {
    Newer,
}

// The errors as the client knows them, which don't include `Newer`.
#[derive(Debug, PartialEq, zlink::ReplyError)]
#[zlink(interface = "org.example.versions")]
enum ProxyError {
    Older,
}
