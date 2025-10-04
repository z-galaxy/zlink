//! Type implementations for special standard library types.
//!
//! This includes time types, network types, filesystem paths, and other
//! standard library types that require special handling.

use super::Type;
use crate::idl;

// ============================================================================
// Unit type
// ============================================================================

/// Unit type maps to an empty object in Varlink.
impl Type for () {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Object(idl::List::Borrowed(&[]));
}

// ============================================================================
// mayheap string type
// ============================================================================

impl<const N: usize> Type for mayheap::string::String<N> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

// ============================================================================
// JSON value type
// ============================================================================

/// serde_json::Value represents a foreign (untyped) object.
impl Type for serde_json::Value {
    const TYPE: &'static idl::Type<'static> = &idl::Type::ForeignObject;
}

// ============================================================================
// Time types
// ============================================================================

/// Core Duration - available in no-std.
impl Type for core::time::Duration {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Float;
}

impl Type for std::time::Instant {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Float;
}

impl Type for std::time::SystemTime {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Float;
}

// ============================================================================
// Path types
// ============================================================================

impl Type for std::path::PathBuf {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for std::path::Path {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

// ============================================================================
// OS string types
// ============================================================================

impl Type for std::ffi::OsString {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for std::ffi::OsStr {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

// ============================================================================
// Network types
// ============================================================================

impl Type for core::net::IpAddr {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for core::net::Ipv4Addr {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for core::net::Ipv6Addr {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for core::net::SocketAddr {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for core::net::SocketAddrV4 {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}

impl Type for core::net::SocketAddrV6 {
    const TYPE: &'static idl::Type<'static> = &idl::Type::String;
}
