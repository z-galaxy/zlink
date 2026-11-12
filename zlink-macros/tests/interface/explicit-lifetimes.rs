//! Build-only test: verifies that explicit lifetime parameters on interface methods compile.
//!
//! Before the fix, `&'a str` in introspection `const` contexts caused a compilation error
//! because `'a` was not in scope.

use serde::{Deserialize, Serialize};
use zlink::introspect::{self, CustomType};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct EchoReply {
    message: String,
}

#[derive(Debug, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.ExplicitLifetimes")]
enum EchoError<'a> {
    InvalidInput { reason: &'a str },
}

#[derive(zlink::Service)]
struct EchoService(Echo);

struct Echo;

// The explicit lifetimes are the point of this test: the interface macro must handle them.
#[allow(clippy::needless_lifetimes)]
#[zlink::interface("org.example.ExplicitLifetimes", types = [EchoReply])]
impl Echo {
    async fn echo<'a>(&self, msg: &'a str) -> Result<EchoReply, EchoError<'_>> {
        if msg.is_empty() {
            Err(EchoError::InvalidInput {
                reason: "empty input",
            })
        } else {
            Ok(EchoReply {
                message: msg.to_string(),
            })
        }
    }
}

#[test]
fn explicit_lifetimes_compile() {
    // Just verify the macro-generated code compiles — the interface, its error type and the
    // introspection constants all reference explicit lifetimes that must be normalized, and the
    // service combines them.
    let _service = EchoService(Echo);
}
