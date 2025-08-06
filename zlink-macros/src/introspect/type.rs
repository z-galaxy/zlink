use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Fields};

use crate::utils;

use super::shared;

/// Main entry point for the Type derive macro.
pub(crate) fn derive_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_type_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn derive_type_impl(input: DeriveInput) -> Result<TokenStream2, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let crate_path = utils::parse_crate_path(&input.attrs)?;

    let expanded = match &input.data {
        Data::Struct(data_struct) => {
            let fields = &data_struct.fields;
            let (field_statics, field_refs) = generate_field_definitions(fields, &crate_path)?;

            quote! {
                impl #impl_generics #crate_path::introspect::Type for #name #ty_generics #where_clause {
                    const TYPE: &'static #crate_path::idl::Type<'static> = &{
                        #(#field_statics)*

                        static FIELD_REFS: &[&#crate_path::idl::Field<'static>] = &[
                            #(#field_refs),*
                        ];

                        #crate_path::idl::Type::Object(#crate_path::idl::List::Borrowed(FIELD_REFS))
                    };
                }
            }
        }
        Data::Enum(data_enum) => {
            let variant_refs = generate_enum_variant_definitions(data_enum, &crate_path)?;

            quote! {
                impl #impl_generics #crate_path::introspect::Type for #name #ty_generics #where_clause {
                    const TYPE: &'static #crate_path::idl::Type<'static> = &{
                        static VARIANT_REFS: &[&#crate_path::idl::EnumVariant<'static>] = &[
                            #(#variant_refs),*
                        ];

                        #crate_path::idl::Type::Enum(#crate_path::idl::List::Borrowed(VARIANT_REFS))
                    };
                }
            }
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "Type derive macro only supports structs and enums, not unions",
            ));
        }
    };

    Ok(expanded)
}

fn generate_field_definitions(
    fields: &Fields,
    crate_path: &TokenStream2,
) -> Result<(Vec<TokenStream2>, Vec<TokenStream2>), Error> {
    shared::generate_field_definitions(fields, crate_path, None)
}

fn generate_enum_variant_definitions(
    data_enum: &DataEnum,
    crate_path: &TokenStream2,
) -> Result<Vec<TokenStream2>, Error> {
    shared::generate_enum_variant_definitions(data_enum, crate_path)
}
