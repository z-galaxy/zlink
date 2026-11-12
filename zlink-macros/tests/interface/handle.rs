//! Tests for the `Interface` implementation, driven through `Interface::handle` with no service.

use serde::{Deserialize, Serialize};
use zlink::{
    Call, Connection, introspect,
    service::{Interface, MethodReply},
    test_utils::mock_socket::MockSocket,
};

#[test]
fn implemented_for_any_state() {
    assert_interface::<()>();
    assert_interface::<u64>();
    assert_interface::<String>();

    let description = <Greeter as Interface<MockSocket>>::DESCRIPTION;
    assert_eq!(description.name().as_str(), "org.example.greeter");
}

#[test_log::test(tokio::test)]
async fn calls_are_handled() {
    let mut greeter = Greeter {
        greeting: String::from("hello"),
    };

    let json = r#"{"method":"org.example.greeter.Greet","parameters":{"name":"world"}}"#;
    assert_eq!(
        dispatch(&mut greeter, json).await.unwrap(),
        r#"{"text":"hello world"}"#
    );

    // The interface knows only its own methods.
    let json = r#"{"method":"org.example.greeter.Nope"}"#;
    let err = dispatch(&mut greeter, json).await.unwrap_err();
    assert!(err.contains("org.varlink.service.MethodNotFound"), "{err}");
}

/// Dispatch a call, given as JSON, to `greeter` as an interface and return the JSON of the
/// parameters of its reply, or of its error.
async fn dispatch(greeter: &mut Greeter, json: &str) -> Result<String, String> {
    let method: <Greeter as Interface<MockSocket>>::MethodCall<'_> =
        serde_json::from_str(json).unwrap();
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let mut state = ();
    let (reply, _fds) = <Greeter as Interface<MockSocket>>::handle(
        greeter,
        &mut state,
        Call::new(&method),
        &mut conn,
        Vec::new(),
    )
    .await;

    match reply {
        MethodReply::Single(Some(params)) => Ok(serde_json::to_string(&params).unwrap()),
        MethodReply::Error(err) => Err(serde_json::to_string(&err).unwrap()),
        reply => panic!("{json} got an unexpected reply: {reply:?}"),
    }
}

/// `Greeter` is an interface of a service with `State`.
fn assert_interface<State>()
where
    Greeter: Interface<MockSocket, State>,
{
}

struct Greeter {
    greeting: String,
}

#[zlink::interface("org.example.greeter")]
impl Greeter {
    /// Greet someone.
    async fn greet(&self, name: String) -> Greeting {
        Greeting {
            text: format!("{} {name}", self.greeting),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Greeting {
    text: String,
}
