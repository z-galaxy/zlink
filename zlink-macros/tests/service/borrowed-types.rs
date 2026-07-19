//! Tests for service macro with borrowed types (lifetimes in error and return types).

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self, CustomType},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn service_macro_borrowed_types() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("test.sock");

    let listener = bind(&socket_path).unwrap();
    let service = Calculator;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = connect(socket_path).await?;

    // Test successful operation.
    let reply = conn.divide(10.0, 2.0).await?.unwrap();
    assert_eq!(reply.result, 5.0);

    // Test error with borrowed string fields.
    let Err(CalculatorError::DivisionByZero { message }) = conn.divide(10.0, 0.0).await? else {
        panic!("Expected DivisionByZero error");
    };
    assert_eq!(message, "Cannot divide by zero");

    // Test another error variant.
    let Err(CalculatorError::InvalidInput { field, reason }) = conn.divide(2000000.0, 2.0).await?
    else {
        panic!("Expected InvalidInput error");
    };
    assert_eq!(field, "dividend");
    assert_eq!(reason, "must be within range");

    // Test method with borrowed params.
    let reply = conn.greet("world").await?.unwrap();
    assert_eq!(reply.message, "Hello, world!");

    Ok(())
}

// Return type with owned data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct CalculationResult {
    result: f64,
}

// Return type with owned data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Greeting {
    message: String,
}

// Error type with borrowed string fields.
#[derive(Debug, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.BorrowedCalc")]
enum CalculatorError<'a> {
    DivisionByZero { message: &'a str },
    InvalidInput { field: &'a str, reason: &'a str },
}

struct Calculator;

#[zlink::service(types = [CalculationResult, Greeting])]
impl Calculator {
    #[zlink(interface = "org.example.BorrowedCalc")]
    async fn divide(
        &self,
        dividend: f64,
        divisor: f64,
    ) -> Result<CalculationResult, CalculatorError<'_>> {
        if divisor == 0.0 {
            Err(CalculatorError::DivisionByZero {
                message: "Cannot divide by zero",
            })
        } else if !(-1000000.0..=1000000.0).contains(&dividend) {
            Err(CalculatorError::InvalidInput {
                field: "dividend",
                reason: "must be within range",
            })
        } else {
            Ok(CalculationResult {
                result: dividend / divisor,
            })
        }
    }

    // Method that takes a borrowed param.
    async fn greet(&self, name: &str) -> Greeting {
        Greeting {
            message: format!("Hello, {name}!"),
        }
    }
}

// Proxy with borrowed error type.
#[zlink::proxy("org.example.BorrowedCalc")]
trait CalculatorProxy {
    async fn divide(
        &mut self,
        dividend: f64,
        divisor: f64,
    ) -> zlink::Result<Result<CalculationResult, CalculatorError<'_>>>;
    async fn greet(&mut self, name: &str) -> zlink::Result<Result<Greeting, CalculatorError<'_>>>;
}
