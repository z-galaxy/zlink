<p align="center">
  <a href="https://crates.io/crates/zlink">
    <img alt="crates.io" src="https://img.shields.io/crates/v/zlink">
  </a>
  <a href="https://docs.rs/zlink/">
    <img alt="API Documentation" src="https://docs.rs/zlink/badge.svg">
  </a>
  <a href="https://github.com/z-galaxy/zlink/actions/workflows/rust.yml">
    <img alt="Build Status" src="https://github.com/z-galaxy/zlink/actions/workflows/rust.yml/badge.svg">
  </a>
</p>

<p align="center">
  <img alt="Project logo" src="https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.svg">
</p>

<h1 align="center">zlink</h1>

A Rust implementation of the [Varlink](https://varlink.org/) IPC protocol. zlink provides a safe,
async API for building Varlink services and clients.

## Overview

Varlink is a simple, JSON-based IPC protocol that enables communication between system services and
applications. zlink makes it easy to implement Varlink services in Rust with:

- **Async-first design**: Built on async/await for efficient concurrent operations.
- **Type safety**: Leverage Rust's type system with derive macros and code generation.
- **Unix domain sockets**: Communicate over Unix domain socket transports.
- **Runtime choice**: Use either the Tokio or smol async runtime.
- **Code generation**: Generate Rust code from Varlink IDL files.

📖 To learn all about using zlink (and Varlink) step by step, check out
[the zlink book](https://z-galaxy.github.io/zlink/). Its sources are in the
[`book` directory](https://github.com/z-galaxy/zlink/tree/main/book) of the repository.

## Project Structure

The zlink project consists of several subcrates:

- **[`zlink`]**: The main unified API crate that re-exports functionality based on enabled features.
  This is the only crate you will want to use directly in your application and services.
- **[`zlink-core`]**: Core no-std foundation providing essential Varlink types and traits.
- **[`zlink-names`]**: Varlink name types and validation (field, type and interface names).
- **[`zlink-macros`]**: Contains the attribute and derive macros.
- **[`zlink-tokio`]**: `Tokio`-based transport implementations and runtime integration.
- **[`zlink-smol`]**: `smol`-based transport implementations and runtime integration.
- **[`zlink-codegen`]**: Code generation tool for creating Rust bindings from Varlink IDL files.

## Examples

### Example: Calculator Service and Client

Here's a complete example showing both service and client implementations using zlink's attribute
macros:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{select, sync::oneshot};
use zlink::{introspect, proxy, service, tokio::unix, ReplyError, Server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a channel to signal when server is ready.
    let (ready_tx, ready_rx) = oneshot::channel();

    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("calculator.sock");

    // Run server and client concurrently.
    select! {
        res = run_server(&socket_path, ready_tx) => res?,
        res = run_client(&socket_path, ready_rx) => res?,
    }

    Ok(())
}

async fn run_client(
    socket_path: &Path,
    ready_rx: oneshot::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Wait for server to be ready.
    ready_rx.await.map_err(|_| "Server failed to start")?;

    // Connect to the calculator service.
    let mut conn = unix::connect(socket_path).await?;

    // Use the proxy-generated methods.
    let result = conn.add(5.0, 3.0).await?.unwrap();
    assert_eq!(result.result, 8.0);

    let result = conn.multiply(4.0, 7.0).await?.unwrap();
    assert_eq!(result.result, 28.0);

    // Handle errors properly.
    let Err(CalculatorError::DivisionByZero { message }) = conn.divide(10.0, 0.0).await? else {
        panic!("Expected DivisionByZero error");
    };
    assert_eq!(message, "Cannot divide by zero");

    // Test invalid input error with large dividend.
    let Err(CalculatorError::InvalidInput { field, reason }) =
        conn.divide(2000000.0, 2.0).await?
    else {
        panic!("Expected InvalidInput error");
    };
    println!("Field: {field}, Reason: {reason}");

    let stats = conn.get_stats().await?.unwrap();
    assert_eq!(stats.count, 2);
    println!("Stats: {stats:?}");

    Ok(())
}

// The client proxy.
#[proxy("org.example.Calculator")]
trait CalculatorProxy {
    async fn add(
        &mut self,
        a: f64,
        b: f64,
    ) -> zlink::Result<Result<CalculationResult, CalculatorError<'_>>>;
    async fn multiply(
        &mut self,
        x: f64,
        y: f64,
    ) -> zlink::Result<Result<CalculationResult, CalculatorError<'_>>>;
    async fn divide(
        &mut self,
        dividend: f64,
        divisor: f64,
    ) -> zlink::Result<Result<CalculationResult, CalculatorError<'_>>>;
    async fn get_stats(
        &mut self,
    ) -> zlink::Result<Result<Statistics<'_>, CalculatorError<'_>>>;
}

// Types shared between client and server.
#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct CalculationResult {
    result: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, introspect::Type)]
struct Statistics<'a> {
    count: u64,
    #[serde(borrow)]
    operations: Vec<&'a str>,
}

#[derive(Debug, PartialEq, ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.Calculator")]
enum CalculatorError<'a> {
    DivisionByZero {
        message: &'a str,
    },
    InvalidInput {
        field: &'a str,
        reason: &'a str,
    },
}

async fn run_server(
    socket_path: &Path,
    ready_tx: oneshot::Sender<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Setup and run the server.
    let listener = unix::bind(socket_path)?;
    let server = Server::new(listener, Calculator::new());

    // Signal that server is ready.
    let _ = ready_tx.send(());

    server.run().await.map_err(|e| e.into())
}

// The calculator service.
struct Calculator {
    operations: Vec<String>,
}

impl Calculator {
    fn new() -> Self {
        Self { operations: Vec::new() }
    }
}

#[service(interface = "org.example.Calculator")]
impl Calculator {
    async fn add(&mut self, a: f64, b: f64) -> CalculationResult {
        self.operations.push(format!("add({a}, {b})"));
        CalculationResult { result: a + b }
    }

    async fn multiply(&mut self, x: f64, y: f64) -> CalculationResult {
        self.operations.push(format!("multiply({x}, {y})"));
        CalculationResult { result: x * y }
    }

    async fn divide(
        &mut self,
        dividend: f64,
        divisor: f64,
    ) -> Result<CalculationResult, CalculatorError<'_>> {
        if divisor == 0.0 {
            Err(CalculatorError::DivisionByZero {
                message: "Cannot divide by zero",
            })
        } else if dividend < -1000000.0 || dividend > 1000000.0 {
            Err(CalculatorError::InvalidInput {
                field: "dividend",
                reason: "must be within range",
            })
        } else {
            self.operations
                .push(format!("divide({dividend}, {divisor})"));
            Ok(CalculationResult {
                result: dividend / divisor,
            })
        }
    }

    async fn get_stats(&self) -> Statistics<'_> {
        let ops: Vec<&str> = self.operations.iter().map(|s| s.as_str()).collect();
        Statistics {
            count: self.operations.len() as u64,
            operations: ops,
        }
    }
}
```

> **Note**: Typically you would want to spawn the server in a separate task but that's not what we
> did in the example above. Please refer to [`Server::run` docs] for the reason.

For ready-to-run examples you can execute with `cargo run`, see the [`examples`](zlink/examples)
directory.

## Code Generation from IDL

zlink-codegen can generate Rust code from Varlink interface description files:

```sh
# Install the code generator
cargo install zlink-codegen

# Let's create a file containing Varlink IDL
cat <<EOF > calculator.varlink
# Calculator service interface
interface org.example.Calculator

type CalculationResult (
    result: float
)

type DivisionByZeroError (
    message: string
)

method Add(a: float, b: float) -> (result: float)
method Multiply(x: float, y: float) -> (result: float)
method Divide(dividend: float, divisor: float) -> (result: float)
error DivisionByZero(message: string)
EOF

# Generate Rust code from the IDL
zlink-codegen calculator.varlink > src/calculator_gen.rs
```

The generated code includes type definitions and proxy traits ready to use in your application.

### Pipelining

zlink supports method call pipelining for improved throughput and reduced latency. The `proxy` macro
adds variants for each method named `chain_<method_name>` and a trait named `<TraitName>Chain` that
allow you to batch multiple requests and send them out at once without waiting for individual
responses.

> **Note**: Chain methods are only generated for proxy methods that use owned types
> (`DeserializeOwned`) in their return type. Methods with borrowed types (non-static lifetimes)
> don't get chain variants since the internal buffer may be reused between stream iterations. Input
> arguments can still use borrowed types.

```rust,no_run
use futures_util::{StreamExt, pin_mut};
use serde::{Deserialize, Serialize};
use zlink::{proxy, tokio::unix, ReplyError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to a batch processing service in the XDG runtime dir
    // (e.g. /run/user/1000/batch_processor.varlink).
    let socket_path = dirs::runtime_dir()
        .ok_or("XDG_RUNTIME_DIR is not set")?
        .join("batch_processor.varlink");
    let mut conn = unix::connect(&socket_path).await?;

    // Send multiple pipelined requests without waiting for responses.
    // Note: chain methods are only generated for proxy methods with owned return types.
    let replies = conn
        .chain_process(1, "first")?
        .process(2, "second")?
        .process(3, "third")?
        .batch_process(vec![
            ProcessRequest { id: 4, data: "batch1" },
            ProcessRequest { id: 5, data: "batch2" },
        ])?
        .send::<ProcessReply, ProcessError>()
        .await?;

    // Collect all responses
    pin_mut!(replies);
    let mut results = Vec::new();
    while let Some(reply) = replies.next().await {
        let (reply, _fds) = reply?;
        if let Ok(response) = reply {
            match response.into_parameters() {
                Some(ProcessReply::Result(result)) => {
                    results.push(result);
                }
                Some(ProcessReply::BatchResult(batch)) => {
                    results.extend(batch.results);
                }
                None => {}
            }
        }
    }

    // Process results
    for result in results {
        println!("Processed item {}: {}", result.id, result.processed);
    }

    Ok(())
}

// Proxy trait with owned return types - chain methods are generated.
#[proxy("org.example.BatchProcessor")]
trait BatchProcessorProxy {
    async fn process(
        &mut self,
        id: u32,
        data: &str,
    ) -> zlink::Result<Result<ProcessReply, ProcessError>>;

    async fn batch_process(
        &mut self,
        requests: Vec<ProcessRequest<'_>>,
    ) -> zlink::Result<Result<ProcessReply, ProcessError>>;
}

// Input types can use borrowed data (they implement Serialize, not DeserializeOwned).
#[derive(Debug, Serialize)]
struct ProcessRequest<'a> {
    id: u32,
    data: &'a str,
}

// Owned reply types - required for chain API (DeserializeOwned).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProcessReply {
    Result(ProcessResult),
    BatchResult(BatchResult),
}

#[derive(Debug, Deserialize)]
struct ProcessResult {
    id: u32,
    processed: String,
}

#[derive(Debug, Deserialize)]
struct BatchResult {
    results: Vec<ProcessResult>,
}

#[derive(Debug, ReplyError)]
#[zlink(interface = "org.example.BatchProcessor")]
enum ProcessError {
    InvalidRequest { message: String },
}
```

## Features

### Main Features

- `tokio` (default): Enable tokio runtime integration, under the `tokio` module.
- `smol`: Enable smol runtime integration, under the `smol` module.
- `server` (default): Enable server-related functionality (Server, Listener, Service).
- `service` (default): Enable the `#[service]` macro. Implies `server` and `introspection`.
- `proxy` (default): Enable the `#[proxy]` macro for type-safe client code.
- `tracing` (default): Enable `tracing`-based logging.
- `defmt`:  Enable `defmt`-based logging. If both `tracing` and `defmt` is enabled, `tracing` is
  used.

The runtime features are additive: both can be enabled at the same time, with the
runtime-specific API (currently the `unix` and `notified` modules) of each residing in its own
module. The rest of the API is runtime-agnostic and available at the crate root.

### IDL and Introspection

- `idl`: Support for IDL type representations.
- `introspection`: Enable runtime introspection of service interfaces.
- `idl-parse`: Parse Varlink IDL files at runtime.

## Getting Help and/or Contributing

If you need help in using these crates, are looking for ways to contribute, or just want to hang out
with the cool kids, please come chat with us in the
[`#zlink:matrix.org`](https://matrix.to/#/#zlink:matrix.org) Matrix room. If something doesn't seem
right, please [file an issue](https://github.com/z-galaxy/zlink/issues/new).

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

## License

This project is licensed under the [MIT License][license].

[cips]: https://github.com/z-galaxy/zlink/actions/workflows/rust.yml
[crates.io]: https://crates.io/crates/zlink
[license]: ./LICENSE
[`zlink`]: https://docs.rs/zlink
[`zlink-core`]: https://docs.rs/zlink-core
[`zlink-names`]: https://docs.rs/zlink-names
[`zlink-tokio`]: https://docs.rs/zlink-tokio
[`zlink-smol`]: https://docs.rs/zlink-smol
[`zlink-codegen`]: https://docs.rs/zlink-codegen
[`zlink-macros`]: https://docs.rs/zlink-macros
[`Server::run` docs]: https://docs.rs/zlink/latest/zlink/struct.Server.html#method.run
