use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse::Parser, parse2, Error, ItemTrait, Lit, TraitItem};

mod chain_extension;
mod chain_method;
mod method_impl;
mod types;
mod utils;

use chain_extension::generate_chain_extension_method;
use chain_method::generate_chain_method;
use method_impl::generate_method_impl;
use types::MethodAttrs;
use utils::build_combined_where_clause;

pub(crate) fn proxy(attr: TokenStream, input: TokenStream) -> TokenStream {
    match proxy_impl(attr, input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

fn proxy_impl(attr: TokenStream, input: TokenStream) -> Result<TokenStream, Error> {
    let mut trait_def = parse2::<ItemTrait>(input)?;

    // Parse the interface name, crate path, and chain name from the attribute
    let (interface_name, crate_path, chain_name) = parse_proxy_attributes(&attr, &trait_def)?;

    // Validate trait definition
    validate_trait(&trait_def)?;

    // Generate implementations for each method
    let mut methods = Vec::new();
    let mut chain_method_traits = Vec::new();
    let mut chain_method_impls = Vec::new();
    let mut chain_extension_methods = Vec::new();
    let mut chain_extension_impls = Vec::new();

    // Process methods first before we need trait_def references
    for item in &mut trait_def.items {
        if let TraitItem::Fn(method) = item {
            // Extract attributes once to avoid multiple mutable borrows
            let method_attrs = MethodAttrs::extract(&mut method.attrs)?;

            // Generate chain extension method
            let (extension_method, extension_impl) = generate_chain_extension_method(
                method,
                &interface_name,
                &trait_def.generics,
                &method_attrs,
                &crate_path,
            )?;
            if !extension_method.is_empty() {
                chain_extension_methods.push(extension_method);
            }
            if !extension_impl.is_empty() {
                chain_extension_impls.push(extension_impl);
            }

            // Generate regular method implementation
            let method_impl = generate_method_impl(
                method,
                &interface_name,
                &trait_def.generics,
                &method_attrs,
                &crate_path,
            )?;
            methods.push(method_impl);

            // Generate chain method
            let (chain_trait, chain_impl) = generate_chain_method(
                method,
                &interface_name,
                &trait_def.generics,
                &method_attrs,
                &crate_path,
            )?;
            if !chain_trait.is_empty() {
                chain_method_traits.push(chain_trait);
            }
            if !chain_impl.is_empty() {
                chain_method_impls.push(chain_impl);
            }
        }
    }

    // Build the output components
    let trait_output = build_trait_output(&mut trait_def, &chain_method_traits, &crate_path)?;
    let impl_output = build_impl_output(
        &trait_def.ident,
        &trait_def.generics,
        &trait_def.generics.where_clause,
        &methods,
        &chain_method_impls,
        &crate_path,
    );
    let chain_extension_trait_output = build_chain_extension_trait(
        &trait_def.ident,
        &chain_extension_methods,
        &chain_extension_impls,
        &crate_path,
        chain_name,
    );

    Ok(quote! {
        #trait_output
        #impl_output
        #chain_extension_trait_output
    })
}

fn parse_proxy_attributes(
    attr: &TokenStream,
    trait_def: &ItemTrait,
) -> Result<(String, TokenStream, Option<syn::Ident>), Error> {
    if attr.is_empty() {
        return Err(Error::new_spanned(
            trait_def,
            "proxy macro requires interface name, e.g. #[proxy(\"org.example.Interface\")] or #[proxy(interface = \"org.example.Interface\")]",
        ));
    }

    // Try parsing as a simple string literal first (backward compatibility)
    if let Ok(interface_lit) = parse2::<Lit>(attr.clone()) {
        match interface_lit {
            Lit::Str(lit_str) => {
                return Ok((lit_str.value(), quote! { ::zlink }, None));
            }
            _ => {}
        }
    }

    // Parse as name-value pairs
    let mut interface_name = None;
    let mut crate_path = None;
    let mut chain_name = None;

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("interface") {
            let value: syn::LitStr = meta.value()?.parse()?;
            interface_name = Some(value.value());
        } else if meta.path.is_ident("crate") {
            let value: syn::LitStr = meta.value()?.parse()?;
            let path_str = value.value();
            crate_path = Some(syn::parse_str(&path_str)?);
        } else if meta.path.is_ident("chain_name") {
            let value: syn::LitStr = meta.value()?.parse()?;
            chain_name = Some(syn::Ident::new(&value.value(), value.span()));
        } else {
            return Err(meta.error("unsupported attribute"));
        }
        Ok(())
    });

    parser.parse2(attr.clone())?;

    let interface_name = interface_name.ok_or_else(|| {
        Error::new_spanned(
            trait_def,
            "proxy macro requires 'interface' parameter, e.g. #[proxy(interface = \"org.example.Interface\")]",
        )
    })?;

    let crate_path = crate_path.unwrap_or_else(|| quote! { ::zlink });

    Ok((interface_name, crate_path, chain_name))
}

fn validate_trait(trait_def: &ItemTrait) -> Result<(), Error> {
    if !trait_def.items.is_empty()
        && trait_def
            .items
            .iter()
            .any(|item| !matches!(item, TraitItem::Fn(_)))
    {
        return Err(Error::new_spanned(
            trait_def,
            "proxy macro only supports traits with method definitions",
        ));
    }
    Ok(())
}

fn build_trait_output(
    trait_def: &mut ItemTrait,
    chain_method_traits: &[TokenStream],
    crate_path: &TokenStream,
) -> Result<TokenStream, Error> {
    // Add the Socket associated type to the trait
    trait_def.items.push(syn::parse2(quote! {
        /// The socket type used for the connection.
        type Socket: #crate_path::connection::socket::Socket;
    })?);

    // Add chain method signatures to the trait definition
    for chain_trait in chain_method_traits {
        trait_def.items.push(syn::parse2(chain_trait.clone())?);
    }

    Ok(quote! {
        #[allow(async_fn_in_trait)]
        #trait_def
    })
}

fn build_impl_output(
    trait_name: &syn::Ident,
    generics: &syn::Generics,
    where_clause: &Option<syn::WhereClause>,
    methods: &[TokenStream],
    chain_method_impls: &[TokenStream],
    crate_path: &TokenStream,
) -> TokenStream {
    // Build impl generics combining trait generics with socket generic
    let mut impl_generics = generics.clone();
    impl_generics.params.push(syn::parse_quote!(S));

    // Create trait generics without bounds for impl line
    let mut trait_generics_no_bounds = generics.clone();
    for param in &mut trait_generics_no_bounds.params {
        if let syn::GenericParam::Type(type_param) = param {
            type_param.bounds.clear();
        }
    }

    // Build where clause combining existing constraints with socket constraint and trait bounds
    let combined_where_clause = Some(build_combined_where_clause(
        where_clause.clone(),
        syn::parse_quote!(S: #crate_path::connection::socket::Socket),
        generics,
    ));

    quote! {
        impl #impl_generics #trait_name #trait_generics_no_bounds for #crate_path::Connection<S>
        #combined_where_clause
        {
            type Socket = S;

            #(#methods)*
            #(#chain_method_impls)*
        }
    }
}

fn build_chain_extension_trait(
    trait_name: &syn::Ident,
    chain_extension_methods: &[TokenStream],
    chain_extension_impls: &[TokenStream],
    crate_path: &TokenStream,
    custom_chain_name: Option<syn::Ident>,
) -> TokenStream {
    if chain_extension_methods.is_empty() {
        return quote! {};
    }

    let chain_trait_name = custom_chain_name
        .unwrap_or_else(|| syn::Ident::new(&format!("{trait_name}Chain"), trait_name.span()));

    quote! {
        /// Extension trait for adding proxy calls to any chain.
        ///
        /// This trait provides methods to add proxy calls to a chain of method calls.
        pub trait #chain_trait_name<'c, S, ReplyParams, ReplyError>
        where
            S: #crate_path::connection::socket::Socket,
            ReplyParams: ::serde::Deserialize<'c> + ::core::fmt::Debug,
            ReplyError: ::serde::Deserialize<'c> + ::core::fmt::Debug,
        {
            #(#chain_extension_methods)*
        }

        impl<'c, S, ReplyParams, ReplyError> #chain_trait_name<'c, S, ReplyParams, ReplyError>
            for #crate_path::connection::chain::Chain<'c, S, ReplyParams, ReplyError>
        where
            S: #crate_path::connection::socket::Socket,
            ReplyParams: ::serde::Deserialize<'c> + ::core::fmt::Debug,
            ReplyError: ::serde::Deserialize<'c> + ::core::fmt::Debug,
        {
            #(#chain_extension_impls)*
        }
    }
}
