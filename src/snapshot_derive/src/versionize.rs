
// Trait that defines a generic behaviour as a field level serialization and
// deseriailization code generator
pub(crate) trait FieldVersionize {
    fn get_default(&self) -> Option<syn::Ident>;
    fn get_semantic_ser(&self) -> Option<syn::Ident> { None }
    fn get_semantic_de(&self) -> Option<syn::Ident> { None }

    fn get_attr(&self, attr: &str) -> Option<&syn::Lit>;

    fn generate_serializer(&self, target_version: u16) -> proc_macro2::TokenStream;
    fn generate_deserializer(&self, source_version: u16) -> proc_macro2::TokenStream;

    fn generate_semantic_serializer(&self, target_version: u16) -> proc_macro2::TokenStream;
    fn generate_semantic_deserializer(&self, source_version: u16) -> proc_macro2::TokenStream;

    fn get_start_version(&self) -> u16;
    fn get_end_version(&self) -> u16;

    fn is_array(&self) -> bool { false }
}