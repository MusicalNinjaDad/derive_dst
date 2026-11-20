#![cfg_attr(unstable_proc_macro_diagnostic, feature(proc_macro_diagnostic))]

//! TODO: implement `#[derive(DST)]` (see README.md)

use proc_macro::TokenStream;

#[proc_macro_derive(DST)]
pub fn derive_dst(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
