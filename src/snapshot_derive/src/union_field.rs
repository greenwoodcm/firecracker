use common::*;
use quote::{format_ident, quote};
use std::collections::hash_map::HashMap;
use versionize::*;

#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) struct UnionField {
    ty: syn::Type,
    name: String,
    start_version: u16,
    end_version: u16,
    attrs: HashMap<String, syn::Lit>,
}

impl UnionField {
    pub fn new(
        base_version: u16,
        ast_field: syn::punctuated::Pair<&syn::Field, &syn::token::Comma>,
    ) -> Self {
        let name = ast_field.value().ident.as_ref().unwrap().to_string();
        let mut field = UnionField {
            ty: ast_field.value().ty.clone(),
            name,
            start_version: base_version,
            end_version: 0,
            attrs: HashMap::new(),
        };

        parse_field_attributes(&mut field.attrs, &ast_field.value().attrs);

        // Adjust version based on attributes.
        if let Some(start_version) = field.get_attr("start_version") {
            match start_version {
                syn::Lit::Int(lit_int) => field.start_version = lit_int.base10_parse().unwrap(),
                _ => panic!("Field start/end version number must be an integer"),
            }
        }

        if let Some(end_version) = field.get_attr("end_version") {
            match end_version {
                syn::Lit::Int(lit_int) => field.end_version = lit_int.base10_parse().unwrap(),
                _ => panic!("Field start/end version number must be an integer"),
            }
        }

        field
    }
}

impl FieldVersionize for UnionField {
    fn get_default(&self) -> Option<syn::Ident> {
        get_ident_attr(&self.attrs, "default_fn")
    }

    fn get_attr(&self, attr: &str) -> Option<&syn::Lit> {
        self.attrs.get(attr)
    }

    fn get_start_version(&self) -> u16 {
        self.start_version
    }
    fn get_end_version(&self) -> u16 {
        self.end_version
    }

    fn get_type(&self) -> syn::Type {
        self.ty.clone()
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn is_array(&self) -> bool {
        match self.ty {
            syn::Type::Array(_) => true,
            _ => false,
        }
    }

    // Semantic serialization not supported for enums.
    fn generate_semantic_serializer(&self, _target_version: u16) -> proc_macro2::TokenStream {
        quote! {}
    }

    // Semantic deserialization not supported for enums.
    fn generate_semantic_deserializer(&self, _source_version: u16) -> proc_macro2::TokenStream {
        quote! {}
    }

    // Emits code that serializes a union field.
    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream {
        let field_ident = format_ident!("{}", self.get_name());
        if self.is_array() {
            return quote! {
                Versionize::serialize(&copy_of_self.#field_ident.to_vec(), writer, version_map, app_version)
            };
        }

        quote! {
            Versionize::serialize(&copy_of_self.#field_ident, writer, version_map, app_version)
        }
    }

    // Emits code that serializes this field.
    fn generate_deserializer(&self, _source_version: u16) -> proc_macro2::TokenStream {
        // We do not need to do anything here, we always deserialize whatever variant is encoded.
        quote! {}
    }
}
