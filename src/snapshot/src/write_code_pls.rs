// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
extern crate snapshot_derive;

#[macro_use]
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;
extern crate array_tool;

use proc_macro::TokenStream;
use syn::*;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::{Read, Write};
use std::path::Path;

use array_tool::vec::{Intersect, Uniq};

#[derive(Debug, Eq, PartialEq, Clone)]
struct SnapshotFieldAttr {
    name: String, 
    value: syn::Lit,
}

// Describes a structure type and fields.
// Is used as input for computing the translation code.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct StructDescriptor {
    ty: String,
    version: u16,
    fields: Vec<String>,
    field_types: Vec<String>,
    field_attrs: Vec<Vec<SnapshotFieldAttr>>,
}

// Returns true if field is snapshotable and the struct version.
fn field_is_snapshotable(descriptors: &Vec<StructDescriptor>, ty: &str) -> (bool, u16) {
    if let Some(desc) = descriptors.iter().find(|&x| x.ty == ty) {
        (true, desc.version)
    } else {
        (false, 0)
    }
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

    field_attributes
}

fn get_struct_version(struct_item: &syn::ItemStruct) -> u16 {
    // Scan struct attrs.
    for attr in &struct_item.attrs {
        let struct_attrs = get_field_attributes(&attr);
        for struct_attr in struct_attrs {
            match struct_attr.value {
                syn::Lit::Int(int_lit) => {
                    if struct_attr.name == "version" {
                        return int_lit.base10_parse().unwrap();
                    }
                }
                _ => {}
            }
        }
    }
    0
}

/// Input must be a string containing a rust source file
/// Output is a vector of struct descriptors.
pub fn scan_structs(input: String) -> syn::parse::Result<Vec<StructDescriptor>> {
    let rust_file: syn::File = syn::parse_file(&input)?;
    let mut descriptors = Vec::new();

    // Well, this is gonna be hard to read ...
    for item in rust_file.items {
        match item {
            syn::Item::Struct(struct_item) => {
                let struct_version = get_struct_version(&struct_item);
                if struct_version == 0 {
                    // Ignore unversioned structs.
                    continue;
                }

                let mut descriptor = StructDescriptor {
                    version: get_struct_version(&struct_item),
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
                         
                            // Obtain field snapshot attributes.
                            let mut field_attrs = Vec::new();

                            for attr in &field.value().attrs {
                                field_attrs.extend(get_field_attributes(&attr));
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

// Generate translations from source descriptor to multiple target descriptors.
fn generate_snapshot_fn(
    parent_indent: &String,
    source: &StructDescriptor,
    targets: &Vec<StructDescriptor>,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    output.write_fmt(format_args!(
        "{}fn snapshot(&self, id: String, version: u16, snapshot: &mut Snapshot) {{\n",
        parent_indent
    ))?;

    let mut indent = String::from(parent_indent) + "    ";
    let fields = &source.fields;
    let version = source.version;
    let field_types = &source.field_types;
    let field_attrs = &source.field_attrs;

    // Start matching version here.
    output.write_fmt(format_args!("{}match version {{\n", indent))?;
    indent = indent + "    ";

    // Same version
    output.write_fmt(format_args!("{}{} => {{\n", indent, source.version))?;
    indent = indent + "    ";
    
    for i in 0..fields.len() {
        output.write_fmt(format_args!("{}// attributes = {:?}\n", indent, field_attrs[i]));
        let (field_snapshotable, struct_version) = field_is_snapshotable(&targets, field_types[i].as_str());
        if field_snapshotable {
            // This struct implements Snapshot, use that interface to serialize.
            output.write_fmt(format_args!("{}self.{}.snapshot(id.clone() + \".{}\", {}, snapshot);\n", indent, fields[i], fields[i], struct_version))?;
        } else {
            output.write_fmt(format_args!("{}snapshot.set_object(SnapshotObjectType::Field, id.clone() + \"{}\", {}, &self.{});\n", indent, fields[i], version, fields[i]))?;
        }
    }

    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent))?;
    // End same version

    for target in targets {
        let common_fields = fields.intersect(target.fields.clone());

        // Target version common fields start 
        output.write_fmt(format_args!("{}{} => {{\n", indent, target.version))?;
        indent = indent + "    ";
        
        // Handle common fields
        for i in 0..common_fields.len() {
            // Find the index of the common field name and use that index to find its attr type
            let common_field_index = fields.iter().position(|x| x == &common_fields[i]).unwrap();
            let (field_snapshotable, struct_version) = field_is_snapshotable(&targets, field_types[common_field_index].as_str());

            if field_snapshotable {
                // This struct implements Snapshot, use that interface to serialize.
                output.write_fmt(format_args!("{}self.{}.snapshot(id.clone() + \".{}\", {}, snapshot);\n", indent, common_fields[i], common_fields[i], struct_version))?;
            } else {
                output.write_fmt(format_args!("{}snapshot.set_object(SnapshotObjectType::Field, id.clone() + \"{}\", {}, &self.{});\n", indent, common_fields[i], target.version, common_fields[i]))?;
            }
        }

        // Source/Target unique fields are not saved. Restore will handle their default values 
        // if needed.
        indent = indent[4..].to_string();
        output.write_fmt(format_args!("{}}}\n", indent))?;
    }

    // Same version
    output.write_fmt(format_args!("{}_ => {{ panic!(\"Attempted to translate to unknown version: {{}}\", version)}}\n", indent))?;
    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent))?;
    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent))?;
    Ok(())
}

fn generate_restore_fn(
    parent_indent: &String,
    struct_descriptor: &StructDescriptor,
    targets: &Vec<StructDescriptor>,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    output.write_fmt(format_args!(
        "{}fn restore(id: String, snapshot: &mut Snapshot) -> Self {{\n",
        parent_indent
    ))?;

    let mut indent = String::from(parent_indent) + "    ";
    output.write_fmt(format_args!("{} {} {{\n", indent, struct_descriptor.ty))?;
    indent = indent + &String::from("    ");
    let fields = &struct_descriptor.fields;
    let field_types = &struct_descriptor.field_types;
    let field_attrs = &struct_descriptor.field_attrs;

    for i in 0..fields.len() {
        // Check if field implements the Snapshot trait
        let (field_snapshotable, _) = field_is_snapshotable(&targets, field_types[i].as_str());

        if field_snapshotable {
            // This struct implements Snapshot, use that interface to serialize.
            output.write_fmt(format_args!("{}{}: {}::restore(id.clone() + \".{}\", snapshot),\n", indent, fields[i], field_types[i], fields[i]))?;
            continue;
        }

        // Get default field value
        if let Some(default_attribute) = field_attrs[i].iter().find(|&x| x.name == "default") {
            output.write_fmt(format_args!("{}// snapshot default attr = {:?}\n", indent, default_attribute));
            match &default_attribute.value {
                syn::Lit::Str(lit_str) => {
                    output.write_fmt(format_args!(
                        "{}{}: snapshot.get_object(id.clone() + \"{}\").unwrap_or(\"{}\".to_owned()),\n",
                        indent, fields[i], fields[i], lit_str.value()
                    ))?;
                }
                syn::Lit::Int(lit_int) => {
                    let literal: u64 = lit_int.base10_parse().unwrap();
                    output.write_fmt(format_args!(
                        "{}{}: snapshot.get_object(id.clone() + \"{}\").unwrap_or({}),\n",
                        indent, fields[i], fields[i], literal
                    ))?;
                }
                syn::Lit::Bool(lit_bool) => {
                    output.write_fmt(format_args!(
                        "{}{}: snapshot.get_object(id.clone() + \"{}\").unwrap_or({}),\n",
                        indent, fields[i], fields[i], lit_bool.value
                    ))?;
                }
                // syn::Lit::Byte(LitByte),
                // syn::Lit::Char(LitChar),
                // syn::Lit::Float(LitFloat),
                _ => {
                    panic!("Unsupported default value literal");
                } 
            }
        } else {
            // Use Default trait.
            output.write_fmt(format_args!("{}{}: snapshot.get_object(id.clone() + \"{}\").unwrap_or_default(),\n", indent, fields[i], fields[i]))?;
        }
    }

    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent))?;
    indent = indent[4..].to_string();
    output.write_fmt(format_args!("{}}}\n", indent))?;
    Ok(())
}


fn generate_snapshot_impl(
    source: &StructDescriptor,
    targets: &Vec<StructDescriptor>,
    output: &mut dyn Write,
) -> std::io::Result<()> {
    let mut indent = String::new();

    output.write_fmt(format_args!(
        "{}// {:?}\n",
        indent, &source
    ))?;

    output.write_fmt(format_args!(
        "{}impl Snapshotable for {} {{\n",
        indent, source.ty
    ))?;
    indent = indent + &String::from("    ");
    
    generate_snapshot_fn(&indent, source, targets, output)?;
    // We do not need the other struct descriptors to perform restore,
    // as the structure is assembled from what is available in the object store
    // We need it to be able to find if a field type is Snapshotable.
    generate_restore_fn(&indent, source, targets, output)?;

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
    let mut struct_descriptors = scan_file(&path);
    file.write_fmt(format_args!(
        "// File fenerated by Snapshot {}\n// DO NOT EDIT!\n", env!("CARGO_PKG_VERSION")
    ))
    .unwrap();
    file.write_fmt(format_args!(
        "// Number of structs: {}\n",
        struct_descriptors.len()
    ))
    .unwrap();

    // Sort by version in reverse
    struct_descriptors.sort_by(|a, b| b.version.cmp(&a.version));
    // Translate from latest to all other
    let source = struct_descriptors.remove(0);
    generate_snapshot_impl(&source, &struct_descriptors, &mut file).unwrap();

    // Debug only: generate snapshot impl for all structs 
    while struct_descriptors.len() > 0 {
        generate_snapshot_impl(&struct_descriptors.remove(0), &struct_descriptors, &mut file).unwrap();
    }

}
