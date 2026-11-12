//! What the `Service` derive reads from the struct that it is applied to.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Error, Expr, Fields, Ident, Index, LitStr, Member, Type};

/// A struct that the `Service` derive is applied to.
pub(super) struct Definition {
    /// The name of the struct.
    pub name: Ident,
    /// The path of the zlink crate (defaults to `::zlink`).
    pub crate_path: TokenStream,
    /// The vendor, as `GetInfo` replies it.
    pub vendor: Option<Expr>,
    /// The product, as `GetInfo` replies it.
    pub product: Option<Expr>,
    /// The version, as `GetInfo` replies it.
    pub version: Option<Expr>,
    /// The URL, as `GetInfo` replies it.
    pub url: Option<Expr>,
    /// The field with the state that the interfaces share, if any.
    pub state: Option<Field>,
    /// The fields that are interfaces, in the order of the struct.
    pub interfaces: Vec<Field>,
}

impl Definition {
    pub(super) fn parse(input: &DeriveInput) -> Result<Self, Error> {
        let Data::Struct(data) = &input.data else {
            return Err(Error::new_spanned(
                &input.ident,
                "`#[derive(Service)]` only supports structs",
            ));
        };
        if !input.generics.params.is_empty() {
            return Err(Error::new_spanned(
                &input.generics,
                "`#[derive(Service)]` doesn't support generic structs",
            ));
        }

        let mut definition = Self {
            name: input.ident.clone(),
            crate_path: quote! { ::zlink },
            vendor: None,
            product: None,
            version: None,
            url: None,
            state: None,
            interfaces: Vec::new(),
        };
        definition.parse_struct_attrs(&input.attrs)?;
        definition.parse_fields(&data.fields)?;

        if definition.interfaces.is_empty() {
            return Err(Error::new_spanned(
                &input.ident,
                "a service needs at least one interface field",
            ));
        }

        Ok(definition)
    }

    fn parse_struct_attrs(&mut self, attrs: &[Attribute]) -> Result<(), Error> {
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("zlink")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("crate") {
                    let path: LitStr = meta.value()?.parse()?;
                    self.crate_path = path.parse()?;
                } else if meta.path.is_ident("vendor") {
                    self.vendor = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("product") {
                    self.product = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("version") {
                    self.version = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("url") {
                    self.url = Some(meta.value()?.parse()?);
                } else {
                    return Err(meta.error(
                        "unsupported attribute. Expected: `crate`, `vendor`, `product`, \
                         `version` or `url`",
                    ));
                }

                Ok(())
            })?;
        }

        Ok(())
    }

    fn parse_fields(&mut self, fields: &Fields) -> Result<(), Error> {
        for (index, field) in fields.iter().enumerate() {
            let member = match &field.ident {
                Some(ident) => Member::Named(ident.clone()),
                None => Member::Unnamed(Index::from(index)),
            };
            let is_state = is_state(&field.attrs)?;
            let parsed = Field {
                member,
                ty: field.ty.clone(),
            };

            if !is_state {
                self.interfaces.push(parsed);
            } else if self.state.is_some() {
                return Err(Error::new_spanned(
                    field,
                    "at most one field can be `#[zlink(state)]`",
                ));
            } else {
                self.state = Some(parsed);
            }
        }

        Ok(())
    }
}

/// A field of the struct.
pub(super) struct Field {
    /// How the field is accessed: its name, or its index in a tuple struct.
    pub member: Member,
    pub ty: Type,
}

/// Whether the attributes of a field mark it as the state that the interfaces share.
fn is_state(attrs: &[Attribute]) -> Result<bool, Error> {
    let mut is_state = false;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("zlink")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("state") {
                return Err(meta.error("unsupported attribute. Expected: `state`"));
            }
            is_state = true;

            Ok(())
        })?;
    }

    Ok(is_state)
}
