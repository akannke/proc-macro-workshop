use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, FieldsNamed, GenericArgument, Ident, Lit, Meta,
    MetaNameValue, NestedMeta, PathArguments, PathSegment, Type, Visibility,
};

enum LitOrError {
    Lit(String),
    Error(syn::Error),
}

#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let ident = input.ident;
    let vis = input.vis;
    let builder_name = format_ident!("{}Builder", ident);

    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields,
            _ => {
                return syn::Error::new(ident.span(), "expects named fields")
                    .to_compile_error()
                    .into()
            }
        },
        _ => {
            return syn::Error::new(ident.span(), "expects struct")
                .to_compile_error()
                .into()
        }
    };

    let builder_struct = build_builder_struct(&fields, &builder_name, &vis);
    let builder_impl = build_builder_impl(&fields, &builder_name, &ident);
    let struct_impl = build_struct_impl(&fields, &builder_name, &ident);

    let expand = quote! {
        #builder_struct
        #builder_impl
        #struct_impl
    };
    proc_macro::TokenStream::from(expand)
}

fn build_builder_struct(
    fields: &FieldsNamed,
    builder_name: &Ident,
    visibility: &Visibility,
) -> TokenStream {
    let struct_fields = fields
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.as_ref();
            let ty = unwrap_option(&field.ty).unwrap_or(&field.ty);
            (ident.unwrap(), ty)
        })
        .map(|(ident, ty)| {
            if is_vector(&ty) {
                quote! {
                    #ident: #ty
                }
            } else {
                quote! {
                    #ident: Option<#ty>
                }
            }
        });
    quote! {
        #visibility struct #builder_name {
            #(#struct_fields),*
        }
    }
    .into()
}

fn build_builder_impl(
    fields: &FieldsNamed,
    builder_name: &Ident,
    struct_name: &Ident,
) -> TokenStream {
    let checks = fields
        .named
        .iter()
        .filter(|field| !is_option(&field.ty))
        .filter(|field| !is_vector(&field.ty))
        .map(|field| {
            let ident = field.ident.as_ref();
            let err = format!("Required field '{}' is missing", ident.unwrap().to_string());
            quote! {
                if self.#ident.is_none() {
                    return Err(#err.into());
                }
            }
        });

    let setters = fields.named.iter().map(|field| {
        let ident_each_name = field
            .attrs
            .first()
            .map(|attr| match attr.parse_meta() {
                Ok(Meta::List(list)) => match list.nested.first() {
                    Some(NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                        ref path,
                        eq_token: _,
                        lit: Lit::Str(ref str),
                    }))) => {
                        if let Some(name) = path.segments.first() {
                            if name.ident.to_string() != "each" {
                                return Some(LitOrError::Error(syn::Error::new_spanned(
                                    list,
                                    "expected `builder(each = \"...\")`",
                                )));
                            }
                        }

                        Some(LitOrError::Lit(str.value()))
                    }
                    _ => None,
                },
                _ => None,
            })
            .flatten();

        let ident = field.ident.as_ref();
        let ty = unwrap_option(&field.ty).unwrap_or(&field.ty);
        // #[builder(each = "name")]
        match ident_each_name {
            Some(LitOrError::Lit(name)) => {
                let ty_each = unwrap_vector(ty).unwrap();
                let ident_each = Ident::new(name.as_str(), Span::call_site());
                // if the name specified in "each" is the same as the field name
                if ident.unwrap().to_string() == name {
                    // Define only a method to add one element
                    quote! {
                        pub fn #ident_each(&mut self, #ident_each:#ty_each) -> &mut Self {
                            self.#ident.push(#ident_each);
                            self
                        }
                    }
                } else {
                    quote! {
                        pub fn #ident(&mut self, #ident: #ty) -> &mut Self {
                            self.#ident = #ident;
                            self
                        }
                        pub fn #ident_each(&mut self, #ident_each: #ty_each) -> &mut Self {
                            self.#ident.push(#ident_each);
                            self
                        }
                    }
                }
            }
            Some(LitOrError::Error(err)) => err.to_compile_error().into(),
            None => {
                if is_vector(&ty) {
                    quote! {
                        pub fn #ident(&mut self, #ident: #ty) -> &mut Self {
                            self.#ident = #ident;
                            self
                        }
                    }
                } else {
                    quote! {
                        pub fn #ident(&mut self, #ident: #ty) -> &mut Self {
                            self.#ident = Some(#ident);
                            self
                        }
                    }
                }
            }
        }
    });

    let struct_fields = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref();
        if is_option(&field.ty) || is_vector(&field.ty) {
            quote! {
                #ident: self.#ident.clone()
            }
        } else {
            quote! {
                #ident: self.#ident.clone().unwrap()
            }
        }
    });

    quote! {
        impl #builder_name {
            #(#setters)*

            pub fn build(&mut self) -> Result<#struct_name, Box<dyn std::error::Error>> {
                #(#checks)*
                Ok(#struct_name {
                    #(#struct_fields),*
                })
            }
        }
    }
}

fn build_struct_impl(
    fields: &FieldsNamed,
    builder_name: &Ident,
    struct_name: &Ident,
) -> TokenStream {
    let field_defaults = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref();
        let ty = &field.ty;
        if is_vector(&ty) {
            quote! {
                #ident: Vec::new()
            }
        } else {
            quote! {
                #ident: None
            }
        }
    });
    quote! {
        impl #struct_name {
            pub fn builder() -> #builder_name {
                #builder_name {
                    #(#field_defaults),*
                }
            }
        }
    }
}

fn get_last_path_segment(ty: &Type) -> Option<&PathSegment> {
    match ty {
        Type::Path(path) => path.path.segments.last(),
        _ => None,
    }
}

fn is_option(ty: &Type) -> bool {
    match get_last_path_segment(ty) {
        Some(seg) => seg.ident == "Option",
        _ => false,
    }
}

fn is_vector(ty: &Type) -> bool {
    match get_last_path_segment(ty) {
        Some(seg) => seg.ident == "Vec",
        _ => false,
    }
}

fn unwrap_option(ty: &Type) -> Option<&Type> {
    if !is_option(ty) {
        return None;
    }
    unwrap_generic_type(ty)
}

fn unwrap_vector(ty: &Type) -> Option<&Type> {
    if !is_vector(ty) {
        return None;
    }
    unwrap_generic_type(ty)
}

fn unwrap_generic_type(ty: &Type) -> Option<&Type> {
    match get_last_path_segment(ty) {
        Some(seg) => match seg.arguments {
            PathArguments::AngleBracketed(ref args) => {
                args.args.first().and_then(|arg| match arg {
                    &GenericArgument::Type(ref ty) => Some(ty),
                    _ => None,
                })
            }
            _ => None,
        },
        None => None,
    }
}
