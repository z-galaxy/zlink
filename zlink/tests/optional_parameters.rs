//! The Varlink spec allows callers to omit `parameters` entirely, which `varlinkctl` does when it
//! has none to send. These tests talk to a service over a raw socket to verify that.

#![cfg(all(feature = "service", feature = "introspection", feature = "tokio"))]

use serde::{Deserialize, Serialize};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    select,
    time::{Duration, timeout},
};
use zlink::{ReplyError, introspect, tokio::unix};

#[derive(Debug, ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.Optional")]
enum Error {
    Failed,
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Echo {
    value: String,
}

struct Service;

#[zlink::service(interface = "org.example.Optional")]
impl Service {
    async fn all_optional(
        &self,
        name: Option<String>,
        verbose: Option<bool>,
    ) -> Result<Echo, Error> {
        let value = format!("{}:{}", name.unwrap_or_default(), verbose.unwrap_or(false));
        Ok(Echo { value })
    }

    async fn no_params(&self) -> Result<Echo, Error> {
        Ok(Echo {
            value: "none".into(),
        })
    }
}

/// Send `call` over a raw socket and return the raw reply (without the NUL terminator).
async fn raw_call(call: &str) -> String {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("optional.sock");
    let listener = unix::bind(&path).unwrap();
    let server = zlink::Server::new(listener, Service);

    let client = async {
        let mut stream = UnixStream::connect(&path).await.unwrap();
        stream.write_all(call.as_bytes()).await.unwrap();
        stream.write_all(&[0]).await.unwrap();
        let mut buf = [0u8; 4096];
        let n = timeout(Duration::from_secs(5), stream.read(&mut buf))
            .await
            .expect("timed out waiting for a reply")
            .unwrap();
        String::from_utf8(buf[..n].to_vec()).unwrap()
    };

    select! {
        res = server.run() => panic!("server exited: {res:?}"),
        reply = client => reply.trim_end_matches('\0').to_string(),
    }
}

const ALL_OPTIONAL: &str = "org.example.Optional.AllOptional";
const NO_PARAMS: &str = "org.example.Optional.NoParams";

#[tokio::test]
async fn all_optional_without_parameters() {
    let reply = raw_call(&format!(r#"{{"method":"{ALL_OPTIONAL}"}}"#)).await;
    assert_eq!(
        reply,
        r#"{"parameters":{"value":":false"},"continues":false}"#
    );
}

#[tokio::test]
async fn all_optional_with_empty_parameters() {
    let reply = raw_call(&format!(
        r#"{{"method":"{ALL_OPTIONAL}","parameters":{{}}}}"#
    ))
    .await;
    assert_eq!(
        reply,
        r#"{"parameters":{"value":":false"},"continues":false}"#
    );
}

#[tokio::test]
async fn all_optional_with_parameters() {
    let reply = raw_call(&format!(
        r#"{{"method":"{ALL_OPTIONAL}","parameters":{{"name":"a","verbose":true}}}}"#
    ))
    .await;
    assert_eq!(
        reply,
        r#"{"parameters":{"value":"a:true"},"continues":false}"#
    );
}

#[tokio::test]
async fn no_params_without_parameters() {
    let reply = raw_call(&format!(r#"{{"method":"{NO_PARAMS}"}}"#)).await;
    assert_eq!(
        reply,
        r#"{"parameters":{"value":"none"},"continues":false}"#
    );
}

#[tokio::test]
async fn no_params_with_empty_parameters() {
    let reply = raw_call(&format!(r#"{{"method":"{NO_PARAMS}","parameters":{{}}}}"#)).await;
    assert_eq!(
        reply,
        r#"{"parameters":{"value":"none"},"continues":false}"#
    );
}

#[tokio::test]
async fn get_info_without_parameters() {
    let reply = raw_call(r#"{"method":"org.varlink.service.GetInfo"}"#).await;
    assert!(reply.contains(r#""interfaces""#), "{reply}");
}

#[tokio::test]
async fn get_info_with_empty_parameters() {
    let reply = raw_call(r#"{"method":"org.varlink.service.GetInfo","parameters":{}}"#).await;
    assert!(reply.contains(r#""interfaces""#), "{reply}");
}
