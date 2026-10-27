/// Implement the API that every name family shares.
///
/// `$name` is the borrowing name type, a tuple struct around a `CheapStr`, and `$owned` is its
/// owned sibling, a tuple struct around `$name<'static>`. `$is_valid` checks a string against the
/// family's rule and `$error` is the error that a failed conversion returns. `$expecting` says what
/// the name is, for the error of a failed deserialization.
macro_rules! impl_name {
    ($name:ident, $owned:ident, $is_valid:path, $error:expr, $expecting:literal) => {
        impl<'a> $name<'a> {
            /// Create a name from a static string, without validating it.
            ///
            /// This is for `const` and `static` items, where the validating conversions can't be
            /// used. `name` must be a valid name: it is not checked, and a wrong one is a bug in
            /// the caller, though not a memory-unsafe one.
            pub const fn from_static_str_unchecked(name: &'static str) -> Self {
                Self(::zcheapstr::CheapStr::from_static(name))
            }

            /// The name as a string slice.
            ///
            /// This is a `const fn`. As a name has a destructor, which can't run in `const`
            /// context, `const` code can only call it on a name that is never dropped, such as one
            /// behind a `const` reference or in a `static`.
            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// An owned copy of the name.
            ///
            /// Data that is static or already owned is shared rather than copied.
            pub fn to_owned(&self) -> $owned {
                $owned($name(self.0.to_owned()))
            }

            /// Convert into an owned name.
            ///
            /// Data that is static or already owned is shared rather than copied.
            pub fn into_owned(self) -> $owned {
                $owned($name(self.0.into_owned()))
            }
        }

        impl<'a> TryFrom<&'a str> for $name<'a> {
            type Error = $crate::Error;

            fn try_from(name: &'a str) -> Result<Self, Self::Error> {
                if !$is_valid(name) {
                    return Err($error);
                }

                Ok(Self(::zcheapstr::CheapStr::from(name)))
            }
        }

        impl TryFrom<::alloc::string::String> for $name<'_> {
            type Error = $crate::Error;

            fn try_from(name: ::alloc::string::String) -> Result<Self, Self::Error> {
                if !$is_valid(&name) {
                    return Err($error);
                }

                Ok(Self(::zcheapstr::CheapStr::from(name)))
            }
        }

        impl<'a> From<&'a $name<'_>> for $name<'a> {
            fn from(name: &'a $name<'_>) -> Self {
                Self(name.0.as_ref())
            }
        }

        impl From<$owned> for $name<'_> {
            fn from(name: $owned) -> Self {
                name.0
            }
        }

        impl<'a> From<&'a $owned> for $name<'a> {
            fn from(name: &'a $owned) -> Self {
                name.as_ref()
            }
        }

        impl ::core::ops::Deref for $name<'_> {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl AsRef<str> for $name<'_> {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl ::core::fmt::Display for $name<'_> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(self.as_str(), f)
            }
        }

        impl PartialEq<str> for $name<'_> {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $name<'_> {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl PartialEq<$name<'_>> for str {
            fn eq(&self, other: &$name<'_>) -> bool {
                self == other.as_str()
            }
        }

        impl PartialEq<$name<'_>> for &str {
            fn eq(&self, other: &$name<'_>) -> bool {
                *self == other.as_str()
            }
        }

        impl PartialEq<$owned> for $name<'_> {
            fn eq(&self, other: &$owned) -> bool {
                *self == other.0
            }
        }

        #[cfg(feature = "serde")]
        impl ::serde::Serialize for $name<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        #[cfg(feature = "serde")]
        impl<'de: 'a, 'a> ::serde::Deserialize<'de> for $name<'a> {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                $crate::de::deserialize(deserializer, $is_valid, $expecting).map(Self)
            }
        }

        impl $owned {
            /// The name as a string slice.
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// A borrowed view of the name.
            ///
            /// It shares the data of the name: nothing is copied and no reference count changes.
            pub fn as_ref(&self) -> $name<'_> {
                $name(self.0.0.as_ref())
            }

            /// The inner name.
            pub fn inner(&self) -> &$name<'static> {
                &self.0
            }

            /// Convert into the inner name.
            pub fn into_inner(self) -> $name<'static> {
                self.0
            }
        }

        impl TryFrom<&str> for $owned {
            type Error = $crate::Error;

            fn try_from(name: &str) -> Result<Self, Self::Error> {
                $name::try_from(name).map($name::into_owned)
            }
        }

        impl TryFrom<::alloc::string::String> for $owned {
            type Error = $crate::Error;

            fn try_from(name: ::alloc::string::String) -> Result<Self, Self::Error> {
                $name::try_from(name).map($name::into_owned)
            }
        }

        impl From<$name<'_>> for $owned {
            fn from(name: $name<'_>) -> Self {
                name.into_owned()
            }
        }

        impl ::core::ops::Deref for $owned {
            type Target = $name<'static>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl ::core::fmt::Display for $owned {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        impl PartialEq<str> for $owned {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }

        impl PartialEq<&str> for $owned {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }

        impl PartialEq<$name<'_>> for $owned {
            fn eq(&self, other: &$name<'_>) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<$owned> for str {
            fn eq(&self, other: &$owned) -> bool {
                self == other.as_str()
            }
        }

        impl PartialEq<$owned> for &str {
            fn eq(&self, other: &$owned) -> bool {
                *self == other.as_str()
            }
        }

        #[cfg(feature = "serde")]
        impl ::serde::Serialize for $owned {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                ::serde::Serialize::serialize(&self.0, serializer)
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> ::serde::Deserialize<'de> for $owned {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                <$name<'_> as ::serde::Deserialize<'de>>::deserialize(deserializer)
                    .map($name::into_owned)
            }
        }
    };
}
