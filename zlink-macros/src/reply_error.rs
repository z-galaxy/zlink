use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_quote, Data, DataEnum, DeriveInput, Error, Fields, FieldsNamed};

use crate::utils::*;

/// Main entry point for the ReplyError derive macro that generates serde implementations.
///
/// This macro:
/// 1. Generates manual `serde::Serialize` implementation for qualified error names
/// 2. Generates `serde::Deserialize` via helper enum with adjacently tagged format
/// 3. Requires `#[zlink(interface = "...")]` to automatically generate qualified error names
/// 4. Handles unit variants without `parameters` field and named variants with `parameters`
pub(crate) fn derive_reply_error(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);

    let result = derive_reply_error_impl(&ast);

    match result {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn derive_reply_error_impl(input: &DeriveInput) -> Result<TokenStream2, Error> {
    let name = &input.ident;
    let generics = &input.generics;

    // Parse the interface from zlink attributes (mandatory).
    let interface = parse_interface_from_attrs(&input.attrs)?;

    let data_enum = extract_enum_data(&input.data)?;

    // Validate that enum variants are supported.
    validate_enum_variants(data_enum)?;

    // Generate manual Serialize implementation (still custom for variant naming).
    let serialize_impl = generate_serialize_impl(name, data_enum, generics, &interface)?;

    // For Deserialize, generate a helper enum with serde derives and convert to original.
    let deserialize_impl = generate_deserialize_with_derive(input, data_enum, &interface)?;

    Ok(quote! {
        #serialize_impl
        #deserialize_impl
    })
}

/// Parse interface attribute from #[zlink(interface = "...")].
fn parse_interface_from_attrs(attrs: &[syn::Attribute]) -> Result<String, Error> {
    for attr in attrs {
        if !attr.path().is_ident("zlink") {
            continue;
        }

        let mut interface_result = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("interface") {
                let value = meta.value()?;
                let lit_str: syn::LitStr = value.parse()?;
                interface_result = Some(lit_str.value());
            } else {
                // Skip unknown attributes by consuming their values.
                let _ = meta.value()?;
                let _: syn::Expr = meta.input.parse()?;
            }
            Ok(())
        })?;

        if let Some(interface) = interface_result {
            return Ok(interface);
        }
    }
    Err(Error::new(
        proc_macro2::Span::call_site(),
        "ReplyError macro requires #[zlink(interface = \"...\")] attribute",
    ))
}

/// Validate that enum variants are supported by the ReplyError derive macro.
fn validate_enum_variants(data_enum: &DataEnum) -> Result<(), Error> {
    for variant in &data_enum.variants {
        match &variant.fields {
            Fields::Unit | Fields::Named(_) => {
                // Unit variants and named field variants are fine.
            }
            Fields::Unnamed(_) => {
                return Err(Error::new_spanned(
                    variant,
                    "ReplyError derive macro does not support tuple variants",
                ));
            }
        }
    }
    Ok(())
}

/// Check if a given Data is an enum and extract it.
fn extract_enum_data(data: &Data) -> Result<&DataEnum, Error> {
    match data {
        Data::Enum(data_enum) => Ok(data_enum),
        Data::Struct(_) => Err(Error::new(
            proc_macro2::Span::call_site(),
            "ReplyError derive macro only supports enums, not structs",
        )),
        Data::Union(_) => Err(Error::new(
            proc_macro2::Span::call_site(),
            "ReplyError derive macro only supports enums, not unions",
        )),
    }
}

fn generate_serialize_impl(
    name: &syn::Ident,
    data_enum: &DataEnum,
    generics: &syn::Generics,
    interface: &str,
) -> Result<TokenStream2, Error> {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let has_lifetimes = !generics.lifetimes().collect::<Vec<_>>().is_empty();

    // Generate match arms for each variant (empty for empty enums).
    let variant_arms = data_enum
        .variants
        .iter()
        .map(|variant| generate_serialize_variant_arm(variant, interface, has_lifetimes))
        .collect::<Result<Vec<_>, _>>()?;

    // For empty enums, we need to dereference self to match the uninhabited type.
    let match_expr = if data_enum.variants.is_empty() {
        quote! { *self }
    } else {
        quote! { self }
    };

    Ok(quote! {
        impl #impl_generics serde::Serialize for #name #ty_generics #where_clause {
            fn serialize<S>(&self, #[allow(unused)] serializer: S) -> core::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                match #match_expr {
                    #(#variant_arms)*
                }
            }
        }
    })
}

fn generate_serialize_variant_arm(
    variant: &syn::Variant,
    interface: &str,
    has_lifetimes: bool,
) -> Result<TokenStream2, Error> {
    let variant_name = &variant.ident;
    let qualified_name = format!("{interface}.{variant_name}");

    match &variant.fields {
        // Unit variant - serialize as tagged enum with just error field.
        Fields::Unit => Ok(quote! {
            Self::#variant_name => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("error", #qualified_name)?;
                map.end()
            }
        }),
        Fields::Named(fields) => {
            // Named fields - serialize as tagged enum with parameters.
            let field_info = FieldInfo::extract(fields);
            let field_count = field_info.names.len();
            let field_names = &field_info.names;
            let field_types = &field_info.types;
            let field_name_strs = &field_info.name_strings;

            // Convert field types to use synthetic lifetime for ParametersSerializer
            // only if enum has lifetimes.
            let serializer_field_types: Vec<syn::Type> = if has_lifetimes {
                field_types
                    .iter()
                    .map(|ty| convert_type_lifetimes(ty, "'__param"))
                    .collect()
            } else {
                field_types.iter().map(|&ty| ty.clone()).collect()
            };

            Ok(quote! {
                Self::#variant_name { #(#field_names,)* } => {
                    use serde::ser::SerializeMap;

                    let mut map = serializer.serialize_map(Some(2))?;
                    map.serialize_entry("error", #qualified_name)?;

                    // Create a nested "parameters" object.
                    map.serialize_entry("parameters", &{
                        use serde::ser::SerializeMap;
                        struct ParametersSerializer<'__param> {
                            #(#field_names: &'__param #serializer_field_types,)*
                        }

                        impl<'__param> serde::Serialize for ParametersSerializer<'__param> {
                            fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
                            where
                                S: serde::Serializer,
                            {
                                let mut map = serializer.serialize_map(Some(#field_count))?;
                                #(
                                    map.serialize_entry(#field_name_strs, self.#field_names)?;
                                )*
                                map.end()
                            }
                        }

                        ParametersSerializer {
                            #(#field_names,)*
                        }
                    })?;

                    map.end()
                }
            })
        }
        Fields::Unnamed(_) => Err(Error::new_spanned(
            variant,
            "ReplyError derive macro does not support tuple variants",
        )),
    }
}

/// Generate Deserialize implementation using a helper enum with serde derives.
fn generate_deserialize_with_derive(
    input: &DeriveInput,
    data_enum: &DataEnum,
    interface: &str,
) -> Result<TokenStream2, Error> {
    let name = &input.ident;
    let generics = &input.generics;

    // First extract field info for all variants from the original enum.
    let variant_field_info: Vec<_> = data_enum
        .variants
        .iter()
        .map(|variant| {
            if let Fields::Named(fields) = &variant.fields {
                Some(FieldInfo::extract(fields))
            } else {
                None
            }
        })
        .collect();

    // Clone and modify the enum to add serde attributes.
    let mut modified_enum = data_enum.clone();

    // Now modify the variants.
    for (i, variant) in modified_enum.variants.iter_mut().enumerate() {
        let field_info = &variant_field_info[i];
        let variant_name = &variant.ident;
        let qualified_name = format!("{interface}.{variant_name}");

        // Add rename attribute for the variant.
        variant
            .attrs
            .push(parse_quote!(#[serde(rename = #qualified_name)]));

        // Add serde rename attributes to fields based on their serialized names.
        if let (Fields::Named(fields), Some(field_info)) = (&mut variant.fields, field_info) {
            for (field, name_str) in fields.named.iter_mut().zip(&field_info.name_strings) {
                // Remove zlink attributes and add serde rename with the serialized name.
                field.attrs.retain(|attr| !attr.path().is_ident("zlink"));
                field.attrs.push(parse_quote!(#[serde(rename = #name_str)]));
            }
        }
    }

    // Add 'de lifetime for Deserialize.
    let mut impl_generics = generics.clone();
    impl_generics.params.insert(0, syn::parse_quote!('de));

    let has_lifetimes = !generics.lifetimes().collect::<Vec<_>>().is_empty();
    if has_lifetimes {
        let enum_lifetimes: Vec<_> = generics.lifetimes().collect();
        for lifetime in &enum_lifetimes {
            let lifetime_ident = &lifetime.lifetime;
            impl_generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!('de: #lifetime_ident));
        }
    }

    let (impl_generics_tokens, _, impl_where_clause) = impl_generics.split_for_impl();
    let (orig_impl_generics, ty_generics, orig_where_clause) = generics.split_for_impl();

    let variants = &modified_enum.variants;

    // Generate conversion match arms from helper to original enum.
    let conversion_arms: Vec<_> = data_enum
        .variants
        .iter()
        .map(|variant| {
            let variant_name = &variant.ident;
            match &variant.fields {
                Fields::Unit => quote! {
                    __ZlinkDeserHelper::#variant_name => #name::#variant_name
                },
                Fields::Named(fields) => {
                    let field_names: Vec<_> = fields
                        .named
                        .iter()
                        .filter_map(|f| f.ident.as_ref())
                        .collect();
                    quote! {
                        __ZlinkDeserHelper::#variant_name { #(#field_names),* } =>
                            #name::#variant_name { #(#field_names),* }
                    }
                }
                Fields::Unnamed(_) => {
                    // Already validated that tuple variants are not supported.
                    unreachable!()
                }
            }
        })
        .collect();

    Ok(quote! {
        // Implement Deserialize using a modified version of the enum with serde attributes.
        #[allow(unreachable_code)]
        impl #impl_generics_tokens serde::Deserialize<'de> for #name #ty_generics #impl_where_clause {
            fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                #[derive(serde::Deserialize)]
                #[serde(tag = "error", content = "parameters")]
                enum __ZlinkDeserHelper #orig_impl_generics #orig_where_clause {
                    #variants
                }

                let helper = __ZlinkDeserHelper::deserialize(deserializer)?;

                // Convert from helper to original enum.
                Ok(match helper {
                    #(#conversion_arms),*
                })
            }
        }
    })
}

/// Field information extracted from named fields for reuse across
/// serialization/deserialization.
struct FieldInfo<'a> {
    names: Vec<&'a syn::Ident>,
    types: Vec<&'a syn::Type>,
    name_strings: Vec<String>,
}

impl<'a> FieldInfo<'a> {
    /// Extract field information from named fields to avoid duplication.
    fn extract(fields: &'a FieldsNamed) -> Self {
        let field_data: Vec<_> = fields
            .named
            .iter()
            .filter_map(|f| {
                f.ident.as_ref().map(|name| {
                    let serialized_name = Self::get_serialized_name(f, name);
                    (name, &f.ty, serialized_name)
                })
            })
            .collect();

        let names: Vec<_> = field_data.iter().map(|(name, _, _)| *name).collect();
        let types: Vec<_> = field_data.iter().map(|(_, ty, _)| *ty).collect();
        let name_strings: Vec<String> = field_data
            .iter()
            .map(|(_, _, sname)| sname.clone())
            .collect();

        Self {
            names,
            types,
            name_strings,
        }
    }

    /// Extract the serialized name from field attributes or use the field name.
    fn get_serialized_name(field: &syn::Field, default_name: &syn::Ident) -> String {
        parse_zlink_string_attr(&field.attrs, "rename").unwrap_or_else(|| default_name.to_string())
    }
}
