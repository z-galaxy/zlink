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

extern crate alloc;

#[macro_use]
#[doc(hidden)]
pub mod log;

mod json_ser;

pub mod connection;
pub use connection::Connection;
mod error;
pub use error::{Error, Result};
mod server;
pub use server::{
    listener::Listener,
    service::{self, Service},
    Server,
};
mod call;
pub use call::Call;
pub mod reply;
pub use reply::Reply;
#[cfg(feature = "idl")]
pub mod idl;
#[cfg(feature = "introspection")]
pub mod introspect;
pub mod varlink_service;

#[cfg(feature = "proxy")]
pub use zlink_macros::proxy;

pub use zlink_macros::ReplyError;

#[doc(hidden)]
pub mod test_utils;
