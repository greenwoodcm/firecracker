// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// The `quote!` macro requires deep recursion.
extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::cmp::max;
use std::collections::hash_map::HashMap;
use syn::{parse_macro_input, DeriveInput};

#[derive(Debug, Eq, PartialEq, Clone)]
enum DescriptorKind {
    None,
    Struct,
    Enum,
}

// Describes a structure type and fields.
// Is used as input for computing the trans`tion code.
struct DataDescriptor {
    ty: syn::Ident,
    kind: DescriptorKind,
    version: u16,
    fields: Vec<Box<dyn FieldVersionize>>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct StructField {
    ty: syn::Type,
    name: String,
    start_version: u16,
    end_version: u16,
    attrs: HashMap<String, syn::Lit>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct EnumVariant {
    ident: syn::Ident,
    discriminant: u16, // Only u16 discriminants allowed.
    start_version: u16,
    end_version: u16,
    attrs: HashMap<String, syn::Lit>,
}

// Trait that defines a generic behaviour as a field level serialization and
// deseriailization code generator
trait FieldVersionize {
    fn get_default(&self) -> Option<&syn::Lit>;
    fn get_attr(&self, attr: &str) -> Option<&syn::Lit>;

    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream;
    fn generate_deserializer(&self, source_version: u16) -> proc_macro2::TokenStream;

    fn get_start_version(&self) -> u16;
    fn get_end_version(&self) -> u16;
}

fn parse_field_attributes(attrs: &mut HashMap<String, syn::Lit>, attributes: &Vec<syn::Attribute>) {
    for attribute in attributes {
        // Check if this is a snapshot attribute.
        match attribute.parse_meta().unwrap().clone() {
            syn::Meta::List(meta_list) => {
                // Check if this is a "snapshot" attribute.
                if meta_list.path.segments[0].ident.to_string() == "snapshot" {
                    // Fetch the specific attribute name
                    for nested_attribute in meta_list.nested {
                        match nested_attribute {
                            syn::NestedMeta::Meta(nested_meta) => {
                                match nested_meta {
                                    syn::Meta::NameValue(attr_name_value) => {
                                        // panic!("{:?}", attr_name_value);
                                        // if attr_name_value.eq_token.to_string() == "=" {
                                        attrs.insert(
                                            attr_name_value.path.segments[0].ident.to_string(),
                                            attr_name_value.lit,
                                        );
                                        // }
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl FieldVersionize for StructField {
    fn get_default(&self) -> Option<&syn::Lit> {
        self.attrs.get("default")
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

    // Emits code that serializes this field.
    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream {
        let field_ident = format_ident!("{}", self.name);

        // Generate serializer for this field only if it exists in target_version.
        if target_version < self.start_version
            || (self.end_version > 0 && target_version > self.end_version)
        {
            return proc_macro2::TokenStream::new();
        }

        match &self.ty {
            syn::Type::Array(_) => quote! {
                Versionize::serialize(&self.#field_ident.to_vec(), writer, version);
            },
            syn::Type::Path(_) => quote! {
                Versionize::serialize(&self.#field_ident, writer, version);
            },
            _ => panic!("Unsupported field type {:?}", self.ty),
        }
    }

    // Emits code that serializes this field.
    fn generate_deserializer(
        &self,
        source_version: u16,
    ) -> proc_macro2::TokenStream {
        let field_ident = format_ident!("{}", self.name);

        // If the field does not exist in source version, use default annotation or Default trait.
        if source_version < self.start_version
            || (self.end_version > 0 && source_version > self.end_version)
        {
            if let Some(default) = self.get_default() {
                return quote! {
                    #field_ident: #default,
                };
            } else {
                return quote! { #field_ident: Default::default(), };
            }
        }

        let ty = &self.ty;

        match ty {
            syn::Type::Array(array) => {
                let array_type_token;
                let array_len: usize;

                match *array.elem.clone() {
                    syn::Type::Path(token) => {
                        array_type_token = token;
                    }
                    _ => panic!("Unsupported array type."),
                }

                match &array.len {
                    syn::Expr::Lit(expr_lit) => match &expr_lit.lit {
                        syn::Lit::Int(lit_int) => array_len = lit_int.base10_parse().unwrap(),
                        _ => panic!("Unsupported array len literal."),
                    },
                    _ => panic!("Unsupported array len expression."),
                }

                quote! {
                    #field_ident: {
                        let v: Vec<#array_type_token> = bincode::deserialize_from(&mut reader).unwrap();
                        vec_to_arr_func!(transform_vec, #array_type_token, #array_len);
                        transform_vec(v)
                    },
                }
            }
            syn::Type::Path(_) => quote! {
                #field_ident: <#ty as Versionize>::deserialize(&mut reader, version),
            },
            _ => panic!("Unsupported field type {:?}", self.ty),
        }
    }
}

impl FieldVersionize for EnumVariant {
    fn get_default(&self) -> Option<&syn::Lit> {
        self.attrs.get("default")
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

    // Emits code that serializes an enum variant.
    // The generated code is expected to be match branch.
    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream {
        let field_ident = &self.ident;
        let discriminant = self.discriminant;

        if target_version < self.start_version || (self.end_version > 0 && target_version > self.end_version)
        {
            if let Some(default_fn) = self.get_attr("default_fn") {
                match default_fn {
                    syn::Lit::Str(lit_str) => {
                        let fn_ident = format_ident!("{}",lit_str.value());
                        return quote! {
                            #field_ident => {
                                let variant = #fn_ident(&self, #target_version);
                                bincode::serialize_into(writer, &variant).unwrap();
                            }
                        }
                    },
                    _ => panic!("Default_fn must be a string."),
                }
            } else {
                panic!("Variant {} does not exist in version {}, please implement a default_fn function that provides a default value for this variant.", field_ident.to_string(), target_version);
            }
        }

        quote! {
            #field_ident => {                
                bincode::serialize_into(writer, &self).unwrap();
            }
        }
    }

    // Emits code that serializes this field.
    fn generate_deserializer(
        &self,
        _source_version: u16,
    ) -> proc_macro2::TokenStream {
        // We do not need to do anything here, we always deserialize whatever variant is encoded.
        quote! {}
    }
}

impl EnumVariant {
    // Parses the abstract syntax tree and create a versioned Field definition.
    fn new(
        base_version: u16,
        ast_variant: &syn::Variant,
    ) -> Self {

        let mut variant = EnumVariant {
            ident: ast_variant.ident.clone(),
            discriminant: 0,
            // Set base version.
            start_version: base_version,
            end_version: 0,
            attrs: HashMap::new(),
        };

        // Get variant discriminant as u16.
        if let Some(discriminant) = &ast_variant.discriminant {
            // We only support ExprLit
            match &discriminant.1 {
                syn::Expr::Lit(lit_expr) => {
                    match &lit_expr.lit {
                        syn::Lit::Int(lit_int) => variant.discriminant = lit_int.base10_parse().unwrap(),
                        _ => panic!("A u16 discriminant is required fior versioning Enums.")
                    }
                },
                _ => panic!("A u16 discriminant is required fior versioning Enums.")
            }
        } else {
            panic!("A u16 discriminant is required fior versioning Enums.")
        }

        // panic!("{:?}", ast_variant.attrs[0]);
        parse_field_attributes(&mut variant.attrs, &ast_variant.attrs);

        if let Some(start_version) = variant.get_attr("start_version") {
            match start_version {
                syn::Lit::Int(lit_int) => variant.start_version = lit_int.base10_parse().unwrap(),
                _ => panic!("Field start/end version number must be an integer"),
            }
        }

        if let Some(end_version) = variant.get_attr("end_version") {
            match end_version {
                syn::Lit::Int(lit_int) => variant.end_version = lit_int.base10_parse().unwrap(),
                _ => panic!("Field start/end version number must be an integer"),
            }
        }
       
        variant
    }
}

impl StructField {
    // Parses the abstract syntax tree and create a versioned Field definition.
    fn new(
        base_version: u16,
        ast_field: syn::punctuated::Pair<&syn::Field, &syn::token::Comma>,
    ) -> Self {
        let name = ast_field.value().ident.as_ref().unwrap().to_string();
        let mut field = StructField {
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

impl DataDescriptor {
    fn new(derive_input: &DeriveInput) -> Self {
        let mut descriptor = DataDescriptor {
            kind: DescriptorKind::None,
            ty: derive_input.ident.clone(),
            version: 1, // struct start at version 1.
            fields: vec![],
        };

        match &derive_input.data {
            syn::Data::Struct(data_struct) => {
                descriptor.kind = DescriptorKind::Struct;
                descriptor.parse_fields(&data_struct.fields);
            }
            syn::Data::Enum(data_enum) => {
                descriptor.kind = DescriptorKind::Enum;
                descriptor.parse_variants(&data_enum.variants);
            }
            _ => {
                panic!("Only structs can be versioned");
            }
        }

        // Compute current struct version.
        for field in &descriptor.fields {
            descriptor.version = max(
                descriptor.version,
                max(field.get_start_version(), field.get_end_version()),
            );
        }
        descriptor
    }

    fn add_field<F: FieldVersionize + 'static>(&mut self, field: F) {
        self.fields.push(Box::new(field));
    }

    // Parses the struct field by field.
    // Returns a vector of Field definitions.
    fn parse_fields(&mut self, fields: &syn::Fields) {
        match fields {
            syn::Fields::Named(ref named_fields) => {
                let pairs = named_fields.named.pairs();
                for field in pairs.into_iter() {
                    self.add_field(StructField::new(self.version, field));
                }
            }
            _ => panic!("Only named fields are supported."),
        }
    }

    fn parse_variants(&mut self, variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>) {
        for variant in variants.iter() {
            self.add_field(EnumVariant::new(self.version, variant));
        }
    }

    // Returns a token stream containing the serializer body.
    fn generate_serializer(&self) -> proc_macro2::TokenStream {
        let mut versioned_serializers = proc_macro2::TokenStream::new();
        for i in 1..=self.version {
            let mut versioned_serializer = proc_macro2::TokenStream::new();
            for field in &self.fields {
                versioned_serializer.extend(field.generate_serializer(i));
            }

            match self.kind {
                DescriptorKind::Struct => versioned_serializers.extend(quote! {
                    #i => {
                        #versioned_serializer
                    }
                }),
                DescriptorKind::Enum => versioned_serializers.extend(quote! {
                    #i => {
                        match self {
                            #versioned_serializer
                        }
                    }
                }),
                DescriptorKind::None => panic!("DataDescriptor kind is None.")
            }
            
        }

        let result = quote! {
            match version {
                #versioned_serializers
                _ => panic!("Unknown version {}.", version)
            }
        };

        result
    }

    // Returns a token stream containing the serializer body.
    fn generate_deserializer(&self) -> proc_macro2::TokenStream {
        let mut versioned_deserializers = proc_macro2::TokenStream::new();
        let struct_ident = format_ident!("{}", self.ty);

        match self.kind { 
            DescriptorKind::Struct => {
                for i in 1..=self.version {
                    let mut versioned_deserializer = proc_macro2::TokenStream::new();
                    for field in &self.fields {
                        versioned_deserializer.extend(field.generate_deserializer(i));
                    }
                    versioned_deserializers.extend(quote! {
                        #i => {
                            #struct_ident {
                                #versioned_deserializer
                            }
                        }
                    });
                }
        
                quote! {
                    use std::convert::TryInto;

                    // This macro will generate a function that copies a vec to an array.
                    // We serialize arrays as vecs.
                    macro_rules! vec_to_arr_func {
                        ($name:ident, $type:ty, $size:expr) => {
                            pub fn $name(data: std::vec::Vec<$type>) -> [$type; $size] {
                                let mut arr = [0; $size];
                                arr.copy_from_slice(&data[0..$size]);
                                arr
                            }
                        };
                    }

                    match version {
                        #versioned_deserializers
                        _ => panic!("Unknown version {}.", version)
                    }
                }
            },
            DescriptorKind::Enum => {
                quote! {
                    let variant: #struct_ident = bincode::deserialize_from(&mut reader).unwrap();
                    variant
                }
            },
            _ => panic!("Unsupported decriptor kind")
        }
    }
}

#[proc_macro_derive(Versionize, attributes(snapshot))]
pub fn generate_versioned(input: TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let descriptor = DataDescriptor::new(&input);
    let ident = &descriptor.ty;
    let name = descriptor.ty.to_string();
    let version = descriptor.version;
    let serializer = descriptor.generate_serializer();
    let deserializer = descriptor.generate_deserializer();

    let output = quote! {
        impl Versionize for #ident {
            #[inline]
            fn serialize<W: std::io::Write>(&self, mut writer: &mut W, version: u16) {
                #serializer
            }

            #[inline]
            fn deserialize<R: std::io::Read>(mut reader: &mut R, version: u16) -> Self {
                #deserializer
            }

            #[inline]
            // Returns struct name as string.
            fn name() -> String {
                #name.to_owned()
            }

            #[inline]
            // Returns struct current version.
            fn version() -> u16 {
                #version
            }
        }
    };

    println!("{}", output.to_string());

    output.into()
}
