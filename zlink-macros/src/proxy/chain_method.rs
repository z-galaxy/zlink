use proc_macro2::TokenStream;
use quote::quote;
use syn::{Error, FnArg, Pat};

use super::{
    types::{ArgInfo, MethodAttrs},
    utils::{
        ParamAttrs, convert_to_single_lifetime, parse_return_type, resolve_serialized_name,
        snake_case_to_pascal_case, type_contains_lifetime,
    },
};

#[cfg(feature = "std")]
type MethodCallResult = (TokenStream, TokenStream);
#[cfg(not(feature = "std"))]
type MethodCallResult = TokenStream;

pub(super) fn generate_chain_method(
    method: &mut syn::TraitItemFn,
    interface_name: &str,
    _trait_generics: &syn::Generics,
    method_attrs: &MethodAttrs,
    crate_path: &TokenStream,
    param_attrs_map: &std::collections::HashMap<String, ParamAttrs>,
    rename_all: Option<crate::naming::RenameAll>,
) -> Result<(TokenStream, TokenStream), Error> {
    // Only the wire name and the `chain_` ident are unraw'd; `method_ident` stays raw so the
    // generated fn still matches the trait it implements.
    let method_name_str = crate::naming::unraw(&method.sig.ident);
    let method_ident = method.sig.ident.clone();
    let method_span = method.sig.ident.span();

    // Check for explicit lifetimes early
    let has_explicit_lifetimes = method.sig.generics.lifetimes().next().is_some();

    // Skip chain methods for methods that return FDs since chains don't support returning FDs.
    if method_attrs.return_fds {
        return Ok((quote! {}, quote! {}));
    }

    // For non-oneway methods, skip if return types have borrowed types.
    // Chain API requires DeserializeOwned for reply and error types.
    // Oneway methods don't have return types so this check doesn't apply to them.
    if !method_attrs.is_oneway {
        let (reply_type, error_type) =
            parse_return_type(&method.sig.output, method_attrs.is_streaming, false)?;
        if type_contains_lifetime(&reply_type) || type_contains_lifetime(&error_type) {
            return Ok((quote! {}, quote! {}));
        }
    }

    // Generate chain method name
    let chain_method_name = syn::Ident::new(&format!("chain_{method_name_str}"), method_span);

    let converted_name = snake_case_to_pascal_case(&method_name_str);
    let actual_method_name = method_attrs.rename.as_deref().unwrap_or(&converted_name);
    let method_path = format!("{interface_name}.{actual_method_name}");

    // Extract data before mutable borrow
    let method_generic_params = method.sig.generics.params.clone();
    let method_where_clause = method.sig.generics.where_clause.clone();

    // Parse method arguments (skip &mut self)
    let arg_infos =
        parse_method_arguments(method, has_explicit_lifetimes, param_attrs_map, rename_all)?;
    let has_any_lifetime = arg_infos.iter().any(|info| info.has_lifetime);

    // Handle lifetimes for function signature - only add if no explicit lifetimes
    let lifetime_bound = if has_any_lifetime && !has_explicit_lifetimes {
        quote! { '__proxy_params, }
    } else {
        quote! {}
    };

    // Combine method generics with our chain-specific generics.
    let all_generics = if !method_generic_params.is_empty() {
        quote! { 'c, #lifetime_bound #method_generic_params }
    } else {
        quote! { 'c, #lifetime_bound }
    };

    let args_with_types: Vec<_> = arg_infos
        .iter()
        .map(|info| {
            let name = info.name;
            let ty = &info.ty_for_params;
            quote! { #name: #ty }
        })
        .collect();

    // Build where clause for chain method (without reply type bounds)
    let chain_where = build_chain_where_clause(&method_where_clause);

    // Generate the trait method signature (declaration only)
    let trait_method = quote! {
        /// Start a chain with this method call.
        fn #chain_method_name<#all_generics>(
            &'c mut self,
            #(#args_with_types),*
        ) -> #crate_path::Result<
            #crate_path::connection::chain::Chain<'c, Self::Socket>
        >
        #chain_where;
    };

    // Validate FD parameters
    let fds_params: Vec<_> = arg_infos.iter().filter(|info| info.is_fds).collect();
    if fds_params.len() > 1 {
        return Err(Error::new_spanned(
            &method.sig,
            "method can have at most one parameter marked with #[zlink(fds)]",
        ));
    }

    // Generate the method call creation code for the implementation
    let ret = generate_method_call_creation(
        &arg_infos,
        &method_ident,
        &method_path,
        &method_generic_params,
        &method_where_clause,
        has_any_lifetime,
        has_explicit_lifetimes,
        method_attrs.is_oneway,
        crate_path,
    );
    #[cfg(feature = "std")]
    let (method_call_creation, fds_init) = ret;
    #[cfg(not(feature = "std"))]
    let method_call_creation = ret;

    #[cfg(feature = "std")]
    let chain_call_args = quote! { &call, #fds_init };
    #[cfg(not(feature = "std"))]
    let chain_call_args = quote! { &call };

    let impl_method = quote! {
        fn #chain_method_name<#all_generics>(
            &'c mut self,
            #(#args_with_types),*
        ) -> #crate_path::Result<
            #crate_path::connection::chain::Chain<'c, Self::Socket>
        >
        #chain_where
        {
            #method_call_creation
            self.chain_call(#chain_call_args)
        }
    };

    Ok((trait_method, impl_method))
}

fn parse_method_arguments<'a>(
    method: &'a mut syn::TraitItemFn,
    has_explicit_lifetimes: bool,
    param_attrs_map: &std::collections::HashMap<String, ParamAttrs>,
    rename_all: Option<crate::naming::RenameAll>,
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
            let param_attrs = param_attrs_map.get(&name.to_string());
            let serialized_name = match resolve_serialized_name(name, param_attrs, rename_all) {
                Ok(serialized_name) => serialized_name,
                Err(err) => return Some(Err(err)),
            };
            let is_fds = param_attrs.map(|attrs| attrs.is_fds).unwrap_or(false);

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
                has_lifetime,
                is_optional: false,
                serialized_name,
                is_fds,
            }))
        })
        .collect()
}

fn build_chain_where_clause(method_where_clause: &Option<syn::WhereClause>) -> TokenStream {
    // Only include method where clause predicates if present
    if let Some(method_where) = method_where_clause {
        quote! { #method_where }
    } else {
        quote! {}
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_method_call_creation(
    arg_infos: &[ArgInfo<'_>],
    method_name: &syn::Ident,
    method_path: &str,
    method_generic_params: &syn::punctuated::Punctuated<syn::GenericParam, syn::Token![,]>,
    method_where_clause: &Option<syn::WhereClause>,
    has_any_lifetime: bool,
    has_explicit_lifetimes: bool,
    is_oneway: bool,
    crate_path: &TokenStream,
) -> MethodCallResult {
    // Separate FD parameters from regular parameters
    let regular_params: Vec<_> = arg_infos.iter().filter(|info| !info.is_fds).collect();
    let regular_arg_names: Vec<_> = regular_params.iter().map(|info| info.name).collect();

    // Generate FD initialization (only for std)
    #[cfg(feature = "std")]
    let fds_params: Vec<_> = arg_infos.iter().filter(|info| info.is_fds).collect();
    #[cfg(feature = "std")]
    let fds_init = if let Some(fd_param) = fds_params.first() {
        let fd_name = fd_param.name;
        quote! { #fd_name }
    } else {
        quote! { ::std::vec::Vec::new() }
    };

    if !regular_params.is_empty() {
        let param_fields: Vec<_> = regular_params
            .iter()
            .map(|info| {
                let name = info.name;
                let ty = &info.ty_for_params;
                let serde_attrs = if let Some(ref renamed) = info.serialized_name {
                    quote! { #[serde(rename = #renamed)] }
                } else {
                    quote! {}
                };
                quote! {
                    #serde_attrs
                    pub #name: #ty
                }
            })
            .collect();

        // Build generics for the structs - combine method generics and lifetime params
        let struct_generics = build_struct_generics(
            method_generic_params,
            has_any_lifetime,
            has_explicit_lifetimes,
        );

        // Build generics without bounds for usage in type paths
        let struct_generics_without_bounds = build_struct_generics_without_bounds(
            method_generic_params,
            has_any_lifetime,
            has_explicit_lifetimes,
        );

        // Build struct where clause adding Serialize and Debug bounds for generic parameters
        let struct_where = build_struct_where_clause(method_generic_params, method_where_clause);

        // Create unique struct names for this method to avoid conflicts
        let params_struct_name = syn::Ident::new(
            &format!(
                "{}Params",
                snake_case_to_pascal_case(&crate::naming::unraw(method_name))
            ),
            method_name.span(),
        );
        let wrapper_enum_name = syn::Ident::new(
            &format!(
                "{}Wrapper",
                snake_case_to_pascal_case(&crate::naming::unraw(method_name))
            ),
            method_name.span(),
        );

        let method_call_creation = quote! {
            #[derive(::serde::Serialize, ::core::fmt::Debug)]
            struct #params_struct_name #struct_generics
            #struct_where
            {
                #(#param_fields,)*
            }

            #[derive(::serde::Serialize, ::core::fmt::Debug)]
            #[serde(tag = "method", content = "parameters")]
            enum #wrapper_enum_name #struct_generics
            #struct_where
            {
                #[serde(rename = #method_path)]
                Method(#params_struct_name #struct_generics_without_bounds),
            }

            let method_call = #wrapper_enum_name::Method(#params_struct_name {
                #(#regular_arg_names,)*
            });
        };

        let call_creation = if is_oneway {
            quote! { let call = #crate_path::Call::new(method_call).set_oneway(true); }
        } else {
            quote! { let call = #crate_path::Call::new(method_call); }
        };

        let method_call_creation = quote! {
            #method_call_creation
            #call_creation
        };

        #[cfg(feature = "std")]
        return (method_call_creation, fds_init);
        #[cfg(not(feature = "std"))]
        return method_call_creation;
    } else {
        // Create unique enum name for this method to avoid conflicts
        let wrapper_enum_name = syn::Ident::new(
            &format!(
                "{}Wrapper",
                snake_case_to_pascal_case(&crate::naming::unraw(method_name))
            ),
            method_name.span(),
        );

        let method_call_creation = quote! {
            #[derive(::serde::Serialize, ::core::fmt::Debug)]
            #[serde(tag = "method")]
            enum #wrapper_enum_name {
                #[serde(rename = #method_path)]
                Method,
            }

            let method_call = #wrapper_enum_name::Method;
        };

        let call_creation = if is_oneway {
            quote! { let call = #crate_path::Call::new(method_call).set_oneway(true); }
        } else {
            quote! { let call = #crate_path::Call::new(method_call); }
        };

        let method_call_creation = quote! {
            #method_call_creation
            #call_creation
        };

        #[cfg(feature = "std")]
        return (method_call_creation, fds_init);
        #[cfg(not(feature = "std"))]
        return method_call_creation;
    }
}

fn build_struct_generics(
    method_generic_params: &syn::punctuated::Punctuated<syn::GenericParam, syn::Token![,]>,
    has_any_lifetime: bool,
    has_explicit_lifetimes: bool,
) -> TokenStream {
    if !method_generic_params.is_empty() {
        if has_any_lifetime && !has_explicit_lifetimes {
            quote! { <'__proxy_params, #method_generic_params> }
        } else {
            quote! { <#method_generic_params> }
        }
    } else if has_any_lifetime && !has_explicit_lifetimes {
        quote! { <'__proxy_params> }
    } else {
        quote! {}
    }
}

fn build_struct_generics_without_bounds(
    method_generic_params: &syn::punctuated::Punctuated<syn::GenericParam, syn::Token![,]>,
    has_any_lifetime: bool,
    has_explicit_lifetimes: bool,
) -> TokenStream {
    if !method_generic_params.is_empty() {
        let generic_names: Vec<_> = method_generic_params
            .iter()
            .map(|param| match param {
                syn::GenericParam::Type(type_param) => {
                    let name = &type_param.ident;
                    quote! { #name }
                }
                syn::GenericParam::Lifetime(lifetime_param) => {
                    let lifetime = &lifetime_param.lifetime;
                    quote! { #lifetime }
                }
                syn::GenericParam::Const(const_param) => {
                    let name = &const_param.ident;
                    quote! { #name }
                }
            })
            .collect();

        if has_any_lifetime && !has_explicit_lifetimes {
            quote! { <'__proxy_params, #(#generic_names),*> }
        } else {
            quote! { <#(#generic_names),*> }
        }
    } else if has_any_lifetime && !has_explicit_lifetimes {
        quote! { <'__proxy_params> }
    } else {
        quote! {}
    }
}

fn build_struct_where_clause(
    method_generic_params: &syn::punctuated::Punctuated<syn::GenericParam, syn::Token![,]>,
    method_where_clause: &Option<syn::WhereClause>,
) -> Option<syn::WhereClause> {
    let mut predicates = syn::punctuated::Punctuated::<syn::WherePredicate, syn::Token![,]>::new();

    // Add Serialize and Debug bounds for all type parameters
    for param in method_generic_params {
        if let syn::GenericParam::Type(type_param) = param {
            let ident = &type_param.ident;
            predicates.push(syn::parse_quote!(#ident: ::serde::Serialize));
            predicates.push(syn::parse_quote!(#ident: ::core::fmt::Debug));
        }
    }

    // Add existing method where clause predicates
    if let Some(method_where) = method_where_clause {
        for predicate in &method_where.predicates {
            predicates.push(predicate.clone());
        }
    }

    if predicates.is_empty() {
        None
    } else {
        Some(syn::WhereClause {
            where_token: syn::token::Where::default(),
            predicates,
        })
    }
}
