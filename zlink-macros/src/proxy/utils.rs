use crate::{
    naming::{self, NameSource, RenameAll},
    utils::*,
};
use std::collections::HashSet;
use syn::{
    Attribute, Error, Expr, GenericArgument, Ident, Lit, Meta, PathArguments, ReturnType, Type,
    punctuated::Punctuated,
};
use zlink_names::{FieldName, OwnedFieldName};

/// Convert snake_case to PascalCase.
pub(super) fn snake_case_to_pascal_case(input: &str) -> String {
    input
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
        })
        .collect()
}

/// Convert any lifetime references to use our single '__proxy_params lifetime.
pub(super) fn convert_to_single_lifetime(ty: &Type) -> Type {
    convert_type_lifetimes(ty, "'__proxy_params")
}

/// Check if a type contains any non-static lifetime references.
/// This recursively checks all nested types.
/// Note: `'static` lifetimes are not considered problematic for chain methods.
pub(super) fn type_contains_lifetime(ty: &Type) -> bool {
    match ty {
        Type::Reference(type_ref) => {
            // Check if it's a 'static reference
            match &type_ref.lifetime {
                Some(lt) if lt.ident == "static" => {
                    // 'static is fine, but check the inner type
                    type_contains_lifetime(&type_ref.elem)
                }
                Some(_) => true, // Non-static lifetime
                None => true,    // Elided lifetime (treated as non-static)
            }
        }
        Type::Path(type_path) => type_path.path.segments.iter().any(|segment| {
            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return false;
            };
            args.args.iter().any(|arg| match arg {
                GenericArgument::Lifetime(lt) => lt.ident != "static",
                GenericArgument::Type(ty) => type_contains_lifetime(ty),
                _ => false,
            })
        }),
        Type::Slice(type_slice) => type_contains_lifetime(&type_slice.elem),
        Type::Array(type_array) => type_contains_lifetime(&type_array.elem),
        Type::Tuple(type_tuple) => type_tuple.elems.iter().any(type_contains_lifetime),
        Type::Ptr(type_ptr) => type_contains_lifetime(&type_ptr.elem),
        Type::Paren(type_paren) => type_contains_lifetime(&type_paren.elem),
        Type::Group(type_group) => type_contains_lifetime(&type_group.elem),
        _ => false,
    }
}

/// Collect all type parameter names used in a type.
pub(super) fn collect_used_type_params(ty: &Type, used: &mut HashSet<String>) {
    match ty {
        Type::Path(type_path) => {
            for segment in &type_path.path.segments {
                // Check if the segment itself is a type parameter (simple identifier)
                if type_path.path.segments.len() == 1 {
                    used.insert(segment.ident.to_string());
                }

                // Check generic arguments
                let PathArguments::AngleBracketed(args) = &segment.arguments else {
                    continue;
                };
                for arg in &args.args {
                    if let GenericArgument::Type(inner_ty) = arg {
                        collect_used_type_params(inner_ty, used);
                    }
                }
            }
        }
        Type::Reference(type_ref) => collect_used_type_params(&type_ref.elem, used),
        Type::Slice(type_slice) => collect_used_type_params(&type_slice.elem, used),
        Type::Array(type_array) => collect_used_type_params(&type_array.elem, used),
        Type::Tuple(type_tuple) => {
            for elem in &type_tuple.elems {
                collect_used_type_params(elem, used);
            }
        }
        Type::Ptr(type_ptr) => collect_used_type_params(&type_ptr.elem, used),
        Type::Paren(type_paren) => collect_used_type_params(&type_paren.elem, used),
        Type::Group(type_group) => collect_used_type_params(&type_group.elem, used),
        _ => {} // Other types don't contain type parameters we care about
    }
}

/// Extract and process zlink attributes from a list of attributes.
/// Returns the processed value and removes the attributes from the list.
pub(super) fn extract_zlink_attrs<T, F>(attrs: &mut Vec<Attribute>, processor: F) -> Option<T>
where
    F: FnOnce(Punctuated<Meta, syn::Token![,]>) -> Result<T, Error>,
{
    let mut zlink_attr_indices = Vec::new();
    let mut meta_items_to_process = None;

    for (i, attr) in attrs.iter().enumerate() {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        let Meta::List(list) = &attr.meta else {
            continue;
        };

        if list.tokens.is_empty() {
            continue;
        }

        // Parse all meta items in this zlink attribute
        if meta_items_to_process.is_none()
            && let Ok(meta_items) =
                list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        {
            meta_items_to_process = Some(meta_items);
            zlink_attr_indices.push(i);
        }
    }

    // Process the found meta items if any
    let result = if let Some(meta_items) = meta_items_to_process {
        processor(meta_items).ok()
    } else {
        None
    };

    // Remove the zlink attributes we processed (in reverse order to preserve indices)
    for &index in zlink_attr_indices.iter().rev() {
        attrs.remove(index);
    }

    result
}

/// Parse a rename value from an expression.
pub(super) fn parse_rename_value(expr: &Expr) -> Result<Option<String>, Error> {
    match expr {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(lit_str),
            ..
        }) => Ok(Some(lit_str.value())),
        _ => Err(Error::new_spanned(
            expr,
            "rename value must be a string literal",
        )),
    }
}

/// Parameter attributes extracted from #[zlink(...)] on parameters.
#[derive(Default)]
pub(super) struct ParamAttrs {
    pub rename: Option<String>,
    pub is_fds: bool,
}

/// The name a parameter should carry on the wire, when it differs from its Rust ident.
///
/// An explicit `#[zlink(rename = "...")]` wins over the trait-level `rename_all_arguments`. A
/// `rename_all_arguments` convention is applied to the unraw'd ident -- `r#` is Rust syntax,
/// never part of the name -- and the name it produces is checked against the Varlink field
/// grammar, which conventions like `kebab-case` cannot always satisfy.
pub(super) fn resolve_serialized_name(
    ident: &Ident,
    param_attrs: Option<&ParamAttrs>,
    rename_all: Option<RenameAll>,
) -> Result<Option<OwnedFieldName>, Error> {
    if let Some(rename) = param_attrs.and_then(|attrs| attrs.rename.clone()) {
        return Ok(Some(naming::validate(
            &rename,
            "renamed parameter name",
            NameSource::Ident(ident),
        )?));
    }

    let Some(rule) = rename_all else {
        // Not renamed, so nothing to emit `#[serde(rename = ...)]` for -- but the ident's own
        // name still has to be a name Varlink can express.
        let _: FieldName<'_> = naming::validate(
            &naming::unraw(ident),
            "parameter name",
            NameSource::Ident(ident),
        )?;
        return Ok(None);
    };

    let name = rule.apply_to_field(&naming::unraw(ident));
    Ok(Some(naming::validate(
        &name,
        "parameter name",
        NameSource::Ident(ident),
    )?))
}

/// Extract parameter attributes from zlink attributes and remove processed attributes.
pub(super) fn extract_param_attrs(attrs: &mut Vec<Attribute>) -> Result<ParamAttrs, Error> {
    let attrs_result = extract_zlink_attrs(attrs, |meta_items| {
        let mut param_attrs = ParamAttrs::default();

        for meta in meta_items {
            match &meta {
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    if param_attrs.rename.is_some() {
                        return Err(Error::new_spanned(
                            &meta,
                            "duplicate `rename` attribute on parameter",
                        ));
                    }
                    param_attrs.rename = parse_rename_value(&nv.value)?;
                }
                Meta::Path(path) if path.is_ident("fds") => {
                    if param_attrs.is_fds {
                        return Err(Error::new_spanned(
                            &meta,
                            "duplicate `fds` attribute on parameter",
                        ));
                    }
                    param_attrs.is_fds = true;
                }
                _ => {
                    return Err(Error::new_spanned(
                        &meta,
                        "unknown zlink attribute on parameter",
                    ));
                }
            }
        }

        Ok(param_attrs)
    });
    Ok(attrs_result.unwrap_or_default())
}

/// Build a combined where clause from existing constraints, new constraint, and generic bounds.
pub(super) fn build_combined_where_clause(
    existing: Option<syn::WhereClause>,
    new_constraint: syn::WherePredicate,
    generics: &syn::Generics,
) -> syn::WhereClause {
    let mut where_clause = existing.unwrap_or_else(|| syn::parse_quote!(where));

    // Add new constraint
    where_clause.predicates.push(new_constraint);

    // Add generic bounds to where clause
    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param
            && !type_param.bounds.is_empty()
        {
            let type_name = &type_param.ident;
            let bounds = &type_param.bounds;
            where_clause
                .predicates
                .push(syn::parse_quote!(#type_name: #bounds));
        }
    }

    where_clause
}

/// Parse the return type of a proxy method.
pub(super) fn parse_return_type(
    output: &ReturnType,
    is_streaming: bool,
    return_fds: bool,
) -> Result<(Type, Type), Error> {
    match output {
        ReturnType::Default => Err(Error::new_spanned(
            output,
            "proxy methods must have a return type",
        )),
        ReturnType::Type(_, ty) => {
            if is_streaming && return_fds {
                // For streaming methods with FDs, expect Result<impl Stream<Item =
                // Result<(Result<T, E>, Vec<OwnedFd>)>>>
                extract_streaming_with_fds_result_types(ty)
            } else if is_streaming {
                // For streaming methods, expect Result<impl Stream<Item = Result<Result<T, E>>>>
                extract_streaming_result_types(ty)
            } else if return_fds {
                // For FD-returning methods, expect Result<(Result<T, E>, Vec<OwnedFd>)>
                extract_fds_result_types(ty)
            } else {
                // Extract Result<Result<T, E>> or impl Future<Output = Result<Result<T, E>>>
                extract_nested_result_types(ty)
            }
        }
    }
}

fn extract_nested_result_types(ty: &Type) -> Result<(Type, Type), Error> {
    const ERROR_MSG: &str = "expected Result<Result<ReplyType, ErrorType>> or \
                             impl Future<Output = Result<Result<ReplyType, ErrorType>>>";

    match ty {
        Type::Path(type_path) => extract_result_from_path(type_path, ERROR_MSG),
        Type::ImplTrait(impl_trait) => extract_from_future_output(impl_trait, ERROR_MSG),
        _ => Err(Error::new_spanned(ty, ERROR_MSG)),
    }
}

fn extract_fds_result_types(ty: &Type) -> Result<(Type, Type), Error> {
    const ERROR_MSG: &str = "expected Result<(Result<ReplyType, ErrorType>, Vec<OwnedFd>)> or \
                             impl Future<Output = Result<(Result<ReplyType, ErrorType>, \
                             Vec<OwnedFd>)>>";

    match ty {
        Type::Path(type_path) => extract_fds_result_from_path(type_path, ERROR_MSG),
        Type::ImplTrait(impl_trait) => extract_fds_from_future_output(impl_trait, ERROR_MSG),
        _ => Err(Error::new_spanned(ty, ERROR_MSG)),
    }
}

fn extract_result_from_path(
    type_path: &syn::TypePath,
    error_msg: &str,
) -> Result<(Type, Type), Error> {
    let segment = type_path
        .path
        .segments
        .last()
        .ok_or_else(|| Error::new_spanned(type_path, error_msg))?;

    if segment.ident != "Result" {
        return Err(Error::new_spanned(type_path, error_msg));
    }

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new_spanned(type_path, error_msg));
    };

    let GenericArgument::Type(inner_ty) = args
        .args
        .first()
        .ok_or_else(|| Error::new_spanned(type_path, error_msg))?
    else {
        return Err(Error::new_spanned(type_path, error_msg));
    };

    extract_inner_result_types(inner_ty)
}

fn extract_from_future_output(
    impl_trait: &syn::TypeImplTrait,
    error_msg: &str,
) -> Result<(Type, Type), Error> {
    impl_trait
        .bounds
        .iter()
        .find_map(|bound| {
            let syn::TypeParamBound::Trait(trait_bound) = bound else {
                return None;
            };
            let segment = trait_bound.path.segments.last()?;
            if segment.ident != "Future" {
                return None;
            }
            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return None;
            };
            args.args.iter().find_map(|arg| match arg {
                GenericArgument::AssocType(assoc) if assoc.ident == "Output" => {
                    Some(extract_nested_result_types(&assoc.ty))
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| Err(Error::new_spanned(impl_trait, error_msg)))
}

fn extract_inner_result_types(ty: &Type) -> Result<(Type, Type), Error> {
    let Type::Path(type_path) = ty else {
        return Err(Error::new_spanned(
            ty,
            "expected inner Result<ReplyType, ErrorType>",
        ));
    };

    let segment = match type_path.path.segments.last() {
        Some(segment) if segment.ident == "Result" => segment,
        _ => {
            return Err(Error::new_spanned(
                ty,
                "expected inner Result<ReplyType, ErrorType>",
            ));
        }
    };

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new_spanned(
            ty,
            "expected inner Result<ReplyType, ErrorType>",
        ));
    };

    match (args.args.get(0), args.args.get(1)) {
        (Some(GenericArgument::Type(reply_ty)), Some(GenericArgument::Type(error_ty)))
            if args.args.len() == 2 =>
        {
            Ok((reply_ty.clone(), error_ty.clone()))
        }
        _ => Err(Error::new_spanned(
            ty,
            "expected inner Result<ReplyType, ErrorType>",
        )),
    }
}

fn extract_fds_result_from_path(
    type_path: &syn::TypePath,
    error_msg: &str,
) -> Result<(Type, Type), Error> {
    let segment = type_path
        .path
        .segments
        .last()
        .ok_or_else(|| Error::new_spanned(type_path, error_msg))?;

    if segment.ident != "Result" {
        return Err(Error::new_spanned(type_path, error_msg));
    }

    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(Error::new_spanned(type_path, error_msg));
    };

    let GenericArgument::Type(tuple_ty) = args
        .args
        .first()
        .ok_or_else(|| Error::new_spanned(type_path, error_msg))?
    else {
        return Err(Error::new_spanned(type_path, error_msg));
    };

    extract_tuple_result_and_fds(tuple_ty, error_msg)
}

fn extract_fds_from_future_output(
    impl_trait: &syn::TypeImplTrait,
    error_msg: &str,
) -> Result<(Type, Type), Error> {
    impl_trait
        .bounds
        .iter()
        .find_map(|bound| {
            let syn::TypeParamBound::Trait(trait_bound) = bound else {
                return None;
            };
            let segment = trait_bound.path.segments.last()?;
            if segment.ident != "Future" {
                return None;
            }
            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return None;
            };
            args.args.iter().find_map(|arg| match arg {
                GenericArgument::AssocType(assoc) if assoc.ident == "Output" => {
                    Some(extract_fds_result_types(&assoc.ty))
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| Err(Error::new_spanned(impl_trait, error_msg)))
}

fn extract_tuple_result_and_fds(ty: &Type, error_msg: &str) -> Result<(Type, Type), Error> {
    let Type::Tuple(tuple) = ty else {
        return Err(Error::new_spanned(ty, error_msg));
    };

    if tuple.elems.len() != 2 {
        return Err(Error::new_spanned(ty, error_msg));
    }

    // Extract Result<T, E> from first element
    extract_inner_result_types(&tuple.elems[0])
}

fn extract_streaming_result_types(ty: &Type) -> Result<(Type, Type), Error> {
    const ERROR_MSG: &str =
        "expected Result<impl Stream<Item = Result<Result<ReplyType, ErrorType>>>>";

    match ty {
        Type::Path(type_path) => {
            // Direct Result<impl Stream<...>>
            let segment = type_path
                .path
                .segments
                .last()
                .ok_or_else(|| Error::new_spanned(type_path, ERROR_MSG))?;

            if segment.ident != "Result" {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            }

            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            };

            let GenericArgument::Type(stream_ty) = args
                .args
                .first()
                .ok_or_else(|| Error::new_spanned(type_path, ERROR_MSG))?
            else {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            };

            extract_stream_item_types(stream_ty)
        }
        Type::ImplTrait(impl_trait) => {
            // impl Future<Output = Result<impl Stream<...>>>
            impl_trait
                .bounds
                .iter()
                .find_map(|bound| {
                    let syn::TypeParamBound::Trait(trait_bound) = bound else {
                        return None;
                    };
                    let segment = trait_bound.path.segments.last()?;
                    if segment.ident != "Future" {
                        return None;
                    }
                    let PathArguments::AngleBracketed(args) = &segment.arguments else {
                        return None;
                    };
                    args.args.iter().find_map(|arg| match arg {
                        GenericArgument::AssocType(assoc) if assoc.ident == "Output" => {
                            Some(extract_streaming_result_types(&assoc.ty))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| Err(Error::new_spanned(impl_trait, ERROR_MSG)))
        }
        _ => Err(Error::new_spanned(ty, ERROR_MSG)),
    }
}

fn extract_stream_item_types(ty: &Type) -> Result<(Type, Type), Error> {
    match ty {
        Type::ImplTrait(impl_trait) => {
            // impl Stream<Item = Result<Result<T, E>>>
            impl_trait
                .bounds
                .iter()
                .find_map(|bound| {
                    let syn::TypeParamBound::Trait(trait_bound) = bound else {
                        return None;
                    };
                    let segment = trait_bound.path.segments.last()?;
                    if segment.ident != "Stream" {
                        return None;
                    }
                    let PathArguments::AngleBracketed(args) = &segment.arguments else {
                        return None;
                    };
                    args.args.iter().find_map(|arg| match arg {
                        GenericArgument::AssocType(assoc) if assoc.ident == "Item" => {
                            Some(extract_nested_result_types(&assoc.ty))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| {
                    Err(Error::new_spanned(
                        ty,
                        "expected impl Stream<Item = Result<Result<ReplyType, ErrorType>>>",
                    ))
                })
        }
        _ => Err(Error::new_spanned(
            ty,
            "expected impl Stream<Item = Result<Result<ReplyType, ErrorType>>>",
        )),
    }
}

fn extract_streaming_with_fds_result_types(ty: &Type) -> Result<(Type, Type), Error> {
    const ERROR_MSG: &str =
        "expected Result<impl Stream<Item = Result<(Result<ReplyType, ErrorType>, Vec<OwnedFd>)>>>";

    match ty {
        Type::Path(type_path) => {
            // Direct Result<impl Stream<...>>
            let segment = type_path
                .path
                .segments
                .last()
                .ok_or_else(|| Error::new_spanned(type_path, ERROR_MSG))?;

            if segment.ident != "Result" {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            }

            let PathArguments::AngleBracketed(args) = &segment.arguments else {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            };

            let GenericArgument::Type(stream_ty) = args
                .args
                .first()
                .ok_or_else(|| Error::new_spanned(type_path, ERROR_MSG))?
            else {
                return Err(Error::new_spanned(type_path, ERROR_MSG));
            };

            extract_stream_item_fds_types(stream_ty)
        }
        Type::ImplTrait(impl_trait) => {
            // impl Future<Output = Result<impl Stream<...>>>
            impl_trait
                .bounds
                .iter()
                .find_map(|bound| {
                    let syn::TypeParamBound::Trait(trait_bound) = bound else {
                        return None;
                    };
                    let segment = trait_bound.path.segments.last()?;
                    if segment.ident != "Future" {
                        return None;
                    }
                    let PathArguments::AngleBracketed(args) = &segment.arguments else {
                        return None;
                    };
                    args.args.iter().find_map(|arg| match arg {
                        GenericArgument::AssocType(assoc) if assoc.ident == "Output" => {
                            Some(extract_streaming_with_fds_result_types(&assoc.ty))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| Err(Error::new_spanned(impl_trait, ERROR_MSG)))
        }
        _ => Err(Error::new_spanned(ty, ERROR_MSG)),
    }
}

fn extract_stream_item_fds_types(ty: &Type) -> Result<(Type, Type), Error> {
    match ty {
        Type::ImplTrait(impl_trait) => {
            // impl Stream<Item = Result<(Result<T, E>, Vec<OwnedFd>)>>
            impl_trait
                .bounds
                .iter()
                .find_map(|bound| {
                    let syn::TypeParamBound::Trait(trait_bound) = bound else {
                        return None;
                    };
                    let segment = trait_bound.path.segments.last()?;
                    if segment.ident != "Stream" {
                        return None;
                    }
                    let PathArguments::AngleBracketed(args) = &segment.arguments else {
                        return None;
                    };
                    args.args.iter().find_map(|arg| match arg {
                        GenericArgument::AssocType(assoc) if assoc.ident == "Item" => {
                            Some(extract_fds_result_types(&assoc.ty))
                        }
                        _ => None,
                    })
                })
                .unwrap_or_else(|| {
                    Err(Error::new_spanned(
                        ty,
                        "expected impl Stream<Item = Result<(Result<ReplyType, ErrorType>, Vec<OwnedFd>)>>",
                    ))
                })
        }
        _ => Err(Error::new_spanned(
            ty,
            "expected impl Stream<Item = Result<(Result<ReplyType, ErrorType>, Vec<OwnedFd>)>>",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    /// `rename_all` must see `type`, not `r#type`: the prefix is Rust syntax, not the name.
    #[test]
    fn rename_all_applies_to_the_unrawed_ident() {
        let ident: Ident = parse_quote!(r#type);
        let name = resolve_serialized_name(&ident, None, Some(RenameAll::Pascal)).unwrap();

        assert_eq!(name.as_deref().map(|n| n.as_str()), Some("Type"));
    }

    #[test]
    fn explicit_rename_beats_rename_all() {
        let ident: Ident = parse_quote!(dry_run);
        let attrs = ParamAttrs {
            rename: Some("customName".into()),
            is_fds: false,
        };
        let name = resolve_serialized_name(&ident, Some(&attrs), Some(RenameAll::Pascal)).unwrap();

        assert_eq!(name.as_deref().map(|n| n.as_str()), Some("customName"));
    }

    #[test]
    fn no_attrs_leave_the_ident_alone() {
        let ident: Ident = parse_quote!(dry_run);

        assert_eq!(resolve_serialized_name(&ident, None, None).unwrap(), None);
    }

    /// A produced name the Varlink field grammar cannot express is a compile error, not a wire
    /// name the peer rejects.
    #[test]
    fn invalid_produced_name_is_rejected() {
        let ident: Ident = parse_quote!(dry_run);
        let err = resolve_serialized_name(&ident, None, Some(RenameAll::Kebab))
            .unwrap_err()
            .to_string();

        assert!(err.contains("`dry-run`"), "must name the bad name: {err}");
    }

    /// Rust idents that the Varlink field grammar rejects must fail even with no rename rule at
    /// all -- `None` is not a license to skip validation.
    #[test]
    fn unrenamed_ident_still_validated() {
        for bad in ["_foo", "foo_", "foo__bar"] {
            let ident: Ident = syn::parse_str(bad).unwrap();
            let err = resolve_serialized_name(&ident, None, None)
                .unwrap_err()
                .to_string();

            assert!(err.contains(&format!("`{bad}`")), "for {bad}: {err}");
        }
    }
}
