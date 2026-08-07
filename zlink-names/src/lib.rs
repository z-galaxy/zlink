//! Varlink name types and validation.
//!
//! This crate provides parsers and validators for the different kinds of names used in Varlink:
//! field names, type names and interface names. It is standalone: it does not pull in the rest of
//! zlink.
mod field_name;
mod interface_name;
mod parser;
mod type_name;

pub use field_name::{is_valid_field_name, parse_field_name};
pub use interface_name::{is_valid_interface_name, parse_interface_name};
pub use type_name::{is_valid_type_name, parse_type_name};
