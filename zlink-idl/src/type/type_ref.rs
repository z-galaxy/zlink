use super::Type;
use alloc::boxed::Box;
use core::{fmt, ops::Deref};

/// A type reference that can be either borrowed or owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef<'a>(TypeRefInner<'a>);

impl<'a> TypeRef<'a> {
    /// Creates a new type reference with a borrowed type reference.
    pub const fn new(inner: &'a Type<'a>) -> Self {
        Self(TypeRefInner::Borrowed(inner))
    }

    /// Creates a new type reference with an owned type.
    pub fn new_owned(inner: Type<'a>) -> Self {
        Self(TypeRefInner::Owned(Box::new(inner)))
    }

    /// Returns a reference to the inner type.
    pub const fn inner(&self) -> &Type<'a> {
        self.0.ty()
    }
}

impl<'a> Deref for TypeRef<'a> {
    type Target = Type<'a>;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl<'a> fmt::Display for TypeRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner())
    }
}

impl<'a> PartialEq<Type<'a>> for TypeRef<'a> {
    fn eq(&self, other: &Type<'a>) -> bool {
        self.inner() == other
    }
}

#[derive(Debug, Clone, Eq)]
enum TypeRefInner<'a> {
    Borrowed(&'a Type<'a>),
    Owned(Box<Type<'a>>),
}

impl<'a> TypeRefInner<'a> {
    /// A reference to the inner type.
    const fn ty(&self) -> &Type<'a> {
        match self {
            TypeRefInner::Borrowed(inner) => inner,
            TypeRefInner::Owned(inner) => inner,
        }
    }
}

impl PartialEq for TypeRefInner<'_> {
    fn eq(&self, other: &Self) -> bool {
        let ty = self.ty();
        let other_ty = other.ty();

        ty.eq(other_ty)
    }
}
