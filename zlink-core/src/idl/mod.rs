//! Interface Definition Language (IDL) support for Varlink.
//!
//! This module provides types and parsers for working with Varlink IDL definitions.

#![deny(missing_docs)]

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

#[cfg(feature = "idl-parse")]
pub mod parse;
