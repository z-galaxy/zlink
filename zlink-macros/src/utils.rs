#[cfg(feature = "introspection")]
use proc_macro2::TokenStream as TokenStream2;
#[cfg(feature = "introspection")]
use quote::quote;
use syn::{Attribute, Type};
#[cfg(feature = "introspection")]
use syn::{Error, GenericArgument, PathArguments};

/// Parse the crate path from attributes, defaulting to `::zlink`.
///
/// Looks for `#[zlink(crate = "...")]` attribute and uses the specified crate path.
/// If no such attribute is found, defaults to `::zlink`.
///
/// # Examples
///
/// ```ignore
/// #[derive(Type)]
/// #[zlink(crate = "crate")]
/// struct MyStruct;
/// ```
#[cfg(feature = "introspection")]
pub(crate) fn parse_crate_path(attrs: &[Attribute]) -> Result<TokenStream2, Error> {
    for attr in attrs {
        if attr.path().is_ident("zlink") {
            let mut result = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("crate") {
                    let value = meta.value()?;
                    let lit_str: syn::LitStr = value.parse()?;
                    let crate_path = lit_str.value();
                    result = Some(syn::parse_str(&crate_path)?);
                } else {
                    skip_unknown_meta(&meta)?;
                }
                Ok(())
            })?;

            if let Some(path) = result {
                return Ok(path);
            }
        }
    }
    // Default to ::zlink
    Ok(quote! { ::zlink })
}

/// Sanitize a string for use inside a generated identifier.
///
/// ASCII alphanumeric characters pass through unchanged; every other character (including
/// `_` itself, so it can never be mistaken for an escape delimiter) is replaced with its code
/// point in hex, wrapped in `_.._`. This keeps the mapping injective, so distinct inputs (e.g.
/// interface names differing only in whether they use `.` or `-`) can never collide on the same
/// generated identifier.
#[cfg(feature = "service")]
pub(crate) fn to_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
            out.push_str(&format!("{:x}", c as u32));
            out.push('_');
        }
    }
    out
}

/// Consume an unknown `#[zlink(..)]` key's payload so the surrounding walk can continue.
///
/// Keys come in three shapes: bare (`borrow`), name-value (`crate = "x"`) and list (`foo(..)`).
/// Bare keys have no payload to consume, so peeking first avoids failing the whole walk.
pub(crate) fn skip_unknown_meta(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let value = meta.value()?;
        let _: syn::Expr = value.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _: proc_macro2::TokenStream = content.parse()?;
    }

    Ok(())
}

/// Extract doc comments from attributes.
///
/// Each `#[doc = "..."]` attribute becomes a single comment string.
#[cfg(feature = "introspection")]
pub(crate) fn extract_doc_comments(attrs: &[Attribute]) -> Vec<String> {
    let mut comments = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("doc") {
            // Try different parsing methods
            if let Ok(lit_str) = attr.parse_args::<syn::LitStr>() {
                comments.push(lit_str.value());
            } else if let syn::Meta::NameValue(meta_name_value) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &meta_name_value.value
            {
                comments.push(lit_str.value());
            }
        }
    }

    comments
}

/// Recursively removes all lifetimes from a type.
#[cfg(feature = "introspection")]
pub(crate) fn remove_lifetimes_from_type(ty: &Type) -> Type {
    match ty {
        Type::Reference(type_ref) => Type::Reference(syn::TypeReference {
            and_token: type_ref.and_token,
            lifetime: None,
            mutability: type_ref.mutability,
            elem: Box::new(remove_lifetimes_from_type(&type_ref.elem)),
        }),
        Type::Path(type_path) => {
            let mut new_type_path = type_path.clone();
            if let Some(ref mut qself) = new_type_path.qself {
                *qself.ty = remove_lifetimes_from_type(&qself.ty);
            }
            for segment in &mut new_type_path.path.segments {
                match &mut segment.arguments {
                    PathArguments::AngleBracketed(args) => {
                        // Filter out lifetime arguments and recursively process type arguments
                        let mut new_args = syn::punctuated::Punctuated::new();
                        for arg in args.args.iter() {
                            match arg {
                                GenericArgument::Type(ty) => {
                                    new_args.push(GenericArgument::Type(
                                        remove_lifetimes_from_type(ty),
                                    ));
                                }
                                GenericArgument::Lifetime(_) => {
                                    // Replace with elided lifetime '_
                                    new_args.push(GenericArgument::Lifetime(syn::Lifetime::new(
                                        "'_",
                                        proc_macro2::Span::call_site(),
                                    )));
                                }
                                other => {
                                    new_args.push(other.clone());
                                }
                            }
                        }
                        args.args = new_args;
                    }
                    PathArguments::Parenthesized(args) => {
                        for input in &mut args.inputs {
                            *input = remove_lifetimes_from_type(input);
                        }
                        if let syn::ReturnType::Type(_, ref mut output) = args.output {
                            **output = remove_lifetimes_from_type(output);
                        }
                    }
                    PathArguments::None => {}
                }
            }
            Type::Path(new_type_path)
        }
        Type::Tuple(type_tuple) => {
            let mut new_type_tuple = type_tuple.clone();
            for elem in &mut new_type_tuple.elems {
                *elem = remove_lifetimes_from_type(elem);
            }
            Type::Tuple(new_type_tuple)
        }
        Type::Array(type_array) => {
            let mut new_type_array = type_array.clone();
            *new_type_array.elem = remove_lifetimes_from_type(&type_array.elem);
            Type::Array(new_type_array)
        }
        Type::Slice(type_slice) => {
            let mut new_type_slice = type_slice.clone();
            *new_type_slice.elem = remove_lifetimes_from_type(&type_slice.elem);
            Type::Slice(new_type_slice)
        }
        Type::Ptr(type_ptr) => {
            let mut new_type_ptr = type_ptr.clone();
            *new_type_ptr.elem = remove_lifetimes_from_type(&type_ptr.elem);
            Type::Ptr(new_type_ptr)
        }
        Type::Group(type_group) => {
            let mut new_type_group = type_group.clone();
            *new_type_group.elem = remove_lifetimes_from_type(&type_group.elem);
            Type::Group(new_type_group)
        }
        Type::Paren(type_paren) => {
            let mut new_type_paren = type_paren.clone();
            *new_type_paren.elem = remove_lifetimes_from_type(&type_paren.elem);
            Type::Paren(new_type_paren)
        }
        // For types that don't contain other types or lifetimes, return as-is
        Type::BareFn(_)
        | Type::ImplTrait(_)
        | Type::Infer(_)
        | Type::Macro(_)
        | Type::Never(_)
        | Type::TraitObject(_)
        | Type::Verbatim(_) => ty.clone(),

        // Handle any new types added to syn that we haven't covered
        _ => ty.clone(),
    }
}

/// Check if a type is `Option<T>`.
/// Handles `Option`, `std::option::Option`, and `core::option::Option`.
pub(crate) fn is_option_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };

    let path = &type_path.path;

    // Check if this is a single segment (just "Option")
    if path.segments.len() == 1 {
        return path.segments.first().unwrap().ident == "Option";
    }

    // Check for multi-segment paths like std::option::Option or core::option::Option
    if path.segments.len() >= 2 {
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        let path_str = segments.join("::");

        return path_str == "std::option::Option"
            || path_str == "core::option::Option"
            || path_str.ends_with("::std::option::Option")
            || path_str.ends_with("::core::option::Option");
    }

    false
}

/// Convert all lifetimes in a type to a specific target lifetime.
/// This is useful for generating synthetic lifetimes in macro expansions.
pub(crate) fn convert_type_lifetimes(ty: &Type, target_lifetime: &str) -> Type {
    let target_lt: syn::Lifetime = syn::parse_str(target_lifetime).unwrap();

    match ty {
        Type::Reference(type_ref) => {
            let elem = convert_type_lifetimes(&type_ref.elem, target_lifetime);
            Type::Reference(syn::TypeReference {
                and_token: type_ref.and_token,
                lifetime: Some(target_lt),
                mutability: type_ref.mutability,
                elem: Box::new(elem),
            })
        }
        Type::Path(type_path) => {
            let mut new_path = type_path.clone();
            for segment in &mut new_path.path.segments {
                let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments else {
                    continue;
                };
                let mut new_args = args.clone();
                new_args.args = args
                    .args
                    .iter()
                    .map(|arg| match arg {
                        syn::GenericArgument::Type(inner_ty) => syn::GenericArgument::Type(
                            convert_type_lifetimes(inner_ty, target_lifetime),
                        ),
                        syn::GenericArgument::Lifetime(_) => {
                            syn::GenericArgument::Lifetime(target_lt.clone())
                        }
                        _ => arg.clone(),
                    })
                    .collect();
                segment.arguments = syn::PathArguments::AngleBracketed(new_args);
            }
            Type::Path(new_path)
        }
        Type::Slice(type_slice) => {
            let elem = convert_type_lifetimes(&type_slice.elem, target_lifetime);
            Type::Slice(syn::TypeSlice {
                bracket_token: type_slice.bracket_token,
                elem: Box::new(elem),
            })
        }
        Type::Array(type_array) => {
            let elem = convert_type_lifetimes(&type_array.elem, target_lifetime);
            Type::Array(syn::TypeArray {
                bracket_token: type_array.bracket_token,
                elem: Box::new(elem),
                semi_token: type_array.semi_token,
                len: type_array.len.clone(),
            })
        }
        Type::Tuple(type_tuple) => {
            let elems = type_tuple
                .elems
                .iter()
                .map(|elem| convert_type_lifetimes(elem, target_lifetime))
                .collect();
            Type::Tuple(syn::TypeTuple {
                paren_token: type_tuple.paren_token,
                elems,
            })
        }
        _ => ty.clone(),
    }
}

/// Check if a boolean zlink attribute with a specific key is present.
///
/// For example, check for `#[zlink(borrow)]` by calling with key "borrow".
/// Returns true if the attribute is found.
pub(crate) fn has_zlink_bool_attr(attrs: &[Attribute], key: &str) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(key) {
                found = true;
            } else {
                skip_unknown_meta(&meta)?;
            }
            Ok(())
        });

        if found {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[cfg(feature = "introspection")]
    #[test]
    fn crate_path_after_valueless_key() {
        let attr: Attribute = parse_quote!(#[zlink(borrow, crate = "crate")]);
        let path = parse_crate_path(&[attr]).unwrap();
        assert_eq!(path.to_string(), "crate");
    }
}
