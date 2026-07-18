//! Service macro attribute parsing.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Error, ItemImpl,
    parse::{Parse, Parser},
};

/// Attributes parsed from the `#[service(...)]` macro invocation.
pub(super) struct ServiceAttrs {
    /// The crate path to use (defaults to `::zlink`).
    pub crate_path: TokenStream,
    /// Custom types for introspection.
    pub custom_types: Vec<syn::Type>,
    /// Default interface name for all methods.
    pub interface: Option<String>,
    /// Service vendor name.
    pub vendor: Option<syn::Expr>,
    /// Service product name.
    pub product: Option<syn::Expr>,
    /// Service version.
    pub version: Option<syn::Expr>,
    /// Service URL.
    pub url: Option<syn::Expr>,
}

impl ServiceAttrs {
    /// Parse attributes from the macro attribute token stream.
    pub(super) fn parse(attr: &TokenStream, item_impl: &ItemImpl) -> Result<Self, Error> {
        let mut crate_path = None;
        let mut custom_types = Vec::new();
        let mut interface = None;
        let mut vendor = None;
        let mut product = None;
        let mut version = None;
        let mut url = None;

        if !attr.is_empty() {
            let parser = syn::meta::parser(|meta| {
                if meta.path.is_ident("crate") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    let path_str = value.value();
                    crate_path = Some(syn::parse_str(&path_str)?);
                } else if meta.path.is_ident("types") {
                    // Parse types = [Type1, Type2, ...]
                    meta.input.parse::<syn::Token![=]>()?;
                    let content;
                    syn::bracketed!(content in meta.input);
                    let types = content.parse_terminated(syn::Type::parse, syn::Token![,])?;
                    custom_types = types.into_iter().collect();
                } else if meta.path.is_ident("interface") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    crate::naming::validate_interface(&value)?;
                    interface = Some(value.value());
                } else if meta.path.is_ident("vendor") {
                    let value: syn::Expr = meta.value()?.parse()?;
                    vendor = Some(value);
                } else if meta.path.is_ident("product") {
                    let value: syn::Expr = meta.value()?.parse()?;
                    product = Some(value);
                } else if meta.path.is_ident("version") {
                    let value: syn::Expr = meta.value()?.parse()?;
                    version = Some(value);
                } else if meta.path.is_ident("url") {
                    let value: syn::Expr = meta.value()?.parse()?;
                    url = Some(value);
                } else {
                    return Err(meta.error("unsupported service attribute"));
                }
                Ok(())
            });

            parser.parse2(attr.clone()).map_err(|e| {
                Error::new_spanned(
                    item_impl,
                    format!(
                        "failed to parse service attributes: {e}. Expected: \
                         #[service], #[service(crate = \"path\")], \
                         #[service(interface = \"...\", types = [T1, T2], \
                         vendor = <expr>, product = <expr>, version = <expr>, \
                         url = <expr>)]"
                    ),
                )
            })?;
        }

        Ok(Self {
            crate_path: crate_path.unwrap_or_else(|| quote! { ::zlink }),
            custom_types,
            interface,
            vendor,
            product,
            version,
            url,
        })
    }
}
