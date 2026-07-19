//! Tests for streaming service methods that emit method errors.
//!
//! Streaming methods can opt in to error replies by yielding `Result<Reply<T>, E>` instead of
//! plain `Reply<T>`. The server dispatches each item as either a success reply or an error
//! reply on the wire.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use zlink::{
    Reply, Server, introspect,
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn streaming_errors() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path).unwrap();
    let server = Server::new(listener, CountingService);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // First call: `to = 3`, expect three success ticks then end-of-stream.
    {
        let mut stream = std::pin::pin!(conn.count(3).await?);

        let mut values = Vec::new();
        while let Some(result) = stream.next().await {
            let tick = result?.expect("no error expected");
            values.push(tick.value);
        }
        assert_eq!(values, vec![1, 2, 3]);
    }

    // Second call: `to = 0`, the server should yield a single error stream item.
    {
        let mut stream = std::pin::pin!(conn.count(0).await?);

        let mut got_error = false;
        let mut other_items = 0;
        while let Some(result) = stream.next().await {
            match result? {
                Ok(_) => other_items += 1,
                Err(CountError::AtZero) => got_error = true,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }
        assert!(got_error, "expected AtZero error from stream");
        assert_eq!(other_items, 0);
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, introspect::Type)]
struct Tick {
    value: u32,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.counter")]
enum CountError {
    AtZero,
    BadInput { reason: String },
}

struct CountingService;

#[zlink::service(interface = "org.example.counter")]
impl CountingService {
    /// Stream values 1..=`to`, or fail with [`CountError::AtZero`] if `to == 0`.
    #[zlink(more)]
    async fn count(
        &self,
        more: bool,
        to: u32,
    ) -> impl futures_util::Stream<Item = Result<Reply<Tick>, CountError>> + Unpin {
        if to == 0 {
            return futures_util::stream::iter(vec![Err(CountError::AtZero)]);
        }
        let to = if more { to } else { 1 };
        let last = to;
        let items: Vec<Result<Reply<Tick>, CountError>> = (1..=to)
            .map(
                move |value| Ok(Reply::new(Some(Tick { value })).set_continues(Some(value < last))),
            )
            .collect();
        futures_util::stream::iter(items)
    }
}

#[zlink::proxy("org.example.counter")]
trait CountingProxy {
    #[zlink(more)]
    async fn count(
        &mut self,
        to: u32,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<Tick, CountError>>>>;
}
