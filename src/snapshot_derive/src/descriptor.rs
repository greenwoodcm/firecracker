use quote::{format_ident, quote};
use std::cmp::max;
use syn::{DeriveInput};
use struct_field::*;
use enum_field::*;
use versionize::*;


#[derive(Debug, Eq, PartialEq, Clone)]
pub(crate) enum DescriptorKind {
    None,
    Struct,
    Enum,
    Union,
}

// Describes a structure type and fields.
// Is used as input for computing the trans`tion code.
pub(crate) struct DataDescriptor {
    pub ty: syn::Ident,
    pub kind: DescriptorKind,
    pub version: u16,
    fields: Vec<Box<dyn FieldVersionize>>,
}

impl DataDescriptor {
    pub fn new(derive_input: &DeriveInput) -> Self {
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
            syn::Data::Union(data_union) => {
                descriptor.kind = DescriptorKind::Union;
                descriptor.parse_fields(&syn::Fields::Named(data_union.fields.clone()));
                //println!("{:?}", data_union);
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
    pub fn generate_serializer(&self) -> proc_macro2::TokenStream {
        let mut versioned_serializers = proc_macro2::TokenStream::new();

        for i in 1..=self.version {
            let mut versioned_serializer = proc_macro2::TokenStream::new();
            let mut semantic_serializer = proc_macro2::TokenStream::new();

            // Emit code for both field serializer and semantic serializer.
            for field in &self.fields {
                versioned_serializer.extend(field.generate_serializer(i));
                semantic_serializer.extend(field.generate_semantic_serializer(i));
            }

            match self.kind {
                // Serialization follows this flow: semantic -> field -> encode.
                DescriptorKind::Struct => versioned_serializers.extend(quote! {
                    #i => {
                        #semantic_serializer
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
                DescriptorKind::Union => panic!("DataDescriptor kind is Union."),
                DescriptorKind::None => panic!("DataDescriptor kind is None.")
            }
            
        }

        let result = quote! {
            // Get the struct version for the input app_version.
            let version = version_map.get_type_version(app_version, &Self::name());
            // We will use this copy to perform semantic serialization.
            let mut copy_of_self = self.clone();
            match version {
                #versioned_serializers
                _ => panic!("Unknown {} version {}.", &Self::name(), version)
            }
        };

        result
    }

    fn generate_deserializer_header(&self) -> proc_macro2::TokenStream {
        // Just checking if there are any array fields present.
        // If so, include the vec2array macro.
        if let Some(_) = self.fields.iter().find(|&field| field.is_array()) {
            return quote!{
                use std::convert::TryInto;

                // This macro will generate a function that copies a vec to an array.
                // We serialize arrays as vecs.
                macro_rules! vec_to_arr_func {
                    ($name:ident, $type:ty, $size:expr) => {
                        pub fn $name(data: &std::vec::Vec<$type>) -> [$type; $size] {
                            let mut arr = [<$type as Default>::default(); $size];
                            arr.copy_from_slice(&data[0..$size]);
                            arr
                        }
                    };
                }
            }
        }

        quote!{}
    }
    // Returns a token stream containing the serializer body.
    pub fn generate_deserializer(&self) -> proc_macro2::TokenStream {
        let mut versioned_deserializers = proc_macro2::TokenStream::new();
        let struct_ident = format_ident!("{}", self.ty);
        let header = self.generate_deserializer_header();

        match self.kind { 
            DescriptorKind::Struct => {
                for i in 1..=self.version {
                    let mut versioned_deserializer = proc_macro2::TokenStream::new();
                    let mut semantic_deserializer = proc_macro2::TokenStream::new();

                    for field in &self.fields {
                        versioned_deserializer.extend(field.generate_deserializer(i));
                        semantic_deserializer.extend(field.generate_semantic_deserializer(i));
                    }
                    versioned_deserializers.extend(quote! {
                        #i => {
                            let mut object = #struct_ident {
                                #versioned_deserializer
                            };
                            #semantic_deserializer
                            object
                        }
                    });
                }
        
                quote! {
                    #header

                    let version = version_map.get_type_version(app_version, &Self::name());
                    match version {
                        #versioned_deserializers
                        _ => panic!("Unknown {} version {}.", Self::name(), version)
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
