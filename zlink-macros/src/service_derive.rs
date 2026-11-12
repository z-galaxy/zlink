//! The `#[derive(Service)]` macro implementation.
//!
//! This module derives `Service` for a struct whose fields are the interfaces of the service. The
//! derive generates a handful of hidden types, one per associated type of `Service`, that combine
//! the types of the interfaces, and the `Service` implementation itself, which hands each method
//! call over to the interface that it belongs to and implements `org.varlink.service`.

mod attrs;
mod codegen;

use proc_macro2::TokenStream;
use syn::{DeriveInput, Error, parse2};

use attrs::Definition;

pub(crate) fn service_derive(input: TokenStream) -> TokenStream {
    match expand(input) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error(),
    }
}

fn expand(input: TokenStream) -> Result<TokenStream, Error> {
    let input = parse2::<DeriveInput>(input)?;
    let definition = Definition::parse(&input)?;

    Ok(codegen::generate(&definition))
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn enums_are_rejected() {
        let err = expand_err(quote! {
            enum Svc { A(Metrics) }
        });
        assert!(err.contains("only supports structs"), "{err}");
    }

    #[test]
    fn unions_are_rejected() {
        let err = expand_err(quote! {
            union Svc { a: Metrics }
        });
        assert!(err.contains("only supports structs"), "{err}");
    }

    #[test]
    fn structs_without_interfaces_are_rejected() {
        for input in [
            quote! { struct Svc; },
            quote! { struct Svc {} },
            quote! { struct Svc(); },
            quote! {
                struct Svc {
                    #[zlink(state)]
                    db: Database,
                }
            },
        ] {
            let err = expand_err(input);
            assert!(err.contains("at least one interface field"), "{err}");
        }
    }

    #[test]
    fn several_state_fields_are_rejected() {
        let err = expand_err(quote! {
            struct Svc {
                #[zlink(state)]
                db: Database,
                #[zlink(state)]
                cache: Cache,
                metrics: Metrics,
            }
        });
        assert!(err.contains("at most one field"), "{err}");
    }

    #[test]
    fn generic_structs_are_rejected() {
        for input in [
            quote! { struct Svc<T> { metrics: T } },
            quote! { struct Svc<'a> { metrics: &'a Metrics } },
            quote! { struct Svc<const N: usize> { metrics: Metrics } },
        ] {
            let err = expand_err(input);
            assert!(err.contains("generic"), "{err}");
        }
    }

    #[test]
    fn unsupported_attributes_are_rejected() {
        let err = expand_err(quote! {
            #[zlink(name = "x")]
            struct Svc { metrics: Metrics }
        });
        assert!(err.contains("unsupported"), "{err}");

        let err = expand_err(quote! {
            struct Svc {
                #[zlink(shared)]
                metrics: Metrics,
            }
        });
        assert!(err.contains("unsupported"), "{err}");
    }

    #[test]
    fn named_and_tuple_structs_are_accepted() {
        let named = expand_ok(quote! {
            #[zlink(vendor = "Acme", product = "Svc", version = "1", url = "https://x")]
            struct Svc {
                #[zlink(state)]
                db: Database,
                metrics: Metrics,
                log: Log,
            }
        });
        assert!(named.contains("Interface<__ZlinkSock,Database>"), "{named}");
        assert!(named.contains("self.metrics"), "{named}");
        assert!(named.contains("self.db"), "{named}");

        let tuple = expand_ok(quote! { struct Svc(Metrics, Log); });
        assert!(tuple.contains("self.0"), "{tuple}");
        assert!(tuple.contains("self.1"), "{tuple}");
        // Without a state field, the interfaces share nothing.
        assert!(tuple.contains("Interface<__ZlinkSock,()>"), "{tuple}");
        assert!(tuple.contains("no_state"), "{tuple}");
    }

    #[test]
    fn crate_path_is_configurable() {
        let tokens = expand_ok(quote! {
            #[zlink(crate = "::my_zlink")]
            struct Svc { metrics: Metrics }
        });
        assert!(
            tokens.contains("::my_zlink::service::Interface"),
            "{tokens}"
        );
        assert!(!tokens.contains("::zlink::"), "{tokens}");
    }

    // The expansion of `input` without any whitespace, so that how the tokens are spaced doesn't
    // matter.
    fn expand_ok(input: TokenStream) -> String {
        expand(input).unwrap().to_string().replace(' ', "")
    }

    fn expand_err(input: TokenStream) -> String {
        expand(input).unwrap_err().to_string()
    }
}
