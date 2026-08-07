#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs
)]
#![warn(unreachable_pub, clippy::std_instead_of_core)]
#![cfg_attr(not(doctest), doc = include_str!("../README.md"))]

#[cfg(not(any(feature = "tracing", feature = "defmt")))]
compile_error!("Either 'tracing' or 'defmt' feature must be enabled.");

#[cfg(doctest)]
mod doctests {
    // Book markdown checks. The other chapters are checked from the `zlink` crate; this one is
    // checked from here because its transport example implements the `no_std` variants of the
    // socket traits, whose signatures differ when the `std` feature is enabled.
    #[cfg(not(feature = "std"))]
    doc_comment::doctest!("../../book/src/embedded.md");
}

extern crate alloc;

#[macro_use]
#[doc(hidden)]
pub mod log;

mod json_ser;

pub mod connection;
pub use connection::Connection;
mod error;
pub use error::{Error, Result};
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use server::{
    Server,
    listener::{Listener, ReadyListener},
    service::{self, Service},
};
mod call;
#[cfg(feature = "server")]
pub mod notified;
pub use call::Call;
pub mod reply;
pub use reply::Reply;
#[cfg(feature = "idl")]
pub mod idl {
    //! Interface Definition Language (IDL) support for Varlink.
    //!
    //! This is a re-export of the [`zlink_idl`] crate, for those who only need the IDL API and not
    //! the rest of zlink.

    #[doc(inline)]
    pub use zlink_idl::*;
}
#[cfg(feature = "introspection")]
pub mod introspect;
pub mod varlink_service;
pub mod names {
    //! Varlink name validation and parsing.
    //!
    //! This is a re-export of the [`zlink_names`] crate, for those who only need the name API and
    //! not the rest of zlink.

    #[doc(inline)]
    pub use zlink_names::*;
}

#[cfg(feature = "proxy")]
pub use zlink_macros::proxy;

#[cfg(feature = "service")]
pub use zlink_macros::service;

/// Re-export of futures-util for use in generated code.
#[doc(hidden)]
pub use futures_util;

/// Re-export of pin-project-lite for use in generated service code.
#[cfg(feature = "service")]
#[doc(hidden)]
pub use pin_project_lite;

pub use zlink_macros::ReplyError;

#[cfg(feature = "std")]
#[doc(hidden)]
pub mod unix_utils;

#[doc(hidden)]
pub mod test_utils;
