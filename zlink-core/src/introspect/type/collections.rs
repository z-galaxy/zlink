//! Type implementations for collection types.

use super::Type;
use crate::{idl, idl::TypeRef};
use alloc::collections::{BTreeMap, BTreeSet};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Array/Vector types
// ============================================================================

/// Standard Vec implementation.
impl<T: Type> Type for Vec<T> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Array(TypeRef::new(T::TYPE));
}

/// Heapless Vec implementation.
impl<T: Type, const N: usize> Type for mayheap::Vec<T, N> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Array(TypeRef::new(T::TYPE));
}

/// Slice implementation.
impl<T: Type> Type for &[T] {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Array(TypeRef::new(T::TYPE));
}

// ============================================================================
// Map types - Varlink maps always have string keys
// ============================================================================

impl<V: Type> Type for HashMap<String, V> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Map(TypeRef::new(V::TYPE));
}

impl<V: Type> Type for HashMap<&str, V> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Map(TypeRef::new(V::TYPE));
}

impl<V: Type> Type for BTreeMap<String, V> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Map(TypeRef::new(V::TYPE));
}

impl<V: Type> Type for BTreeMap<&str, V> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Map(TypeRef::new(V::TYPE));
}

// ============================================================================
// Set types - represented as arrays in Varlink
// ============================================================================

impl<T: Type> Type for HashSet<T> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Array(TypeRef::new(T::TYPE));
}

impl<T: Type> Type for BTreeSet<T> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Array(TypeRef::new(T::TYPE));
}
