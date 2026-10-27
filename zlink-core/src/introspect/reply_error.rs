use crate::idl::Error;

/// Trait providing description of a interface method reply error type.
pub trait ReplyError {
    /// The list of possible errors variants.
    const VARIANTS: &'static [&'static Error<'static>];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::TypeName;

    #[test]
    fn reply_errors() {
        // Test with a type that implements ReplyErrors.
        struct MyType;

        // `const` items can't use the validating conversions.
        const MY_ERROR: Error<'static> =
            Error::new(TypeName::from_static_str_unchecked("MyError"), &[], &[]);

        impl ReplyError for MyType {
            const VARIANTS: &'static [&'static Error<'static>] = &[&MY_ERROR];
        }

        assert_eq!(MyType::VARIANTS.len(), 1);
        assert_eq!(MyType::VARIANTS[0].name(), "MyError");
    }
}
