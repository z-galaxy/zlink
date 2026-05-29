use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::{Error, FnArg, Pat, Type, punctuated::Punctuated};

use super::{
    types::{ArgInfo, MethodAttrs},
    utils::{
        ParamAttrs, collect_used_type_params, convert_to_single_lifetime, parse_return_type,
        snake_case_to_pascal_case, type_contains_lifetime,
    },
};
use crate::utils::is_option_type;

pub(super) fn generate_method_impl(
    method: &mut syn::TraitItemFn,
    interface_name: &str,
    trait_generics: &syn::Generics,
    method_attrs: &MethodAttrs,
    crate_path: &TokenStream,
    param_attrs_map: &std::collections::HashMap<String, ParamAttrs>,
) -> Result<TokenStream, Error> {
    let method_name = &method.sig.ident;
    let method_name_str = method_name.to_string();

    let converted_name = snake_case_to_pascal_case(&method_name_str);
    let actual_method_name = method_attrs.rename.as_deref().unwrap_or(&converted_name);

    // Build the full method path: interface.method
    let method_path = format!("{interface_name}.{actual_method_name}");

    // Parse method arguments (skip &mut self)
    let has_explicit_lifetimes = method.sig.generics.lifetimes().next().is_some();

    // Extract data before mutable borrow
    let method_generics = method.sig.generics.clone();
    let method_output = method.sig.output.clone();
    let is_self_by_value = match method.sig.inputs.first() {
        Some(FnArg::Receiver(receiver)) => receiver.reference.is_none(),
        _ => false,
    };

    // Process all method arguments in a single pass
    let arg_infos = parse_method_arguments(method, has_explicit_lifetimes, param_attrs_map)?;

    // Extract the data we need from the processed arguments
    let has_any_lifetime = arg_infos.iter().any(|info| info.has_lifetime);

    // Check for incompatible attributes
    if method_attrs.is_streaming && method_attrs.is_oneway {
        return Err(Error::new_spanned(
            &method.sig,
            "method cannot be both streaming (`more`) and oneway (`oneway`)",
        ));
    }
    if method_attrs.is_oneway && method_attrs.return_fds {
        return Err(Error::new_spanned(
            &method.sig,
            "method cannot be both oneway (`oneway`) and return file descriptors (`return_fds`)",
        ));
    }
    if method_attrs.is_upgrade && method_attrs.is_streaming {
        return Err(Error::new_spanned(
            &method.sig,
            "method cannot be both upgrade (`upgrade`) and streaming (`more`)",
        ));
    }
    if method_attrs.is_upgrade && method_attrs.is_oneway {
        return Err(Error::new_spanned(
            &method.sig,
            "method cannot be both upgrade (`upgrade`) and oneway (`oneway`)",
        ));
    }
    if method_attrs.is_upgrade && method_attrs.return_fds {
        return Err(Error::new_spanned(
            &method.sig,
            "method cannot be both upgrade (`upgrade`) and return file descriptors (`return_fds`)",
        ));
    }

    // Validate that upgrade method takes `self` by value
    if method_attrs.is_upgrade && !is_self_by_value {
        return Err(Error::new_spanned(
            &method.sig,
            "upgrade method must take `self` by value",
        ));
    }

    // Validate FD parameters
    let fds_params: Vec<_> = arg_infos.iter().filter(|info| info.is_fds).collect();
    if fds_params.len() > 1 {
        return Err(Error::new_spanned(
            &method.sig,
            "method can have at most one parameter marked with #[zlink(fds)]",
        ));
    }

    // Validate that FD attributes require the `std` feature
    #[cfg(not(feature = "std"))]
    {
        if !fds_params.is_empty() || method_attrs.return_fds {
            return Err(Error::new_spanned(
                &method.sig,
                "FD-related attributes (`#[zlink(fds)]` and `#[zlink(return_fds)]`) require the `std` feature to be enabled.",
            ));
        }
    }

    // Parse return type
    let (reply_type, error_type) = if method_attrs.is_oneway {
        // For oneway methods, we don't parse the return type - just use dummy values
        // since we don't use them in the generated code
        (syn::parse_quote!(()), syn::parse_quote!(#crate_path::Error))
    } else {
        parse_return_type(
            &method_output,
            method_attrs.is_streaming,
            method_attrs.return_fds,
            method_attrs.is_upgrade,
        )?
    };

    // Streaming methods require owned return types (DeserializeOwned) because the internal buffer
    // may be reused between stream iterations.
    if method_attrs.is_streaming
        && (type_contains_lifetime(&reply_type) || type_contains_lifetime(&error_type))
    {
        return Err(Error::new_spanned(
            &method.sig.output,
            "streaming methods (`more`) require owned return types (no non-static lifetimes) \
             because the internal buffer may be reused between stream iterations",
        ));
    }

    // Generate the method parameters as an Option
    #[cfg(feature = "std")]
    let (params_struct_def, params_init, fds_init) = generate_method_params(
        &arg_infos,
        &method_generics,
        trait_generics,
        has_any_lifetime,
        has_explicit_lifetimes,
    );
    #[cfg(not(feature = "std"))]
    let (params_struct_def, params_init, _fds_init) = generate_method_params(
        &arg_infos,
        &method_generics,
        trait_generics,
        has_any_lifetime,
        has_explicit_lifetimes,
    );

    // Common method call setup
    let method_call_setup = quote! {
        #params_struct_def
        #params_init

        #[derive(::serde::Serialize, ::core::fmt::Debug)]
        struct MethodCall<T> {
            method: &'static str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parameters: Option<T>,
        }

        let method_call = MethodCall {
            method: #method_path,
            parameters: params,
        };
    };

    let out_params_extract = match &reply_type {
        Type::Tuple(tuple) if tuple.elems.is_empty() => {
            // Unit type ()
            quote!(Ok(Ok(())))
        }
        _ => {
            quote!(match reply.into_parameters() {
                Some(params) => Ok(Ok(params)),
                None => Err(#crate_path::Error::MissingParameters),
            })
        }
    };

    // Generate return type and implementation based on method attributes
    let (return_type, implementation) = if method_attrs.is_oneway {
        generate_oneway_method(
            method_call_setup,
            #[cfg(feature = "std")]
            fds_init,
            crate_path,
        )
    } else if method_attrs.is_streaming {
        generate_streaming_method(
            method_call_setup,
            &reply_type,
            &error_type,
            out_params_extract,
            method_attrs.return_fds,
            #[cfg(feature = "std")]
            fds_init,
            crate_path,
        )
    } else if method_attrs.is_upgrade {
        generate_upgrade_method(
            method_call_setup,
            &reply_type,
            &error_type,
            #[cfg(feature = "std")]
            fds_init,
            crate_path,
        )
    } else {
        generate_regular_method(
            method_call_setup,
            &reply_type,
            &error_type,
            out_params_extract,
            method_attrs.return_fds,
            #[cfg(feature = "std")]
            fds_init,
            crate_path,
        )
    };

    // Generate the method implementation using the original signature but with new return type and
    // body
    let mut method_sig = method.sig.clone();
    method_sig.output = syn::parse2(quote! { -> #return_type })?;

    Ok(quote! {
        #method_sig
        {
            #implementation
        }
    })
}

fn parse_method_arguments<'a>(
    method: &'a mut syn::TraitItemFn,
    has_explicit_lifetimes: bool,
    param_attrs_map: &std::collections::HashMap<String, ParamAttrs>,
) -> Result<Vec<ArgInfo<'a>>, Error> {
    method
        .sig
        .inputs
        .iter_mut()
        .skip(1)
        .filter_map(|arg| {
            let FnArg::Typed(pat_type) = arg else {
                return None;
            };
            let Pat::Ident(pat_ident) = &*pat_type.pat else {
                return None;
            };

            let name = &pat_ident.ident;
            let ty = &pat_type.ty;

            // Get pre-extracted parameter attributes
            let param_name = name.to_string();
            let param_attrs = param_attrs_map.get(&param_name);
            let serialized_name = param_attrs.and_then(|attrs| attrs.rename.clone());
            let is_fds = param_attrs.map(|attrs| attrs.is_fds).unwrap_or(false);

            // Check if the type is optional
            let is_optional = is_option_type(ty);

            // Only convert to single lifetime if there are no explicit lifetimes
            let ty_for_params = if has_explicit_lifetimes {
                (**ty).clone()
            } else {
                convert_to_single_lifetime(ty)
            };

            // Check if this argument has lifetimes
            let has_lifetime = type_contains_lifetime(&ty_for_params);

            Some(Ok(ArgInfo {
                name,
                ty_for_params,
                is_optional,
                has_lifetime,
                serialized_name,
                is_fds,
            }))
        })
        .collect()
}

fn generate_method_params(
    arg_infos: &[ArgInfo<'_>],
    method_generics: &syn::Generics,
    trait_generics: &syn::Generics,
    has_any_lifetime: bool,
    has_explicit_lifetimes: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    // Find FD parameters
    let fds_params: Vec<_> = arg_infos.iter().filter(|info| info.is_fds).collect();

    // Generate FD initialization
    let fds_init = if let Some(fd_param) = fds_params.first() {
        let fd_name = fd_param.name;
        quote! { #fd_name }
    } else {
        quote! { ::std::vec::Vec::new() }
    };

    // Check if we have any regular (non-FD) parameters
    let has_regular_params = arg_infos.iter().any(|info| !info.is_fds);

    if has_regular_params {
        // Collect which type parameters are actually used in regular (non-FD) method arguments
        let mut used_type_params = HashSet::new();
        for info in arg_infos.iter().filter(|info| !info.is_fds) {
            collect_used_type_params(&info.ty_for_params, &mut used_type_params);
        }

        // Include only used trait generics and method generics for Params struct (without bounds)
        let mut combined_generics: Punctuated<syn::GenericParam, syn::Token![,]> =
            Punctuated::new();

        // Add trait generics that are actually used (without bounds)
        for param in &trait_generics.params {
            match param {
                syn::GenericParam::Type(type_param) => {
                    if used_type_params.contains(&type_param.ident.to_string()) {
                        let mut clean_param = type_param.clone();
                        clean_param.bounds.clear();
                        combined_generics.push(syn::GenericParam::Type(clean_param));
                    }
                }
                other => combined_generics.push(other.clone()),
            }
        }

        // Add method generics that are actually used (without bounds)
        for param in &method_generics.params {
            match param {
                syn::GenericParam::Type(type_param) => {
                    if used_type_params.contains(&type_param.ident.to_string()) {
                        let mut clean_param = type_param.clone();
                        clean_param.bounds.clear();
                        combined_generics.push(syn::GenericParam::Type(clean_param));
                    }
                }
                other => combined_generics.push(other.clone()),
            }
        }

        // Add lifetime if needed
        let generics_decl = if !combined_generics.is_empty() {
            quote! { <#combined_generics> }
        } else if has_any_lifetime && !has_explicit_lifetimes {
            quote! { <'__proxy_params> }
        } else {
            quote! {}
        };

        // Generate struct fields with optional serde attributes (excluding FD parameters)
        let struct_fields: Vec<_> = arg_infos
            .iter()
            .filter(|info| !info.is_fds) // Filter out FD parameters
            .map(|info| {
                let name = info.name;
                let ty = &info.ty_for_params;

                let serde_attrs = if let Some(ref renamed) = info.serialized_name {
                    if info.is_optional {
                        quote! {
                            #[serde(rename = #renamed, skip_serializing_if = "Option::is_none")]
                        }
                    } else {
                        quote! {
                            #[serde(rename = #renamed)]
                        }
                    }
                } else if info.is_optional {
                    quote! {
                        #[serde(skip_serializing_if = "Option::is_none")]
                    }
                } else {
                    quote! {}
                };

                quote! {
                    #serde_attrs
                    #name: #ty
                }
            })
            .collect();

        // Add where clause with bounds from method's where clause for used type parameters
        let params_where_clause = build_params_where_clause(method_generics, &used_type_params);

        let struct_def = quote! {
            #[derive(::serde::Serialize, ::core::fmt::Debug)]
            struct Params #generics_decl
            #params_where_clause
            {
                #(#struct_fields,)*
            }
        };

        // Get names from regular params for struct initialization
        let regular_arg_names: Vec<_> = arg_infos
            .iter()
            .filter(|info| !info.is_fds)
            .map(|info| info.name)
            .collect();

        let init = quote! {
            let params = Some(Params {
                #(#regular_arg_names,)*
            });
        };

        (struct_def, init, fds_init)
    } else {
        (
            quote! {},
            quote! {
                // No parameters for methods without arguments
                let params: Option<()> = None;
            },
            fds_init,
        )
    }
}

fn build_params_where_clause(
    method_generics: &syn::Generics,
    used_type_params: &HashSet<String>,
) -> Option<syn::WhereClause> {
    match &method_generics.where_clause {
        None => None,
        Some(method_where_clause) => {
            let mut where_predicates = syn::punctuated::Punctuated::new();

            for predicate in &method_where_clause.predicates {
                let syn::WherePredicate::Type(type_predicate) = predicate else {
                    continue;
                };
                let syn::Type::Path(type_path) = &type_predicate.bounded_ty else {
                    continue;
                };
                if type_path.path.segments.len() != 1 {
                    continue;
                }
                let type_name = &type_path.path.segments[0].ident;
                if used_type_params.contains(&type_name.to_string()) {
                    where_predicates.push(predicate.clone());
                }
            }

            if !where_predicates.is_empty() {
                Some(syn::WhereClause {
                    where_token: syn::token::Where::default(),
                    predicates: where_predicates,
                })
            } else {
                None
            }
        }
    }
}

fn generate_oneway_method(
    method_call_setup: TokenStream,
    #[cfg(feature = "std")] fds_init: TokenStream,
    crate_path: &TokenStream,
) -> (TokenStream, TokenStream) {
    let return_type = quote! {
        #crate_path::Result<()>
    };

    #[cfg(feature = "std")]
    let send_call_args = quote! { &call, #fds_init };
    #[cfg(not(feature = "std"))]
    let send_call_args = quote! { &call };

    let implementation = quote! {
        #method_call_setup
        let call = #crate_path::Call::new(method_call).set_oneway(true);
        self.send_call(#send_call_args).await?;
        Ok(())
    };

    (return_type, implementation)
}

#[allow(clippy::too_many_arguments)]
fn generate_streaming_method(
    method_call_setup: TokenStream,
    reply_type: &Type,
    error_type: &Type,
    out_params_extract: TokenStream,
    return_fds: bool,
    #[cfg(feature = "std")] fds_init: TokenStream,
    crate_path: &TokenStream,
) -> (TokenStream, TokenStream) {
    let return_type = if return_fds {
        quote! {
            #crate_path::Result<
                impl #crate_path::futures_util::stream::Stream<
                    Item = #crate_path::Result<(::core::result::Result<#reply_type, #error_type>, ::std::vec::Vec<::std::os::fd::OwnedFd>)>
                >
            >
        }
    } else {
        quote! {
            #crate_path::Result<
                impl #crate_path::futures_util::stream::Stream<
                    Item = #crate_path::Result<::core::result::Result<#reply_type, #error_type>>
                >
            >
        }
    };

    #[cfg(feature = "std")]
    let send_call_args = quote! { &call, #fds_init };
    #[cfg(not(feature = "std"))]
    let send_call_args = quote! { &call };

    let send_and_receive = quote! {
        #method_call_setup
        let call = #crate_path::Call::new(method_call).set_more(true);
        self.send_call(#send_call_args).await?;
    };

    #[cfg(feature = "std")]
    let map_stream = if return_fds {
        quote! {
            stream.map(|result| {
                match result {
                    Ok((Ok(reply), fds)) => {
                        #out_params_extract.map(|r| (r, fds))
                    }
                    Ok((Err(error), fds)) => Ok((Err(error), fds)),
                    Err(err) => Err(err),
                }
            })
        }
    } else {
        quote! {
            stream.map(|result| {
                match result {
                    Ok((Ok(reply), _fds)) => #out_params_extract,
                    Ok((Err(error), _fds)) => Ok(Err(error)),
                    Err(err) => Err(err),
                }
            })
        }
    };

    #[cfg(not(feature = "std"))]
    let map_stream = quote! {
        stream.map(|result| {
            match result {
                Ok(Ok(reply)) => #out_params_extract,
                Ok(Err(error)) => Ok(Err(error)),
                Err(err) => Err(err),
            }
        })
    };

    let implementation = quote! {
        #send_and_receive

        let stream = #crate_path::connection::chain::ReplyStream::<#reply_type, #error_type>::new(
            self.read_mut(),
            1,
        );

        use #crate_path::futures_util::stream::{Stream, StreamExt};
        Ok(#map_stream)
    };
    (return_type, implementation)
}

#[allow(clippy::too_many_arguments)]
fn generate_regular_method(
    method_call_setup: TokenStream,
    reply_type: &Type,
    error_type: &Type,
    out_params_extract: TokenStream,
    return_fds: bool,
    #[cfg(feature = "std")] fds_init: TokenStream,
    crate_path: &TokenStream,
) -> (TokenStream, TokenStream) {
    // Generate return type - use FD types only if method returns FDs
    let return_type = if return_fds {
        quote! {
            #crate_path::Result<(::core::result::Result<#reply_type, #error_type>, ::std::vec::Vec<::std::os::fd::OwnedFd>)>
        }
    } else {
        quote! {
            #crate_path::Result<::core::result::Result<#reply_type, #error_type>>
        }
    };

    #[cfg(feature = "std")]
    let call_method_args = quote! { &call, #fds_init };
    #[cfg(not(feature = "std"))]
    let call_method_args = quote! { &call };

    #[cfg(feature = "std")]
    let (receive_result, ok_arm, err_arm) = if return_fds {
        // Method returns FDs - must receive FDs from API
        (
            quote! { let (result, fds) = },
            quote! { #out_params_extract.map(|r| (r, fds)) },
            quote! { Ok((Err(error), fds)) },
        )
    } else {
        // Method doesn't return FDs - ignore any returned FDs
        (
            quote! { let (result, _fds) = },
            quote! { #out_params_extract },
            quote! { Ok(Err(error)) },
        )
    };

    #[cfg(not(feature = "std"))]
    let (receive_result, ok_arm, err_arm) = (
        quote! { let result = },
        quote! { #out_params_extract },
        quote! { Ok(Err(error)) },
    );

    let implementation = quote! {
        #method_call_setup
        let call = #crate_path::Call::new(method_call);
        #receive_result self.call_method::<_, #reply_type, #error_type>(#call_method_args).await?;
        match result {
            Ok(reply) => #ok_arm,
            Err(error) => #err_arm,
        }
    };

    (return_type, implementation)
}

#[allow(clippy::too_many_arguments)]
fn generate_upgrade_method(
    method_call_setup: TokenStream,
    reply_type: &Type,
    error_type: &Type,
    #[cfg(feature = "std")] fds_init: TokenStream,
    crate_path: &TokenStream,
) -> (TokenStream, TokenStream) {
    let return_type = quote! {
        #crate_path::Result<#crate_path::connection::UpgradeReply<Self::Socket, #reply_type, #error_type>>
    };

    #[cfg(feature = "std")]
    let call_upgrade_args = quote! { &call, #fds_init };
    #[cfg(not(feature = "std"))]
    let call_upgrade_args = quote! { &call };

    let implementation = quote! {
        #method_call_setup
        let call = #crate_path::Call::new(method_call).set_upgrade(true);
        self.call_upgrade::<_, #reply_type, #error_type>(#call_upgrade_args).await
    };

    (return_type, implementation)
}
