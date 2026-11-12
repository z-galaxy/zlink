//! Parsing of the arguments of the `interface` and `service` attribute macros.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Error, ItemImpl, LitStr, Token,
    parse::{Parse, ParseStream, Parser},
};

/// Arguments parsed from the `#[interface(...)]` or `#[service(...)]` macro invocation.
pub(super) struct ServiceAttrs {
    /// The macro that the arguments are of.
    pub mode: Mode,
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
    pub(super) fn parse(
        attr: &TokenStream,
        item_impl: &ItemImpl,
        mode: Mode,
    ) -> Result<Self, Error> {
        let mut crate_path = None;
        let mut custom_types = Vec::new();
        let mut interface = None;
        let mut vendor = None;
        let mut product = None;
        let mut version = None;
        let mut url = None;

        if !attr.is_empty() {
            let metas = |input: ParseStream<'_>| -> syn::Result<()> {
                // The interface can be the first argument on its own: `#[interface("org.name")]`.
                if mode == Mode::Interface && input.peek(LitStr) {
                    let value: LitStr = input.parse()?;
                    crate::naming::validate_interface(&value)?;
                    interface = Some(value.value());
                    if input.is_empty() {
                        return Ok(());
                    }
                    input.parse::<Token![,]>()?;
                }

                let rest: TokenStream = input.parse()?;
                syn::meta::parser(|meta| {
                    if meta.path.is_ident("crate") {
                        let value: LitStr = meta.value()?.parse()?;
                        let path_str = value.value();
                        crate_path = Some(syn::parse_str(&path_str)?);
                    } else if meta.path.is_ident("types") {
                        // Parse types = [Type1, Type2, ...]
                        meta.input.parse::<Token![=]>()?;
                        let content;
                        syn::bracketed!(content in meta.input);
                        let types = content.parse_terminated(syn::Type::parse, Token![,])?;
                        custom_types = types.into_iter().collect();
                    } else if meta.path.is_ident("interface") {
                        let value: LitStr = meta.value()?.parse()?;
                        crate::naming::validate_interface(&value)?;
                        interface = Some(value.value());
                    } else if mode == Mode::Interface {
                        let metadata = ["vendor", "product", "version", "url"];
                        if metadata.iter().any(|name| meta.path.is_ident(name)) {
                            return Err(meta.error(
                                "the metadata of a service goes on the struct that derives \
                                 `Service`, as `#[zlink(...)]`",
                            ));
                        }
                        return Err(meta.error("unsupported interface attribute"));
                    } else if meta.path.is_ident("vendor") {
                        vendor = Some(meta.value()?.parse()?);
                    } else if meta.path.is_ident("product") {
                        product = Some(meta.value()?.parse()?);
                    } else if meta.path.is_ident("version") {
                        version = Some(meta.value()?.parse()?);
                    } else if meta.path.is_ident("url") {
                        url = Some(meta.value()?.parse()?);
                    } else {
                        return Err(meta.error("unsupported service attribute"));
                    }
                    Ok(())
                })
                .parse2(rest)
            };

            metas.parse2(attr.clone()).map_err(|e| {
                let expected = match mode {
                    Mode::Interface => {
                        "#[interface(\"org.example.name\")], \
                         #[interface(interface = \"...\", types = [T1, T2], crate = \"path\")]"
                    }
                    Mode::Service => {
                        "#[service], #[service(crate = \"path\")], \
                         #[service(interface = \"...\", types = [T1, T2], \
                         vendor = <expr>, product = <expr>, version = <expr>, \
                         url = <expr>)]"
                    }
                };
                Error::new_spanned(
                    item_impl,
                    format!(
                        "failed to parse {} attributes: {e}. Expected: {expected}",
                        mode.name()
                    ),
                )
            })?;
        }

        Ok(Self {
            mode,
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

/// Which of the two attribute macros an `impl` block is expanded for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// `#[interface]`: the block is one interface, to be made part of a service.
    Interface,
    /// `#[service]`: the block is a whole service.
    Service,
}

impl Mode {
    /// The name of the attribute macro.
    pub(super) fn name(self) -> &'static str {
        match self {
            Mode::Interface => "interface",
            Mode::Service => "service",
        }
    }
}
