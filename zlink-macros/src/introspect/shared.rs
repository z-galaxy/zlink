use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DataEnum, Error, Fields, FieldsNamed, FieldsUnnamed};

use crate::{
    naming::{self, RenameAll},
    utils,
};

/// Generate comment objects from a list of comments.
pub(crate) fn generate_comment_objects(
    comments: &[String],
    crate_path: &TokenStream2,
) -> Vec<TokenStream2> {
    comments
        .iter()
        .map(|c| quote! { &#crate_path::idl::Comment::new(#c) })
        .collect()
}

/// Generate field definitions for struct fields.
/// If variant_prefix is provided, it's used to create unique static names for variant
/// fields.
pub(super) fn generate_field_definitions(
    fields: &Fields,
    crate_path: &TokenStream2,
    variant_prefix: Option<&syn::Ident>,
    rename_all: Option<RenameAll>,
) -> Result<(Vec<TokenStream2>, Vec<TokenStream2>), Error> {
    match fields {
        Fields::Named(FieldsNamed { named, .. }) => {
            let mut field_statics = Vec::new();
            let mut field_refs = Vec::new();

            for field in named {
                let field_name = field
                    .ident
                    .as_ref()
                    .ok_or_else(|| Error::new_spanned(field, "Field must have a name"))?;

                let field_type = utils::remove_lifetimes_from_type(&field.ty);
                let field_name_str = naming::field_name(&field.attrs, field_name, rename_all)?;

                let static_name = if let Some(variant_ident) = variant_prefix {
                    quote::format_ident!(
                        "FIELD_{}_{}",
                        naming::unraw(variant_ident).to_uppercase(),
                        naming::unraw(field_name).to_uppercase()
                    )
                } else {
                    quote::format_ident!("FIELD_{}", naming::unraw(field_name).to_uppercase())
                };

                let comments = utils::extract_doc_comments(&field.attrs);
                let comment_objects = generate_comment_objects(&comments, crate_path);

                let field_static = quote! {
                    static #static_name: #crate_path::idl::Field<'static> =
                        #crate_path::idl::Field::new(
                            #field_name_str,
                            <#field_type as #crate_path::introspect::Type>::TYPE,
                            &[#(#comment_objects),*]
                        );
                };

                let field_ref = quote! { &#static_name };

                field_statics.push(field_static);
                field_refs.push(field_ref);
            }

            Ok((field_statics, field_refs))
        }
        Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => Err(Error::new_spanned(
            unnamed,
            "Only named fields are supported",
        )),
        Fields::Unit => {
            // Unit structs have no fields.
            Ok((Vec::new(), Vec::new()))
        }
    }
}

/// Generate enum variant definitions for unit variants only.
pub(super) fn generate_enum_variant_definitions(
    data_enum: &DataEnum,
    crate_path: &TokenStream2,
    rename_all: Option<RenameAll>,
) -> Result<Vec<TokenStream2>, Error> {
    let mut variant_refs = Vec::new();

    for variant in &data_enum.variants {
        // Only support unit variants (no associated data).
        match &variant.fields {
            Fields::Unit => {
                let variant_name =
                    naming::enum_variant_name(&variant.attrs, &variant.ident, rename_all)?;
                let comments = utils::extract_doc_comments(&variant.attrs);
                let comment_objects = generate_comment_objects(&comments, crate_path);
                let variant_ref = quote! {
                    &#crate_path::idl::EnumVariant::new(
                        #variant_name,
                        &[#(#comment_objects),*]
                    )
                };

                variant_refs.push(variant_ref);
            }
            Fields::Named(_) => {
                return Err(Error::new_spanned(
                    variant,
                    "Type derive macro only supports unit enum variants, not struct \
                     variants",
                ));
            }
            Fields::Unnamed(_) => {
                return Err(Error::new_spanned(
                    variant,
                    "Type derive macro only supports unit enum variants, not tuple \
                     variants",
                ));
            }
        }
    }

    Ok(variant_refs)
}
