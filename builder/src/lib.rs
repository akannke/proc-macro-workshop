use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, FieldsNamed, GenericArgument, Ident,
    PathArguments, PathSegment, Type, Visibility,
};

#[proc_macro_derive(Builder)]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let ident = input.ident;
    let vis = input.vis;
    let builder_name = format_ident!("{}Builder", ident);

    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields,
            _ => panic!("no unnamed fields are allowed"),
        },
        _ => panic!("this macro can be applied only to struct"),
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
    let (idents, types): (Vec<&Ident>, Vec<&Type>) = fields
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.as_ref();
            let ty = unwrap_option(&field.ty).unwrap_or(&field.ty);
            (ident.unwrap(), ty)
        })
        .unzip();

    quote! {
        #visibility struct #builder_name {
            #(#idents: Option<#types>),*
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
        .map(|field| {
            let ident = field.ident.as_ref();
            let err = format!("Required field '{}' is missing", ident.unwrap().to_string());
            quote! {
                if self.#ident.is_none() {
                    return Err(#err.into())
                }
            }
        });

    let setters = fields.named.iter().map(|field| {
        let ident = &field.ident;
        let t = unwrap_option(&field.ty).unwrap_or(&field.ty);
        quote! {
            pub fn #ident(&mut self, #ident: #t) -> &mut Self {
                self.#ident = Some(#ident);
                self
            }
        }
    });

    let struct_fields = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref();
        if is_option(&field.ty) {
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
        quote! {
            #ident: None
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

fn unwrap_option(ty: &Type) -> Option<&Type> {
    if !is_option(ty) {
        return None;
    }
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|seg| match seg.arguments {
                PathArguments::AngleBracketed(ref args) => {
                    args.args.first().and_then(|arg| match arg {
                        &GenericArgument::Type(ref ty) => Some(ty),
                        _ => None,
                    })
                }
                _ => None,
            }),
        _ => None,
    }
}
