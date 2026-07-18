//! Field-list helpers for `#[zlink(flatten)]` code generation.
//!
//! These `const fn`s exist **solely** to be called by the code that `zlink-macros` generates for
//! the `Type`, `CustomType`, `ReplyError`, and `#[service]` derives/macros. They are not a
//! supported public API — the module is `#[doc(hidden)]` and may change without notice.

use crate::{Field, Type};

/// The total number of fields once every group in `groups` is inlined.
///
/// For zlink-macros-generated code only; not a supported public API.
pub const fn total_field_count(groups: &[&[&Field<'static>]]) -> usize {
    let mut total = 0;
    let mut i = 0;
    while i < groups.len() {
        total += groups[i].len();
        i += 1;
    }
    total
}

/// Concatenate every group in `groups` into a single array, preserving order.
///
/// `N` must equal [`total_field_count`]`(groups)`; a mismatch is a compile-time panic.
///
/// For zlink-macros-generated code only; not a supported public API.
pub const fn concat_fields<const N: usize>(
    groups: &[&[&'static Field<'static>]],
) -> [&'static Field<'static>; N] {
    // Fills the accumulator before the real refs are written in. Overwritten in every slot unless
    // `N == 0`, in which case the array is empty and the filler is never observed.
    const FILLER: &Field<'static> = &Field::new("", &Type::Bool, &[]);
    let mut out: [&Field<'static>; N] = [FILLER; N];

    let mut oi = 0;
    let mut gi = 0;
    while gi < groups.len() {
        let group = groups[gi];
        let mut fi = 0;
        while fi < group.len() {
            out[oi] = group[fi];
            oi += 1;
            fi += 1;
        }
        gi += 1;
    }

    // A generated `N` that disagrees with the groups is a macro bug; fail loudly rather than
    // silently leave fillers or truncate.
    if oi != N {
        panic!("concat_fields: `N` does not match the total number of fields in `groups`");
    }

    out
}

/// The object fields of `ty`, for inlining a `#[zlink(flatten)]` field.
///
/// Panics at compile time if `ty` is not a borrowed object type — this is what rejects a flatten of
/// a named custom type (whose `TYPE` is `Custom`) or a scalar.
///
/// For zlink-macros-generated code only; not a supported public API.
pub const fn object_fields(ty: &'static Type<'static>) -> &'static [&'static Field<'static>] {
    match ty.as_object() {
        Some(list) => match list.as_borrowed() {
            Some(fields) => fields,
            // A derive's `TYPE` const is always `Borrowed`; `Owned` only arises from
            // deserialization.
            None => panic!("#[zlink(flatten)]: expected a borrowed object type"),
        },
        None => panic!(
            "#[zlink(flatten)] requires an inline object type (a `#[derive(Type)]` struct); \
             named custom types and scalars are not supported"
        ),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::List;

    #[test]
    fn counts_and_concatenates_in_order() {
        static A: Field<'static> = Field::new("a", &Type::Int, &[]);
        static B: Field<'static> = Field::new("b", &Type::String, &[]);
        static C: Field<'static> = Field::new("c", &Type::Bool, &[]);
        static G_A: &[&Field<'static>] = &[&A];
        static G_BC: &[&Field<'static>] = &[&B, &C];
        static GROUPS: &[&[&Field<'static>]] = &[G_A, G_BC];

        assert_eq!(total_field_count(&[]), 0);
        assert_eq!(total_field_count(GROUPS), 3);

        const N: usize = total_field_count(GROUPS);
        static OUT: [&Field<'static>; N] = concat_fields::<N>(GROUPS);
        let names: Vec<&str> = OUT.iter().map(|f| f.name()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn concatenates_empty_groups() {
        static GROUPS: &[&[&Field<'static>]] = &[];
        const N: usize = total_field_count(GROUPS);
        static OUT: [&Field<'static>; N] = concat_fields::<N>(GROUPS);
        assert_eq!(OUT.len(), 0);
    }

    #[test]
    fn object_fields_returns_the_members() {
        static F: Field<'static> = Field::new("x", &Type::Int, &[]);
        static FIELDS: &[&Field<'static>] = &[&F];
        static OBJ: Type<'static> = Type::Object(List::Borrowed(FIELDS));

        let got = object_fields(&OBJ);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name(), "x");
    }

    #[test]
    #[should_panic(expected = "requires an inline object type")]
    fn object_fields_rejects_a_scalar() {
        static SCALAR: Type<'static> = Type::Int;
        object_fields(&SCALAR);
    }
}
