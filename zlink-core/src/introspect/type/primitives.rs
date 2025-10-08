//! Type implementations for primitive types.

use super::Type;
use crate::idl;

// Local macro for this module - simpler version for primitive types.
macro_rules! impl_type {
    ($($ty:ty),* => $variant:expr) => {
        $(
            impl Type for $ty {
                const TYPE: &'static idl::Type<'static> = &$variant;
            }
        )*
    };
}

// Boolean type.
impl_type!(bool => idl::Type::Bool);

// Integer types - all map to 64-bit signed integer in Varlink.
impl_type!(i8, i16, i32, i64 => idl::Type::Int);
impl_type!(u8, u16, u32, u64 => idl::Type::Int);
impl_type!(isize, usize => idl::Type::Int);

// Floating-point types - all map to 64-bit float in Varlink.
impl_type!(f32, f64 => idl::Type::Float);

// String types.
impl_type!(&str, str => idl::Type::String);
impl_type!(char => idl::Type::String);

impl_type!(String => idl::Type::String);
