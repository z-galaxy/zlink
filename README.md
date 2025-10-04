<p align="center">
  <a href="https://crates.io/crates/zlink">
    <img alt="crates.io" src="https://img.shields.io/crates/v/zlink">
  </a>
  <a href="https://docs.rs/zlink/">
    <img alt="API Documentation" src="https://docs.rs/zlink/badge.svg">
  </a>
  <a href="https://github.com/zeenix/zlink/actions/workflows/rust.yml">
    <img alt="Build Status" src="https://github.com/zeenix/zlink/actions/workflows/rust.yml/badge.svg">
  </a>
</p>

<p align="center">
  <img alt="Project logo" src="https://raw.githubusercontent.com/zeenix/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.svg">
</p>

<h1 align="center">zlink</h1>

A Rust implementation of the [Varlink](https://varlink.org/) IPC protocol. zlink provides a safe,
async API for building Varlink services and clients with support for both standard and embedded
(no-std) environments.

## Overview

Varlink is a simple, JSON-based IPC protocol that enables communication between system services and
applications. zlink makes it easy to implement Varlink services in Rust with:

- **Async-first design**: Built on async/await for efficient concurrent operations.
- **Type safety**: Leverage Rust's type system with derive macros and code generation.
- **No-std support**: Run on embedded systems.
- **Multiple transports**: Unix domain sockets and (upcoming) USB support.
- **Code generation**: Generate Rust code from Varlink IDL files.

## Project Structure

The zlink project consists of several subcrates:

- **[`zlink`]**: The main unified API crate that re-exports functionality based on enabled features.
  This is the only crate you will want to use directly in your application and services.
- **[`zlink-core`]**: Core no-std foundation providing essential Varlink types and traits.
- **[`zlink-macros`]**: Contains the attribute and derive macros.
- **[`zlink-tokio`]**: `Tokio`-based transport implementations and runtime integration.
- **[`zlink-codegen`]**: Code generation tool for creating Rust bindings from Varlink IDL files.

## Examples

### Example: Calculator Service and Client

> **Note**: For service implementation, zlink currently only provides a low-level API. A high-level
> service API with attribute macros (similar to the `proxy` macro for clients) is planned for the
> near future.

Here's a complete example showing both service implementation and client usage through the `proxy`
macro:

```rust
use serde::{Deserialize, Serialize};
use tokio::{select, sync::oneshot, fs::remove_file};
use zlink::{
    proxy,
    service::{MethodReply, Service},
    unix, Call, ReplyError, Server,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a channel to signal when server is ready
    let (ready_tx, ready_rx) = oneshot::channel();

    // Run server and client concurrently
    select! {
        res = run_server(ready_tx) => res?,
        res = run_client(ready_rx) => res?,
    }

    Ok(())
}

async fn run_client(ready_rx: oneshot::Receiver<()>) -> Result<(), Box<dyn std::error::Error>> {
    // Wait for server to be ready
    ready_rx.await.map_err(|_| "Server failed to start")?;

    // Connect to the calculator service
    let mut conn = unix::connect(SOCKET_PATH).await?;

    // Use the proxy-generated methods
    let result = conn.add(5.0, 3.0).await?.unwrap();
    assert_eq!(result.result, 8.0);

    let result = conn.multiply(4.0, 7.0).await?.unwrap();
    assert_eq!(result.result, 28.0);

    // Handle errors properly
    let Err(CalculatorError::DivisionByZero { message }) = conn.divide(10.0, 0.0).await? else {
        panic!("Expected DivisionByZero error");
    };
    assert_eq!(message, "Cannot divide by zero");

    // Test invalid input error with large dividend
    let Err(CalculatorError::InvalidInput {
        field,
        reason,
    }) = conn.divide(2000000.0, 2.0).await? else {
        panic!("Expected InvalidInput error");
    };
    println!("Field: {}, Reason: {}", field, reason);

    let stats = conn.get_stats().await?.unwrap();
    assert_eq!(stats.count, 2);
    println!("Stats: {:?}", stats);

    Ok(())
}

// The client proxy - this implements the trait for `Connection<S>`
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

// Types shared between client and server
#[derive(Debug, Serialize, Deserialize)]
struct CalculationResult {
    result: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Statistics<'a> {
    count: u64,
    #[serde(borrow)]
    operations: Vec<&'a str>,
}

#[derive(Debug, ReplyError)]
#[zlink(interface = "org.example.Calculator")]
enum CalculatorError<'a> {
    DivisionByZero {
        message: &'a str
    },
    InvalidInput {
        field: &'a str,
        reason: &'a str,
    },
}

async fn run_server(ready_tx: oneshot::Sender<()>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = remove_file(SOCKET_PATH).await;

    // Setup the server
    let listener = unix::bind(SOCKET_PATH)?;
    let service = Calculator::new();
    let server = Server::new(listener, service);

    // Signal that server is ready
    let _ = ready_tx.send(());

    server.run().await.map_err(|e| e.into())
}

// The calculator service
struct Calculator {
    operations: Vec<String>,
}

impl Calculator {
    fn new() -> Self {
        Self {
            operations: Vec::new(),
        }
    }
}

// Implement the Service trait
impl Service for Calculator {
    type MethodCall<'de> = CalculatorMethod;
    type ReplyParams<'ser> = CalculatorReply<'ser>;
    type ReplyStreamParams = ();
    type ReplyStream = futures_util::stream::Empty<zlink::Reply<()>>;
    type ReplyError<'ser> = CalculatorError<'ser>;

    async fn handle(
        &mut self,
        call: Call<Self::MethodCall<'_>>,
    ) -> MethodReply<Self::ReplyParams<'_>, Self::ReplyStream, Self::ReplyError<'_>> {
        match call.method() {
            CalculatorMethod::Add { a, b } => {
                self.operations.push(format!("add({}, {})", a, b));
                MethodReply::Single(Some(CalculatorReply::Result(CalculationResult { result: a + b })))
            }
            CalculatorMethod::Multiply { x, y } => {
                self.operations.push(format!("multiply({}, {})", x, y));
                MethodReply::Single(Some(CalculatorReply::Result(CalculationResult { result: x * y })))
            }
            CalculatorMethod::Divide { dividend, divisor } => {
                if *divisor == 0.0 {
                    MethodReply::Error(CalculatorError::DivisionByZero {
                        message: "Cannot divide by zero",
                    })
                } else if dividend < &-1000000.0 || dividend > &1000000.0 {
                    MethodReply::Error(CalculatorError::InvalidInput {
                        field: "dividend",
                        reason: "must be within range",
                    })
                } else {
                    self.operations.push(format!("divide({}, {})", dividend, divisor));
                    MethodReply::Single(Some(CalculatorReply::Result(CalculationResult {
                        result: dividend / divisor,
                    })))
                }
            }
            CalculatorMethod::GetStats => {
                let ops: Vec<&str> = self.operations.iter().map(|s| s.as_str()).collect();
                MethodReply::Single(Some(CalculatorReply::Stats(Statistics {
                    count: self.operations.len() as u64,
                    operations: ops,
                })))
            }
        }
    }
}

// Method calls the service handles
#[derive(Debug, Deserialize)]
#[serde(tag = "method", content = "parameters")]
enum CalculatorMethod {
    #[serde(rename = "org.example.Calculator.Add")]
    Add { a: f64, b: f64 },
    #[serde(rename = "org.example.Calculator.Multiply")]
    Multiply { x: f64, y: f64 },
    #[serde(rename = "org.example.Calculator.Divide")]
    Divide { dividend: f64, divisor: f64 },
    #[serde(rename = "org.example.Calculator.GetStats")]
    GetStats,
}

// Reply types
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CalculatorReply<'a> {
    Result(CalculationResult),
    #[serde(borrow)]
    Stats(Statistics<'a>),
}

const SOCKET_PATH: &str = "/tmp/calculator_example.varlink";
```

> **Note**: Typically you would want to spawn the server in a separate task but that's not what we
> did in the example above. Please refer to [`Server::run` docs] for the reason.

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
responses:

```rust,no_run
use futures_util::{StreamExt, pin_mut};
use serde::{Deserialize, Serialize};
use zlink::{proxy, unix, ReplyError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to a batch processing service
    let mut conn = unix::connect("/tmp/batch_processor.varlink").await?;

    // Send multiple pipelined requests without waiting for responses
    let replies = conn
        .chain_process::<ProcessReply, ProcessError>(1, "first")?
        .process(2, "second")?
        .process(3, "third")?
        .batch_process(vec![
            ProcessRequest { id: 4, data: "batch1" },
            ProcessRequest { id: 5, data: "batch2" },
        ])?
        .send()
        .await?;

    // Collect all responses
    pin_mut!(replies);
    let mut results = Vec::new();
    while let Some(reply) = replies.next().await {
        let reply = reply?;
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

#[proxy("org.example.BatchProcessor")]
trait BatchProcessorProxy {
    async fn process(
        &mut self,
        id: u32,
        data: &str,
    ) -> zlink::Result<Result<ProcessReply<'_>, ProcessError>>;

    async fn batch_process(
        &mut self,
        requests: Vec<ProcessRequest<'_>>,
    ) -> zlink::Result<Result<ProcessReply<'_>, ProcessError>>;
}

#[derive(Debug, Serialize)]
struct ProcessRequest<'a> {
    id: u32,
    #[serde(borrow)]
    data: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ProcessReply<'a> {
    #[serde(borrow)]
    Result(ProcessResult<'a>),
    #[serde(borrow)]
    BatchResult(BatchResult<'a>),
}

#[derive(Debug, Deserialize)]
struct ProcessResult<'a> {
    id: u32,
    #[serde(borrow)]
    processed: &'a str,
}

#[derive(Debug, Deserialize)]
struct BatchResult<'a> {
    #[serde(borrow)]
    results: Vec<ProcessResult<'a>>,
}

#[derive(Debug, ReplyError)]
#[zlink(interface = "org.example.BatchProcessor")]
enum ProcessError {
    InvalidRequest,
}
```

## Examples

The repository includes a few examples:

- **[resolved.rs](zlink/examples/resolved.rs)**: DNS resolution using systemd-resolved's Varlink
  service
- **[varlink-inspect.rs](zlink/examples/varlink-inspect.rs)**: Service introspection tool

Run examples with:

```bash
cargo run --example resolved -- example.com systemd.io
cargo run \
  --example varlink-inspect \
  --features idl-parse,introspection -- \
  /run/systemd/resolve/io.systemd.Resolve
```

## Features

### Main Features

- `tokio` (default): Enable tokio runtime integration and use of standard library, `serde_json` and
  `tracing`. This is **currently** the only supported backend and therefore required.
- `proxy` (default): Enable the `#[proxy]` macro for type-safe client code.

### IDL and Introspection

- `idl`: Support for IDL type representations.
- `introspection`: Enable runtime introspection of service interfaces.
- `idl-parse`: Parse Varlink IDL files at runtime (requires `std`).

### Buffer Size Features

Control the I/O buffer size (only one can be enabled at a time):

- `io-buffer-2kb` (default): 2KB buffers for minimal memory usage.
- `io-buffer-4kb`: 4KB buffers.
- `io-buffer-16kb`: 16KB buffers for better performance with larger messages.
- `io-buffer-1mb`: 1MB buffers for high-throughput scenarios.

> **Note**: These feature flags are mainly of interest to embedded systems. With `tokio` enabled,
> these only represent the initial buffer sizes.

## Upcoming Features & Crates

- `embedded`: No-std support for embedded systems. It will enable use of:
  - `serde-json-core` for JSON serialization and deserialization.
  - `embassy-usb` for communication with a host via a USB device.
  - `defmt` for logging.
- `usb`: USB transport support for host-side communication.

Behind the scenes, `zlink` will make use of the upcoming `zlink-micro` and `zlink-usb` crates.
Together these will enable RPC between a (Linux) host and microcontroller(s).

## Getting Help and/or Contributing

If you need help in using these crates, are looking for ways to contribute, or just want to hang out
with the cool kids, please come chat with us in the
[`#zlink:matrix.org`](https://matrix.to/#/#zlink:matrix.org) Matrix room. If something doesn't seem
right, please [file an issue](https://github.com/zeenix/zlink/issues/new).

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

## License

This project is licensed under the [MIT License][license].

[cips]: https://github.com/zeenix/zlink/actions/workflows/rust.yml
[crates.io]: https://crates.io/crates/zlink
[license]: ./LICENSE
[`zlink`]: https://docs.rs/zlink
[`zlink-core`]: https://docs.rs/zlink-core
[`zlink-tokio`]: https://docs.rs/zlink-tokio
[`zlink-codegen`]: https://docs.rs/zlink-codegen
[`zlink-macros`]: https://docs.rs/zlink-macros
[`Server::run` docs]: https://docs.rs/zlink/latest/zlink/struct.Server.html#method.run
