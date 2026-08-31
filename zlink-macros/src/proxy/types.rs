use syn::{Attribute, Error, Meta};
use zlink_names::OwnedFieldName;

use super::utils::{extract_zlink_attrs, parse_rename_value};

/// Attributes that can be applied to proxy methods via #[zlink(...)].
#[derive(Default)]
pub(super) struct MethodAttrs {
    /// Rename the method for the Varlink call.
    pub rename: Option<String>,
    /// Method returns a stream of responses.
    pub is_streaming: bool,
    /// Method is one-way (fire and forget).
    pub is_oneway: bool,
    /// Method returns file descriptors.
    pub return_fds: bool,
}

impl MethodAttrs {
    /// Extract method attributes from a method's attribute list.
    pub(super) fn extract(attrs: &mut Vec<Attribute>) -> Result<Self, Error> {
        let attrs_result = extract_zlink_attrs(attrs, |meta_items| {
            let mut method_attrs = Self::default();

            for meta in meta_items {
                match &meta {
                    Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                        method_attrs.rename = parse_rename_value(&nv.value)?;
                    }
                    Meta::Path(path) if path.is_ident("more") => {
                        if method_attrs.is_streaming {
                            return Err(Error::new_spanned(&meta, "duplicate `more` attribute"));
                        }
                        method_attrs.is_streaming = true;
                    }
                    Meta::Path(path) if path.is_ident("oneway") => {
                        if method_attrs.is_oneway {
                            return Err(Error::new_spanned(&meta, "duplicate `oneway` attribute"));
                        }
                        method_attrs.is_oneway = true;
                    }
                    Meta::Path(path) if path.is_ident("return_fds") => {
                        if method_attrs.return_fds {
                            return Err(Error::new_spanned(
                                &meta,
                                "duplicate `return_fds` attribute",
                            ));
                        }
                        method_attrs.return_fds = true;
                    }
                    _ => {
                        return Err(Error::new_spanned(&meta, "unknown zlink attribute"));
                    }
                }
            }

            Ok(method_attrs)
        });
        Ok(attrs_result.unwrap_or_default())
    }
}

/// Information about a method argument.
pub(super) struct ArgInfo<'a> {
    pub name: &'a syn::Ident,
    pub ty_for_params: syn::Type,
    pub has_lifetime: bool,
    pub is_optional: bool,
    pub serialized_name: Option<OwnedFieldName>,
    pub is_fds: bool,
}
