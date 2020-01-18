// Copyright 2020 Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// The `quote!` macro requires deep recursion.
extern crate proc_macro;
extern crate proc_macro2;

use proc_macro::TokenStream;

// We use this macro to allow the 'snapshot' attribute to be used on structs.
// The version translator code generator will use custom attr 'default'.
#[proc_macro_derive(Snapshot, attributes(snapshot))]
pub fn snapshot_attributes(input: TokenStream) -> TokenStream {
    TokenStream::new()
}
