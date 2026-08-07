//! Method information extraction for service macro.

use crate::naming::{self, NameSource};
use proc_macro2::TokenStream;
use syn::{
    Attribute, Error, Expr, GenericArgument, Ident, ImplItemFn, Lit, LitStr, Meta, PathArguments,
    ReturnType, Type, parse::Parse,
};

use crate::naming::Grammar;
use zlink_names::OwnedInterfaceName;

use super::types::ParamInfo;

/// Information extracted from a service method.
pub(super) struct MethodInfo {
    /// The original method name (snake_case).
    pub name: Ident,
    /// The Varlink method name (PascalCase or renamed).
    pub varlink_name: String,
    /// The interface name for this method.
    pub interface: Option<OwnedInterfaceName>,
    /// Custom types scoped to this method's interface (from `#[zlink(types = [...])]`).
    pub custom_types: Vec<Type>,
    /// Parameters for this method (excluding self).
    pub params: Vec<ParamInfo>,
    /// The return type for serialization (the `T` in `Result<T, E>`, or the type itself).
    pub return_type: Option<Type>,
    /// Whether the method returns `Result<T, E>` (true) or just `T` (false).
    pub returns_result: bool,
    /// The error type if the method returns `Result<T, E>` (the `E`).
    pub error_type: Option<Type>,
    /// The method body tokens (for inlining methods with connection params).
    pub body: TokenStream,
    /// Whether this method returns a stream of replies (has `#[zlink(more)]`).
    pub is_streaming: bool,
    /// The item type of the stream (the `T` in `Stream<Item = Reply<T>>` or
    /// `Stream<Item = Result<Reply<T>, E>>`).
    pub stream_item_type: Option<Type>,
    /// The error type for streams that yield `Result<Reply<T>, E>` (the `E`).
    pub stream_error_type: Option<Type>,
    /// The full return type for streaming methods (for generating enum variants).
    pub stream_return_type: Option<Type>,
    /// Whether the streaming method returns `impl Trait` (requires boxing).
    pub stream_uses_impl_trait: bool,
    /// Whether this method returns file descriptors (`#[zlink(return_fds)]`).
    pub return_fds: bool,
    /// Doc-comments on this method, for introspection.
    pub comments: Vec<String>,
}

impl MethodInfo {
    /// Extract method information from an impl method, updating current_interface if a new
    /// interface attribute is found.
    pub(super) fn extract(
        method: &mut ImplItemFn,
        current_interface: &mut Option<OwnedInterfaceName>,
    ) -> Result<Self, Error> {
        let name = method.sig.ident.clone();

        // Extract doc comments before any attribute processing.
        let comments = crate::utils::extract_doc_comments(&method.attrs);

        // Extract method attributes.
        let method_attrs = MethodAttrs::extract(&mut method.attrs)?;

        // Update current interface if this method specifies one.
        if let Some(ref iface) = method_attrs.interface {
            *current_interface = Some(iface.clone());
        }

        // Determine the Varlink method name. The ident is unraw'd first: `r#` is Rust syntax, not
        // part of the name, and it would otherwise reach `format_ident!` as `R#type`.
        let (varlink_name, name_source) = match &method_attrs.rename {
            Some(lit) => (lit.value(), NameSource::Rename(lit)),
            None => (
                snake_case_to_pascal_case(&naming::unraw(&name)),
                NameSource::Ident(&name),
            ),
        };
        // A method is named like a type, so it must be expressible as one.
        naming::validate(&varlink_name, Grammar::Type, "method name", name_source)?;

        // Check if this is a streaming method.
        let is_streaming = method_attrs.is_streaming;

        // Extract parameters (skip self).
        let params: Vec<ParamInfo> = method
            .sig
            .inputs
            .iter()
            .skip(1)
            .enumerate()
            .filter_map(|(idx, arg)| {
                let mut param_info = ParamInfo::from_fn_arg(arg)?;
                // Try to extract zlink attributes from the parameter.
                if let syn::FnArg::Typed(pat_type) = arg {
                    let param_attrs = extract_param_attrs(&pat_type.attrs);
                    param_info.serialized_name = param_attrs.rename;
                    param_info.is_connection = param_attrs.is_connection;
                    param_info.is_fds = param_attrs.is_fds;
                }
                // For streaming methods, the first param must be `more: bool`.
                if is_streaming && idx == 0 {
                    param_info.is_more = true;
                }
                Some(param_info)
            })
            .collect();

        // For streaming methods, validate the `more` parameter.
        if is_streaming {
            let first_param = params.first().ok_or_else(|| {
                Error::new_spanned(
                    &method.sig,
                    "streaming methods must have `more: bool` as the first parameter after `self`",
                )
            })?;
            if !is_bool_type(&first_param.ty) {
                return Err(Error::new_spanned(
                    &method.sig.inputs,
                    "streaming methods must have `more: bool` as the first parameter after `self`",
                ));
            }
        }

        // Serialized parameters (the ones that end up on the wire and in the IDL) must carry a
        // name Varlink can express. The `_`-prefixed case falls out of this: `_foo` fails the
        // field grammar at its first character.
        for param in params
            .iter()
            .filter(|p| !p.is_connection && !p.is_more && !p.is_fds)
        {
            let (wire_name, source) = match &param.serialized_name {
                Some(lit) => (lit.value(), NameSource::Rename(lit)),
                None => (naming::unraw(&param.name), NameSource::Ident(&param.name)),
            };
            naming::validate(&wire_name, Grammar::Field, "parameter name", source)?;
        }

        // Validate FD attributes.
        let return_fds = method_attrs.return_fds;
        let fds_params: Vec<_> = params.iter().filter(|p| p.is_fds).collect();
        if fds_params.len() > 1 {
            return Err(Error::new_spanned(
                &method.sig,
                "at most one `#[zlink(fds)]` parameter is allowed per method",
            ));
        }
        #[cfg(not(feature = "std"))]
        if !fds_params.is_empty() || return_fds {
            return Err(Error::new_spanned(
                &method.sig,
                "FD-related attributes (`#[zlink(fds)]` and `#[zlink(return_fds)]`) \
                 require the `std` feature to be enabled",
            ));
        }

        // Extract return type and check if it's a Result or Stream.
        let (
            return_type,
            returns_result,
            error_type,
            stream_item_type,
            stream_error_type,
            stream_return_type,
            stream_uses_impl_trait,
        ) = if is_streaming && return_fds {
            // For streaming methods with FD passing, the stream yields:
            //   `(Reply<T>, Vec<OwnedFd>)`              — no error path
            //   `(Result<Reply<T>, E>, Vec<OwnedFd>)`   — with error path
            match &method.sig.output {
                ReturnType::Default => {
                    return Err(Error::new_spanned(
                        &method.sig,
                        "streaming methods with return_fds must return a Stream<Item = \
                         (Reply<T>, Vec<OwnedFd>)> or Stream<Item = (Result<Reply<T>, E>, \
                         Vec<OwnedFd>)>",
                    ));
                }
                ReturnType::Type(_, ty) => {
                    let stream_item = extract_stream_item_type(ty).ok_or_else(|| {
                        Error::new_spanned(
                            ty,
                            "streaming methods with return_fds must return a Stream<Item = \
                             (Reply<T>, Vec<OwnedFd>)> or Stream<Item = (Result<Reply<T>, E>, \
                             Vec<OwnedFd>)> (could not extract Stream's Item type)",
                        )
                    })?;
                    // Extract first element from (X, Vec<OwnedFd>).
                    let first = extract_first_tuple_element(&stream_item).ok_or_else(|| {
                        Error::new_spanned(
                            ty,
                            "streaming methods with return_fds must return a Stream<Item = \
                             (Reply<T>, Vec<OwnedFd>)> or Stream<Item = (Result<Reply<T>, E>, \
                             Vec<OwnedFd>)> (stream item must be a tuple)",
                        )
                    })?;
                    let (inner_type, err_type) =
                        extract_reply_or_result_reply(&first).ok_or_else(|| {
                            Error::new_spanned(
                                ty,
                                "streaming methods with return_fds must return a Stream<Item = \
                                 (Reply<T>, Vec<OwnedFd>)> or Stream<Item = (Result<Reply<T>, \
                                 E>, Vec<OwnedFd>)> (first tuple element must be Reply<T> or \
                                 Result<Reply<T>, E>)",
                            )
                        })?;
                    // Check if return type uses `impl Trait`.
                    let uses_impl_trait = matches!(**ty, Type::ImplTrait(_));
                    (
                        None,
                        false,
                        None,
                        Some(inner_type),
                        err_type,
                        Some((**ty).clone()),
                        uses_impl_trait,
                    )
                }
            }
        } else if is_streaming {
            // For streaming methods, extract the Stream's Item type. Streaming methods can yield:
            // - `Reply<T>`                — no error path
            // - `Result<Reply<T>, E>`     — with error path
            // The macro accepts both `impl Stream<Item = ...>` and concrete stream types.
            match &method.sig.output {
                ReturnType::Default => {
                    return Err(Error::new_spanned(
                        &method.sig,
                        "streaming methods must return a Stream<Item = Reply<T>> or \
                         Stream<Item = Result<Reply<T>, E>>",
                    ));
                }
                ReturnType::Type(_, ty) => {
                    let stream_item = extract_stream_item_type(ty).ok_or_else(|| {
                        Error::new_spanned(
                            ty,
                            "streaming methods must return a Stream<Item = Reply<T>> or \
                             Stream<Item = Result<Reply<T>, E>> (could not extract Stream's Item \
                             type)",
                        )
                    })?;
                    let (inner_type, err_type) = extract_reply_or_result_reply(&stream_item)
                        .ok_or_else(|| {
                            Error::new_spanned(
                                ty,
                                "streaming methods must return a Stream<Item = Reply<T>> or \
                                 Stream<Item = Result<Reply<T>, E>> (stream item must be \
                                 Reply<T> or Result<Reply<T>, E>)",
                            )
                        })?;
                    // Check if return type uses `impl Trait`.
                    let uses_impl_trait = matches!(**ty, Type::ImplTrait(_));
                    (
                        None,
                        false,
                        None,
                        Some(inner_type),
                        err_type,
                        Some((**ty).clone()),
                        uses_impl_trait,
                    )
                }
            }
        } else if return_fds {
            // For return_fds methods, the return type is a tuple whose second element
            // is `Vec<OwnedFd>`. The first element is either:
            // - `Result<T, E>` → `(Result<T, E>, Vec<OwnedFd>)` — extract T and E
            // - `T` → `(T, Vec<OwnedFd>)` — extract T, no error type
            match &method.sig.output {
                ReturnType::Default => {
                    return Err(Error::new_spanned(
                        &method.sig,
                        "`return_fds` methods must have a return type",
                    ));
                }
                ReturnType::Type(_, ty) => {
                    // Extract the first element of the tuple.
                    let first = extract_first_tuple_element(ty).ok_or_else(|| {
                        Error::new_spanned(
                            ty,
                            "`return_fds` methods must return \
                             `(T, Vec<OwnedFd>)` or `(Result<T, E>, Vec<OwnedFd>)`",
                        )
                    })?;

                    if let Some((inner_ty, err_ty)) = extract_result_types(&first) {
                        // (Result<T, E>, Vec<OwnedFd>).
                        (inner_ty, true, Some(err_ty), None, None, None, false)
                    } else {
                        // (T, Vec<OwnedFd>).
                        let data_ty = if is_unit_type(&first) {
                            None
                        } else {
                            Some(first)
                        };
                        (data_ty, false, None, None, None, None, false)
                    }
                }
            }
        } else {
            // For non-streaming methods, extract Result types as before.
            match &method.sig.output {
                ReturnType::Default => (None, false, None, None, None, None, false),
                ReturnType::Type(_, ty) => {
                    if let Some((inner_ty, err_ty)) = extract_result_types(ty) {
                        (inner_ty, true, Some(err_ty), None, None, None, false)
                    } else {
                        (Some((**ty).clone()), false, None, None, None, None, false)
                    }
                }
            }
        };

        // Capture the method body.
        let block = &method.block;
        let body = quote::quote! { #block };

        Ok(Self {
            name,
            varlink_name,
            interface: current_interface.clone(),
            custom_types: method_attrs.custom_types,
            params,
            return_type,
            returns_result,
            error_type,
            body,
            is_streaming,
            stream_item_type,
            stream_error_type,
            stream_return_type,
            stream_uses_impl_trait,
            return_fds,
            comments,
        })
    }

    /// Get the full Varlink method path (interface.MethodName).
    pub(super) fn full_method_path(&self) -> Option<String> {
        self.interface
            .as_ref()
            .map(|iface| format!("{}.{}", iface, self.varlink_name))
    }

    /// Check if this method has a connection parameter.
    pub(super) fn has_connection_param(&self) -> bool {
        self.params.iter().any(|p| p.is_connection)
    }

    /// Get parameters that are serialized (excludes connection, more, and fds parameters).
    pub(super) fn serialized_params(&self) -> impl Iterator<Item = &ParamInfo> {
        self.params
            .iter()
            .filter(|p| !p.is_connection && !p.is_more && !p.is_fds)
    }
}

/// Attributes extracted from method-level `#[zlink(...)]`.
#[derive(Default)]
struct MethodAttrs {
    /// The interface name for this method.
    interface: Option<OwnedInterfaceName>,
    /// Custom types scoped to this method's interface.
    custom_types: Vec<Type>,
    /// Custom method name.
    rename: Option<LitStr>,
    /// Whether this method returns a stream of replies.
    is_streaming: bool,
    /// Whether this method returns file descriptors.
    return_fds: bool,
}

impl MethodAttrs {
    /// Extract method attributes from a method's attribute list, removing processed attributes.
    fn extract(attrs: &mut Vec<Attribute>) -> Result<Self, Error> {
        let mut result = Self::default();
        let mut indices_to_remove = Vec::new();

        for (i, attr) in attrs.iter().enumerate() {
            if !attr.path().is_ident("zlink") {
                continue;
            }

            indices_to_remove.push(i);

            let Meta::List(list) = &attr.meta else {
                continue;
            };

            if list.tokens.is_empty() {
                continue;
            }

            // Use syn::meta::parser to handle both standard Meta items and custom
            // `types = [...]` syntax.
            let parser = syn::meta::parser(|meta| {
                if meta.path.is_ident("interface") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    let validated = naming::validate_interface(&value)?;
                    result.interface = Some(validated);
                } else if meta.path.is_ident("rename") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    result.rename = Some(value);
                } else if meta.path.is_ident("types") {
                    meta.input.parse::<syn::Token![=]>()?;
                    let content;
                    syn::bracketed!(content in meta.input);
                    let types = content.parse_terminated(Type::parse, syn::Token![,])?;
                    result.custom_types = types.into_iter().collect();
                } else if meta.path.is_ident("more") {
                    if result.is_streaming {
                        return Err(meta.error("duplicate `more` attribute"));
                    }
                    result.is_streaming = true;
                } else if meta.path.is_ident("return_fds") {
                    if result.return_fds {
                        return Err(meta.error("duplicate `return_fds` attribute"));
                    }
                    result.return_fds = true;
                } else {
                    return Err(meta.error("unknown zlink attribute"));
                }
                Ok(())
            });

            syn::parse::Parser::parse2(parser, list.tokens.clone())?;
        }

        // Remove zlink attributes in reverse order to preserve indices.
        for &index in indices_to_remove.iter().rev() {
            attrs.remove(index);
        }

        Ok(result)
    }
}

/// Attributes extracted from parameter-level `#[zlink(...)]`.
#[derive(Default)]
struct ParamAttrs {
    /// Custom serialized name for the parameter.
    rename: Option<LitStr>,
    /// Whether this parameter should receive the connection.
    is_connection: bool,
    /// Whether this parameter receives file descriptors.
    is_fds: bool,
}

/// Extract zlink attributes from parameter attributes.
fn extract_param_attrs(attrs: &[Attribute]) -> ParamAttrs {
    let mut result = ParamAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        let Meta::List(list) = &attr.meta else {
            continue;
        };

        let Ok(nested) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in nested {
            match &meta {
                Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                    if let Expr::Lit(expr_lit) = &nv.value
                        && let Lit::Str(lit_str) = &expr_lit.lit
                    {
                        result.rename = Some(lit_str.clone());
                    }
                }
                Meta::Path(path) if path.is_ident("connection") => {
                    result.is_connection = true;
                }
                Meta::Path(path) if path.is_ident("fds") => {
                    result.is_fds = true;
                }
                _ => {}
            }
        }
    }

    result
}

/// Convert snake_case to PascalCase.
fn snake_case_to_pascal_case(input: &str) -> String {
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

/// Extract `T` and `E` types from `Result<T, E>`. Returns `None` if the type is not a `Result`.
/// Returns `Some((None, E))` if the Result's Ok type is `()`.
fn extract_result_types(ty: &Type) -> Option<(Option<Type>, Type)> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let last_segment = type_path.path.segments.last()?;
    if last_segment.ident != "Result" {
        return None;
    }

    let PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return None;
    };

    // Get the first generic argument (the Ok type).
    let first_arg = args.args.first()?;
    let GenericArgument::Type(ok_type) = first_arg else {
        return None;
    };

    // Get the second generic argument (the Err type).
    let second_arg = args.args.iter().nth(1)?;
    let GenericArgument::Type(err_type) = second_arg else {
        return None;
    };

    // Check if the Ok type is `()`.
    let ok_type = if let Type::Tuple(tuple) = ok_type {
        if tuple.elems.is_empty() {
            None
        } else {
            Some(ok_type.clone())
        }
    } else {
        Some(ok_type.clone())
    };

    Some((ok_type, err_type.clone()))
}

/// Extract the `Item` type from `impl Stream<Item = T>` or similar stream types.
/// Returns `None` if the type is not a recognizable stream type.
///
/// For `impl Stream<Item = Reply<T>>`, returns `Reply<T>`.
/// For concrete types like `SomeStream<T>`, returns `Reply<T>` (assuming the generic param is T).
pub(super) fn extract_stream_item_type(ty: &Type) -> Option<Type> {
    match ty {
        // Handle `impl Stream<Item = T> + ...` (impl trait syntax).
        Type::ImplTrait(impl_trait) => {
            for bound in &impl_trait.bounds {
                if let syn::TypeParamBound::Trait(trait_bound) = bound
                    && let Some(item_type) = extract_stream_item_from_trait_bound(trait_bound)
                {
                    return Some(item_type);
                }
            }
            None
        }
        // Handle dyn trait syntax (e.g., `Box<dyn Stream<Item = T>>`).
        Type::TraitObject(trait_object) => {
            for bound in &trait_object.bounds {
                if let syn::TypeParamBound::Trait(trait_bound) = bound
                    && let Some(item_type) = extract_stream_item_from_trait_bound(trait_bound)
                {
                    return Some(item_type);
                }
            }
            None
        }
        // Handle concrete path types like `notified::Stream<T>` or `SomeStream<T>`.
        // For these, we assume the first generic parameter is the stream item type T,
        // and the stream yields `Reply<T>`.
        Type::Path(type_path) => {
            let last_segment = type_path.path.segments.last()?;
            let PathArguments::AngleBracketed(args) = &last_segment.arguments else {
                return None;
            };
            // Get the first generic argument as the item type.
            let first_arg = args.args.first()?;
            let GenericArgument::Type(item_type) = first_arg else {
                return None;
            };
            // Wrap it in Reply<T> since concrete stream types yield Reply<T>.
            Some(syn::parse_quote!(Reply<#item_type>))
        }
        _ => None,
    }
}

/// Extract the `Item` type from a trait bound like `Stream<Item = T>`.
fn extract_stream_item_from_trait_bound(trait_bound: &syn::TraitBound) -> Option<Type> {
    // Check if this is a Stream trait.
    let last_segment = trait_bound.path.segments.last()?;
    if last_segment.ident != "Stream" {
        return None;
    }

    // Get the angle-bracketed arguments.
    let PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return None;
    };

    // Find the `Item = T` binding.
    for arg in &args.args {
        if let GenericArgument::AssocType(assoc_type) = arg
            && assoc_type.ident == "Item"
        {
            return Some(assoc_type.ty.clone());
        }
    }

    None
}

/// Extract the success and (optional) error types from `Reply<T>` or `Result<Reply<T>, E>`.
///
/// Returns `Some((T, None))` for `Reply<T>`, `Some((T, Some(E)))` for `Result<Reply<T>, E>`,
/// and `None` if the type matches neither shape.
pub(super) fn extract_reply_or_result_reply(ty: &Type) -> Option<(Type, Option<Type>)> {
    if let Some(inner) = extract_reply_inner_type(ty) {
        return Some((inner, None));
    }
    let (ok_ty, err_ty) = extract_result_types(ty)?;
    let ok_ty = ok_ty?;
    let inner = extract_reply_inner_type(&ok_ty)?;
    Some((inner, Some(err_ty)))
}

/// Extract the inner type `T` from `Reply<T>`.
/// Returns `None` if the type is not a `Reply<T>`.
fn extract_reply_inner_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let last_segment = type_path.path.segments.last()?;
    if last_segment.ident != "Reply" {
        return None;
    }

    let PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return None;
    };

    // Get the first generic argument (the inner type).
    let first_arg = args.args.first()?;
    let GenericArgument::Type(inner_type) = first_arg else {
        return None;
    };

    Some(inner_type.clone())
}

/// Check if a type is `bool`.
fn is_bool_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path.path.is_ident("bool")
}

/// Check if a type is the unit type `()`.
fn is_unit_type(ty: &Type) -> bool {
    let Type::Tuple(tuple) = ty else {
        return false;
    };
    tuple.elems.is_empty()
}

/// Extract the first element type from a tuple type `(T, ...)`.
/// Returns `None` if the type is not a tuple with at least one element.
pub(super) fn extract_first_tuple_element(ty: &Type) -> Option<Type> {
    let Type::Tuple(tuple) = ty else {
        return None;
    };
    tuple.elems.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::{FnArg, parse_quote};

    /// Validation must judge the same string `ParamInfo::wire_name` does, or a raw ident slips past
    /// it and reaches the IDL unraw'd as `_foo`. Both spellings resolve to `_foo`, which the field
    /// grammar rejects at its first character.
    #[test]
    fn underscore_prefixed_params_rejected_raw_or_not() {
        for src in ["_foo: String", "r#_foo: String"] {
            let param: FnArg = syn::parse_str(src).unwrap();
            let mut method: ImplItemFn = parse_quote! {
                async fn probe(&self, #param) -> Result<Reply, Error> { todo!() }
            };
            let mut interface = Some(OwnedInterfaceName::try_from("org.example.probe").unwrap());

            let Err(err) = MethodInfo::extract(&mut method, &mut interface) else {
                panic!("`{src}` must be rejected: it reaches the IDL as `_foo`");
            };

            let err = err.to_string();
            assert!(
                err.contains("`_foo`") && err.contains("parameter name"),
                "message must name the offending parameter: {err}"
            );
            assert!(
                err.contains("rename"),
                "message must point at the `rename` escape hatch: {err}"
            );
        }
    }
}
