//! Type implementations for wrapper types.
//!
//! This module contains implementations for types that transparently wrap other types,
//! such as smart pointers, cells, and optional types.

use super::Type;
use crate::{idl, idl::TypeRef};

// ============================================================================
// Optional type
// ============================================================================

impl<T: Type> Type for Option<T> {
    const TYPE: &'static idl::Type<'static> = &idl::Type::Optional(TypeRef::new(T::TYPE));
}

// ============================================================================
// Smart pointer types - transparent wrappers
// ============================================================================

impl<T: Type + ?Sized> Type for Box<T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}

impl<T: Type + ?Sized> Type for std::rc::Rc<T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}

impl<T: Type + ?Sized> Type for std::sync::Arc<T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}

// ============================================================================
// Cell types - transparent wrappers
// ============================================================================

impl<T: Type + ?Sized> Type for std::cell::Cell<T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}

impl<T: Type + ?Sized> Type for std::cell::RefCell<T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}

// ============================================================================
// Cow type - transparent wrapper
// ============================================================================

impl<T: Type + ToOwned + ?Sized> Type for std::borrow::Cow<'_, T> {
    const TYPE: &'static idl::Type<'static> = T::TYPE;
}
