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
enum DescriptorType {
    None,
    Struct(String),
    Enum,
}

// Describes a structure type and fields.
// Is used as input for computing the trans`tion code.
struct DataDescriptor {
    ty: String,
    version: u16,
    fields: Vec<Box<dyn FieldVersionize>>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
enum StructFieldType {
    Path(syn::TypePath),
    Array(syn::TypeArray),
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct StructField {
    ty: StructFieldType,
    name: String,
    start_version: u16,
    end_version: u16,
    attrs: HashMap<String, syn::Lit>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct EnumVariant {
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
    fn generate_deserializer(
        &self,
        reader: &proc_macro2::Ident,
        source_version: u16,
    ) -> proc_macro2::TokenStream;

    fn get_start_version(&self) -> u16;
    fn get_end_version(&self) -> u16;
}

impl FieldVersionize for StructField {
    fn get_default(&self) -> Option<&syn::Lit> {
        self.attrs.get("default")
    }

    fn get_attr(&self, attr: &str) -> Option<&syn::Lit> {
        self.attrs.get(attr)
    }

    // Emits code that serializes this field.
    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream {
        let field_ident = format_ident!("{}", self.name);
        //result.extend(bincode::serialize(&self.{}).unwrap());

        // Generate serializer for this field only if it exists in target_version.
        if target_version < self.start_version
            || (self.end_version > 0 && target_version > self.end_version)
        {
            return proc_macro2::TokenStream::new();
        }

        match self.ty {
            StructFieldType::Path(_) => {
                quote! {
                    Versionize::serialize(&self.#field_ident, writer, version);
                }
            }
            StructFieldType::Array(_) => {
                quote! {
                    bincode::serialize_into(writer, &self.#field_ident.to_vec()).unwrap();
                }
            }
        }
    }

    // Emits code that serializes this field.
    fn generate_deserializer(
        &self,
        reader: &proc_macro2::Ident,
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

        match &self.ty {
            StructFieldType::Path(path) => {
                quote! {
                    #field_ident: <#path as Versionize>::deserialize(&mut #reader, version),
                }
            }
            StructFieldType::Array(array) => {
                let array_type;
                let array_type_token;
                let array_len: usize;

                match *array.elem.clone() {
                    syn::Type::Path(token) => {
                        let mut ty_path = String::new();
                        for segment in token.path.segments.iter() {
                            if ty_path.len() > 0 {
                                ty_path = ty_path + "::" + &segment.ident.to_string();
                            } else {
                                ty_path = segment.ident.to_string();
                            }
                        }
                        array_type = ty_path;
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
                        let v: Vec<#array_type_token> = bincode::deserialize_from(&mut #reader).unwrap();
                        vec_to_arr_func!(transform_vec, #array_type_token, #array_len);
                        transform_vec(v)
                    },
                }
            }
        }
    }
    fn get_start_version(&self) -> u16 {
        self.start_version
    }
    fn get_end_version(&self) -> u16 {
        self.end_version
    }
}

impl StructField {
    // Parses the abstract syntax tree and create a versioned Field definition.
    fn new(
        base_version: u16,
        ast_field: syn::punctuated::Pair<&syn::Field, &syn::token::Comma>,
    ) -> Self {
        let name = ast_field.value().ident.as_ref().unwrap().to_string();
        let ty;

        match &ast_field.value().ty {
            syn::Type::Path(token) => {
                ty = StructFieldType::Path(token.clone());
            }
            syn::Type::Array(type_slice) => {
                // panic!("{:?}", type_slice.;
                ty = StructFieldType::Array(type_slice.clone())
            }
            _ => {
                panic!("Unspported field type");
            }
        }

        let mut field = StructField {
            ty,
            name,
            // Set base version.
            start_version: base_version,
            end_version: 0,
            attrs: HashMap::new(),
        };

        field.parse_field_attributes(&ast_field.value().attrs);

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
    fn parse_field_attributes(&mut self, attributes: &Vec<syn::Attribute>) {
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
                                            self.attrs.insert(
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
}

impl DataDescriptor {
    fn new(derive_input: &DeriveInput) -> Self {
        let mut descriptor = DataDescriptor {
            ty: DescriptorType::None,
            version: 1, // struct start at version 1.
            fields: vec![]
        };

        match &derive_input.data {
            syn::Data::Struct(data_struct) => {
                descriptor.ty = DescriptorType::Struct(derive_input.ident.to_string());
                descriptor.parse_fields(&data_struct.fields);
            }
            syn::Data::Enum(data_enum) => {
                descriptor.ty = DescriptorType::Enum;
                panic!("{:?}", data_enum.variants);
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
        match self.ty {
            DescriptorType::Struct(_) => self.parse_struct_fields(fields),
            DescriptorType::Enum => self.parse_enum_fields(fields),
            _ => panic!("Unknown descriptor type")
        }
    }

    fn parse_struct_fields(&mut self, fields: &syn::Fields) {
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

    fn parse_enum_fields(&mut self, fields: &syn::Fields) {
        match fields {
            syn::Fields::Unnamed(ref unnamed_fields) => {
                
            }
            _ => panic!("Only named fields are supported."),
        }
    }
    // Returns a token stream containing the serializer body.
    fn generate_serializer(&self) -> proc_macro2::TokenStream {
        let mut versioned_serializers = proc_macro2::TokenStream::new();
        // Iterate through all fields and emit serialization code.
        // TODO: add struct base version to support removal of older versions.
        for i in 1..=self.version {
            let mut versioned_serializer = proc_macro2::TokenStream::new();
            for field in &self.fields {
                versioned_serializer.extend(field.generate_serializer(i));
            }

            versioned_serializers.extend(quote! {
                #i => {
                    #versioned_serializer
                }
            });
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
    fn generate_deserializer(&self, reader: &proc_macro2::Ident) -> proc_macro2::TokenStream {
        let mut versioned_deserializers = proc_macro2::TokenStream::new();
        let struct_ident = format_ident!("{}", self.ty);

        // Iterate through all fields and versions and emit deserialization code.
        // TODO: add struct base version to support removal of older versions.
        for i in 1..=self.version {
            let mut versioned_deserializer = proc_macro2::TokenStream::new();
            for field in &self.fields {
                versioned_deserializer.extend(field.generate_deserializer(reader, i));
            }
            versioned_deserializers.extend(quote! {
                #i => {
                    #struct_ident {
                        #versioned_deserializer
                    }
                }
            });
        }

        let result = quote! {
            match version {
                #versioned_deserializers
                _ => panic!("Unknown version {}.", version)
            }
        };

        result
    }
}

// We use this macro to allow the 'snapshot' attribute to be used on structs.
// The version translator code generator will use custom attr 'default'.
#[proc_macro_derive(Versionize, attributes(snapshot))]
pub fn generate_versioned(input: TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let descriptor = DataDescriptor::new(&input);
    let struct_ident = format_ident!("{}", descriptor.ty);
    let name = descriptor.ty.to_owned();
    let version = descriptor.version;
    let serializer = descriptor.generate_serializer();
    let reader_ident = format_ident!("reader");
    let deserializer = descriptor.generate_deserializer(&reader_ident);

    let output = quote! {
        impl Versionize for #struct_ident {
            #[inline]
            fn serialize<W: std::io::Write>(&self, mut writer: &mut W, version: u16) {
                #serializer
            }

            #[inline]
            fn deserialize<R: std::io::Read>(mut #reader_ident: &mut R, version: u16) -> Self {
                use std::convert::TryInto;

                // This macro will generate a function  to copy vec to array.
                macro_rules! vec_to_arr_func {
                    ($name:ident, $type:ty, $size:expr) => {
                        pub fn $name(data: std::vec::Vec<$type>) -> [$type; $size] {
                            let mut arr = [0; $size];
                            arr.copy_from_slice(&data[0..$size]);
                            arr
                        }
                    };
                }

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
