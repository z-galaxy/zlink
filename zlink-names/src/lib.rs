//! Varlink name types and validation.
//!
//! This crate provides parsers and validators for the different kinds of names used in Varlink:
//! field names, type names and interface names. It is standalone: it does not pull in the rest of
//! zlink.
#![no_std]

extern crate alloc;

mod error;
mod field_name;
mod interface_name;
mod parser;
mod pattern;
mod type_name;

pub use error::TryFromError;
pub use field_name::{FieldName, OwnedFieldName};
#[deprecated(note = "Use OwnedFieldName or FieldName in zlink-names instead")]
pub use field_name::{is_valid_field_name, parse_field_name};
#[deprecated(note = "Use OwnedInterfaceName or InterfaceName in zlink-names instead")]
pub use interface_name::is_valid_interface_name;
pub use interface_name::{InterfaceName, OwnedInterfaceName, parse_interface_name};
pub use pattern::FromPattern;
#[deprecated(note = "Use OwnedTypeName or TypeName in zlink-names instead")]
pub use type_name::is_valid_type_name;
pub use type_name::{OwnedTypeName, TypeName, parse_type_name};
