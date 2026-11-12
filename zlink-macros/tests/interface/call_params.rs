//! Tests for how the parameters of an interface method are read from a call.
//!
//! The calls are dispatched to the services of the interfaces from the JSON that a client sends,
//! so that what is on the wire is what is under test.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use zlink::{
    Call, Connection, Service, introspect, service::MethodReply,
    test_utils::mock_socket::MockSocket,
};

// What a call with nothing to pass can have for its `parameters`: nothing at all, an empty object
// (e.g. what `varlinkctl call .. '{}'` sends) or `null`.
const NO_PARAMETERS: [Option<&str>; 3] = [None, Some("{}"), Some("null")];

#[test_log::test(tokio::test)]
async fn method_without_parameters() {
    let mut service = OptionalsService(Optionals);

    for parameters in NO_PARAMETERS {
        let json = optionals_call("Ping", parameters);
        assert_eq!(
            dispatch(&mut service, &json).await.unwrap(),
            r#"{"text":"pong"}"#,
            "{json}"
        );
    }

    // Parameters that the method doesn't take are rejected.
    let json = optionals_call("Ping", Some(r#"{"unexpected":true}"#));
    dispatch(&mut service, &json).await.unwrap_err();
}

#[test_log::test(tokio::test)]
async fn method_with_optional_parameters() {
    let mut service = OptionalsService(Optionals);

    for parameters in NO_PARAMETERS {
        let json = optionals_call("Greet", parameters);
        assert_eq!(
            dispatch(&mut service, &json).await.unwrap(),
            r#"{"text":"hello nobody"}"#,
            "{json}"
        );
    }

    // They can still be passed, in part or in whole, and unknown ones are ignored.
    let json = optionals_call("Greet", Some(r#"{"times":2}"#));
    assert_eq!(
        dispatch(&mut service, &json).await.unwrap(),
        r#"{"text":"hello nobody hello nobody"}"#
    );
    let json = optionals_call("Greet", Some(r#"{"name":"you"}"#));
    assert_eq!(
        dispatch(&mut service, &json).await.unwrap(),
        r#"{"text":"hello you"}"#
    );
    let json = optionals_call("Greet", Some(r#"{"times":2,"name":"you","unknown":0}"#));
    assert_eq!(
        dispatch(&mut service, &json).await.unwrap(),
        r#"{"text":"hello you hello you"}"#
    );
}

#[test_log::test(tokio::test)]
async fn method_with_required_parameters() {
    let mut service = OptionalsService(Optionals);

    // A required parameter can't be left out, whichever way the call leaves it out.
    for parameters in NO_PARAMETERS {
        let json = optionals_call("Lookup", parameters);
        dispatch(&mut service, &json).await.unwrap_err();
    }
    let json = optionals_call("Lookup", Some(r#"{"label":"a"}"#));
    dispatch(&mut service, &json).await.unwrap_err();

    let json = optionals_call("Lookup", Some(r#"{"id":7}"#));
    assert_eq!(
        dispatch(&mut service, &json).await.unwrap(),
        r#"{"text":"7"}"#
    );
    let json = optionals_call("Lookup", Some(r#"{"id":7,"label":"a"}"#));
    assert_eq!(
        dispatch(&mut service, &json).await.unwrap(),
        r#"{"text":"7 a"}"#
    );
}

#[test_log::test(tokio::test)]
async fn parameters_before_method() {
    let mut service = OptionalsService(Optionals);

    let jsons = [
        r#"{"parameters":{},"method":"org.example.optionals.Ping"}"#,
        r#"{"parameters":null,"method":"org.example.optionals.Ping"}"#,
    ];
    for json in jsons {
        assert_eq!(
            dispatch(&mut service, json).await.unwrap(),
            r#"{"text":"pong"}"#,
            "{json}"
        );
    }

    let jsons = [
        r#"{"parameters":{},"method":"org.example.optionals.Greet"}"#,
        r#"{"parameters":null,"method":"org.example.optionals.Greet"}"#,
        r#"{"more":false,"parameters":{},"method":"org.example.optionals.Greet"}"#,
    ];
    for json in jsons {
        assert_eq!(
            dispatch(&mut service, json).await.unwrap(),
            r#"{"text":"hello nobody"}"#,
            "{json}"
        );
    }
}

// A method can borrow from the call, no matter how the other methods of its interface get their
// parameters.
#[test_log::test(tokio::test)]
async fn borrowed_parameters() {
    let json = r#"{"method":"org.example.joiner.Join","parameters":{"parts":["a","b"]}}"#;
    assert_eq!(
        dispatch(&mut JoinerService(Joiner), json).await.unwrap(),
        r#"{"text":"a+b"}"#
    );

    let json = r#"{"method":"org.example.noter.Note","parameters":{"note":{"title":"a"}}}"#;
    assert_eq!(
        dispatch(&mut NoterService(Noter), json).await.unwrap(),
        r#"{"text":"a"}"#
    );

    let calls_and_replies = [
        (
            r#"{"method":"org.example.borrower.Join","parameters":{"parts":["a","b"]}}"#,
            r#"{"text":"a+b"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Shout","parameters":{"text":"a"}}"#,
            r#"{"text":"A!"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Maybe","parameters":{"text":"a"}}"#,
            r#"{"text":"a"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Maybe"}"#,
            r#"{"text":""}"#,
        ),
        // A `Cow` takes the escapes in a string in its stride.
        (
            r#"{"method":"org.example.borrower.Cow","parameters":{"text":"a\nb"}}"#,
            r#"{"text":"a\nb"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Note","parameters":{"note":{"title":"a"}}}"#,
            r#"{"text":"a"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Count","parameters":{"count":3}}"#,
            r#"{"text":"3"}"#,
        ),
        (
            r#"{"method":"org.example.borrower.Ping"}"#,
            r#"{"text":"pong"}"#,
        ),
    ];
    let mut service = BorrowerService(Borrower);
    for (json, reply) in calls_and_replies {
        assert_eq!(dispatch(&mut service, json).await.unwrap(), reply, "{json}");
    }
}

#[test_log::test(tokio::test)]
async fn parameters_named_like_markers() {
    let mut service = NamesService(Names);

    let json = r#"{"method":"org.example.names.Marker","parameters":{"marker":1}}"#;
    assert_eq!(
        dispatch(&mut service, json).await.unwrap(),
        r#"{"text":"1"}"#
    );

    let json = r#"{"method":"org.example.names.ZlinkMarkers","parameters":{"first":1,"second":2}}"#;
    assert_eq!(
        dispatch(&mut service, json).await.unwrap(),
        r#"{"text":"1 2"}"#
    );

    let json = r#"{"method":"org.example.names.RawZlinkMarker","parameters":{"value":1}}"#;
    assert_eq!(
        dispatch(&mut service, json).await.unwrap(),
        r#"{"text":"1"}"#
    );
}

/// The JSON of a call to `method` of the `Optionals` interface.
fn optionals_call(method: &str, parameters: Option<&str>) -> String {
    match parameters {
        Some(parameters) => {
            format!(r#"{{"method":"org.example.optionals.{method}","parameters":{parameters}}}"#)
        }
        None => format!(r#"{{"method":"org.example.optionals.{method}"}}"#),
    }
}

/// Dispatch a call, given as JSON, to `service` and return the JSON of the reply.
///
/// This fails if the call can't be read, in which case a server closes the connection.
async fn dispatch<S>(service: &mut S, json: &str) -> serde_json::Result<String>
where
    S: Service<MockSocket>,
{
    let call: Call<S::MethodCall<'_>> = serde_json::from_str(json)?;
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let (reply, _fds) = service.handle(&call, &mut conn, Vec::new()).await;
    let MethodReply::Single(Some(params)) = reply else {
        panic!("{json} wasn't replied to with parameters");
    };

    serde_json::to_string(&params)
}

/// The reply of all the methods: the parameters they got, in a way that tells them apart.
#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Received {
    text: String,
}

impl Received {
    fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(zlink::Service)]
struct OptionalsService(Optionals);

struct Optionals;

#[zlink::interface("org.example.optionals")]
impl Optionals {
    /// Takes no parameters.
    async fn ping(&self) -> Received {
        Received::new("pong")
    }

    /// Takes only optional parameters.
    async fn greet(&self, name: Option<String>, times: Option<u32>) -> Received {
        let greeting = format!("hello {}", name.as_deref().unwrap_or("nobody"));

        Received::new(vec![greeting; times.unwrap_or(1) as usize].join(" "))
    }

    /// Takes a required parameter and an optional one.
    async fn lookup(&self, id: u32, label: Option<String>) -> Received {
        match label {
            Some(label) => Received::new(format!("{id} {label}")),
            None => Received::new(id.to_string()),
        }
    }
}

// An interface whose only method borrows from the call through a type that `serde` doesn't borrow
// for on its own.
#[derive(zlink::Service)]
struct JoinerService(Joiner);

struct Joiner;

#[zlink::interface("org.example.joiner")]
impl Joiner {
    async fn join(&self, parts: Vec<&str>) -> Received {
        Received::new(parts.join("+"))
    }
}

// The same, with a type of the user.
#[derive(zlink::Service)]
struct NoterService(Noter);

struct Noter;

#[zlink::interface("org.example.noter")]
impl Noter {
    async fn note(&self, note: Note<'_>) -> Received {
        Received::new(note.title)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, introspect::Type)]
struct Note<'a> {
    title: &'a str,
}

// An interface with methods that borrow in different ways, next to ones that don't.
#[derive(zlink::Service)]
struct BorrowerService(Borrower);

struct Borrower;

#[zlink::interface("org.example.borrower")]
impl Borrower {
    async fn join(&self, parts: Vec<&str>) -> Received {
        Received::new(parts.join("+"))
    }

    async fn shout(&self, text: &str) -> Received {
        Received::new(format!("{}!", text.to_uppercase()))
    }

    async fn maybe(&self, text: Option<&str>) -> Received {
        Received::new(text.unwrap_or_default())
    }

    async fn cow(&self, text: Cow<'_, str>) -> Received {
        Received::new(text)
    }

    async fn note(&self, note: Note<'_>) -> Received {
        Received::new(note.title)
    }

    async fn count(&self, count: u32) -> Received {
        Received::new(count.to_string())
    }

    async fn ping(&self) -> Received {
        Received::new("pong")
    }
}

// An interface with parameters that are named like the fields that the macro may add to the types
// it reads the parameters into.
#[derive(zlink::Service)]
struct NamesService(Names);

struct Names;

#[zlink::interface("org.example.names")]
impl Names {
    async fn marker(&self, #[zlink(rename = "marker")] __marker: u32) -> Received {
        Received::new(__marker.to_string())
    }

    // The names that the macro tries for its own field, one after the other.
    async fn zlink_markers(
        &self,
        #[zlink(rename = "first")] __zlink_marker: u32,
        #[zlink(rename = "second")] __zlink_marker_: u32,
    ) -> Received {
        Received::new(format!("{__zlink_marker} {__zlink_marker_}"))
    }

    // The first of them, spelled as a raw identifier.
    async fn raw_zlink_marker(&self, #[zlink(rename = "value")] r#__zlink_marker: u32) -> Received {
        Received::new(r#__zlink_marker.to_string())
    }
}
