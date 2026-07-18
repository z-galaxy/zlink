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
///
/// If variant_prefix is provided, it's used to create unique static names for variant fields.
///
/// Returns the field `static` definitions and an expression of type
/// `&'static [&idl::Field<'static>]` — the RHS of the `static FIELD_REFS` the caller declares.
pub(super) fn generate_field_definitions(
    fields: &Fields,
    crate_path: &TokenStream2,
    variant_prefix: Option<&syn::Ident>,
    rename_all: Option<RenameAll>,
) -> Result<(Vec<TokenStream2>, TokenStream2), Error> {
    let named = match fields {
        Fields::Named(FieldsNamed { named, .. }) => named,
        Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
            return Err(Error::new_spanned(
                unnamed,
                "Only named fields are supported",
            ));
        }
        // Unit structs have no fields.
        Fields::Unit => return Ok((Vec::new(), quote! { &[] })),
    };

    let mut field_statics = Vec::new();
    // Simple `&STATIC` refs for the common (no-flatten) path, kept byte-identical to today.
    let mut simple_refs = Vec::new();
    // Each group is an expression of type `&[&Field<'static>]`: a singleton for a normal field, or
    // the inner object's fields for a flattened one. Only used when at least one field is
    // flattened.
    let mut groups = Vec::new();
    let mut any_flatten = false;

    for field in named {
        let field_name = field
            .ident
            .as_ref()
            .ok_or_else(|| Error::new_spanned(field, "Field must have a name"))?;

        match crate::attr_mode::field_mode(&field.attrs, field_name)? {
            crate::attr_mode::FieldMode::Skip => continue,
            crate::attr_mode::FieldMode::Flatten => {
                any_flatten = true;
                let field_type = utils::remove_lifetimes_from_type(&field.ty);
                // Specific, field-named compile error when the target is not an inline object.
                // Fires at const-eval alongside `object_fields`'s generic panic;
                // this one names the field.
                let msg = format!(
                    "#[zlink(flatten)] on field `{}`: its type must be an inline object \
                     type (a `#[derive(Type)]` struct); named custom types and scalars \
                     are not supported",
                    naming::unraw(field_name),
                );
                field_statics.push(quote! {
                    const _: () = {
                        if <#field_type as #crate_path::introspect::Type>::TYPE
                            .as_object()
                            .is_none()
                        {
                            ::core::panic!(#msg);
                        }
                    };
                });
                groups.push(quote! {
                    #crate_path::idl::flatten::object_fields(
                        <#field_type as #crate_path::introspect::Type>::TYPE
                    )
                });
            }
            crate::attr_mode::FieldMode::Normal => {
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

                field_statics.push(quote! {
                    static #static_name: #crate_path::idl::Field<'static> =
                        #crate_path::idl::Field::new(
                            #field_name_str,
                            <#field_type as #crate_path::introspect::Type>::TYPE,
                            &[#(#comment_objects),*]
                        );
                });
                simple_refs.push(quote! { &#static_name });
                groups.push(quote! { &[&#static_name] });
            }
        }
    }

    // No flatten: emit the plain `&[&FIELD_A, &FIELD_B, …]` slice exactly as before.
    if !any_flatten {
        return Ok((field_statics, quote! { &[ #(#simple_refs),* ] }));
    }

    let init = quote! {
        {
            const GROUPS: &[&[&#crate_path::idl::Field<'static>]] = &[ #(#groups),* ];
            const __N: usize = #crate_path::idl::flatten::total_field_count(GROUPS);
            static __FIELD_REFS: [&#crate_path::idl::Field<'static>; __N] =
                #crate_path::idl::flatten::concat_fields::<__N>(GROUPS);
            &__FIELD_REFS
        }
    };
    Ok((field_statics, init))
}

/// Generate enum variant definitions for unit variants only.
pub(super) fn generate_enum_variant_definitions(
    data_enum: &DataEnum,
    crate_path: &TokenStream2,
    rename_all: Option<RenameAll>,
) -> Result<Vec<TokenStream2>, Error> {
    let mut variant_refs = Vec::new();

    for variant in &data_enum.variants {
        match crate::attr_mode::field_mode(&variant.attrs, &variant.ident)? {
            crate::attr_mode::FieldMode::Skip => continue,
            crate::attr_mode::FieldMode::Flatten => {
                return Err(Error::new_spanned(
                    &variant.ident,
                    "`#[zlink(flatten)]` is not supported on enum variants",
                ));
            }
            crate::attr_mode::FieldMode::Normal => {}
        }

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
