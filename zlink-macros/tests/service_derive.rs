#![cfg(feature = "service")]

//! Tests for `#[derive(Service)]`, with interfaces that implement `Interface` by hand.

use core::{
    pin::Pin,
    task::{Context, Poll},
};
use futures_util::{Stream, StreamExt, stream::Empty};
use serde::{Deserialize, Serialize};
use std::os::fd::OwnedFd;
use zlink::{
    Call, Connection, Reply,
    connection::Socket,
    idl::{Interface as Description, Method},
    names::{InterfaceName, TypeName},
    service::{
        HandleResult, Infallible, Interface, MethodReply, ReplyStreamItem, Service, TakeOver,
        Upgraded,
    },
    test_utils::mock_socket::MockSocket,
    varlink_service::{self, InterfaceDescription},
};

#[test]
fn method_first() {
    let call = parse::<Svc>(r#"{"method":"org.example.metrics.Tag","parameters":{"tag":"a"}}"#);
    assert_eq!(method_of(&call), r#"Tag { tag: "a" }"#);
}

#[test]
fn flags_are_kept() {
    let call = parse::<Svc>(r#"{"method":"org.example.metrics.Get","more":true,"oneway":true}"#);
    assert!(call.more());
    assert!(call.oneway());
    assert!(!call.upgrade());
}

#[test]
fn parameters_first() {
    let call = parse::<Svc>(r#"{"parameters":{"tag":"a"},"method":"org.example.metrics.Tag"}"#);
    assert_eq!(method_of(&call), r#"Tag { tag: "a" }"#);

    let call = parse::<Svc>(r#"{"parameters":{},"method":"org.varlink.service.GetInfo"}"#);
    assert_eq!(method_of(&call), "GetInfo");

    let call = parse::<Svc>(r#"{"more":true,"method":"org.example.peek.Get"}"#);
    assert_eq!(method_of(&call), "Current");
    assert!(call.more());
}

#[test]
fn the_interface_is_found_by_the_name_of_the_method() {
    // Both of these interfaces have a method called `Get`.
    for (json, expected) in [
        (r#"{"method":"org.example.metrics.Get"}"#, "Get"),
        (r#"{"method":"org.example.log.Get"}"#, "Lines"),
        (r#"{"method":"org.example.peek.Get"}"#, "Current"),
        (r#"{"method":"org.varlink.service.GetInfo"}"#, "GetInfo"),
    ] {
        assert_eq!(method_of(&parse::<Svc>(json)), expected, "{json}");
    }
}

#[test]
fn unknown_methods_keep_their_name() {
    for json in [
        r#"{"method":"org.example.nope.Get","parameters":{"a":[1,2]}}"#,
        r#"{"parameters":{"a":[1,2]},"method":"org.example.nope.Get"}"#,
        r#"{"method":"org.example.nope.Get"}"#,
    ] {
        assert_eq!(
            method_of(&parse::<Svc>(json)),
            r#""org.example.nope.Get""#,
            "{json}"
        );
    }

    // A method that is not in the interface that its name says.
    for json in [
        r#"{"method":"org.example.log.Nope"}"#,
        r#"{"parameters":{},"method":"org.example.log.Nope"}"#,
    ] {
        assert_eq!(
            method_of(&parse::<Svc>(json)),
            r#""org.example.log.Nope""#,
            "{json}"
        );
    }
    let call = parse::<Svc>(r#"{"method":"org.varlink.service.Nope"}"#);
    assert_eq!(method_of(&call), r#""org.varlink.service.Nope""#);
}

#[test]
fn invalid_parameters_are_an_error() {
    for json in [
        r#"{"method":"org.example.metrics.Tag","parameters":{"tag":1}}"#,
        r#"{"method":"org.example.metrics.Tag"}"#,
        r#"{"parameters":{"tag":1},"method":"org.example.metrics.Tag"}"#,
        r#"{"parameters":{"other":"a"},"method":"org.example.metrics.Tag"}"#,
        r#"{"method":"org.varlink.service.GetInterfaceDescription","parameters":{}}"#,
        r#"{"parameters":{},"method":"org.varlink.service.GetInterfaceDescription"}"#,
    ] {
        let result =
            serde_json::from_str::<Call<<Svc as Service<MockSocket>>::MethodCall<'_>>>(json);
        assert!(result.is_err(), "{json}");
    }
}

#[test]
fn interfaces_of_the_interface_macro_only_get_their_own_calls() {
    // An interface that the `interface` macro implements takes the calls to the methods it doesn't
    // have, if they come without parameters, to report the methods as not found. As part of a
    // service, it must not get the calls to the methods of the interfaces after it, in whichever
    // order the fields of a call come.
    for (json, method) in [
        (r#"{"method":"org.example.second.Get"}"#, "Second"),
        (
            r#"{"parameters":null,"method":"org.example.second.Get"}"#,
            "Second",
        ),
        (r#"{"x":1,"method":"org.example.second.Get"}"#, "Second"),
        (
            r#"{"parameters":{"name":"a"},"method":"org.example.second.Name"}"#,
            "Second",
        ),
        (
            r#"{"parameters":{},"method":"org.example.first.Get"}"#,
            "First",
        ),
        (
            r#"{"x":1,"method":"org.varlink.service.GetInfo"}"#,
            "GetInfo",
        ),
    ] {
        let call = parse::<Generated>(json);
        let got = method_of(&call);
        assert!(got.contains(method), "{json}: {got}");
    }

    // A method that no interface has is named, with or without parameters.
    for json in [
        r#"{"parameters":null,"method":"org.example.second.Nope"}"#,
        r#"{"parameters":{"a":1},"method":"org.example.third.Get"}"#,
        r#"{"x":1,"method":"org.example.third.Get"}"#,
    ] {
        let call = parse::<Generated>(json);
        let name = json
            .rsplit_once(r#""method":"#)
            .unwrap()
            .1
            .trim_end_matches('}');
        assert_eq!(method_of(&call), name, "{json}");
    }

    // Parameters that don't fit the method are an error, not a method that isn't found.
    for json in [
        r#"{"parameters":{"name":1},"method":"org.example.second.Name"}"#,
        r#"{"parameters":null,"method":"org.example.second.Name"}"#,
        r#"{"method":"org.example.second.Name","parameters":{"name":1}}"#,
    ] {
        let result =
            serde_json::from_str::<Call<<Generated as Service<MockSocket>>::MethodCall<'_>>>(json);
        assert!(result.is_err(), "{json}");
    }
}

#[test]
fn tuple_structs_are_services() {
    let call = parse::<Pair>(r#"{"method":"org.example.greeter.Get"}"#);
    assert_eq!(method_of(&call), "Greet");
    let call = parse::<Pair>(r#"{"method":"org.example.log.Get"}"#);
    assert_eq!(method_of(&call), "Lines");
}

#[tokio::test]
async fn tuple_structs_route_calls() {
    let mut pair = Pair(
        Log {
            lines: vec!["a".into()],
        },
        Greeter,
    );

    assert_eq!(
        handle(&mut pair, r#"{"method":"org.example.greeter.Get"}"#).await,
        Outcome::Single(r#"{"greeting":"hello"}"#.into())
    );
    assert_eq!(
        handle(&mut pair, r#"{"method":"org.example.log.Get"}"#).await,
        Outcome::Single(r#"{"lines":["a"]}"#.into())
    );
    let Outcome::Single(info) =
        handle(&mut pair, r#"{"method":"org.varlink.service.GetInfo"}"#).await
    else {
        panic!("no info");
    };
    // No metadata was given.
    assert_eq!(
        info,
        concat!(
            r#"{"vendor":"","product":"","version":"","url":"","#,
            r#""interfaces":["org.example.log","org.example.greeter","org.varlink.service"]}"#
        )
    );
}

#[tokio::test]
async fn calls_are_routed_to_their_interface() {
    let mut svc = svc();

    assert_eq!(
        handle(&mut svc, r#"{"method":"org.example.metrics.Get"}"#).await,
        Outcome::Single(r#"{"hits":1}"#.into())
    );
    assert_eq!(
        handle(
            &mut svc,
            r#"{"method":"org.example.metrics.Tag","parameters":{"tag":"x"}}"#
        )
        .await,
        Outcome::Single(r#"{"tag":"x"}"#.into())
    );
    assert_eq!(
        handle(
            &mut svc,
            r#"{"parameters":{"tag":"y"},"method":"org.example.metrics.Tag"}"#
        )
        .await,
        Outcome::Single(r#"{"tag":"y"}"#.into())
    );
    // The reply of this one borrows from the interface.
    svc.log.lines = vec!["one".into(), "two".into()];
    assert_eq!(
        handle(&mut svc, r#"{"method":"org.example.log.Get"}"#).await,
        Outcome::Single(r#"{"lines":["one","two"]}"#.into())
    );
}

#[tokio::test]
async fn interfaces_share_state() {
    let mut svc = svc();

    handle(&mut svc, r#"{"method":"org.example.metrics.Get"}"#).await;
    handle(&mut svc, r#"{"method":"org.example.metrics.Get"}"#).await;
    assert_eq!(svc.hits, 2);
    assert_eq!(
        handle(&mut svc, r#"{"method":"org.example.peek.Get"}"#).await,
        Outcome::Single(r#"{"hits":2}"#.into())
    );
}

#[tokio::test]
async fn errors_are_serialized_as_they_are() {
    let mut svc = svc();

    let expected = serde_json::to_string(&LogError::Empty { count: &0 }).unwrap();
    assert_eq!(
        handle(&mut svc, r#"{"method":"org.example.log.Get"}"#).await,
        Outcome::Error(expected)
    );
}

#[tokio::test]
async fn streams_are_forwarded() {
    let mut svc = svc();

    assert_eq!(
        handle(
            &mut svc,
            r#"{"method":"org.example.metrics.Get","more":true}"#
        )
        .await,
        Outcome::Multi(vec![r#"{"hits":2}"#.into(), r#"{"hits":1}"#.into()])
    );
}

#[tokio::test]
async fn upgrades_are_forwarded() {
    let mut svc = svc();

    assert_eq!(
        handle(
            &mut svc,
            r#"{"method":"org.example.metrics.Get","upgrade":true}"#
        )
        .await,
        Outcome::Upgrade(r#"{"hits":1}"#.into())
    );
}

#[tokio::test]
async fn get_info_lists_the_interfaces() {
    let mut svc = svc();

    let info = varlink_service::Info::new(
        "Acme",
        "Test service",
        "1.2",
        "https://example.org",
        [
            "org.example.metrics",
            "org.example.log",
            "org.example.peek",
            "org.varlink.service",
        ],
    )
    .unwrap();
    let expected = serde_json::to_string(&varlink_service::Reply::Info(info)).unwrap();
    assert_eq!(
        handle(&mut svc, r#"{"method":"org.varlink.service.GetInfo"}"#).await,
        Outcome::Single(expected)
    );
}

#[tokio::test]
async fn interface_descriptions_are_replied() {
    let mut svc = svc();

    let descriptions = [
        (
            "org.example.metrics",
            <Metrics as Interface<MockSocket, u64>>::DESCRIPTION,
        ),
        (
            "org.example.log",
            <Log as Interface<MockSocket, u64>>::DESCRIPTION,
        ),
        (
            "org.example.peek",
            <Peek as Interface<MockSocket, u64>>::DESCRIPTION,
        ),
        ("org.varlink.service", varlink_service::DESCRIPTION),
    ];
    for (name, description) in descriptions {
        let json = description_call(name);
        let expected = serde_json::to_string(&varlink_service::Reply::InterfaceDescription(
            InterfaceDescription::from(description),
        ))
        .unwrap();
        assert_eq!(
            handle(&mut svc, &json).await,
            Outcome::Single(expected),
            "{name}"
        );
    }

    let json = description_call("org.example.nope");
    let expected = serde_json::to_string(&varlink_service::Error::InterfaceNotFound {
        interface: "org.example.nope".into(),
    })
    .unwrap();
    assert_eq!(handle(&mut svc, &json).await, Outcome::Error(expected));
}

#[tokio::test]
async fn unknown_methods_are_not_found() {
    let mut svc = svc();

    for json in [
        r#"{"method":"org.example.nope.Get","parameters":{"x":1}}"#,
        r#"{"parameters":{"x":1},"method":"org.example.nope.Get"}"#,
    ] {
        let expected = serde_json::to_string(&varlink_service::Error::MethodNotFound {
            method: "org.example.nope.Get".into(),
        })
        .unwrap();
        assert_eq!(
            handle(&mut svc, json).await,
            Outcome::Error(expected),
            "{json}"
        );
    }
    // The state wasn't touched.
    assert_eq!(svc.hits, 0);
}

#[tokio::test]
async fn varlink_methods_refuse_upgrades() {
    let mut svc = svc();

    let expected = serde_json::to_string(&varlink_service::Error::InvalidParameter {
        parameter: "upgrade".into(),
    })
    .unwrap();
    for json in [
        r#"{"method":"org.varlink.service.GetInfo","upgrade":true}"#,
        r#"{"method":"org.varlink.service.GetInfo","upgrade":true,"parameters":{}}"#,
    ] {
        assert_eq!(
            handle(&mut svc, json).await,
            Outcome::Error(expected.clone()),
            "{json}"
        );
    }
}

fn svc() -> Svc {
    Svc {
        hits: 0,
        metrics: Metrics,
        log: Log { lines: Vec::new() },
        peek: Peek,
    }
}

// The first field is the state that the interfaces share, which `Metrics` counts hits in and
// `Peek` reads from.
#[derive(zlink::Service)]
#[zlink(
    vendor = "Acme",
    product = "Test service",
    version = "1.2",
    url = "https://example.org"
)]
struct Svc {
    #[zlink(state)]
    hits: u64,
    metrics: Metrics,
    log: Log,
    peek: Peek,
}

// A tuple struct, whose interfaces don't need any state.
#[derive(zlink::Service)]
struct Pair(Log, Greeter);

// A service whose interfaces the `interface` macro implements.
#[derive(zlink::Service)]
struct Generated(First, Second);

// The JSON of a call asking for the description of `interface`.
fn description_call(interface: &str) -> String {
    format!(
        concat!(
            r#"{{"method":"org.varlink.service.GetInterfaceDescription","#,
            r#""parameters":{{"interface":"{interface}"}}}}"#
        ),
        interface = interface
    )
}

async fn handle<S>(service: &mut S, json: &str) -> Outcome
where
    S: Service<MockSocket>,
{
    let call: Call<S::MethodCall<'_>> = serde_json::from_str(json).unwrap();
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let (reply, fds) = service.handle(&call, &mut conn, Vec::new()).await;
    assert!(fds.is_empty());

    match reply {
        MethodReply::Single(params) => Outcome::Single(serde_json::to_string(&params).unwrap()),
        MethodReply::Error(error) => Outcome::Error(serde_json::to_string(&error).unwrap()),
        MethodReply::Multi(mut stream) => {
            let mut items = Vec::new();
            while let Some((item, _fds)) = stream.next().await {
                let Ok(reply) = item else {
                    panic!("error in the stream");
                };
                items.push(serde_json::to_string(&reply.into_parameters()).unwrap());
            }

            Outcome::Multi(items)
        }
        MethodReply::Upgrade(params, _state) => {
            Outcome::Upgrade(serde_json::to_string(&params).unwrap())
        }
        _ => panic!("unexpected reply"),
    }
}

// What a call was replied with, as JSON.
#[derive(Debug, PartialEq)]
enum Outcome {
    Single(String),
    Error(String),
    Multi(Vec<String>),
    Upgrade(String),
}

fn parse<S>(json: &str) -> Call<S::MethodCall<'_>>
where
    S: Service<MockSocket>,
{
    serde_json::from_str(json).unwrap()
}

// The debug output of the method of the call.
fn method_of<M>(call: &Call<M>) -> String
where
    M: core::fmt::Debug,
{
    format!("{:?}", call.method())
}

// An interface with two methods, a stream and an upgrade, that counts calls in the state.
struct Metrics;

impl<Sock> Interface<Sock, u64> for Metrics
where
    Sock: Socket,
{
    const DESCRIPTION: &'static Description<'static> = &Description::new(
        InterfaceName::from_static_str_unchecked("org.example.metrics"),
        &[GET, TAG],
        &[],
        &[],
        &[],
    );
    type MethodCall<'de> = MetricsCall<'de>;
    type ReplyParams<'ser> = MetricsReply;
    type ReplyStreamParams = MetricsReply;
    type ReplyStreamError = Infallible;
    type ReplyStream = Ticks;
    type ReplyError<'ser> = Infallible;
    type Upgrade = Raw;

    async fn handle<'ser>(
        &'ser mut self,
        state: &'ser mut u64,
        call: Call<&'ser <Self as Interface<Sock, u64>>::MethodCall<'_>>,
        _conn: &mut Connection<Sock>,
        _fds: Vec<OwnedFd>,
    ) -> HandleResult<
        <Self as Interface<Sock, u64>>::ReplyParams<'ser>,
        Ticks,
        <Self as Interface<Sock, u64>>::ReplyError<'ser>,
        Raw,
    > {
        let reply = match call.method() {
            MetricsCall::Get => {
                *state += 1;
                let hits = *state;
                if call.more() {
                    MethodReply::Multi(Ticks(2))
                } else if call.upgrade() {
                    MethodReply::Upgrade(Some(MetricsReply::Hits { hits }), Raw)
                } else {
                    MethodReply::Single(Some(MetricsReply::Hits { hits }))
                }
            }
            MetricsCall::Tag { tag } => {
                MethodReply::Single(Some(MetricsReply::Tag { tag: (*tag).into() }))
            }
        };

        (reply, Vec::new())
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "method", content = "parameters")]
enum MetricsCall<'a> {
    #[serde(rename = "org.example.metrics.Get")]
    Get,
    #[serde(rename = "org.example.metrics.Tag")]
    Tag { tag: &'a str },
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum MetricsReply {
    Hits { hits: u64 },
    Tag { tag: String },
}

struct Raw;

impl<Sock> TakeOver<Sock> for Raw
where
    Sock: Socket,
{
    async fn take_over(self, _socket: Upgraded<Sock>) {}
}

struct Ticks(u64);

impl Stream for Ticks {
    type Item = ReplyStreamItem<MetricsReply, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.0 == 0 {
            return Poll::Ready(None);
        }
        let hits = self.0;
        self.0 -= 1;

        Poll::Ready(Some((
            Ok(Reply::new(Some(MetricsReply::Hits { hits }))),
            Vec::new(),
        )))
    }
}

// An interface that works with any state, and whose replies borrow from itself.
struct Log {
    lines: Vec<String>,
}

impl<Sock, State> Interface<Sock, State> for Log
where
    Sock: Socket,
{
    const DESCRIPTION: &'static Description<'static> = &Description::new(
        InterfaceName::from_static_str_unchecked("org.example.log"),
        &[GET],
        &[],
        &[],
        &[],
    );
    type MethodCall<'de> = LogCall;
    type ReplyParams<'ser>
        = Lines<'ser>
    where
        State: 'ser;
    type ReplyStreamParams = ();
    type ReplyStreamError = Infallible;
    type ReplyStream = Empty<ReplyStreamItem<(), Infallible>>;
    type ReplyError<'ser>
        = LogError<'ser>
    where
        State: 'ser;
    type Upgrade = ();

    async fn handle<'ser>(
        &'ser mut self,
        _state: &'ser mut State,
        _call: Call<&'ser <Self as Interface<Sock, State>>::MethodCall<'_>>,
        _conn: &mut Connection<Sock>,
        _fds: Vec<OwnedFd>,
    ) -> HandleResult<
        <Self as Interface<Sock, State>>::ReplyParams<'ser>,
        Empty<ReplyStreamItem<(), Infallible>>,
        <Self as Interface<Sock, State>>::ReplyError<'ser>,
        (),
    > {
        let reply = if self.lines.is_empty() {
            MethodReply::Error(LogError::Empty { count: &0 })
        } else {
            MethodReply::Single(Some(Lines { lines: &self.lines }))
        };

        (reply, Vec::new())
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "method", content = "parameters")]
enum LogCall {
    #[serde(rename = "org.example.log.Get")]
    Lines,
}

#[derive(Serialize, Debug)]
struct Lines<'a> {
    lines: &'a [String],
}

#[derive(Serialize, Debug)]
#[serde(tag = "error", content = "parameters")]
enum LogError<'a> {
    #[serde(rename = "org.example.log.Empty")]
    Empty { count: &'a usize },
}

// An interface that reads the state.
struct Peek;

impl<Sock> Interface<Sock, u64> for Peek
where
    Sock: Socket,
{
    const DESCRIPTION: &'static Description<'static> = &Description::new(
        InterfaceName::from_static_str_unchecked("org.example.peek"),
        &[GET],
        &[],
        &[],
        &[],
    );
    type MethodCall<'de> = PeekCall;
    type ReplyParams<'ser> = Hits;
    type ReplyStreamParams = ();
    type ReplyStreamError = Infallible;
    type ReplyStream = Empty<ReplyStreamItem<(), Infallible>>;
    type ReplyError<'ser> = Infallible;
    type Upgrade = ();

    async fn handle<'ser>(
        &'ser mut self,
        state: &'ser mut u64,
        _call: Call<&'ser <Self as Interface<Sock, u64>>::MethodCall<'_>>,
        _conn: &mut Connection<Sock>,
        _fds: Vec<OwnedFd>,
    ) -> HandleResult<
        <Self as Interface<Sock, u64>>::ReplyParams<'ser>,
        Empty<ReplyStreamItem<(), Infallible>>,
        <Self as Interface<Sock, u64>>::ReplyError<'ser>,
        (),
    > {
        (MethodReply::Single(Some(Hits { hits: *state })), Vec::new())
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "method", content = "parameters")]
enum PeekCall {
    #[serde(rename = "org.example.peek.Get")]
    Current,
}

#[derive(Serialize, Debug)]
struct Hits {
    hits: u64,
}

// A second interface that works with any state.
struct Greeter;

impl<Sock, State> Interface<Sock, State> for Greeter
where
    Sock: Socket,
{
    const DESCRIPTION: &'static Description<'static> = &Description::new(
        InterfaceName::from_static_str_unchecked("org.example.greeter"),
        &[GET],
        &[],
        &[],
        &[],
    );
    type MethodCall<'de> = GreeterCall;
    type ReplyParams<'ser>
        = Greeting
    where
        State: 'ser;
    type ReplyStreamParams = ();
    type ReplyStreamError = Infallible;
    type ReplyStream = Empty<ReplyStreamItem<(), Infallible>>;
    type ReplyError<'ser>
        = Infallible
    where
        State: 'ser;
    type Upgrade = ();

    async fn handle<'ser>(
        &'ser mut self,
        _state: &'ser mut State,
        _call: Call<&'ser <Self as Interface<Sock, State>>::MethodCall<'_>>,
        _conn: &mut Connection<Sock>,
        _fds: Vec<OwnedFd>,
    ) -> HandleResult<
        <Self as Interface<Sock, State>>::ReplyParams<'ser>,
        Empty<ReplyStreamItem<(), Infallible>>,
        <Self as Interface<Sock, State>>::ReplyError<'ser>,
        (),
    > {
        (
            MethodReply::Single(Some(Greeting { greeting: "hello" })),
            Vec::new(),
        )
    }
}

#[derive(Deserialize, Debug)]
#[serde(tag = "method", content = "parameters")]
enum GreeterCall {
    #[serde(rename = "org.example.greeter.Get")]
    Greet,
}

#[derive(Serialize, Debug)]
struct Greeting {
    greeting: &'static str,
}

const GET: &Method<'static> =
    &Method::new(TypeName::from_static_str_unchecked("Get"), &[], &[], &[]);
const TAG: &Method<'static> =
    &Method::new(TypeName::from_static_str_unchecked("Tag"), &[], &[], &[]);

struct First;

#[zlink::interface("org.example.first")]
impl First {
    async fn get(&self) {}
}

struct Second;

#[zlink::interface("org.example.second")]
impl Second {
    async fn get(&self) {}

    async fn name(&self, name: &str) {
        let _ = name;
    }
}
