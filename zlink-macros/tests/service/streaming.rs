//! Tests for streaming service methods (#[zlink(more)]).

use serde::{Deserialize, Serialize};
use zlink::{
    Server, introspect,
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn streaming() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    // Setup the server with a streaming service.
    let listener = bind(&socket_path).unwrap();
    let service = StreamingService {
        values: vec![10, 20, 30, 40, 50],
    };
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use futures_util::StreamExt;

    let mut conn = connect(socket_path).await?;

    // Test streaming method.
    let mut stream = std::pin::pin!(conn.get_values().await?);

    // Collect all values from the stream.
    let mut values = Vec::new();
    while let Some(result) = stream.next().await {
        let value = result?.unwrap();
        values.push(value.value);
    }

    assert_eq!(values, vec![10, 20, 30, 40, 50]);

    Ok(())
}

/// Response type for streaming values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, introspect::Type)]
struct StreamValue {
    value: i64,
}

/// A service that has a streaming method.
struct StreamingService {
    values: Vec<i64>,
}

#[zlink::service(interface = "org.example.streaming")]
impl StreamingService {
    #[zlink(more)]
    async fn get_values(
        &self,
        more: bool,
    ) -> impl futures_util::Stream<Item = zlink::Reply<StreamValue>> + Unpin {
        // Clone values to create an owned iterator (avoids lifetime issues).
        let values: Vec<StreamValue> = self
            .values
            .iter()
            .map(|&v| StreamValue { value: v })
            .collect();
        // If more=false, only return the first value.
        let values: Vec<StreamValue> = if more {
            values
        } else {
            values.into_iter().take(1).collect()
        };
        // For finite streams, manually set continues flag.
        let n = values.len();
        futures_util::stream::iter(
            values
                .into_iter()
                .enumerate()
                .map(move |(i, v)| zlink::Reply::new(Some(v)).set_continues(Some(i < n - 1))),
        )
    }
}

/// Proxy for streaming service.
#[zlink::proxy("org.example.streaming")]
trait StreamingProxy {
    #[zlink(more)]
    async fn get_values(
        &mut self,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<StreamValue, ()>>>>;
}
