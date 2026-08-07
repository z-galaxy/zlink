//! Code generation for the service macro.

use std::collections::{HashMap, HashSet};

use proc_macro2::{Ident, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{Error, GenericParam, ItemImpl, Type};
use zlink_names::{InterfaceName, OwnedInterfaceName};

use super::{attrs::ServiceAttrs, method::MethodInfo};
use crate::utils::convert_type_lifetimes;

/// Context for generating the `handle` method body.
struct HandleBodyContext<'a> {
    crate_path: &'a TokenStream,
    method_call_name: &'a Ident,
    user_methods_name: &'a Ident,
    reply_params_name: &'a Ident,
    reply_error_name: &'a Ident,
    reply_stream_params_name: &'a Ident,
    /// The token stream for the resolved `ReplyStreamError` type (either the generated enum or
    /// the [`Infallible`](crate::service::Infallible) sentinel).
    reply_stream_error_ty: &'a TokenStream,
    reply_stream_name: &'a Ident,
    error_type_map: &'a HashMap<String, usize>,
    stream_item_type_map: &'a HashMap<String, Ident>,
    interfaces: &'a [InterfaceName<'a>],
    type_name: &'a str,
    /// Whether streaming methods require boxing (any uses `impl Trait`).
    needs_stream_boxing: bool,
}

/// Extract a simple type name from a Type for generating auxiliary type names.
fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

/// Collect all unique interfaces from methods.
fn collect_interfaces(methods_info: &[MethodInfo]) -> Vec<OwnedInterfaceName> {
    let mut seen = HashSet::new();
    let mut interfaces = Vec::new();
    for method in methods_info {
        if let Some(ref iface) = method.interface
            && seen.insert(iface.clone())
        {
            interfaces.push(iface.clone());
        }
    }
    interfaces
}

/// Generate the Service trait implementation.
pub(super) fn generate_service_impl(
    item_impl: &ItemImpl,
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
    impl_comments: &[String],
) -> Result<TokenStream, Error> {
    let crate_path = &service_attrs.crate_path;
    let self_ty = &item_impl.self_ty;

    // Extract the type name for generating auxiliary type names.
    // All generated types use `__` prefix to indicate they are internal implementation details.
    let type_name = extract_type_name(self_ty).unwrap_or_else(|| "Service".to_string());
    let method_call_name = format_ident!("__{}MethodCall", type_name);
    let user_methods_name = format_ident!("__{}UserMethods", type_name);
    let reply_params_name = format_ident!("__{}ReplyParams", type_name);
    let reply_error_name = format_ident!("__{}ReplyError", type_name);
    let reply_stream_params_name = format_ident!("__{}ReplyStreamParams", type_name);
    let reply_stream_error_name = format_ident!("__{}ReplyStreamError", type_name);
    let reply_stream_name = format_ident!("__{}ReplyStream", type_name);

    // Collect interfaces for introspection.
    let owned_interface_names = collect_interfaces(methods_info);

    // Generate the MethodCall enum (outer untagged wrapper + inner user methods).
    let method_call_enum = generate_method_call_enum(
        methods_info,
        &method_call_name,
        &user_methods_name,
        crate_path,
    )?;

    // Generate the Reply enum for parameters.
    let reply_params_enum = generate_reply_params_enum(
        methods_info,
        &method_call_name,
        &reply_params_name,
        crate_path,
    )?;

    // Generate the combo ReplyError enum (always include varlink_service::Error).
    let (reply_error_enum, error_type_map) =
        generate_reply_error_enum(methods_info, &reply_error_name, crate_path);

    // Error type is always the reply error enum (includes varlink_service::Error).
    // The lifetime 'ser is used in the Service trait definition.
    let error_type: syn::Type = syn::parse_quote!(#reply_error_name<'ser>);

    let interfaces: Vec<InterfaceName<'_>> = owned_interface_names
        .iter()
        .map(|owned| owned.as_ref())
        .collect();

    // Generate interface description constants.
    let interface_descriptions = generate_interface_descriptions(
        methods_info,
        service_attrs,
        &interfaces,
        crate_path,
        &type_name,
        impl_comments,
    )?;

    // Generate the ReplyStreamParams enum for streaming methods.
    let (reply_stream_params_enum, stream_item_type_map) =
        generate_reply_stream_params_enum(methods_info, &reply_stream_params_name);

    // Generate the ReplyStreamError enum (only when at least one streaming method emits errors).
    let (reply_stream_error_enum, stream_error_type_map) =
        generate_reply_stream_error_enum(methods_info, &reply_stream_error_name);
    let reply_stream_error_ty = if stream_error_type_map.is_empty() {
        quote! { #crate_path::service::Infallible }
    } else {
        quote! { #reply_stream_error_name }
    };

    // Check if any streaming method uses `impl Trait` (requires boxing).
    let needs_stream_boxing = methods_info
        .iter()
        .any(|m| m.is_streaming && m.stream_uses_impl_trait);

    // Generate the `handle` method body.
    let handle_body_ctx = HandleBodyContext {
        crate_path,
        method_call_name: &method_call_name,
        user_methods_name: &user_methods_name,
        reply_params_name: &reply_params_name,
        reply_error_name: &reply_error_name,
        reply_stream_params_name: &reply_stream_params_name,
        reply_stream_error_ty: &reply_stream_error_ty,
        reply_stream_name: &reply_stream_name,
        error_type_map: &error_type_map,
        stream_item_type_map: &stream_item_type_map,
        interfaces: &interfaces,
        type_name: &type_name,
        needs_stream_boxing,
    };
    let handle_body = generate_handle_body(methods_info, service_attrs, &handle_body_ctx)?;

    // Extract socket type parameter: use user-provided first type param, or default to __ZlinkSock.
    let (socket_ty, generics, user_where_clause) = item_impl
        .generics
        .params
        .iter()
        .find_map(|p| match p {
            GenericParam::Type(ty) => Some((
                ty.ident.clone(),
                item_impl.generics.clone(),
                item_impl.generics.where_clause.clone(),
            )),
            _ => None,
        })
        .unwrap_or_else(|| {
            let default_ident: Ident = format_ident!("__ZlinkSock");
            let mut generics = syn::Generics::default();
            generics
                .params
                .push(GenericParam::Type(syn::TypeParam::from(
                    default_ident.clone(),
                )));
            (default_ident, generics, None)
        });

    // Build the where clause: always add Socket bound, then user's additional predicates.
    let user_predicates = user_where_clause.map(|w| w.predicates).unwrap_or_default();
    let where_clause = quote! {
        where
            #socket_ty: #crate_path::connection::Socket,
            #user_predicates
    };

    // Check for streaming methods and determine stream types.
    let has_streaming_methods = methods_info.iter().any(|m| m.is_streaming);

    // Generate the ReplyStream type and enum (if using concrete types).
    let (reply_stream_type, reply_stream_params_type, reply_stream_enum) = if has_streaming_methods
    {
        if needs_stream_boxing {
            // Use boxing when any streaming method uses `impl Trait`.
            (
                quote! {
                    ::std::boxed::Box<
                        dyn #crate_path::futures_util::Stream<
                                Item = #crate_path::service::ReplyStreamItem<
                                    #reply_stream_params_name,
                                    #reply_stream_error_ty,
                                >
                            > + ::core::marker::Unpin
                    >
                },
                quote! { #reply_stream_params_name },
                quote! {},
            )
        } else {
            // Generate an enum stream type when all methods return concrete types.
            let reply_stream_enum = generate_reply_stream_enum(
                methods_info,
                &reply_stream_name,
                &reply_stream_params_name,
                &reply_stream_error_ty,
                &stream_item_type_map,
                crate_path,
            );
            (
                quote! { #reply_stream_name },
                quote! { #reply_stream_params_name },
                reply_stream_enum,
            )
        }
    } else {
        // No streaming methods - use empty stream. The error type is `Infallible` (uninhabited)
        // so the `Err` arm of the stream item is statically unreachable.
        (
            quote! {
                #crate_path::futures_util::stream::Empty<
                    #crate_path::service::ReplyStreamItem<
                        (),
                        #crate_path::service::Infallible,
                    >
                >
            },
            quote! { () },
            quote! {},
        )
    };

    // Generate the FDs parameter for the handle method (std only).
    #[cfg(feature = "std")]
    let handle_fds_param = quote! { __zlink_fds: ::std::vec::Vec<::std::os::fd::OwnedFd>, };
    #[cfg(not(feature = "std"))]
    let handle_fds_param = quote! {};

    // Generate the impl block.
    let service_impl = quote! {
        impl #generics #crate_path::Service<#socket_ty> for #self_ty
        #where_clause
        {
            type MethodCall<'de> = #method_call_name<'de>;
            type ReplyParams<'ser> = #reply_params_name<'ser> where Self: 'ser;
            type ReplyStream = #reply_stream_type;
            type ReplyStreamParams = #reply_stream_params_type;
            type ReplyStreamError = #reply_stream_error_ty;
            type ReplyError<'ser> = #error_type where Self: 'ser;

            async fn handle<'__zlink_ser>(
                &'__zlink_ser mut self,
                __zlink_call: &'__zlink_ser #crate_path::Call<Self::MethodCall<'_>>,
                __zlink_conn: &mut #crate_path::Connection<#socket_ty>,
                #handle_fds_param
            ) -> #crate_path::service::HandleResult<
                Self::ReplyParams<'__zlink_ser>,
                Self::ReplyStream,
                Self::ReplyError<'__zlink_ser>,
            > {
                #handle_body
            }
        }
    };

    Ok(quote! {
        #interface_descriptions

        #method_call_enum

        #reply_params_enum

        #reply_stream_params_enum

        #reply_stream_error_enum

        #reply_stream_enum

        #reply_error_enum

        #service_impl
    })
}

/// Generate the MethodCall enum for deserializing incoming calls.
/// This uses an untagged wrapper to combine varlink service methods with user methods.
fn generate_method_call_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
    user_methods_name: &Ident,
    crate_path: &TokenStream,
) -> Result<TokenStream, Error> {
    let variants: Vec<TokenStream> = methods_info
        .iter()
        .filter_map(|method| {
            let full_path = method.full_method_path()?;
            let variant_name = format_ident!("{}", method.varlink_name);

            // Only include serialized params (exclude connection params).
            let serialized_params: Vec<_> = method.serialized_params().collect();

            let fields = if serialized_params.is_empty() {
                quote! {}
            } else {
                let field_defs: Vec<TokenStream> = serialized_params
                    .iter()
                    .map(|param| {
                        let name = &param.name;
                        let converted_ty = convert_type_lifetimes(&param.ty, "'__de");

                        // Rename whenever the wire name differs from the Rust name, so that
                        // dispatch stays consistent with the introspection data.
                        let wire_name = param.wire_name();
                        let serde_attr = if *name != wire_name {
                            quote! { #[serde(rename = #wire_name)] }
                        } else {
                            quote! {}
                        };

                        quote! {
                            #serde_attr
                            #name: #converted_ty
                        }
                    })
                    .collect();

                quote! {
                    { #(#field_defs),* }
                }
            };

            Some(quote! {
                #[serde(rename = #full_path)]
                #variant_name #fields
            })
        })
        .collect();

    let unused_variant = format_ident!("__{}Unused", user_methods_name);
    let unknown_variant = format_ident!("__{}Unknown", user_methods_name);

    // Generate the inner user methods enum.
    let user_methods_enum = if variants.is_empty() {
        quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Deserialize)]
            #[serde(tag = "method", content = "parameters")]
            pub enum #user_methods_name<'__de> {
                #unused_variant(::core::marker::PhantomData<&'__de ()>),
            }
        }
    } else {
        // Note: #[serde(other)] must be on the last variant.
        quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Deserialize)]
            #[serde(tag = "method", content = "parameters")]
            pub enum #user_methods_name<'__de> {
                #unused_variant(::core::marker::PhantomData<&'__de ()>),
                #(#variants,)*
                #[serde(other)]
                #unknown_variant,
            }
        }
    };

    // Generate the outer untagged wrapper enum.
    // VarlinkService is tried first (specific matches only), then UserMethods.
    let outer_enum = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Deserialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__de> {
            #[serde(borrow)]
            __VarlinkService(#crate_path::varlink_service::Method<'__de>),
            __UserMethods(#user_methods_name<'__de>),
        }
    };

    Ok(quote! {
        #user_methods_enum

        #outer_enum
    })
}

/// Build a mapping from return type string representation to variant index.
/// This ensures consistent variant naming between reply enum generation and handle body.
fn build_return_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, usize> {
    let mut type_to_variant: HashMap<String, usize> = HashMap::new();
    let mut variant_idx = 0;

    for method in methods_info {
        let Some(ref return_type) = method.return_type else {
            continue;
        };

        // Use a simple string representation to check for duplicates.
        let type_str = return_type.to_token_stream().to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            e.insert(variant_idx);
            variant_idx += 1;
        }
    }

    type_to_variant
}

/// Build a mapping from error type string representation to variant index.
/// This ensures consistent variant naming for the combo error enum.
fn build_error_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, usize> {
    let mut type_to_variant: HashMap<String, usize> = HashMap::new();
    let mut variant_idx = 0;

    for method in methods_info {
        let Some(ref error_type) = method.error_type else {
            continue;
        };

        // Use a simple string representation to check for duplicates.
        let type_str = error_type.to_token_stream().to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            e.insert(variant_idx);
            variant_idx += 1;
        }
    }

    type_to_variant
}

/// Build a mapping from stream error type string representation to variant index.
/// This ensures consistent variant naming for the stream error enum.
fn build_stream_error_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, usize> {
    let mut type_to_variant: HashMap<String, usize> = HashMap::new();
    let mut variant_idx = 0;

    for method in methods_info {
        if !method.is_streaming {
            continue;
        }

        let Some(ref err_type) = method.stream_error_type else {
            continue;
        };

        let type_str = err_type.to_token_stream().to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            e.insert(variant_idx);
            variant_idx += 1;
        }
    }

    type_to_variant
}

/// Build a mapping from stream item type string representation to variant name.
/// This ensures consistent variant naming for the stream params enum.
fn build_stream_item_type_variant_map(methods_info: &[MethodInfo]) -> HashMap<String, Ident> {
    let mut type_to_variant: HashMap<String, Ident> = HashMap::new();

    for method in methods_info {
        if !method.is_streaming {
            continue;
        }

        let Some(ref stream_item_type) = method.stream_item_type else {
            continue;
        };

        // Use a simple string representation to check for duplicates.
        let type_str = stream_item_type.to_token_stream().to_string();
        if let std::collections::hash_map::Entry::Vacant(e) = type_to_variant.entry(type_str) {
            // Use the type name as the variant name.
            let variant_name = extract_type_name(stream_item_type)
                .map(|name| format_ident!("{}", name))
                .unwrap_or_else(|| format_ident!("__Unknown"));
            e.insert(variant_name);
        }
    }

    type_to_variant
}

/// Generate the combo ReplyError enum and From impls for each error type.
/// Always includes varlink_service::Error for introspection support.
/// Returns the enum definition, From impls, and the error type map.
fn generate_reply_error_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
    crate_path: &TokenStream,
) -> (TokenStream, HashMap<String, usize>) {
    let error_type_map = build_error_type_variant_map(methods_info);

    // Build variants in order of their indices.
    let mut type_variant_pairs: Vec<_> = error_type_map.iter().collect();
    type_variant_pairs.sort_by_key(|(_, idx)| *idx);

    let mut variants: Vec<TokenStream> = Vec::new();
    let mut from_impls: Vec<TokenStream> = Vec::new();

    for (type_str, idx) in type_variant_pairs {
        // Find the actual error type from methods.
        for method in methods_info {
            let Some(ref error_type) = method.error_type else {
                continue;
            };
            if &error_type.to_token_stream().to_string() == type_str {
                let variant_name = format_ident!("__{}Variant{}", enum_name, idx);
                let converted = convert_type_lifetimes(error_type, "'__ser");
                variants.push(quote! {
                    #variant_name(#converted)
                });

                from_impls.push(quote! {
                    impl<'__ser> ::core::convert::From<#converted> for #enum_name<'__ser> {
                        fn from(e: #converted) -> Self {
                            #enum_name::#variant_name(e)
                        }
                    }
                });
                break;
            }
        }
    }

    // Always add varlink_service::Error variant for introspection.
    let varlink_error_variant = format_ident!("__{}VarlinkService", enum_name);

    let enum_def = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__ser> {
            #varlink_error_variant(#crate_path::varlink_service::Error<'__ser>),
            #(#variants,)*
        }

        impl<'__ser> ::core::convert::From<#crate_path::varlink_service::Error<'__ser>>
            for #enum_name<'__ser>
        {
            fn from(e: #crate_path::varlink_service::Error<'__ser>) -> Self {
                #enum_name::#varlink_error_variant(e)
            }
        }

        #(#from_impls)*
    };

    (enum_def, error_type_map)
}

/// Generate the ReplyStreamError enum for errors emitted as streaming method items.
///
/// Returns the (optional) enum definition and the type map. When no streaming method emits
/// errors, the enum is omitted and `Infallible` is used as the associated type.
fn generate_reply_stream_error_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
) -> (TokenStream, HashMap<String, usize>) {
    let type_map = build_stream_error_type_variant_map(methods_info);

    if type_map.is_empty() {
        return (quote! {}, type_map);
    }

    let mut type_variant_pairs: Vec<_> = type_map.iter().collect();
    type_variant_pairs.sort_by_key(|(_, idx)| *idx);

    let mut variants: Vec<TokenStream> = Vec::new();
    let mut from_impls: Vec<TokenStream> = Vec::new();

    for (type_str, idx) in type_variant_pairs {
        for method in methods_info {
            if !method.is_streaming {
                continue;
            }
            let Some(ref err_type) = method.stream_error_type else {
                continue;
            };
            if &err_type.to_token_stream().to_string() == type_str {
                let variant_name = format_ident!("__{}Variant{}", enum_name, idx);
                variants.push(quote! {
                    #variant_name(#err_type)
                });

                from_impls.push(quote! {
                    impl ::core::convert::From<#err_type> for #enum_name {
                        fn from(e: #err_type) -> Self {
                            #enum_name::#variant_name(e)
                        }
                    }
                });
                break;
            }
        }
    }

    let enum_def = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name {
            #(#variants,)*
        }

        #(#from_impls)*
    };

    (enum_def, type_map)
}

/// Generate the ReplyStreamParams enum for streaming method replies.
/// Returns the enum definition, From impls, and the type map.
fn generate_reply_stream_params_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
) -> (TokenStream, HashMap<String, Ident>) {
    let type_map = build_stream_item_type_variant_map(methods_info);

    let mut variants: Vec<TokenStream> = Vec::new();
    let mut from_impls: Vec<TokenStream> = Vec::new();

    for (type_str, variant_name) in &type_map {
        // Find the actual stream item type from methods.
        for method in methods_info {
            if !method.is_streaming {
                continue;
            }
            let Some(ref item_type) = method.stream_item_type else {
                continue;
            };
            if &item_type.to_token_stream().to_string() == type_str {
                variants.push(quote! {
                    #variant_name(#item_type)
                });

                from_impls.push(quote! {
                    impl ::core::convert::From<#item_type> for #enum_name {
                        fn from(v: #item_type) -> Self {
                            #enum_name::#variant_name(v)
                        }
                    }
                });
                break;
            }
        }
    }

    // If no streaming methods, generate an empty enum with a unit variant.
    if variants.is_empty() {
        let enum_def = quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Serialize)]
            #[serde(untagged)]
            pub enum #enum_name {
                __Unused(()),
            }
        };
        return (enum_def, type_map);
    }

    let enum_def = quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name {
            #(#variants,)*
        }

        #(#from_impls)*
    };

    (enum_def, type_map)
}

/// Generate the ReplyStream enum for concrete stream types (using pin-project-lite).
/// Build the body that maps `__r` (the user stream's item) into the trait's required
/// `Result<Reply<ReplyStreamParams>, ReplyStreamError>` shape.
///
/// Used by both the boxed (`impl Trait`) and concrete-type stream wrappers.
fn build_stream_map_body(
    has_err: bool,
    params_variant: &Ident,
    reply_stream_params_name: &Ident,
    reply_stream_error_ty: &TokenStream,
    crate_path: &TokenStream,
) -> TokenStream {
    let result_ty = quote! {
        ::core::result::Result::<
            #crate_path::Reply<#reply_stream_params_name>,
            #reply_stream_error_ty,
        >
    };
    let wrap_ok = quote! {
        #result_ty::Ok(__rep.map(|__p| #reply_stream_params_name::#params_variant(__p)))
    };
    if has_err {
        quote! {
            match __r {
                ::core::result::Result::Ok(__rep) => { #wrap_ok }
                ::core::result::Result::Err(__err) => {
                    #result_ty::Err(::core::convert::Into::into(__err))
                }
            }
        }
    } else {
        // Alias `__rep` so the `wrap_ok` template above is identical in both arms.
        quote! { { let __rep = __r; #wrap_ok } }
    }
}

/// Build the closure that adapts a stream item from the user's stream into the trait's stream
/// item shape (`ReplyStreamItem<P, E>`).
///
/// On `std`:
///   * `return_fds` → `|(__r, __fds)| (#map_body, __fds)`
///   * otherwise    → `|__r| (#map_body, Vec::new())`
///
/// On `no_std`: `|__r| #map_body` (no FDs).
fn build_stream_item_map_closure(map_body: &TokenStream, return_fds: bool) -> TokenStream {
    #[cfg(feature = "std")]
    {
        let fds_expr = if return_fds {
            quote! { __fds }
        } else {
            quote! { ::std::vec::Vec::new() }
        };
        let arg_pat = if return_fds {
            quote! { (__r, __fds) }
        } else {
            quote! { __r }
        };
        quote! {
            |#arg_pat| {
                let __mapped = #map_body;
                (__mapped, #fds_expr)
            }
        }
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = return_fds;
        quote! { |__r| { #map_body } }
    }
}

/// This avoids boxing when all streaming methods return concrete types.
fn generate_reply_stream_enum(
    methods_info: &[MethodInfo],
    enum_name: &Ident,
    reply_stream_params_name: &Ident,
    reply_stream_error_ty: &TokenStream,
    stream_item_type_map: &HashMap<String, Ident>,
    crate_path: &TokenStream,
) -> TokenStream {
    let projection_name = format_ident!("{}Proj", enum_name);

    // Collect streaming methods with their return types and variant names.
    let streaming_methods: Vec<_> = methods_info
        .iter()
        .filter(|m| m.is_streaming && m.stream_return_type.is_some())
        .collect();

    if streaming_methods.is_empty() {
        return quote! {};
    }

    // Generate enum variants (one per streaming method, using method's varlink name).
    let variants: Vec<TokenStream> = streaming_methods
        .iter()
        .map(|method| {
            let variant_name = format_ident!("{}", method.varlink_name);
            let stream_type = method.stream_return_type.as_ref().unwrap();
            quote! {
                #variant_name { #[pin] stream: #stream_type }
            }
        })
        .collect();

    // Generate poll_next match arms. The user method's stream might yield:
    //   * `Reply<T>`                                       — wrap in `Ok(...)`
    //   * `Result<Reply<T>, E>`                             — convert error via `Into`
    //   * `(Reply<T>, Vec<OwnedFd>)`        (return_fds)    — wrap in `Ok(...)` (with FDs)
    //   * `(Result<Reply<T>, E>, Vec<OwnedFd>)` (return_fds + error)
    //
    // The trait's stream item shape is built by `build_stream_map_body` /
    // `build_stream_item_map_closure`.
    let poll_arms: Vec<TokenStream> = streaming_methods
        .iter()
        .map(|method| {
            let variant_name = format_ident!("{}", method.varlink_name);
            let item_type = method.stream_item_type.as_ref().unwrap();
            let type_str = item_type.to_token_stream().to_string();
            let params_variant = stream_item_type_map
                .get(&type_str)
                .cloned()
                .unwrap_or_else(|| format_ident!("__Unknown"));
            let map_body = build_stream_map_body(
                method.stream_error_type.is_some(),
                &params_variant,
                reply_stream_params_name,
                reply_stream_error_ty,
                crate_path,
            );
            let item_map = build_stream_item_map_closure(&map_body, method.return_fds);
            quote! {
                #projection_name::#variant_name { stream } => {
                    stream.poll_next(cx).map(|__opt| __opt.map(#item_map))
                }
            }
        })
        .collect();

    quote! {
        #crate_path::pin_project_lite::pin_project! {
            #[allow(private_interfaces)]
            #[project = #projection_name]
            pub enum #enum_name {
                #(#variants,)*
            }
        }

        impl #crate_path::futures_util::Stream for #enum_name {
            type Item = #crate_path::service::ReplyStreamItem<
                #reply_stream_params_name,
                #reply_stream_error_ty,
            >;

            fn poll_next(
                self: ::core::pin::Pin<&mut Self>,
                cx: &mut ::core::task::Context<'_>,
            ) -> ::core::task::Poll<::core::option::Option<Self::Item>> {
                match self.project() {
                    #(#poll_arms)*
                }
            }
        }
    }
}

/// Generate the ReplyParams enum for serializing outgoing replies.
fn generate_reply_params_enum(
    methods_info: &[MethodInfo],
    method_call_name: &Ident,
    enum_name: &Ident,
    crate_path: &TokenStream,
) -> Result<TokenStream, Error> {
    // Collect unique return types from methods.
    let mut variants: Vec<TokenStream> = Vec::new();
    let type_to_variant = build_return_type_variant_map(methods_info);

    // Build variants in order of their indices.
    let mut type_variant_pairs: Vec<_> = type_to_variant.iter().collect();
    type_variant_pairs.sort_by_key(|(_, idx)| *idx);

    for (type_str, idx) in type_variant_pairs {
        // Find the actual return type from methods.
        for method in methods_info {
            let Some(ref return_type) = method.return_type else {
                continue;
            };
            if &return_type.to_token_stream().to_string() == type_str {
                let variant_name = format_ident!("__{}Variant{}", method_call_name, idx);
                let converted = convert_type_lifetimes(return_type, "'__ser");
                variants.push(quote! {
                    #variant_name(#converted)
                });
                break;
            }
        }
    }

    let unused_variant = format_ident!("__{}Unused", enum_name);

    // Always add varlink service reply variant for introspection.
    let varlink_reply_variant = format_ident!("__{}VarlinkService", enum_name);

    if variants.is_empty() {
        return Ok(quote! {
            #[allow(private_interfaces)]
            #[derive(::core::fmt::Debug, ::serde::Serialize)]
            #[serde(untagged)]
            pub enum #enum_name<'__ser> {
                #varlink_reply_variant(#crate_path::varlink_service::Reply<'__ser>),
                #unused_variant(::core::marker::PhantomData<&'__ser ()>),
            }
        });
    }

    Ok(quote! {
        #[allow(private_interfaces)]
        #[derive(::core::fmt::Debug, ::serde::Serialize)]
        #[serde(untagged)]
        pub enum #enum_name<'__ser> {
            #varlink_reply_variant(#crate_path::varlink_service::Reply<'__ser>),
            #(#variants,)*
            #unused_variant(::core::marker::PhantomData<&'__ser ()>),
        }
    })
}

/// Generate interface description constants for each interface.
fn generate_interface_descriptions<'name>(
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
    interfaces: &[InterfaceName<'name>],
    crate_path: &TokenStream,
    type_name: &str,
    impl_comments: &[String],
) -> Result<TokenStream, Error> {
    let mut descriptions: Vec<TokenStream> = Vec::new();

    for interface in interfaces {
        let const_name = interface_name_to_ident(type_name, interface);

        // Collect methods for this interface.
        let interface_methods: Vec<&MethodInfo> = methods_info
            .iter()
            .filter(|m| m.interface.as_ref().map(|i| i.inner()) == Some(interface))
            .collect();

        // Collect custom types scoped to this interface from method-level `types = [...]`,
        // deduplicating by type path string representation.
        let mut seen_custom_types = HashSet::new();
        let per_interface_types: Vec<&Type> = methods_info
            .iter()
            .filter(|m| m.interface.as_ref().map(|i| i.inner()) == Some(interface))
            .flat_map(|m| m.custom_types.iter())
            .filter(|ty| {
                let type_str = ty.to_token_stream().to_string();
                seen_custom_types.insert(type_str)
            })
            .collect();

        // Use per-interface types if any were specified; otherwise fall back to the global list.
        // This maintains backward compatibility for single-interface services that use the
        // top-level `types = [...]` attribute.
        let types_source: Vec<&Type> = if !per_interface_types.is_empty() {
            per_interface_types
        } else if interfaces.len() == 1 {
            service_attrs.custom_types.iter().collect()
        } else {
            Vec::new()
        };

        // Pre-compute the declared-names token slice used in const assertions for both
        // input parameters (outer block) and output parameters (inner per-method block).
        let declared_names: Vec<TokenStream> = types_source
            .iter()
            .map(|ty| {
                let ty = convert_type_lifetimes(ty, "'static");
                quote! {
                    #crate_path::introspect::assert::custom_type_name(
                        <#ty as #crate_path::introspect::Type>::TYPE,
                    ).expect("types = [...] entry is not a custom type")
                }
            })
            .collect();

        // Generate inner const method definitions.
        // Each method needs its own const to avoid destructor issues.
        let method_consts: Vec<TokenStream> = interface_methods
            .iter()
            .enumerate()
            .map(|(idx, method)| {
                let method_const_name = format_ident!("__METHOD_{}", idx);
                let method_name = &method.varlink_name;

                // Input parameters (excluding connection params).
                let in_params: Vec<TokenStream> = method
                    .serialized_params()
                    .map(|p| {
                        let param_name = p.wire_name();
                        let ty = convert_type_lifetimes(&p.ty, "'static");
                        quote! {
                            &#crate_path::idl::Parameter::new(
                                #param_name,
                                <#ty as #crate_path::introspect::Type>::TYPE,
                                &[],
                            )
                        }
                    })
                    .collect();

                // Generate a const for this method's in params slice.
                let in_params_const = if in_params.is_empty() {
                    quote! {
                        const __IN_PARAMS: &[&#crate_path::idl::Parameter<'static>] = &[];
                    }
                } else {
                    quote! {
                        const __IN_PARAMS: &[&#crate_path::idl::Parameter<'static>] =
                            &[#(#in_params),*];
                    }
                };

                // Collect all custom types visible to this method: service-level types
                // plus any per-interface types declared on the method itself.
                let all_custom_types: Vec<&Type> = service_attrs
                    .custom_types
                    .iter()
                    .chain(method.custom_types.iter())
                    .collect();

                // TODO: Varlink methods natively express flat named outputs (`-> (field: type, …)`),
                // but Rust lacks anonymous named structs. Today, users must wrap their output in a
                // named struct (e.g. `struct Reply { books: Vec<Book> }`), which the macro then
                // "unwraps" by emitting the struct fields as the method's out-params. This causes
                // wrapper structs to appear redundantly as top-level type definitions in the IDL.
                // A future enhancement could let users write `-> (field: type)` directly in the
                // method attribute to sidestep the wrapper-struct requirement.
                let out_params_const = if method.is_streaming {
                    method.stream_item_type.clone()
                } else {
                    method.return_type.clone()
                }
                .map(|ty| {
                    if all_custom_types.contains(&&ty) {
                        // Declared custom type: must be an object (struct) so its fields
                        // can be emitted as named out-params. Custom enums are not valid
                        // Varlink return types — the spec requires `-> (field: type, …)`.
                        quote! {
                            const __OUT_PARAMS: &[&#crate_path::idl::Parameter<'static>] =
                                <#ty as #crate_path::introspect::CustomType>::CUSTOM_TYPE
                                    .as_object()
                                    .expect("Varlink method return type must be a struct (object); \
                                             enums and other types are not valid Varlink return types")
                                    .fields_as_slice()
                                    .unwrap();
                        }
                    } else {
                        // Inline/anonymous object type via introspect::Type.
                        quote! {
                            const __OUT_PARAMS: &[&#crate_path::idl::Parameter<'static>] =
                                <#ty as #crate_path::introspect::Type>::TYPE
                                    .as_object()
                                    .expect("Varlink method return type must be a struct (object); \
                                             arrays, primitives and other types cannot be used \
                                             as top-level return types — wrap them in a struct, \
                                             e.g. `struct Reply { items: Vec<T> }`")
                                    .as_borrowed()
                                    .unwrap();
                        }
                    }
                })
                .unwrap_or_else(|| {
                    quote! {
                        const __OUT_PARAMS: &[&#crate_path::idl::Parameter<'static>] = &[];
                    }
                });

                let comment_objects = crate::introspect::shared::generate_comment_objects(
                    &method.comments,
                    crate_path,
                );

                Ok(quote! {
                    const #method_const_name: &#crate_path::idl::Method<'static> = &{
                        #in_params_const
                        #out_params_const
                        // Verify all custom types in method output parameters are declared.
                        const _: () = {
                            const __DECLARED: &[&str] = &[#(#declared_names),*];
                            #crate_path::introspect::assert::assert_params_declared(
                                __OUT_PARAMS,
                                __DECLARED,
                            );
                        };
                        #crate_path::idl::Method::new(
                            #method_name,
                            __IN_PARAMS,
                            __OUT_PARAMS,
                            &[#(#comment_objects),*],
                        )
                    };
                })
            })
            .collect::<Result<_, Error>>()?;

        // Generate the list of method references.
        let method_refs: Vec<TokenStream> = (0..interface_methods.len())
            .map(|idx| {
                let method_const_name = format_ident!("__METHOD_{}", idx);
                quote! { #method_const_name }
            })
            .collect();

        let custom_types: Vec<TokenStream> = types_source
            .iter()
            .map(|ty| {
                quote! {
                    <#ty as #crate_path::introspect::CustomType>::CUSTOM_TYPE
                }
            })
            .collect();

        // Collect error types for this interface, including those raised by streaming method
        // items. Deduplicate using string representation.
        let mut seen_error_types = HashSet::new();
        let error_types: Vec<TokenStream> = methods_info
            .iter()
            .filter(|m| m.interface.as_ref().map(|i| i.inner()) == Some(interface))
            .flat_map(|m| {
                m.error_type
                    .as_ref()
                    .into_iter()
                    .chain(m.stream_error_type.as_ref())
            })
            .filter(|err_ty| {
                let type_str = err_ty.to_token_stream().to_string();
                seen_error_types.insert(type_str)
            })
            .map(|err_ty| {
                let err_ty = convert_type_lifetimes(err_ty, "'static");
                quote! {
                    <#err_ty as #crate_path::introspect::ReplyError>::VARIANTS
                }
            })
            .collect();

        // Flatten error variants into a single slice.
        let error_variants_expr = if error_types.is_empty() {
            quote! { &[] }
        } else {
            // For simplicity, we collect the first error type's variants.
            // In practice, a service usually has one error type per interface.
            let first_err = &error_types[0];
            quote! { #first_err }
        };

        // Generate compile-time assertions that custom types referenced in method input
        // parameters are declared in `types = [...]`.  Output parameters are asserted
        // inside each __METHOD_N block (where __OUT_PARAMS is in scope).
        let type_assertions: Vec<TokenStream> = interface_methods
            .iter()
            .flat_map(|method| {
                method
                    .serialized_params()
                    .map(|p| {
                        let ty = convert_type_lifetimes(&p.ty, "'static");
                        quote! {
                            #crate_path::introspect::assert::assert_type_declared(
                                <#ty as #crate_path::introspect::Type>::TYPE,
                                __DECLARED,
                            );
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let interface_comment_objects =
            crate::introspect::shared::generate_comment_objects(impl_comments, crate_path);

        let interface_str = interface.as_str();
        descriptions.push(quote! {
            #[doc(hidden)]
            const #const_name: &#crate_path::idl::Interface<'static> = &{
                #(#method_consts)*
                #crate_path::idl::Interface::new(
                    #crate_path::names::InterfaceName::from_static_str_unchecked(#interface_str),
                    &[#(#method_refs),*],
                    &[#(#custom_types),*],
                    #error_variants_expr,
                    &[#(#interface_comment_objects),*],
                )
            };

            // Verify all custom types in method parameters are declared.
            const _: () = {
                const __DECLARED: &[&str] = &[#(#declared_names),*];
                #(#type_assertions)*
            };
        });
    }

    Ok(quote! { #(#descriptions)* })
}

/// Wrap a `MethodReply` expression into a `HandleResult` with no file descriptors.
///
/// On std: `(method_reply, Vec::new())`
/// On no_std: `method_reply`
fn wrap_handle_result_no_fds(inner: TokenStream) -> TokenStream {
    #[cfg(feature = "std")]
    {
        quote! {
            {
                let __method_reply = { #inner };
                (__method_reply, ::std::vec::Vec::new())
            }
        }
    }
    #[cfg(not(feature = "std"))]
    {
        inner
    }
}

/// Wrap a `MethodReply` expression into a `HandleResult` with the given file descriptors.
///
/// On std: `(method_reply, fds_expr)`
/// On no_std: `method_reply`
#[cfg(feature = "std")]
fn wrap_handle_result_with_fds(inner: TokenStream, fds_expr: TokenStream) -> TokenStream {
    quote! {
        {
            let __method_reply = { #inner };
            (__method_reply, #fds_expr)
        }
    }
}

/// On no_std, FDs are ignored.
#[cfg(not(feature = "std"))]
fn wrap_handle_result_with_fds(inner: TokenStream, _fds_expr: TokenStream) -> TokenStream {
    inner
}

/// Generate the `handle` method body with match arms.
fn generate_handle_body(
    methods_info: &[MethodInfo],
    service_attrs: &ServiceAttrs,
    ctx: &HandleBodyContext<'_>,
) -> Result<TokenStream, Error> {
    let HandleBodyContext {
        crate_path,
        method_call_name,
        user_methods_name,
        reply_params_name,
        reply_error_name,
        reply_stream_params_name,
        reply_stream_error_ty,
        reply_stream_name,
        error_type_map,
        stream_item_type_map,
        interfaces,
        type_name,
        needs_stream_boxing,
    } = ctx;

    let mut user_match_arms: Vec<TokenStream> = Vec::new();
    let type_to_variant = build_return_type_variant_map(methods_info);

    for method in methods_info {
        let Some(_full_path) = method.full_method_path() else {
            continue;
        };

        let enum_variant_name = format_ident!("{}", method.varlink_name);
        let method_name = &method.name;

        // Only include serialized params in the pattern (exclude connection params).
        let serialized_params: Vec<_> = method.serialized_params().collect();

        // Build the pattern for the match arm.
        let pattern = if serialized_params.is_empty() {
            quote! { #user_methods_name::#enum_variant_name }
        } else {
            let param_names: Vec<_> = serialized_params.iter().map(|p| &p.name).collect();
            quote! { #user_methods_name::#enum_variant_name { #(#param_names),* } }
        };

        // Build the method call expression.
        // For methods with connection params, inline the body (the method isn't in the output
        // impl). For other methods, call the method normally.
        let method_call = if method.has_connection_param() {
            // Inline the method body with variable bindings.
            let body = &method.body;

            // Set up bindings for connection params.
            let conn_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| p.is_connection)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = __zlink_conn; }
                })
                .collect();

            // Set up bindings for the `more` param (from call request).
            let more_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| p.is_more)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = __zlink_call.more(); }
                })
                .collect();

            // Set up bindings for FD params.
            let fds_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| p.is_fds)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = __zlink_fds; }
                })
                .collect();

            // Set up bindings for regular params (clone from pattern match).
            let param_bindings: Vec<TokenStream> = method
                .params
                .iter()
                .filter(|p| !p.is_connection && !p.is_more && !p.is_fds)
                .map(|p| {
                    let name = &p.name;
                    quote! { let #name = ::core::clone::Clone::clone(#name); }
                })
                .collect();

            quote! {
                {
                    #(#conn_bindings)*
                    #(#more_bindings)*
                    #(#fds_bindings)*
                    #(#param_bindings)*
                    async move #body
                }.await
            }
        } else {
            // Build the method call arguments.
            let call_args: Vec<TokenStream> = method
                .params
                .iter()
                .map(|p| {
                    if p.is_more {
                        // `more` param comes from the call request.
                        quote! { __zlink_call.more() }
                    } else if p.is_fds {
                        // FD param gets the incoming FDs.
                        quote! { __zlink_fds }
                    } else {
                        // Clone from the pattern match.
                        let name = &p.name;
                        quote! { ::core::clone::Clone::clone(#name) }
                    }
                })
                .collect();

            quote! { self.#method_name(#(#call_args),*).await }
        };

        // Build the return expression based on method type.
        // Each branch must produce a `HandleResult`, not a raw `MethodReply`.
        let return_expr = if method.is_streaming {
            // Streaming method - wrap the stream to convert items to the enum type.
            // The stream produces Reply<T> items, we map to Reply<ReplyStreamParams>.
            let item_type = method
                .stream_item_type
                .as_ref()
                .expect("streaming method must have stream_item_type");

            // Get the variant name for this stream item type.
            let type_str = item_type.to_token_stream().to_string();
            let stream_variant_name = stream_item_type_map
                .get(&type_str)
                .cloned()
                .unwrap_or_else(|| format_ident!("__Unknown"));

            // Use the method's varlink name for the enum variant.
            let method_variant_name = format_ident!("{}", method.varlink_name);

            let streaming_reply = if *needs_stream_boxing {
                // Use boxing when any streaming method uses `impl Trait`.
                let map_body = build_stream_map_body(
                    method.stream_error_type.is_some(),
                    &stream_variant_name,
                    reply_stream_params_name,
                    reply_stream_error_ty,
                    crate_path,
                );
                let item_map = build_stream_item_map_closure(&map_body, method.return_fds);

                quote! {
                    let __stream = #method_call;
                    let __mapped =
                        #crate_path::futures_util::StreamExt::map(__stream, #item_map);
                    let __boxed: ::std::boxed::Box<
                        dyn #crate_path::futures_util::Stream<
                                Item = #crate_path::service::ReplyStreamItem<
                                    #reply_stream_params_name,
                                    #reply_stream_error_ty,
                                >
                            > + ::core::marker::Unpin
                    > = ::std::boxed::Box::new(__mapped);
                    #crate_path::service::MethodReply::Multi(__boxed)
                }
            } else {
                // Use enum variant when all streaming methods return concrete types.
                quote! {
                    let __stream = #method_call;
                    #crate_path::service::MethodReply::Multi(
                        #reply_stream_name::#method_variant_name { stream: __stream }
                    )
                }
            };
            wrap_handle_result_no_fds(streaming_reply)
        } else if method.return_fds && method.returns_result {
            // return_fds + (Result<T, E>, Vec<OwnedFd>).
            // FDs are available on both Ok and Err arms.
            let error_variant = method.error_type.as_ref().map(|err_ty| {
                let type_str = err_ty.to_token_stream().to_string();
                let variant_idx = error_type_map.get(&type_str).copied().unwrap_or(0);
                format_ident!("__{}Variant{}", reply_error_name, variant_idx)
            });
            let error_convert = if let Some(ref err_variant) = error_variant {
                quote! { #reply_error_name::#err_variant(__err) }
            } else {
                quote! { ::core::convert::From::from(__err) }
            };
            let err_reply = quote! {
                #crate_path::service::MethodReply::Error(#error_convert)
            };
            let err_arm = wrap_handle_result_with_fds(err_reply, quote! { __out_fds });

            if let Some(ref return_type) = method.return_type {
                let type_str = return_type.to_token_stream().to_string();
                let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
                let reply_variant_name =
                    format_ident!("__{}Variant{}", method_call_name, variant_idx);
                let ok_reply = quote! {
                    #crate_path::service::MethodReply::Single(Some(
                        #reply_params_name::#reply_variant_name(__ok)
                    ))
                };
                let ok_arm = wrap_handle_result_with_fds(ok_reply, quote! { __out_fds });
                quote! {
                    let (__result, __out_fds) = #method_call;
                    match __result {
                        ::core::result::Result::Ok(__ok) => {
                            #ok_arm
                        }
                        ::core::result::Result::Err(__err) => {
                            #err_arm
                        }
                    }
                }
            } else {
                // (Result<(), E>, Vec<OwnedFd>).
                let ok_reply = quote! {
                    #crate_path::service::MethodReply::Single(None)
                };
                let ok_arm = wrap_handle_result_with_fds(ok_reply, quote! { __out_fds });
                quote! {
                    let (__result, __out_fds) = #method_call;
                    match __result {
                        ::core::result::Result::Ok(()) => {
                            #ok_arm
                        }
                        ::core::result::Result::Err(__err) => {
                            #err_arm
                        }
                    }
                }
            }
        } else if method.return_fds {
            // return_fds without Result: (T, Vec<OwnedFd>).
            if let Some(ref return_type) = method.return_type {
                let type_str = return_type.to_token_stream().to_string();
                let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
                let reply_variant_name =
                    format_ident!("__{}Variant{}", method_call_name, variant_idx);
                let ok_reply = quote! {
                    #crate_path::service::MethodReply::Single(Some(
                        #reply_params_name::#reply_variant_name(__ok)
                    ))
                };
                let ok_arm = wrap_handle_result_with_fds(ok_reply, quote! { __out_fds });
                quote! {
                    let (__ok, __out_fds) = #method_call;
                    #ok_arm
                }
            } else {
                // ((), Vec<OwnedFd>).
                let ok_reply = quote! {
                    #crate_path::service::MethodReply::Single(None)
                };
                let ok_arm = wrap_handle_result_with_fds(ok_reply, quote! { __out_fds });
                quote! {
                    let ((), __out_fds) = #method_call;
                    #ok_arm
                }
            }
        } else if method.returns_result {
            // Method returns Result<T, E>. Get the error variant for this method's error type.
            let error_variant = method.error_type.as_ref().map(|err_ty| {
                let type_str = err_ty.to_token_stream().to_string();
                let variant_idx = error_type_map.get(&type_str).copied().unwrap_or(0);
                format_ident!("__{}Variant{}", reply_error_name, variant_idx)
            });

            if let Some(ref return_type) = method.return_type {
                // Result<T, E> where T is not ().
                let type_str = return_type.to_token_stream().to_string();
                let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
                let reply_variant_name =
                    format_ident!("__{}Variant{}", method_call_name, variant_idx);
                let error_convert = if let Some(err_variant) = error_variant {
                    quote! { #reply_error_name::#err_variant(__err) }
                } else {
                    quote! { ::core::convert::From::from(__err) }
                };
                wrap_handle_result_no_fds(quote! {
                    match #method_call {
                        ::core::result::Result::Ok(__ok) => {
                            #crate_path::service::MethodReply::Single(Some(
                                #reply_params_name::#reply_variant_name(__ok)
                            ))
                        }
                        ::core::result::Result::Err(__err) => {
                            #crate_path::service::MethodReply::Error(#error_convert)
                        }
                    }
                })
            } else {
                // Result<(), E>.
                let error_convert = if let Some(err_variant) = error_variant {
                    quote! { #reply_error_name::#err_variant(__err) }
                } else {
                    quote! { ::core::convert::From::from(__err) }
                };
                wrap_handle_result_no_fds(quote! {
                    match #method_call {
                        ::core::result::Result::Ok(()) => {
                            #crate_path::service::MethodReply::Single(None)
                        }
                        ::core::result::Result::Err(__err) => {
                            #crate_path::service::MethodReply::Error(#error_convert)
                        }
                    }
                })
            }
        } else if let Some(ref return_type) = method.return_type {
            // Method returns T directly (not a Result).
            let type_str = return_type.to_token_stream().to_string();
            let variant_idx = type_to_variant.get(&type_str).copied().unwrap_or(0);
            let reply_variant_name = format_ident!("__{}Variant{}", method_call_name, variant_idx);
            wrap_handle_result_no_fds(quote! {
                let __result = #method_call;
                #crate_path::service::MethodReply::Single(Some(
                    #reply_params_name::#reply_variant_name(__result)
                ))
            })
        } else {
            // Method has no return type.
            wrap_handle_result_no_fds(quote! {
                let _ = #method_call;
                #crate_path::service::MethodReply::Single(None)
            })
        };

        user_match_arms.push(quote! {
            #pattern => {
                #return_expr
            }
        });
    }

    let unused_variant = format_ident!("__{}Unused", user_methods_name);
    let unknown_variant = format_ident!("__{}Unknown", user_methods_name);
    let varlink_error_variant = format_ident!("__{}VarlinkService", reply_error_name);
    let varlink_reply_variant = format_ident!("__{}VarlinkService", reply_params_name);

    // Add the unused variant arm first (to match enum order).
    user_match_arms.insert(
        0,
        quote! {
            #user_methods_name::#unused_variant(_) => {
                unreachable!("unused variant should never be matched")
            }
        },
    );

    // Add a catch-all arm for unknown methods (returns MethodNotFound error).
    let unknown_method_reply = wrap_handle_result_no_fds(quote! {
        #crate_path::service::MethodReply::Error(
            #reply_error_name::#varlink_error_variant(
                #crate_path::varlink_service::Error::MethodNotFound {
                    method: ::std::borrow::Cow::Borrowed("unknown"),
                }
            )
        )
    });
    user_match_arms.push(quote! {
        #user_methods_name::#unknown_variant => {
            #unknown_method_reply
        }
    });

    // Generate the user methods match.
    let user_methods_match = quote! {
        #method_call_name::__UserMethods(__user_method) => {
            match __user_method {
                #(#user_match_arms)*
            }
        }
    };

    // Generate interface description match arms for GetInterfaceDescription.
    let interface_match_arms: Vec<TokenStream> = interfaces
        .iter()
        .map(|interface| {
            let const_name = interface_name_to_ident(type_name, interface);
            let desc_reply = wrap_handle_result_no_fds(quote! {
                #crate_path::service::MethodReply::Single(Some(
                    #reply_params_name::#varlink_reply_variant(
                        #crate_path::varlink_service::Reply::InterfaceDescription(desc)
                    )
                ))
            });
            let interface_str = interface.as_str();
            quote! {
                #interface_str => {
                    let desc =
                        #crate_path::varlink_service::InterfaceDescription::from(#const_name);
                    #desc_reply
                }
            }
        })
        .collect();

    // Build the interfaces list for GetInfo.
    let interfaces_list: Vec<TokenStream> = interfaces
        .iter()
        .map(|iface| {
            let iface_str = iface.as_str();
            quote! { #iface_str }
        })
        .collect();

    // Service metadata.
    let vendor = service_attrs
        .vendor
        .as_ref()
        .map(|v| quote! { #v })
        .unwrap_or_else(|| quote! { "" });
    let product = service_attrs
        .product
        .as_ref()
        .map(|v| quote! { #v })
        .unwrap_or_else(|| quote! { "" });
    let version = service_attrs
        .version
        .as_ref()
        .map(|v| quote! { #v })
        .unwrap_or_else(|| quote! { "" });
    let url = service_attrs
        .url
        .as_ref()
        .map(|v| quote! { #v })
        .unwrap_or_else(|| quote! { "" });

    // Generate the varlink service methods match.
    let get_info_reply = wrap_handle_result_no_fds(quote! {
        #crate_path::service::MethodReply::Single(Some(
            #reply_params_name::#varlink_reply_variant(
                #crate_path::varlink_service::Reply::Info(info)
            )
        ))
    });
    let varlink_desc_reply = wrap_handle_result_no_fds(quote! {
        #crate_path::service::MethodReply::Single(Some(
            #reply_params_name::#varlink_reply_variant(
                #crate_path::varlink_service::Reply::InterfaceDescription(desc)
            )
        ))
    });
    let interface_not_found_reply = wrap_handle_result_no_fds(quote! {
        #crate_path::service::MethodReply::Error(
            #reply_error_name::#varlink_error_variant(
                #crate_path::varlink_service::Error::InterfaceNotFound {
                    interface: ::std::borrow::Cow::Borrowed(interface),
                }
            )
        )
    });
    let varlink_service_match = quote! {
        #method_call_name::__VarlinkService(__varlink_method) => {
            match __varlink_method {
                #crate_path::varlink_service::Method::GetInfo => {
                    let info = #crate_path::varlink_service::Info::from_static_str_unchecked(
                        #vendor,
                        #product,
                        #version,
                        #url,
                        ::std::vec![
                            #(#interfaces_list,)*
                            #crate_path::varlink_service::INTERFACE_NAME,
                        ],
                    );
                    #get_info_reply
                }
                #crate_path::varlink_service::Method::GetInterfaceDescription { interface } => {
                    match interface.as_str() {
                        #(#interface_match_arms)*
                        #crate_path::varlink_service::INTERFACE_NAME => {
                            let desc =
                                #crate_path::varlink_service::InterfaceDescription::from(
                                    #crate_path::varlink_service::DESCRIPTION
                                );
                            #varlink_desc_reply
                        }
                        _ => {
                            #interface_not_found_reply
                        }
                    }
                }
            }
        }
    };

    // Check if any method uses the connection parameter.
    let uses_connection = methods_info.iter().any(|m| m.has_connection_param());

    let conn_suppression = if uses_connection {
        // Connection is used, no suppression needed.
        quote! {}
    } else {
        // Suppress unused warning when no methods use the connection.
        quote! { let _ = __zlink_conn; }
    };

    Ok(quote! {
        #conn_suppression
        match __zlink_call.method() {
            #varlink_service_match
            #user_methods_match
        }
    })
}

/// Replaces . and - with _ and uppercases the interface name
fn interface_name_to_ident<'name>(type_name: &str, interface: &InterfaceName<'name>) -> Ident {
    format_ident!(
        "__{}_INTERFACE_{}",
        type_name.to_uppercase(),
        interface
            .chars()
            .map(|c| if matches!(c, '.' | '-') { '_' } else { c })
            .flat_map(char::to_uppercase)
            .collect::<String>()
    )
}
