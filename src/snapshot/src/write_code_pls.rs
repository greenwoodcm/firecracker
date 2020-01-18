// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![recursion_limit = "512"]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro::TokenStream;
use syn::*;

use std::env;
use std::path::Path;
use std::fs::File;
use std::io::{Write, Read};
use std::error::Error;
use std::io::prelude::*;

#[derive(Debug)]
pub struct StructDescriptor {
    ty: String,
    fields: Vec<String>,
    field_types: Vec<String>,
}

/// Input must be a string containing a rust struct definition
/// Output is a struct descriptor.
pub fn scan_struct(input: String) -> syn::parse::Result<StructDescriptor> {
    let ast: syn::ItemStruct = syn::parse_str(&input)?;
    
    let mut descriptor = StructDescriptor {
        ty: ast.ident.to_string(),
        fields: vec![],
        field_types: vec![],
    };

    match ast.fields {
        syn::Fields::Named(ref named_fields) => {
            let pairs = named_fields.named.pairs();
            for field in pairs.into_iter() {
                let field_name = field.value().ident.as_ref().unwrap().to_string();
                let mut field_type = String::new();

                match &field.value().ty {
                    syn::Type::Path(token) => {
                        for segment in token.path.segments.iter() {
                            field_type = field_type+ &segment.ident.to_string();
                        }
                        descriptor.fields.push(field_name);
                        descriptor.field_types.push(field_type);
                    }
                    _ => {}
                }
            }

        },
        _ => { }
    }


    Ok(descriptor)
}

fn scan_structs() -> StructDescriptor {
    let path = Path::new("./src/struct.rs");
    let display = path.display();
    let mut file = match File::open(&path) {
        Err(why) => panic!("couldn't open {}: {}", display, why.description()),
        Ok(file) => file,
    };
    
    let mut s = String::new();
    match file.read_to_string(&mut s) {
        Err(why) => panic!("couldn't read {}: {}", display,
                                                   why.description()),
        Ok(_) => print!("{} contains:\n{}", display, s),
    }

    scan_struct(s).unwrap()
}

fn main() {
    let path = Path::new("/tmp/translator.rs");
    let display = path.display();

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why.description()),
        Ok(file) => file,
    };

    file.write_all(format!("// {:?}",scan_structs()).as_bytes() ).unwrap();
}