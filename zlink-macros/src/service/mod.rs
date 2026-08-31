//! Service macro implementation.
//!
//! This module provides the `#[service]` attribute macro that transforms an impl block into a
//! Service trait implementation.

mod attrs;
mod codegen;
mod method;
mod types;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ImplItem, ItemImpl, parse2};

use attrs::ServiceAttrs;
use method::MethodInfo;

pub(crate) fn service(attr: TokenStream, input: TokenStream) -> TokenStream {
    match service_impl(attr, input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

fn service_impl(attr: TokenStream, input: TokenStream) -> Result<TokenStream, Error> {
    let mut item_impl = parse2::<ItemImpl>(input)?;

    // Parse macro attributes.
    let service_attrs = ServiceAttrs::parse(&attr, &item_impl)?;

    // Extract doc-comments from the impl block for interface-level introspection.
    let impl_comments = crate::utils::extract_doc_comments(&item_impl.attrs);

    // Validate impl block.
    validate_impl(&item_impl)?;

    // Process methods and collect method information.
    let mut methods_info = Vec::new();
    let mut current_interface = service_attrs.interface.clone();
    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let method_info = MethodInfo::extract(method, &mut current_interface)?;
        methods_info.push(method_info);
    }

    // Validate that we have at least one method with an interface.
    if methods_info.is_empty() {
        return Err(Error::new_spanned(
            &item_impl,
            "service impl block must have at least one method",
        ));
    }

    if methods_info.iter().all(|m| m.interface.is_none()) {
        return Err(Error::new_spanned(
            &item_impl,
            "an interface must be specified via #[zlink::service(interface = \"...\")] or \
             #[zlink(interface = \"...\")] on the first method",
        ));
    }

    // Validate that connection parameters are only used with explicit generic socket type.
    let has_explicit_generics = item_impl
        .generics
        .params
        .iter()
        .any(|p| matches!(p, syn::GenericParam::Type(_)));

    if !has_explicit_generics && methods_info.iter().any(|m| m.has_connection_param()) {
        return Err(Error::new_spanned(
            &item_impl,
            "#[zlink(connection)] parameter requires an explicit generic socket type parameter. \
             Use `impl<Sock> YourType` to specify a socket type.",
        ));
    }

    // Generate the Service trait implementation (uses generics from item_impl if present).
    let service_impl =
        codegen::generate_service_impl(&item_impl, &methods_info, &service_attrs, &impl_comments)?;

    // Prepare the output impl block.
    let mut output_impl = item_impl;

    // Remove methods that have connection parameters from the output impl.
    // These methods are only callable via the Service trait (they need the socket type parameter),
    // and including them would result in unconstrained type parameter errors.
    let methods_with_conn: std::collections::HashSet<_> = methods_info
        .iter()
        .filter(|m| m.has_connection_param())
        .map(|m| m.name.to_string())
        .collect();

    output_impl.items.retain(|item| {
        let ImplItem::Fn(method) = item else {
            return true;
        };
        !methods_with_conn.contains(&method.sig.ident.to_string())
    });

    // Strip generics from the original impl block - they are only for the Service impl.
    output_impl.generics = Default::default();

    // Remove zlink attributes from method parameters.
    remove_zlink_param_attrs(&mut output_impl);

    // Add `+ 'static` to streaming methods with `impl Trait` return types. This is required
    // because the returned stream must outlive the `handle` call (it's stored in a Vec and polled
    // independently). In Rust edition 2024, `impl Trait` captures all in-scope lifetimes by
    // default, including the anonymous `&self` lifetime, making the stream non-`'static` without
    // this explicit bound.
    add_static_to_streaming_impl_trait(&mut output_impl, &methods_info);

    // Output the original impl block plus the generated Service impl.
    Ok(quote! {
        #output_impl
        #service_impl
    })
}

fn validate_impl(item_impl: &ItemImpl) -> Result<(), Error> {
    // The impl must be for a concrete type (not a trait impl).
    if item_impl.trait_.is_some() {
        return Err(Error::new_spanned(
            item_impl,
            "service macro cannot be applied to trait implementations",
        ));
    }

    // Check that the impl has at least some items.
    if item_impl.items.is_empty() {
        return Err(Error::new_spanned(
            item_impl,
            "service impl block must have at least one method",
        ));
    }

    Ok(())
}

/// Remove `#[zlink(...)]` attributes from method parameters.
fn remove_zlink_param_attrs(item_impl: &mut ItemImpl) {
    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        for arg in &mut method.sig.inputs {
            let syn::FnArg::Typed(pat_type) = arg else {
                continue;
            };

            pat_type.attrs.retain(|attr| !attr.path().is_ident("zlink"));
        }
    }
}

/// Add `+ 'static` bound to streaming methods that return `impl Trait`.
///
/// The `Service` trait requires `ReplyStream` to be `'static` since the stream outlives the
/// `handle` call. In Rust 2024, `impl Trait` captures all in-scope lifetimes including `&self`,
/// so we must explicitly add `'static` to prevent the capture.
fn add_static_to_streaming_impl_trait(item_impl: &mut ItemImpl, methods_info: &[MethodInfo]) {
    let streaming_impl_trait_methods: std::collections::HashSet<_> = methods_info
        .iter()
        .filter(|m| m.is_streaming && m.stream_uses_impl_trait)
        .map(|m| m.name.to_string())
        .collect();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        if !streaming_impl_trait_methods.contains(&method.sig.ident.to_string()) {
            continue;
        }

        let syn::ReturnType::Type(_, return_type) = &mut method.sig.output else {
            continue;
        };

        if let syn::Type::ImplTrait(impl_trait) = return_type.as_mut() {
            impl_trait
                .bounds
                .push(syn::TypeParamBound::Lifetime(syn::Lifetime::new(
                    "'static",
                    proc_macro2::Span::call_site(),
                )));
        }
    }
}
