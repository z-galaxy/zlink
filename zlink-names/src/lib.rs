//! Varlink names and their validation.
//!
//! Varlink names fields, types and interfaces each by their own rule. This crate provides a type
//! for each kind of name, which can only hold a valid one, along with functions that check whether
//! a string is a valid name. It is standalone: it does not pull in the rest of zlink.
//!
//! # Features
//!
//! * `serde`: The name types implement serde's `Serialize` and `Deserialize`. A name serializes as
//!   a string, and a string is checked when it is deserialized into a name.
//!
//! # Examples
//!
//! ```
//! use zlink_names::{InterfaceName, is_valid_type_name};
//!
//! let name = InterfaceName::try_from("org.example.Foo")?;
//! assert_eq!(name, "org.example.Foo");
//! assert!(InterfaceName::try_from("Foo").is_err());
//! assert!(!is_valid_type_name("fileNotFound"));
//! # Ok::<(), zlink_names::Error>(())
//! ```
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

#[macro_use]
mod macros;

mod error;
pub use error::Error;

mod field_name;
pub use field_name::{FieldName, OwnedFieldName, is_valid_field_name};

mod type_name;
pub use type_name::{OwnedTypeName, TypeName, is_valid_type_name};

mod interface_name;
pub use interface_name::{InterfaceName, OwnedInterfaceName, is_valid_interface_name};

#[cfg(feature = "serde")]
mod de;

#[doc(hidden)]
pub mod parse;
