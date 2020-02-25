// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// The `quote!` macro requires deep recursion.
extern crate proc_macro;
extern crate proc_macro2;
extern crate quote;
extern crate syn;

mod common;
mod descriptor;
mod enum_field;
mod struct_field;
mod union_field;
mod versionize;

use descriptor::*;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

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
            fn serialize<W: std::io::Write>(&self, writer: &mut W, version_map: &VersionMap, app_version: u16) {
                #serializer
            }

            #[inline]
            fn deserialize<R: std::io::Read>(mut reader: &mut R, version_map: &VersionMap, app_version: u16) -> Self {
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

    // if descriptor.kind == DescriptorKind::Struct {
    //     println!("{}", output.to_string());

    // }

    output.into()
}
