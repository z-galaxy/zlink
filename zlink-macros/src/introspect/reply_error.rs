use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Fields};

use crate::utils;

use super::shared;

/// Main entry point for the ReplyError derive macro.
pub(crate) fn derive_reply_error(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_reply_error_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn derive_reply_error_impl(input: DeriveInput) -> Result<TokenStream2, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let crate_path = utils::parse_crate_path(&input.attrs)?;

    let expanded = match &input.data {
        Data::Enum(data_enum) => {
            let error_variants = generate_error_definitions(data_enum, &crate_path)?;

            quote! {
                impl #impl_generics #crate_path::introspect::ReplyError for #name #ty_generics #where_clause {
                    const VARIANTS: &'static [&'static #crate_path::idl::Error<'static>] = &[
                        #(#error_variants),*
                    ];
                }
            }
        }
        Data::Struct(_) => {
            return Err(Error::new_spanned(
                input,
                "ReplyError derive macro only supports enums, not structs",
            ));
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "ReplyError derive macro only supports enums, not unions",
            ));
        }
    };

    Ok(expanded)
}

fn generate_error_definitions(
    data_enum: &DataEnum,
    crate_path: &TokenStream2,
) -> Result<Vec<TokenStream2>, Error> {
    let mut error_variants = Vec::new();

    for variant in &data_enum.variants {
        let variant_name = variant.ident.to_string();

        match &variant.fields {
            Fields::Unit => {
                let comments = utils::extract_doc_comments(&variant.attrs);
                let comment_objects = shared::generate_comment_objects(&comments, crate_path);
                let error_variant = quote! {
                    &#crate_path::idl::Error::new(#variant_name, &[], &[#(#comment_objects),*])
                };
                error_variants.push(error_variant);
            }
            Fields::Named(fields) => {
                let (field_statics, field_refs) = shared::generate_field_definitions(
                    &Fields::Named(fields.clone()),
                    crate_path,
                    Some(&variant.ident),
                )?;

                let comments = utils::extract_doc_comments(&variant.attrs);
                let comment_objects = shared::generate_comment_objects(&comments, crate_path);

                let error_variant = quote! {
                    &{
                        #(#field_statics)*

                        static FIELD_REFS: &[&#crate_path::idl::Field<'static>] = &[
                            #(#field_refs),*
                        ];

                        #crate_path::idl::Error::new(#variant_name, FIELD_REFS, &[#(#comment_objects),*])
                    }
                };
                error_variants.push(error_variant);
            }
            Fields::Unnamed(fields) => {
                if fields.unnamed.len() != 1 {
                    return Err(Error::new_spanned(
                        variant,
                        "ReplyError derive macro only supports tuple variants with exactly one field",
                    ));
                }

                let field_type =
                    utils::remove_lifetimes_from_type(&fields.unnamed.first().unwrap().ty);
                let comments = utils::extract_doc_comments(&variant.attrs);
                let comment_objects = shared::generate_comment_objects(&comments, crate_path);
                let error_variant = quote! {
                    &{
                        match <#field_type as #crate_path::introspect::Type>::TYPE {
                            #crate_path::idl::Type::Object(fields) => {
                                let #crate_path::idl::List::Borrowed(field_slice) = fields else {
                                    panic!("Owned List not supported in const context")
                                };
                                #crate_path::idl::Error::new(#variant_name, field_slice, &[#(#comment_objects),*])
                            }
                            _ => panic!("Tuple variant field type must have Type::Object"),
                        }
                    }
                };
                error_variants.push(error_variant);
            }
        }
    }

    Ok(error_variants)
}
