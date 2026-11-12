//! Tests for the deprecated `service` attribute macro, which implements `Service` for the type of
//! an `impl` block. They keep it covered until it is removed.

#![cfg(feature = "service")]
#![allow(deprecated)]

use serde::{Deserialize, Serialize};
use zlink::{
    Call, Connection, Service,
    introspect::{self, CustomType},
    service::MethodReply,
    test_utils::mock_socket::MockSocket,
};

#[test_log::test(tokio::test)]
async fn single_interface() {
    let mut greeter = Greeter;
    let json = r#"{"method":"org.example.greeter.Greet","parameters":{"name":"world"}}"#;
    assert_eq!(
        dispatch(&mut greeter, json).await,
        r#"{"text":"hello world"}"#
    );

    let info = dispatch(&mut greeter, r#"{"method":"org.varlink.service.GetInfo"}"#).await;
    assert_eq!(
        info,
        concat!(
            r#"{"vendor":"Test Vendor","product":"Test Product","version":"1.0","#,
            r#""url":"https://example.org","#,
            r#""interfaces":["org.example.greeter","org.varlink.service"]}"#
        )
    );

    // A method that the service doesn't have isn't named in the error.
    let call: Call<<Greeter as Service<MockSocket>>::MethodCall<'_>> =
        serde_json::from_str(r#"{"method":"org.example.greeter.Nope"}"#).unwrap();
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let (reply, _fds) = Service::handle(&mut greeter, &call, &mut conn, Vec::new()).await;
    let MethodReply::Error(err) = reply else {
        panic!("an unknown method got {reply:?}");
    };
    assert_eq!(
        serde_json::to_string(&err).unwrap(),
        r#"{"error":"org.varlink.service.MethodNotFound","parameters":{"method":"unknown"}}"#
    );
}

#[test_log::test(tokio::test)]
async fn several_interfaces_in_one_block() {
    let mut service = CaseDistinct;

    // Interface names that differ only in case are told apart, in calls and descriptions.
    let json = r#"{"method":"org.example.foo.LowerMethod"}"#;
    assert_eq!(dispatch(&mut service, json).await, r#"{"text":"lower"}"#);
    let json = r#"{"method":"org.example.Foo.UpperMethod"}"#;
    assert_eq!(dispatch(&mut service, json).await, r#"{"text":"upper"}"#);

    let info = dispatch(&mut service, r#"{"method":"org.varlink.service.GetInfo"}"#).await;
    assert!(
        info.contains(
            r#""interfaces":["org.example.foo","org.example.Foo","org.varlink.service"]"#
        ),
        "{info}"
    );
    let lower = description(&mut service, "org.example.foo").await;
    assert!(lower.contains("method LowerMethod()"), "{lower}");
    assert!(!lower.contains("UpperMethod"), "{lower}");
    let upper = description(&mut service, "org.example.Foo").await;
    assert!(upper.contains("method UpperMethod()"), "{upper}");
    assert!(!upper.contains("LowerMethod"), "{upper}");
}

#[test_log::test(tokio::test)]
async fn per_interface_types() {
    let mut service = Catalog;

    let json = r#"{"method":"org.example.music.ListAlbums"}"#;
    assert_eq!(
        dispatch(&mut service, json).await,
        r#"{"albums":[{"title":"Oxidized Beats"}]}"#
    );

    // Each interface describes its own types only, each once.
    let books = description(&mut service, "org.example.books").await;
    assert_eq!(books.matches("type Book (").count(), 1, "{books}");
    assert!(books.contains("type BookList ("), "{books}");
    assert!(!books.contains("Album"), "{books}");
    let music = description(&mut service, "org.example.music").await;
    assert!(music.contains("type Album ("), "{music}");
    assert!(music.contains("type AlbumList ("), "{music}");
    assert!(!music.contains("Book"), "{music}");
}

/// The description of `interface` that `service` replies with.
async fn description<S>(service: &mut S, interface: &str) -> String
where
    S: Service<MockSocket>,
{
    let json = format!(
        concat!(
            r#"{{"method":"org.varlink.service.GetInterfaceDescription","#,
            r#""parameters":{{"interface":"{interface}"}}}}"#
        ),
        interface = interface
    );
    let reply: serde_json::Value = serde_json::from_str(&dispatch(service, &json).await).unwrap();

    reply["description"].as_str().unwrap().to_owned()
}

/// Dispatch a call, given as JSON, to `service` and return the JSON of the parameters of its reply.
async fn dispatch<S>(service: &mut S, json: &str) -> String
where
    S: Service<MockSocket>,
{
    let call: Call<S::MethodCall<'_>> = serde_json::from_str(json).unwrap();
    let mut conn = Connection::new(MockSocket::with_responses(&[]));
    let (reply, _fds) = service.handle(&call, &mut conn, Vec::new()).await;
    let MethodReply::Single(Some(params)) = reply else {
        panic!("{json} wasn't replied to with parameters");
    };

    serde_json::to_string(&params).unwrap()
}

// A service of one interface, with metadata.
struct Greeter;

#[zlink::service(
    interface = "org.example.greeter",
    vendor = "Test Vendor",
    product = "Test Product",
    version = "1.0",
    url = "https://example.org"
)]
impl Greeter {
    async fn greet(&self, name: &str) -> Text {
        Text {
            text: format!("hello {name}"),
        }
    }
}

// A service of two interfaces whose names differ only in case.
struct CaseDistinct;

#[zlink::service]
impl CaseDistinct {
    #[zlink(interface = "org.example.foo")]
    async fn lower_method(&self) -> Text {
        Text {
            text: "lower".into(),
        }
    }

    #[zlink(interface = "org.example.Foo")]
    async fn upper_method(&self) -> Text {
        Text {
            text: "upper".into(),
        }
    }
}

// A service of two interfaces, each with types of its own.
struct Catalog;

#[zlink::service]
impl Catalog {
    #[zlink(interface = "org.example.books", types = [Book, BookList])]
    async fn list_books(&self) -> BookList {
        BookList { books: Vec::new() }
    }

    // `Book` is listed again, and described once.
    #[zlink(types = [Book])]
    async fn get_book(&self) -> Book {
        Book {
            title: "The Rust Programming Language".into(),
        }
    }

    #[zlink(interface = "org.example.music", types = [Album, AlbumList])]
    async fn list_albums(&self) -> AlbumList {
        AlbumList {
            albums: vec![Album {
                title: "Oxidized Beats".into(),
            }],
        }
    }
}

/// The reply of the methods that need no type of their own.
#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Text {
    text: String,
}

#[derive(Debug, Serialize, Deserialize, CustomType)]
struct BookList {
    books: Vec<Book>,
}

#[derive(Debug, Serialize, Deserialize, CustomType)]
struct Book {
    title: String,
}

#[derive(Debug, Serialize, Deserialize, CustomType)]
struct AlbumList {
    albums: Vec<Album>,
}

#[derive(Debug, Serialize, Deserialize, CustomType)]
struct Album {
    title: String,
}
