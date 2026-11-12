//! Test a service that is split into one type per interface, which `#[derive(Service)]` combines.

#![cfg(all(
    feature = "service",
    feature = "proxy",
    feature = "introspection",
    feature = "idl-parse"
))]

use std::{path::Path, pin::pin};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::select;
use zlink::{
    Call, Server, introspect,
    varlink_service::{self, Proxy as _},
};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix::{bind, connect};
#[cfg(feature = "tokio")]
use zlink::tokio::unix::{bind, connect};
#[cfg(all(feature = "zruntime", not(any(feature = "tokio", feature = "smol"))))]
use zlink::zruntime::unix::{bind, connect};

#[test_log::test(tokio::test)]
async fn interfaces() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("interfaces.sock");

    let service = Example {
        database: Database::default(),
        metrics: Metrics,
        log: Log,
        ticker: Ticker,
    };
    let server = Server::new(bind(&socket_path)?, service);
    select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // Both interfaces have a method named `Get`, and see the state change.
    assert_eq!(
        MetricsProxy::get(&mut conn).await?.unwrap(),
        OwnedHits { hits: 1 }
    );
    assert_eq!(
        MetricsProxy::get(&mut conn).await?.unwrap(),
        OwnedHits { hits: 2 }
    );
    let lines = LogProxy::get(&mut conn).await?.unwrap().lines;
    assert_eq!(lines, ["hit 1", "hit 2"]);
    assert_eq!(
        MetricsProxy::get(&mut conn).await?.unwrap(),
        OwnedHits { hits: 3 }
    );
    let lines = LogProxy::get(&mut conn).await?.unwrap().lines;
    assert_eq!(lines, ["hit 1", "hit 2", "hit 3"]);

    // The streaming method yields all of its items.
    let ticks = {
        let mut stream = pin!(conn.ticks().await?);
        let mut ticks = Vec::new();
        while let Some(tick) = stream.next().await {
            ticks.push(tick?.unwrap().tick);
        }
        ticks
    };
    assert_eq!(ticks, [1, 2, 3]);

    // The error of a method comes back typed, and the connection remains usable.
    assert_eq!(conn.limit(5).await?.unwrap().limit, 5);
    assert_eq!(
        conn.limit(50).await?.unwrap_err(),
        TickerError::TooHigh { max: 10 }
    );
    assert_eq!(conn.limit(6).await?.unwrap().limit, 6);

    // `GetInfo` has the metadata of the service and the interfaces of all the fields.
    let info = conn.get_info().await?.map_err(|e| e.to_string())?;
    assert_eq!(info.vendor, "Example Inc.");
    assert_eq!(info.product, "Example");
    assert_eq!(info.version, "1.0");
    assert_eq!(info.url, "https://example.org/");
    let interfaces: Vec<_> = info.interfaces.iter().map(|i| i.as_str()).collect();
    assert_eq!(
        interfaces,
        [
            "org.example.metrics",
            "org.example.log",
            "org.example.ticker",
            "org.varlink.service",
        ]
    );

    // Each of the interfaces can be described, as well as `org.varlink.service`.
    for (name, method) in [
        ("org.example.metrics", "Get"),
        ("org.example.log", "Get"),
        ("org.example.ticker", "Ticks"),
        ("org.varlink.service", "GetInfo"),
    ] {
        let description = conn
            .get_interface_description(name.try_into()?)
            .await?
            .map_err(|e| e.to_string())?;
        let description = description.parse().unwrap();
        assert_eq!(description.name(), name);
        assert!(description.methods().any(|m| m.name().as_str() == method));
    }
    let error = conn
        .get_interface_description("org.example.unknown".try_into()?)
        .await
        .unwrap_err();
    let zlink::Error::VarlinkService(error) = error else {
        panic!("unexpected error: {error:?}");
    };
    let varlink_service::Error::InterfaceNotFound { interface } = error.inner() else {
        panic!("unexpected error: {error:?}");
    };
    assert_eq!(interface, "org.example.unknown");

    // Calls to methods that don't exist get `MethodNotFound`, with the full name of the method,
    // whether the interface exists or not.
    for method in ["org.example.metrics.Nope", "org.example.unknown.Get"] {
        let call = Call::new(RawCall { method });
        let error = conn
            .call_method::<_, Value, ()>(&call, vec![])
            .await
            .unwrap_err();
        let zlink::Error::VarlinkService(error) = error else {
            panic!("unexpected error: {error:?}");
        };
        assert!(matches!(
            error.inner(),
            varlink_service::Error::MethodNotFound { method: m } if m == method
        ));
    }

    // The connection still works after all that.
    assert_eq!(
        MetricsProxy::get(&mut conn).await?.unwrap(),
        OwnedHits { hits: 4 }
    );

    Ok(())
}

#[derive(zlink::Service)]
#[zlink(
    vendor = "Example Inc.",
    product = "Example",
    version = "1.0",
    url = "https://example.org/"
)]
struct Example {
    #[zlink(state)]
    database: Database,
    metrics: Metrics,
    log: Log,
    ticker: Ticker,
}

/// The interface that counts the hits, mutating the shared state.
struct Metrics;

#[zlink::interface("org.example.metrics")]
impl Metrics {
    async fn get(&mut self, #[zlink(state)] database: &mut Database) -> Hits {
        database.hits += 1;
        database.log.push(format!("hit {}", database.hits));

        Hits {
            hits: database.hits,
        }
    }
}

/// The interface that has the log of the hits, reading the shared state.
struct Log;

#[zlink::interface("org.example.log")]
impl Log {
    async fn get<'a>(&self, #[zlink(state)] database: &'a Database) -> Lines<'a> {
        Lines {
            lines: &database.log,
        }
    }
}

/// The interface that shares nothing, with a streaming method and one that can fail.
struct Ticker;

#[zlink::interface("org.example.ticker")]
impl Ticker {
    #[zlink(more)]
    async fn ticks(
        &self,
        more: bool,
    ) -> impl futures_util::Stream<Item = zlink::Reply<Tick>> + Unpin {
        let count = if more { 3 } else { 1 };

        futures_util::stream::iter((1..=count).map(move |tick| {
            zlink::Reply::new(Some(Tick { tick })).set_continues(Some(tick < count))
        }))
    }

    async fn limit(&self, limit: u32) -> Result<Limit, TickerError> {
        if limit > 10 {
            return Err(TickerError::TooHigh { max: 10 });
        }

        Ok(Limit { limit })
    }
}

/// The state that the `metrics` and `log` interfaces share.
#[derive(Default)]
struct Database {
    hits: u64,
    log: Vec<String>,
}

#[derive(Debug, Serialize, introspect::Type)]
struct Hits {
    hits: u64,
}

#[derive(Debug, Serialize, introspect::Type)]
struct Lines<'a> {
    lines: &'a [String],
}

#[derive(Debug, Serialize, introspect::Type)]
struct Tick {
    tick: u32,
}

#[derive(Debug, Serialize, introspect::Type)]
struct Limit {
    limit: u32,
}

#[derive(Debug, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.ticker")]
enum TickerError {
    TooHigh { max: u32 },
}

#[zlink::proxy("org.example.metrics")]
trait MetricsProxy {
    async fn get(&mut self) -> zlink::Result<Result<OwnedHits, ()>>;
}

#[zlink::proxy("org.example.log")]
trait LogProxy {
    async fn get(&mut self) -> zlink::Result<Result<OwnedLines, ()>>;
}

#[zlink::proxy("org.example.ticker")]
trait TickerProxy {
    #[zlink(more)]
    async fn ticks(
        &mut self,
    ) -> zlink::Result<impl futures_util::Stream<Item = zlink::Result<Result<OwnedTick, ()>>>>;

    async fn limit(&mut self, limit: u32) -> zlink::Result<Result<OwnedLimit, TickerError>>;
}

#[derive(Debug, PartialEq, Deserialize)]
struct OwnedHits {
    hits: u64,
}

#[derive(Debug, Deserialize)]
struct OwnedLines {
    lines: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OwnedTick {
    tick: u32,
}

#[derive(Debug, Deserialize)]
struct OwnedLimit {
    limit: u32,
}

/// A call without parameters, to a method of any name.
#[derive(Debug, Serialize)]
struct RawCall<'a> {
    method: &'a str,
}
