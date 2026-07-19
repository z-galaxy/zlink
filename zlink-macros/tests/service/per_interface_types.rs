//! Tests for per-interface custom type scoping in introspection.
//!
//! When a service implements multiple interfaces, custom types specified via
//! `#[zlink(interface = "...", types = [...])]` should only appear in the IDL
//! output for that specific interface, not in all interfaces.
//!
//! Varlink method return types must always be structs (objects) so that the
//! response can be expressed as named fields: `-> (field: type, …)`.  Arrays
//! and primitives cannot be top-level return types; wrap them in a struct.

use serde::{Deserialize, Serialize};
use zlink::{
    Server,
    introspect::{self, CustomType},
    tokio::unix::{bind, connect},
};

#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn per_interface_types() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket_path = dir.path().join("per-iface-types.sock");

    let listener = bind(&socket_path).unwrap();
    let service = CatalogService;
    let server = Server::new(listener, service);
    tokio::select! {
        res = server.run() => res?,
        res = run_client(&socket_path) => res?,
    }

    Ok(())
}

async fn run_client(socket_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use zlink::varlink_service::Proxy as VarlinkProxy;

    let mut conn = connect(socket_path).await?;

    // Smoke-test the methods.
    let reply = conn.list_books().await?.unwrap();
    assert_eq!(reply.books.len(), 1);
    assert_eq!(reply.books[0].title, "The Rust Programming Language");

    let reply = conn.list_albums().await?.unwrap();
    assert_eq!(reply.albums.len(), 1);
    assert_eq!(reply.albums[0].artist, "Ferris & The Crabs");

    // --- Introspection: verify types are scoped per interface ---

    // org.example.books should have Book and BookList but NOT Album/AlbumList.
    let desc = conn
        .get_interface_description("org.example.books")
        .await?
        .unwrap();
    let interface = desc.parse()?;
    assert_eq!(interface.name(), "org.example.books");

    let type_names: Vec<_> = interface.custom_types().map(|t| t.name()).collect();
    assert!(
        type_names.contains(&"Book"),
        "org.example.books should contain Book, got: {type_names:?}"
    );
    assert!(
        type_names.contains(&"BookList"),
        "org.example.books should contain BookList, got: {type_names:?}"
    );
    assert!(
        !type_names.contains(&"Album"),
        "org.example.books should NOT contain Album, got: {type_names:?}"
    );
    assert!(
        !type_names.contains(&"AlbumList"),
        "org.example.books should NOT contain AlbumList, got: {type_names:?}"
    );
    // Book is listed in types = [] on two methods; verify it appears only once.
    assert_eq!(
        type_names.iter().filter(|&&n| n == "Book").count(),
        1,
        "Book should appear exactly once despite being listed on multiple methods, got: {type_names:?}"
    );

    // Verify list_books method has the correct out-params (fields of BookList).
    let list_books_method = interface
        .methods()
        .find(|m| m.name() == "ListBooks")
        .expect("ListBooks method should be present");
    let out_param_names: Vec<_> = list_books_method.outputs().map(|p| p.name()).collect();
    assert!(
        out_param_names.contains(&"books"),
        "ListBooks should have 'books' as an out-param, got: {out_param_names:?}"
    );

    // org.example.music should have Album and AlbumList but NOT Book/BookList.
    let desc = conn
        .get_interface_description("org.example.music")
        .await?
        .unwrap();
    let interface = desc.parse()?;
    assert_eq!(interface.name(), "org.example.music");

    let type_names: Vec<_> = interface.custom_types().map(|t| t.name()).collect();
    assert!(
        type_names.contains(&"Album"),
        "org.example.music should contain Album, got: {type_names:?}"
    );
    assert!(
        type_names.contains(&"AlbumList"),
        "org.example.music should contain AlbumList, got: {type_names:?}"
    );
    assert!(
        !type_names.contains(&"Book"),
        "org.example.music should NOT contain Book, got: {type_names:?}"
    );
    assert!(
        !type_names.contains(&"BookList"),
        "org.example.music should NOT contain BookList, got: {type_names:?}"
    );

    Ok(())
}

/// A book entry (belongs to org.example.books).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Book {
    title: String,
    author: String,
}

/// Wrapper struct for list_books reply — Varlink requires a named-field object
/// as the return type, so `Vec<Book>` must be wrapped.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct BookList {
    books: Vec<Book>,
}

/// A music album entry (belongs to org.example.music).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Album {
    title: String,
    artist: String,
}

/// Wrapper struct for list_albums reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct AlbumList {
    albums: Vec<Album>,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.books")]
enum BookError {
    NotFound,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.music")]
enum MusicError {
    NotFound,
}

/// A service exposing two catalog interfaces with distinct custom types.
#[derive(Default)]
struct CatalogService;

#[zlink::service]
impl CatalogService {
    #[zlink(interface = "org.example.books", types = [Book, BookList])]
    async fn list_books(&self) -> BookList {
        BookList {
            books: vec![Book {
                title: "The Rust Programming Language".to_string(),
                author: "Steve Klabnik & Carol Nichols".to_string(),
            }],
        }
    }

    // Book is listed again here to verify deduplication (should only appear once in IDL).
    #[zlink(types = [Book])]
    async fn get_book(&self, title: String) -> Result<Book, BookError> {
        if title == "The Rust Programming Language" {
            Ok(Book {
                title,
                author: "Steve Klabnik & Carol Nichols".to_string(),
            })
        } else {
            Err(BookError::NotFound)
        }
    }

    #[zlink(interface = "org.example.music", types = [Album, AlbumList])]
    async fn list_albums(&self) -> AlbumList {
        AlbumList {
            albums: vec![Album {
                title: "Oxidized Beats".to_string(),
                artist: "Ferris & The Crabs".to_string(),
            }],
        }
    }

    #[zlink(types = [Album])]
    async fn get_album(&self, title: String) -> Result<Album, MusicError> {
        if title == "Oxidized Beats" {
            Ok(Album {
                title,
                artist: "Ferris & The Crabs".to_string(),
            })
        } else {
            Err(MusicError::NotFound)
        }
    }
}

#[zlink::proxy("org.example.books")]
trait BookProxy {
    async fn list_books(&mut self) -> zlink::Result<Result<BookList, BookError>>;
    async fn get_book(&mut self, title: String) -> zlink::Result<Result<Book, BookError>>;
}

#[zlink::proxy("org.example.music")]
trait MusicProxy {
    async fn list_albums(&mut self) -> zlink::Result<Result<AlbumList, MusicError>>;
    async fn get_album(&mut self, title: String) -> zlink::Result<Result<Album, MusicError>>;
}
