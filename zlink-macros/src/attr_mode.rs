//! Resolve the `#[zlink(skip)]` / `#[zlink(flatten)]` disposition of a field or parameter, and
//! reject contradictory combinations.

use quote::ToTokens;
use syn::{Attribute, Error};

use crate::{naming, utils::has_zlink_bool_attr};

/// How a field or parameter should be emitted.
pub(crate) enum FieldMode {
    /// Emit the field as usual.
    Normal,
    /// Omit the field from the IDL (and, where zlink owns the serde, from the wire).
    Skip,
    /// Inline the field type's object fields in place.
    Flatten,
}

/// The [`FieldMode`] for `attrs`, erroring on contradictory combinations. `span` is what a
/// diagnostic points at (the field or the parameter name).
pub(crate) fn field_mode<S>(attrs: &[Attribute], span: &S) -> Result<FieldMode, Error>
where
    S: ToTokens,
{
    let skip = has_zlink_bool_attr(attrs, "skip");
    let flatten = has_zlink_bool_attr(attrs, "flatten");
    let renamed = naming::parse_rename(attrs)?.is_some();

    match (skip, flatten) {
        (true, true) => Err(Error::new_spanned(
            span,
            "`#[zlink(skip)]` and `#[zlink(flatten)]` are mutually exclusive",
        )),
        (true, false) if renamed => Err(Error::new_spanned(
            span,
            "`#[zlink(skip)]` cannot be combined with `#[zlink(rename)]`: a skipped field has no \
             wire name to rename",
        )),
        (true, false) => Ok(FieldMode::Skip),
        (false, true) if renamed => Err(Error::new_spanned(
            span,
            "`#[zlink(rename)]` has no effect on a `#[zlink(flatten)]` field: the field's own name \
             never appears on the wire; the inlined fields keep their own names",
        )),
        (false, true) => Ok(FieldMode::Flatten),
        (false, false) => Ok(FieldMode::Normal),
    }
}
