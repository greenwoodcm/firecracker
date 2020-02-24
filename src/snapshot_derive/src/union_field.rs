use std::collections::hash_map::HashMap;

#[derive(Debug, Eq, PartialEq, Clone)]
struct UnionField {
    ty: syn::Type,
    size: u32,
    name: String,
    start_version: u16,
    end_version: u16,
    attrs: HashMap<String, syn::Lit>,
}
