use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Fields};
use zlink_names::TypeName;

use crate::{
    naming::{self, RenameAll},
    utils,
};

use super::shared;

/// Main entry point for the custom Type derive macro.
pub(crate) fn derive_custom_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);

    match derive_custom_type_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn derive_custom_type_impl(input: DeriveInput) -> Result<TokenStream2, Error> {
    let name = &input.ident;
    // Not `naming::resolve`: a container has no `rename_all` of its own to apply to itself, only
    // one it hands down to its fields. The name still has to be one Varlink can say.
    let rename_lit = naming::parse_rename(&input.attrs)?;
    let name_string = match &rename_lit {
        Some(lit) => lit.value(),
        None => naming::unraw(name),
    };
    let name_source = match &rename_lit {
        Some(lit) => naming::NameSource::Rename(lit),
        None => naming::NameSource::Ident(name),
    };
    let type_name: TypeName<'_> = naming::validate(&name_string, "type name", name_source)?;
    let name_str = type_name.as_str();
    let rename_all = naming::RenameAllParser::new(&input.attrs).try_for_field_name()?;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let crate_path = utils::parse_crate_path(&input.attrs)?;

    let type_comments = utils::extract_doc_comments(&input.attrs);
    let type_comment_objects = shared::generate_comment_objects(&type_comments, &crate_path);

    let custom_type = match &input.data {
        Data::Struct(data_struct) => {
            let fields = &data_struct.fields;
            let (field_statics, field_refs_init) =
                generate_field_definitions(fields, &crate_path, rename_all)?;

            quote!({
                #(#field_statics)*

                static FIELD_REFS: &[&#crate_path::idl::Field<'static>] = #field_refs_init;

                #crate_path::idl::CustomType::Object(
                    #crate_path::idl::CustomObject::new(#name_str, FIELD_REFS, &[#(#type_comment_objects),*])
                )
            })
        }
        Data::Enum(data_enum) => {
            let variant_refs =
                generate_enum_variant_definitions(data_enum, &crate_path, rename_all)?;

            quote!({
                static VARIANT_REFS: &[&#crate_path::idl::EnumVariant<'static>] = &[
                    #(#variant_refs),*
                ];

                #crate_path::idl::CustomType::Enum(
                   #crate_path::idl::CustomEnum::new(#name_str, VARIANT_REFS, &[#(#type_comment_objects),*])
                )
            })
        }
        Data::Union(_) => {
            return Err(Error::new_spanned(
                input,
                "Type derive macro only supports structs and enums, not unions",
            ));
        }
    };

    Ok(quote! {
        impl #impl_generics #crate_path::introspect::CustomType for #name #ty_generics #where_clause {
            const CUSTOM_TYPE: &'static #crate_path::idl::CustomType<'static> = &#custom_type;
        }

        impl #impl_generics #crate_path::introspect::Type for #name #ty_generics #where_clause {
            const TYPE: &'static #crate_path::idl::Type<'static> = &#crate_path::idl::Type::Custom(#name_str);
        }
    })
}

fn generate_field_definitions(
    fields: &Fields,
    crate_path: &TokenStream2,
    rename_all: Option<RenameAll>,
) -> Result<(Vec<TokenStream2>, TokenStream2), Error> {
    shared::generate_field_definitions(fields, crate_path, None, rename_all)
}

fn generate_enum_variant_definitions(
    data_enum: &DataEnum,
    crate_path: &TokenStream2,
    rename_all: Option<RenameAll>,
) -> Result<Vec<TokenStream2>, Error> {
    shared::generate_enum_variant_definitions(data_enum, crate_path, rename_all)
}
