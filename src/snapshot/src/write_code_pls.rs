// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
extern crate snapshot_derive;

#[macro_use]
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro::TokenStream;
use syn::*;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug)]
struct SnapshotFieldAttr {
    name: String, 
    value: syn::Lit,
}

// Describes a structure type and fields.
// Is used as input for computing the translation code.
#[derive(Debug)]
pub struct StructDescriptor {
    ty: String,
    version: u16,
    fields: Vec<String>,
    field_types: Vec<String>,
    field_attrs: Vec<Vec<SnapshotFieldAttr>>,
}

// Returns the snapshot attribute name
fn get_field_attributes(attribute: &syn::Attribute) -> Vec<SnapshotFieldAttr> {
    let mut field_attributes = Vec::new();
    let meta: syn::Meta = attribute.parse_meta().unwrap();

    // Check if this is a snapshot attribute.
    match meta.clone() {
        syn::Meta::List(meta_list) => {
            // Check if this is a "snapshot" attribute.
            if meta_list.path.segments[0].ident.to_string() == "snapshot" {
                // Fetch the specific attribute name
                for nested_attribute in meta_list.nested {
                    // let snapshot_field_attr = SnapshotFieldAttr {
                    //     name:
                    // }

                    match nested_attribute {
                        syn::NestedMeta::Meta(nested_meta) => {
                            match nested_meta {
                                syn::Meta::NameValue(attr_name_value) => {
                                    // panic!("{:?}", attr_name_value);
                                    // if attr_name_value.eq_token.to_string() == "=" {
                                        field_attributes.push(
                                            SnapshotFieldAttr {
                                                name: attr_name_value.path.segments[0].ident.to_string(),
                                                value: attr_name_value.lit
                                            }
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
    // panic!("{:?}", meta);

    field_attributes
}

/// Input must be a string containing a rust source file
/// Output is a vector of struct descriptors.
pub fn scan_structs(input: String) -> syn::parse::Result<Vec<StructDescriptor>> {
    let rust_file: syn::File = syn::parse_file(&input)?;
    let mut descriptors = Vec::new();

    for item in rust_file.items {
        match item {
            syn::Item::Struct(struct_item) => {
                let mut descriptor = StructDescriptor {
                    version: 0,
                    ty: struct_item.ident.to_string(),
                    fields: vec![],
                    field_types: vec![],
                    field_attrs: vec![],
                };
                match struct_item.fields {
                    syn::Fields::Named(ref named_fields) => {
                        let pairs = named_fields.named.pairs();
                        for field in pairs.into_iter() {
                            let field_name = field.value().ident.as_ref().unwrap().to_string();
                            let mut field_type = String::new();

                            match &field.value().ty {
                                syn::Type::Path(token) => {
                                    for segment in token.path.segments.iter() {
                                        field_type = field_type + &segment.ident.to_string();
                                    }

                                    descriptor.fields.push(field_name);
                                    descriptor.field_types.push(field_type);
                                }
                                _ => {}
                            }
                            // Obtain struct attrs.
                            for attr in &struct_item.attrs {
                                let struct_attrs = get_field_attributes(&attr);
                                if struct_attrs.len() > 0 {
                                    for struct_attr in struct_attrs {
                                        match struct_attr.value {
                                            syn::Lit::Int(int_lit) => {
                                                if struct_attr.name == "version" {
                                                    descriptor.version = int_lit.base10_parse().unwrap();
                                                }
                                            }
                                            _ => {}
                                        }
                                        
                                    }
                                    break;
                                }
                            }
                            // Obtain field snapshot attributes.
                            let mut field_attrs = Vec::new();

                            for attr in &field.value().attrs {
                                let new_field_attrs = get_field_attributes(&attr);
                                if new_field_attrs.len() > 0 {
                                    field_attrs.extend(new_field_attrs);
                                    break;
                                }
                            }
                            descriptor.field_attrs.push(field_attrs);
                        }
                    }
                    _ => {}
                }
                descriptors.push(descriptor);
            }
            _ => {}
        }
    }

    Ok(descriptors)
}

fn generate_snapshot_impl(
    struct_descriptor: &StructDescriptor,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    let mut indent = "".to_owned();

    output.write_fmt(format_args!(
        "{}// Struct descriptor {:?}\n",
        indent, &struct_descriptor
    ))?;

    output.write_fmt(format_args!(
        "{}impl Snapshotable for {} {{\n",
        indent, struct_descriptor.ty
    ))?;
    indent += "    ";

    // Snapshot
    output.write_fmt(format_args!(
        "{}fn snapshot(&self, id: String, engine: &mut Snapshot) {{\n",
        indent
    ))?;
    indent += "    ";
    let fields = &struct_descriptor.fields;
    let field_types = &struct_descriptor.field_types;
    let field_attrs = &struct_descriptor.field_attrs;

    for i in 0..fields.len() {
        output.write_fmt(format_args!("{}// attributes = {:?}\n", indent, field_attrs[i]));
        output.write_fmt(format_args!("{}engine.set_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + \"{}\", 0, &self.{});\n", indent, fields[i], fields[i]))?;
    }

    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent));

    // Restore
    output.write_fmt(format_args!(
        "{}fn restore(id: String, engine: &mut Snapshot) -> Self {{\n",
        indent
    ))?;
    indent += "    ";
    output.write_fmt(format_args!("{} {} {{\n", indent, struct_descriptor.ty))?;
    indent += "    ";

    for i in 0..fields.len() {
        output.write_fmt(format_args!("{}{}: engine.get_snapshot_property(SnapshotPropKind::CONFIG, id.clone() + \"{}\").unwrap_or_default(),\n", indent, fields[i], fields[i]))?;
    }

    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent));
    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent));
    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent));

    Ok(())
}

fn scan_file(path: &Path) -> Vec<StructDescriptor> {
    let display = path.display();
    let mut file = match File::open(&path) {
        Err(why) => panic!("couldn't open {}: {}", display, why.description()),
        Ok(file) => file,
    };

    let mut s = String::new();
    match file.read_to_string(&mut s) {
        Err(why) => panic!("couldn't read {}: {}", display, why.description()),
        Ok(_) => print!("{} contains:\n{}", display, s),
    }

    scan_structs(s).unwrap()
}

fn main() {
    let path = Path::new("/tmp/translator.rs");
    let display = path.display();

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why.description()),
        Ok(file) => file,
    };

    let path = Path::new("./src/structs.rs");
    let struct_descriptors = scan_file(&path);
    file.write_fmt(format_args!(
        "// Code autogenerated by the Snapshot crate\n"
    ))
    .unwrap();
    file.write_fmt(format_args!(
        "// Number of structs: {}\n",
        struct_descriptors.len()
    ))
    .unwrap();

    for descriptor in struct_descriptors {
        generate_snapshot_impl(&descriptor, &mut file).unwrap();
    }
}
