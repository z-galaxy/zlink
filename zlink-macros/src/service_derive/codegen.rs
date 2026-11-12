//! Code generation of the `Service` derive.
//!
//! The generated code combines the interfaces that are the fields of the struct into one service.
//! The hidden types it defines are named after the struct and mirror the associated types of
//! `Service`, each with a variant for the `org.varlink.service` interface where it matters, and
//! one for each field. The ones that are serialized or deserialized do so as the value they hold
//! does, without a tag of their own.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Ident, Lifetime, Member};

use super::attrs::{Definition, Field};

pub(super) fn generate(definition: &Definition) -> TokenStream {
    let codegen = Codegen::new(definition);
    let method_call = codegen.method_call();
    let reply_params = codegen.reply_params();
    let reply_error = codegen.reply_error();
    let reply_stream_params = codegen.reply_stream_params();
    let reply_stream_error = codegen.reply_stream_error();
    let reply_stream = codegen.reply_stream();
    let upgrade = codegen.upgrade();
    let service = codegen.service();

    quote! {
        #method_call
        #reply_params
        #reply_error
        #reply_stream_params
        #reply_stream_error
        #reply_stream
        #upgrade
        #service
    }
}

/// The names and pieces that the generated code is made of.
struct Codegen<'a> {
    definition: &'a Definition,
    /// The path of the zlink crate.
    krate: &'a TokenStream,
    /// The type parameter of the generated types and of the `Service` implementation.
    sock: Ident,
    /// The type of the state that the interfaces share.
    state_ty: TokenStream,
    /// The bounds that the generated code needs: one on the socket and one for each interface.
    predicates: TokenStream,
    /// The attributes of the generated types.
    type_attrs: TokenStream,
    method_call: Ident,
    reply_params: Ident,
    reply_error: Ident,
    reply_stream_params: Ident,
    reply_stream_error: Ident,
    reply_stream: Ident,
    upgrade: Ident,
}

impl<'a> Codegen<'a> {
    fn new(definition: &'a Definition) -> Self {
        let krate = &definition.crate_path;
        let name = &definition.name;
        let sock = format_ident!("__ZlinkSock");
        let state_ty = match &definition.state {
            Some(state) => {
                let ty = &state.ty;
                quote! { #ty }
            }
            None => quote! { () },
        };
        let interface_predicates = definition.interfaces.iter().map(|field| {
            let ty = &field.ty;
            quote! { #ty: #krate::service::Interface<#sock, #state_ty>, }
        });
        let predicates = quote! {
            #sock: #krate::connection::Socket,
            #(#interface_predicates)*
        };
        // The types are public so that they can be the associated types of a public service, but
        // they are not meant to be named, and the fields of the service may be less visible.
        let type_attrs = quote! {
            #[doc(hidden)]
            #[allow(
                private_interfaces,
                private_bounds,
                unreachable_pub,
                missing_docs,
                missing_debug_implementations
            )]
        };

        Self {
            definition,
            krate,
            sock,
            state_ty,
            predicates,
            type_attrs,
            method_call: format_ident!("__{}MethodCall", name),
            reply_params: format_ident!("__{}ReplyParams", name),
            reply_error: format_ident!("__{}ReplyError", name),
            reply_stream_params: format_ident!("__{}ReplyStreamParams", name),
            reply_stream_error: format_ident!("__{}ReplyStreamError", name),
            reply_stream: format_ident!("__{}ReplyStream", name),
            upgrade: format_ident!("__{}Upgrade", name),
        }
    }

    /// The type of method call: what the interfaces handle, plus `org.varlink.service`'s methods
    /// and the method of a call that no interface has.
    fn method_call(&self) -> TokenStream {
        let krate = self.krate;
        let name = &self.method_call;
        let lt = Some("'de");
        let mut variants = vec![(
            varlink_variant(),
            quote! { #krate::varlink_service::Method<'de> },
        )];
        variants.extend(self.field_variants(quote! { MethodCall<'de> }));
        variants.push((unknown_variant(), quote! { ::std::borrow::Cow<'de, str> }));

        let definition = self.enum_definition(name, lt, &variants);
        let debug = self.debug_impl(name, lt, &variants);
        let deserialize = self.deserialize_impl();
        let distinct_interfaces = self.distinct_interfaces();

        quote! {
            #definition
            #debug
            #deserialize
            #distinct_interfaces
        }
    }

    fn reply_params(&self) -> TokenStream {
        let krate = self.krate;
        let lt = Some("'ser");
        let mut variants = vec![(
            varlink_variant(),
            quote! { #krate::varlink_service::Reply<'ser> },
        )];
        variants.extend(self.field_variants(quote! { ReplyParams<'ser> }));

        self.serialized_enum(&self.reply_params, lt, &variants)
    }

    fn reply_error(&self) -> TokenStream {
        let krate = self.krate;
        let lt = Some("'ser");
        let mut variants = vec![(
            varlink_variant(),
            quote! { #krate::varlink_service::Error<'ser> },
        )];
        variants.extend(self.field_variants(quote! { ReplyError<'ser> }));

        self.serialized_enum(&self.reply_error, lt, &variants)
    }

    fn reply_stream_params(&self) -> TokenStream {
        let variants = self.field_variants(quote! { ReplyStreamParams });

        self.serialized_enum(&self.reply_stream_params, None, &variants)
    }

    fn reply_stream_error(&self) -> TokenStream {
        let variants = self.field_variants(quote! { ReplyStreamError });

        self.serialized_enum(&self.reply_stream_error, None, &variants)
    }

    /// The type of the stream: one of the streams of the interfaces, whose items it maps.
    fn reply_stream(&self) -> TokenStream {
        let krate = self.krate;
        let name = &self.reply_stream;
        let item_params = &self.reply_stream_params;
        let item_error = &self.reply_stream_error;
        let sock = &self.sock;
        let predicates = &self.predicates;
        let variants = self.field_variants(quote! { ReplyStream });
        let definition = self.enum_definition(name, None, &variants);
        let arms = variants.iter().map(|(variant, _)| {
            let map_item = map_stream_item(
                quote! { #item_params::#variant },
                quote! {
                    #item_error::#variant
                },
            );
            quote! {
                Self::#variant(__stream) => {
                    ::core::pin::Pin::new(__stream).poll_next(__cx).map(|__item| {
                        __item.map(#map_item)
                    })
                }
            }
        });

        quote! {
            #definition

            impl<#sock> #krate::futures_util::Stream for #name<#sock>
            where
                #predicates
            {
                type Item = #krate::service::ReplyStreamItem<
                    #item_params<#sock>,
                    #item_error<#sock>,
                >;

                fn poll_next(
                    self: ::core::pin::Pin<&mut Self>,
                    __cx: &mut ::core::task::Context<'_>,
                ) -> ::core::task::Poll<::core::option::Option<Self::Item>> {
                    use #krate::futures_util::Stream as _;

                    match ::core::pin::Pin::get_mut(self) {
                        #(#arms)*
                    }
                }
            }
        }
    }

    /// The type of the state that takes over an upgraded connection: the state of the interface
    /// that confirmed the upgrade.
    fn upgrade(&self) -> TokenStream {
        let krate = self.krate;
        let name = &self.upgrade;
        let sock = &self.sock;
        let predicates = &self.predicates;
        let variants = self.field_variants(quote! { Upgrade });
        let definition = self.enum_definition(name, None, &variants);
        let arms = variants.iter().map(|(variant, _)| {
            quote! {
                Self::#variant(__state) => {
                    #krate::service::TakeOver::<#sock>::take_over(__state, __socket).await
                }
            }
        });

        quote! {
            #definition

            impl<#sock> #krate::service::TakeOver<#sock> for #name<#sock>
            where
                #predicates
            {
                async fn take_over(self, __socket: #krate::service::Upgraded<#sock>) {
                    match self {
                        #(#arms)*
                    }
                }
            }
        }
    }

    /// The `Service` implementation of the struct.
    fn service(&self) -> TokenStream {
        let krate = self.krate;
        let name = &self.definition.name;
        let sock = &self.sock;
        let predicates = &self.predicates;
        let method_call = &self.method_call;
        let reply_params = &self.reply_params;
        let reply_error = &self.reply_error;
        let reply_stream_params = &self.reply_stream_params;
        let reply_stream_error = &self.reply_stream_error;
        let reply_stream = &self.reply_stream;
        let upgrade = &self.upgrade;
        let body = self.handle_body();
        #[cfg(feature = "std")]
        let fds_param = quote! { __zlink_fds: ::std::vec::Vec<::std::os::fd::OwnedFd>, };
        #[cfg(not(feature = "std"))]
        let fds_param = quote! {};

        quote! {
            impl<#sock> #krate::service::Service<#sock> for #name
            where
                #predicates
            {
                type MethodCall<'de> = #method_call<'de, #sock>;
                type ReplyParams<'ser> = #reply_params<'ser, #sock> where Self: 'ser;
                type ReplyStreamParams = #reply_stream_params<#sock>;
                type ReplyStreamError = #reply_stream_error<#sock>;
                type ReplyStream = #reply_stream<#sock>;
                type ReplyError<'ser> = #reply_error<'ser, #sock> where Self: 'ser;
                type Upgrade = #upgrade<#sock>;

                async fn handle<'__zlink_ser>(
                    &'__zlink_ser mut self,
                    __zlink_call: &'__zlink_ser #krate::Call<Self::MethodCall<'_>>,
                    __zlink_conn: &mut #krate::Connection<#sock>,
                    #fds_param
                ) -> #krate::service::HandleResult<
                    Self::ReplyParams<'__zlink_ser>,
                    Self::ReplyStream,
                    Self::ReplyError<'__zlink_ser>,
                    Self::Upgrade,
                > {
                    #body
                }
            }
        }
    }

    /// The body of `Service::handle`.
    fn handle_body(&self) -> TokenStream {
        let krate = self.krate;
        let method_call = &self.method_call;
        let varlink = varlink_variant();
        let unknown = unknown_variant();
        let field_arms = self
            .definition
            .interfaces
            .iter()
            .enumerate()
            .map(|(index, field)| self.field_arm(index, field));
        // A successful reply confirms an upgrade (see `Call::upgrade`), so the calls asking for
        // one are refused by the methods that don't support upgrades.
        let refuse_upgrade = self.varlink_error(quote! {
            #krate::varlink_service::Error::InvalidParameter {
                parameter: ::core::convert::From::from("upgrade"),
            }
        });
        let varlink_replies = self.varlink_replies();
        let method_not_found = self.varlink_error(quote! {
            #krate::varlink_service::Error::MethodNotFound {
                method: ::std::borrow::Cow::Borrowed(&**__method),
            }
        });

        let sock = &self.sock;

        quote! {
            // Evaluating the constant is what runs the check of the names of the interfaces.
            let () = <#method_call<'static, #sock>>::__DISTINCT_INTERFACES;

            match __zlink_call.method() {
                #(#field_arms)*
                #method_call::#varlink(_) if __zlink_call.upgrade() => {
                    #refuse_upgrade
                }
                #method_call::#varlink(__method) => {
                    #varlink_replies
                }
                #method_call::#unknown(__method) => {
                    #method_not_found
                }
            }
        }
    }

    /// The arm that hands a call over to the interface of a field.
    fn field_arm(&self, index: usize, field: &Field) -> TokenStream {
        let krate = self.krate;
        let sock = &self.sock;
        let state_ty = &self.state_ty;
        let method_call = &self.method_call;
        let variant = field_variant(index);
        let member = &field.member;
        let ty = &field.ty;
        let state = match &self.definition.state {
            Some(state) => {
                let member = &state.member;
                quote! { &mut self.#member }
            }
            None => quote! { #krate::service::dispatch::no_state() },
        };
        #[cfg(feature = "std")]
        let fds_arg = quote! { __zlink_fds, };
        #[cfg(not(feature = "std"))]
        let fds_arg = quote! {};
        let lift = self.lift(&variant);

        quote! {
            #method_call::#variant(__method) => {
                let __result = <#ty as #krate::service::Interface<#sock, #state_ty>>::handle(
                    &mut self.#member,
                    #state,
                    #krate::Call::new(__method)
                        .set_oneway(__zlink_call.oneway())
                        .set_more(__zlink_call.more())
                        .set_upgrade(__zlink_call.upgrade()),
                    __zlink_conn,
                    #fds_arg
                )
                .await;

                #lift
            }
        }
    }

    /// The code that turns the result of an interface into that of the service.
    #[cfg(feature = "std")]
    fn lift(&self, variant: &Ident) -> TokenStream {
        let lifted = self.lift_reply(variant);

        quote! {
            let (__reply, __fds) = __result;

            (#lifted, __fds)
        }
    }

    /// The code that turns the result of an interface into that of the service.
    #[cfg(not(feature = "std"))]
    fn lift(&self, variant: &Ident) -> TokenStream {
        let lifted = self.lift_reply(variant);

        quote! {
            let __reply = __result;

            #lifted
        }
    }

    /// The expression that turns the method reply `__reply` of an interface into that of the
    /// service.
    fn lift_reply(&self, variant: &Ident) -> TokenStream {
        let krate = self.krate;
        let reply_params = &self.reply_params;
        let reply_error = &self.reply_error;
        let reply_stream = &self.reply_stream;
        let upgrade = &self.upgrade;

        quote! {
            match __reply {
                #krate::service::MethodReply::Single(__params) => {
                    #krate::service::MethodReply::Single(__params.map(#reply_params::#variant))
                }
                #krate::service::MethodReply::Error(__error) => {
                    #krate::service::MethodReply::Error(#reply_error::#variant(__error))
                }
                #krate::service::MethodReply::Multi(__stream) => {
                    #krate::service::MethodReply::Multi(#reply_stream::#variant(__stream))
                }
                #krate::service::MethodReply::Upgrade(__params, __state) => {
                    #krate::service::MethodReply::Upgrade(
                        __params.map(#reply_params::#variant),
                        #upgrade::#variant(__state),
                    )
                }
                // `MethodReply` may get more variants, which the service can't forward.
                _ => unreachable!("an interface replied in a way that the service can't forward"),
            }
        }
    }

    /// The code that replies to the method of `org.varlink.service` that is `__method`.
    fn varlink_replies(&self) -> TokenStream {
        let krate = self.krate;
        let reply_params = &self.reply_params;
        let varlink = varlink_variant();
        let metadata = |expr: &Option<syn::Expr>| match expr {
            Some(expr) => quote! { #expr },
            None => quote! { "" },
        };
        let vendor = metadata(&self.definition.vendor);
        let product = metadata(&self.definition.product);
        let version = metadata(&self.definition.version);
        let url = metadata(&self.definition.url);
        // The names of the interfaces come from their descriptions.
        let descriptions = self.descriptions();
        let info_reply = self.reply(quote! {
            #krate::service::MethodReply::Single(::core::option::Option::Some(
                #reply_params::#varlink(#krate::varlink_service::Reply::Info(__info))
            ))
        });
        let description_reply = self.reply(quote! {
            #krate::service::MethodReply::Single(::core::option::Option::Some(
                #reply_params::#varlink(
                    #krate::varlink_service::Reply::InterfaceDescription(__description)
                )
            ))
        });
        let interface_not_found = self.varlink_error(quote! {
            #krate::varlink_service::Error::InterfaceNotFound {
                interface: ::std::borrow::Cow::Borrowed(__interface.as_str()),
            }
        });

        // `Info::new` converts the interface names it is given. They are `InterfaceName`s already,
        // so the conversion can't fail, which makes the `Result` pattern irrefutable.
        quote! {
            match __method {
                #krate::varlink_service::Method::GetInfo => {
                    let ::core::result::Result::Ok(__info) =
                        #krate::varlink_service::Info::new(
                            #vendor,
                            #product,
                            #version,
                            #url,
                            [
                                #(::core::clone::Clone::clone(#descriptions.name()),)*
                                ::core::clone::Clone::clone(
                                    #krate::varlink_service::INTERFACE_NAME,
                                ),
                            ],
                        );

                    #info_reply
                }
                #krate::varlink_service::Method::GetInterfaceDescription {
                    interface: __interface,
                } => {
                    let __name = __interface.as_str();
                    let __description = #(
                        if __name == #descriptions.name().as_str() {
                            #descriptions
                        } else
                    )* if __name == #krate::varlink_service::INTERFACE_NAME.as_str() {
                        #krate::varlink_service::DESCRIPTION
                    } else {
                        return #interface_not_found;
                    };
                    let __description =
                        #krate::varlink_service::InterfaceDescription::from(__description);

                    #description_reply
                }
            }
        }
    }

    /// The expressions of the descriptions of the interfaces, in the order of the fields.
    fn descriptions(&self) -> Vec<TokenStream> {
        let krate = self.krate;
        let sock = &self.sock;
        let state_ty = &self.state_ty;

        self.definition
            .interfaces
            .iter()
            .map(|field| {
                let ty = &field.ty;
                quote! { <#ty as #krate::service::Interface<#sock, #state_ty>>::DESCRIPTION }
            })
            .collect()
    }

    /// The result of `Service::handle` that replies with an `org.varlink.service` error.
    fn varlink_error(&self, error: TokenStream) -> TokenStream {
        let krate = self.krate;
        let reply_error = &self.reply_error;
        let varlink = varlink_variant();

        self.reply(quote! {
            #krate::service::MethodReply::Error(#reply_error::#varlink(#error))
        })
    }

    /// The result of `Service::handle` that replies with `reply` and no file descriptors.
    #[cfg(feature = "std")]
    fn reply(&self, reply: TokenStream) -> TokenStream {
        quote! { (#reply, ::std::vec::Vec::new()) }
    }

    /// The result of `Service::handle` that replies with `reply`.
    #[cfg(not(feature = "std"))]
    fn reply(&self, reply: TokenStream) -> TokenStream {
        reply
    }

    /// The variants for the fields: each holds `assoc`, an associated type of the interface of its
    /// field.
    fn field_variants(&self, assoc: TokenStream) -> Vec<Variant> {
        let krate = self.krate;
        let sock = &self.sock;
        let state_ty = &self.state_ty;

        self.definition
            .interfaces
            .iter()
            .enumerate()
            .map(|(index, field)| {
                let ty = &field.ty;
                let held = quote! { <#ty as #krate::service::Interface<#sock, #state_ty>>::#assoc };

                (field_variant(index), held)
            })
            .collect()
    }

    /// An enum that is serialized as the value it holds.
    fn serialized_enum(&self, name: &Ident, lt: Option<&str>, variants: &[Variant]) -> TokenStream {
        let predicates = &self.predicates;
        let generics = self.generics(lt);
        let definition = self.enum_definition(name, lt, variants);
        let debug = self.debug_impl(name, lt, variants);
        let arms = variants.iter().map(|(variant, _)| {
            quote! {
                Self::#variant(__value) => ::serde::Serialize::serialize(__value, __serializer)
            }
        });

        quote! {
            #definition
            #debug

            impl #generics ::serde::Serialize for #name #generics
            where
                #predicates
            {
                fn serialize<__S>(
                    &self,
                    __serializer: __S,
                ) -> ::core::result::Result<__S::Ok, __S::Error>
                where
                    __S: ::serde::Serializer,
                {
                    match self {
                        #(#arms,)*
                    }
                }
            }
        }
    }

    fn enum_definition(&self, name: &Ident, lt: Option<&str>, variants: &[Variant]) -> TokenStream {
        let type_attrs = &self.type_attrs;
        let predicates = &self.predicates;
        let generics = self.generics(lt);
        let variants = variants
            .iter()
            .map(|(variant, held)| quote! { #variant(#held), });

        quote! {
            #type_attrs
            pub enum #name #generics
            where
                #predicates
            {
                #(#variants)*
            }
        }
    }

    /// The `Debug` implementation of an enum. It delegates to the value that the enum holds, as
    /// deriving it would bound the socket type by `Debug`, which the values don't need.
    fn debug_impl(&self, name: &Ident, lt: Option<&str>, variants: &[Variant]) -> TokenStream {
        let predicates = &self.predicates;
        let generics = self.generics(lt);
        let arms = variants.iter().map(|(variant, _)| {
            quote! { Self::#variant(__value) => ::core::fmt::Debug::fmt(__value, __f) }
        });

        quote! {
            impl #generics ::core::fmt::Debug for #name #generics
            where
                #predicates
            {
                fn fmt(&self, __f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        #(#arms,)*
                    }
                }
            }
        }
    }

    /// The `Deserialize` implementation of the method call.
    ///
    /// The interface of a call is found from its method name, so that the parameters are read as
    /// those of the right interface only, and an error in them is reported as such.
    fn deserialize_impl(&self) -> TokenStream {
        let krate = self.krate;
        let sock = &self.sock;
        let predicates = &self.predicates;
        let name = &self.method_call;
        let varlink = varlink_variant();
        let unknown = unknown_variant();
        let descriptions = self.descriptions();
        let dispatch = quote! { #krate::service::dispatch };
        let variants = (0..descriptions.len()).map(field_variant);

        quote! {
            impl<'de, #sock> ::serde::Deserialize<'de> for #name<'de, #sock>
            where
                #predicates
            {
                fn deserialize<__D>(
                    __deserializer: __D,
                ) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'de>,
                {
                    struct __Visitor<#sock>(::core::marker::PhantomData<fn() -> #sock>);

                    impl<'de, #sock> ::serde::de::Visitor<'de> for __Visitor<#sock>
                    where
                        #predicates
                    {
                        type Value = #name<'de, #sock>;

                        fn expecting(
                            &self,
                            __f: &mut ::core::fmt::Formatter<'_>,
                        ) -> ::core::result::Result<(), ::core::fmt::Error> {
                            __f.write_str("a method call")
                        }

                        fn visit_map<__A>(
                            self,
                            __map: __A,
                        ) -> ::core::result::Result<Self::Value, __A::Error>
                        where
                            __A: ::serde::de::MapAccess<'de>,
                        {
                            let __call = #dispatch::start(__map)?;
                            if #dispatch::has_method(
                                #krate::varlink_service::DESCRIPTION,
                                __call.method(),
                            ) {
                                return __call.finish().map(#name::#varlink);
                            }
                            #(
                                if #dispatch::has_method(#descriptions, __call.method()) {
                                    return __call.finish().map(#name::#variants);
                                }
                            )*

                            __call.skip().map(#name::#unknown)
                        }
                    }

                    __deserializer.deserialize_map(__Visitor(::core::marker::PhantomData))
                }
            }
        }
    }

    /// The compile-time check that the interfaces of the service all differ in name, and from
    /// `org.varlink.service`.
    ///
    /// A call is routed by the name of its interface, so of two interfaces of the same name, only
    /// the first would be described to the clients. The names are only known once the socket type
    /// is, so the check is a constant of the method call type, which `Service::handle` evaluates.
    fn distinct_interfaces(&self) -> TokenStream {
        let krate = self.krate;
        let sock = &self.sock;
        let predicates = &self.predicates;
        let name = &self.method_call;
        let service = &self.definition.name;
        let dispatch = quote! { #krate::service::dispatch };
        let descriptions = self.descriptions();
        let fields: Vec<String> = self
            .definition
            .interfaces
            .iter()
            .map(|field| match &field.member {
                Member::Named(ident) => ident.to_string(),
                Member::Unnamed(index) => index.index.to_string(),
            })
            .collect();
        let mut checks = Vec::new();
        for (i, first) in descriptions.iter().enumerate() {
            let message = format!(
                "the interface of the field `{}` of `{service}` is named `org.varlink.service`, \
                 which the service implements itself",
                fields[i],
            );
            checks.push(quote! {
                ::core::assert!(
                    !#dispatch::same_name(#first, #krate::varlink_service::DESCRIPTION),
                    #message
                );
            });
            for (j, second) in descriptions.iter().enumerate().skip(i + 1) {
                let message = format!(
                    "the interfaces of the fields `{}` and `{}` of `{service}` have the same name",
                    fields[i], fields[j],
                );
                checks.push(quote! {
                    ::core::assert!(!#dispatch::same_name(#first, #second), #message);
                });
            }
        }

        quote! {
            impl<'de, #sock> #name<'de, #sock>
            where
                #predicates
            {
                #[doc(hidden)]
                pub const __DISTINCT_INTERFACES: () = {
                    #(#checks)*
                };
            }
        }
    }

    /// The generic parameters of the generated types: an optional lifetime and the socket.
    fn generics(&self, lt: Option<&str>) -> TokenStream {
        let sock = &self.sock;

        match lt {
            Some(lt) => {
                let lt = Lifetime::new(lt, Span::call_site());
                quote! { <#lt, #sock> }
            }
            None => quote! { <#sock> },
        }
    }
}

/// A variant of a generated enum: its name and the type that it holds.
type Variant = (Ident, TokenStream);

fn varlink_variant() -> Ident {
    format_ident!("__Varlink")
}

fn unknown_variant() -> Ident {
    format_ident!("__Unknown")
}

fn field_variant(index: usize) -> Ident {
    format_ident!("__Field{}", index)
}

/// The closure that maps an item of the stream of an interface to that of the service.
#[cfg(feature = "std")]
fn map_stream_item(params: TokenStream, error: TokenStream) -> TokenStream {
    quote! {
        |(__result, __fds)| {
            (__result.map(|__reply| __reply.map(#params)).map_err(#error), __fds)
        }
    }
}

/// The closure that maps an item of the stream of an interface to that of the service.
#[cfg(not(feature = "std"))]
fn map_stream_item(params: TokenStream, error: TokenStream) -> TokenStream {
    quote! {
        |__result| __result.map(|__reply| __reply.map(#params)).map_err(#error)
    }
}
