//! Interface Definition Language (IDL) support for Varlink.
//!
//! This crate provides the types and parser for working with Varlink IDL definitions. It is
//! standalone: it does not pull in the rest of zlink.
#![cfg_attr(not(test), no_std)]
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

extern crate alloc;

mod list;
pub use list::List;

mod r#type;
pub use r#type::{Type, TypeRef};

mod custom_object;
pub use custom_object::CustomObject;

mod custom_enum;
pub use custom_enum::CustomEnum;

mod enum_variant;
pub use enum_variant::EnumVariant;

mod custom_type;
pub use custom_type::CustomType;

mod field;
pub use field::{Field, Parameter};

mod method;
pub use method::Method;

mod error;
pub use error::Error;

mod comment;
pub use comment::Comment;

mod interface;
pub use interface::Interface;

#[cfg(feature = "parse")]
pub mod parse;
