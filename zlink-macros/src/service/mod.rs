//! The implementation of the `interface` and `service` attribute macros.
//!
//! `#[interface]` implements the `Interface` trait for the type of an impl block, whose methods
//! all belong to one interface. `#[service]` implements the `Service` trait for it instead: a
//! whole service, of one or more interfaces.

mod attrs;
mod codegen;
mod method;
mod types;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, ImplItem, ItemImpl, parse2};

use attrs::{Mode, ServiceAttrs};
use method::MethodInfo;

pub(crate) fn interface(attr: TokenStream, input: TokenStream) -> TokenStream {
    match expand(attr, input, Mode::Interface) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

pub(crate) fn service(attr: TokenStream, input: TokenStream) -> TokenStream {
    match expand(attr, input, Mode::Service) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

fn expand(attr: TokenStream, input: TokenStream, mode: Mode) -> Result<TokenStream, Error> {
    let mut item_impl = parse2::<ItemImpl>(input)?;

    // Parse macro attributes.
    let service_attrs = ServiceAttrs::parse(&attr, &item_impl, mode)?;

    // Extract doc-comments from the impl block for interface-level introspection.
    let impl_comments = crate::utils::extract_doc_comments(&item_impl.attrs);

    // Validate impl block.
    validate_impl(&item_impl, mode)?;

    // Process methods and collect method information.
    let mut methods_info = Vec::new();
    let mut current_interface: Option<String> = service_attrs.interface.clone();

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
            format!("{} impl block must have at least one method", mode.name()),
        ));
    }

    if methods_info.iter().all(|m| m.interface.is_none()) {
        let message = match mode {
            Mode::Interface => {
                "the interface must be specified: #[zlink::interface(\"org.example.name\")]"
            }
            Mode::Service => {
                "an interface must be specified via #[zlink::service(interface = \"...\")] or \
                 #[zlink(interface = \"...\")] on the first method"
            }
        };
        return Err(Error::new_spanned(&item_impl, message));
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

    let interfaces: std::collections::HashSet<_> = methods_info
        .iter()
        .filter_map(|m| m.interface.as_ref())
        .collect();
    match mode {
        // An impl block is one interface. A service has one type for each of its interfaces.
        Mode::Interface if interfaces.len() > 1 => {
            return Err(Error::new_spanned(
                &item_impl,
                "an `#[interface]` impl block implements one interface: give each interface a \
                 type and an impl block of its own, and make them the fields of a struct that \
                 derives `Service`",
            ));
        }
        Mode::Interface => (),
        // The state is lent by the service that an interface is part of.
        Mode::Service => {
            if let Some(state) = methods_info.iter().find_map(|m| m.state_param()) {
                return Err(Error::new_spanned(
                    &state.name,
                    "`#[zlink(state)]` parameters require `#[zlink::interface]` instead of \
                     `#[zlink::service]`: the state is lent to an interface by the service it is \
                     part of, a struct that derives `Service`",
                ));
            }
        }
    }

    // Generate the trait implementations (uses generics from item_impl if present).
    let trait_impls =
        codegen::generate_trait_impls(&item_impl, &methods_info, &service_attrs, &impl_comments)?;

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

    // Output the original impl block plus the generated trait impls.
    Ok(quote! {
        #output_impl
        #trait_impls
    })
}

fn validate_impl(item_impl: &ItemImpl, mode: Mode) -> Result<(), Error> {
    // The impl must be for a concrete type (not a trait impl).
    if item_impl.trait_.is_some() {
        return Err(Error::new_spanned(
            item_impl,
            format!(
                "{} macro cannot be applied to trait implementations",
                mode.name()
            ),
        ));
    }

    // Check that the impl has at least some items.
    if item_impl.items.is_empty() {
        return Err(Error::new_spanned(
            item_impl,
            format!("{} impl block must have at least one method", mode.name()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_states_can_be_spelled_differently() {
        // The compiler checks that the upgrade methods return the same type of state, however it's
        // spelled, e.g. through an alias or a qualified path.
        let upgrade = expand_compact(quote! {
            impl Blobs {
                #[zlink(upgrade)]
                async fn download(&self) -> ((), DownloadState) { todo!() }

                #[zlink(upgrade)]
                async fn upload(&self) -> Result<(Reply, crate::Protocol), Error> { todo!() }
            }
        });
        assert!(upgrade.contains("typeUpgrade=DownloadState;"));
    }

    #[test]
    fn upgrade_state_is_the_upgrade_of_the_service() {
        let regular = expand_compact(quote! {
            impl Blobs {
                async fn download(&self) {}
            }
        });
        assert!(regular.contains("typeUpgrade=();"));

        // No explicit socket type parameter is needed.
        let upgrade = expand_compact(quote! {
            impl Blobs {
                #[zlink(upgrade)]
                async fn download(&self) -> ((), SendBlob) { todo!() }
            }
        });
        assert!(upgrade.contains("typeUpgrade=SendBlob;"));
        // The state may take connections over for only some of the sockets.
        assert!(upgrade.contains("SendBlob:::zlink::service::TakeOver<__ZlinkSock>"));
        // The server takes over with the state itself, and the method is a regular one.
        assert!(!upgrade.contains("fnupgrade"));
        assert!(upgrade.contains("fndownload"));
    }

    #[test]
    fn interface_is_an_interface_only() {
        let expanded = interface_compact(quote! {
            impl Blobs {
                async fn download(&self) {}
            }
        });
        // It fits into a service with any state.
        assert!(expanded.contains(
            "impl<__ZlinkSock,__ZlinkState>::zlink::service::Interface<__ZlinkSock,__ZlinkState>\
             forBlobs"
        ));
        assert!(!expanded.contains("::zlink::Service<"));
    }

    #[test]
    fn interface_takes_its_name_alone_or_by_key() {
        for attr in [
            quote!("org.example.blobs"),
            quote!("org.example.blobs", types = [Blob]),
            quote!(interface = "org.example.blobs", crate = "::zlink"),
        ] {
            let expanded = super::expand(
                attr,
                quote! {
                    impl Blobs {
                        async fn download(&self) {}
                    }
                },
                Mode::Interface,
            )
            .unwrap()
            .to_string();
            assert!(expanded.contains("org.example.blobs"));
        }
    }

    #[test]
    fn interface_is_one_interface() {
        let err = interface(quote! {
            impl Blobs {
                async fn download(&self) {}

                #[zlink(interface = "org.example.trash")]
                async fn empty(&self) {}
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("one interface"), "{err}");

        // The metadata of a service isn't that of one of its interfaces.
        let err = super::expand(
            quote!("org.example.blobs", vendor = "Example"),
            quote! {
                impl Blobs {
                    async fn download(&self) {}
                }
            },
            Mode::Interface,
        )
        .unwrap_err();
        assert!(err.to_string().contains("derives `Service`"), "{err}");

        let err = super::expand(
            quote!(),
            quote! {
                impl Blobs {
                    async fn download(&self) {}
                }
            },
            Mode::Interface,
        )
        .unwrap_err();
        assert!(err.to_string().contains("must be specified"), "{err}");
    }

    #[test]
    fn service_is_a_service_only() {
        for block in [
            quote! {
                impl Blobs {
                    async fn download(&self) {}
                }
            },
            quote! {
                impl Blobs {
                    async fn download(&self) {}

                    #[zlink(interface = "org.example.trash")]
                    async fn empty(&self) {}
                }
            },
        ] {
            let expanded = expand_compact(block);
            assert!(!expanded.contains("::zlink::service::Interface<"));
            assert!(expanded.contains("::zlink::Service<__ZlinkSock>forBlobs"));
        }
    }

    #[test]
    fn service_has_no_state() {
        let err = expand(quote! {
            impl Blobs {
                async fn download(&self, #[zlink(state)] count: &mut u64) {}
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("`#[zlink::interface]`"), "{err}");
    }

    #[test]
    fn state_is_borrowed_by_the_methods() {
        let expanded = interface_compact(quote! {
            impl Blobs {
                async fn download(&self, #[zlink(state)] count: &mut u64) {}

                async fn count(&self, #[zlink(state)] count: &u64) -> u64 { *count }
            }
        });
        assert!(
            expanded
                .contains("impl<__ZlinkSock>::zlink::service::Interface<__ZlinkSock,u64>forBlobs")
        );
        // The state is passed as the parameters take it, and isn't a parameter of the calls.
        assert!(expanded.contains("self.download(&mut*__zlink_state)"));
        assert!(expanded.contains("self.count(&*__zlink_state)"));
        assert!(!expanded.contains("count:&'__deu64"));
        // The attribute is gone from the methods.
        assert!(expanded.contains("asyncfndownload(&self,count:&mutu64)"));
    }

    #[test]
    fn invalid_state_params_are_rejected() {
        for (params, message) in [
            (quote! { #[zlink(state)] count: u64 }, "must be a reference"),
            (
                quote! { #[zlink(state, connection)] count: &mut u64 },
                "can't also be `#[zlink(connection)]`",
            ),
            (
                quote! { #[zlink(state)] first: &u64, #[zlink(state)] second: &u64 },
                "at most one `#[zlink(state)]`",
            ),
        ] {
            let err = interface(quote! {
                impl Blobs {
                    async fn download(&self, #params) {}
                }
            })
            .unwrap_err();
            assert!(err.to_string().contains(message), "{err}");
        }

        // The first parameter of a streaming method gets the `more` flag.
        let err = interface(quote! {
            impl Blobs {
                #[zlink(more)]
                async fn download(&self, #[zlink(state)] count: &u64) -> Stream<u64> { todo!() }
            }
        })
        .unwrap_err();
        assert!(err.to_string().contains("`more` flag"), "{err}");
    }

    // The expansion of `input` without any whitespace, so that how the tokens are spaced doesn't
    // matter.
    fn expand_compact(input: TokenStream) -> String {
        expand(input).unwrap().to_string().replace(' ', "")
    }

    fn expand(input: TokenStream) -> Result<TokenStream, Error> {
        super::expand(
            quote!(interface = "org.example.blobs"),
            input,
            Mode::Service,
        )
    }

    // The same for an interface.
    fn interface_compact(input: TokenStream) -> String {
        interface(input).unwrap().to_string().replace(' ', "")
    }

    fn interface(input: TokenStream) -> Result<TokenStream, Error> {
        super::expand(quote!("org.example.blobs"), input, Mode::Interface)
    }
}
