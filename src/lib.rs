//! `derive_dst` provides `#[derive(DST)]` for `#[repr(C)]` structs whose **last field is an
//! unsized slice** (`[T]`) — the usual way to mirror a C/C++ "flexible array member" struct such
//! as `SP_DEVICE_INTERFACE_DETAIL_DATA_W`.
//!
//! # What is generated
//!
//! For
//!
//! ```
//! #![feature(default_field_values)]
//! #![feature(ptr_metadata)]
//! use derive_dst::DST;
//!
//! #[repr(C)]
//! #[derive(Debug, PartialEq, Eq, DST)]
//! struct DeviceDetails {
//!     cbSize: u32 = ::core::mem::size_of::<u32>() as u32,
//!     DevicePath: [u16],
//! }
//!
//! let details = DeviceDetails::new(8).unwrap();
//! assert_eq!(details.cbSize, 4);
//! assert_eq!(details.DevicePath.len(), 2);
//! ```
//!
//! the derive generates (with the same visibility as the struct):
//!
//! - `InvalidDeviceDetails` — a dedicated error enum with variants
//!   `InvalidLayout(::core::alloc::LayoutError)` and `InvalidSize { requested: usize, valid: usize }`,
//!   implementing `Debug`, `Clone`, `PartialEq`, `Eq`, `Display`, `From<LayoutError>` and
//!   `std::error::Error` — including `Error::source`.
//! - `DeviceDetails::new(size: usize) -> Result<Box<Self>, InvalidDeviceDetails>` — validates the
//!   requested size via [`layout_for`][Self::layout_for], allocates a zeroed buffer of the correct
//!   layout, initialises every sized field with its declared default (`field: Type = expr`) and
//!   returns the value boxed.
//! - `DeviceDetails::layout_for(size: usize) -> Result<(Layout, usize, usize), InvalidDeviceDetails>`
//!   — returns the [`Layout`], the byte offset of the unsized tail and the number of tail elements
//!   for a buffer of `size` bytes.
//!
//! # Requirements
//!
//! - a **nightly** toolchain (pinned via `rust-toolchain.toml`); the crate *using* the derive needs
//!   - `#![feature(default_field_values)]` for the `field: Type = expr` defaults, and
//!   - `#![feature(ptr_metadata)]` for the generated `ptr::from_raw_parts_mut` call in `new`.
//!
//!   The proc-macro itself gets rich, spanned compiler errors from
//!   [`proc_macro2_diagnostic`](https://github.com/MusicalNinjaDad/proc_macro2_diagnostic), which
//!   uses `#![feature(proc_macro_diagnostic)]` internally when available (detected via
//!   `build_safely`).
//! - the annotated type must be a **struct with named fields** and **`#[repr(C)]`**;
//! - the **last field must be an unsized slice** `[T]`; only the last field may be unsized;
//! - every other (sized) field must declare a **default value** (`cbSize: u32 = ...`), which `new`
//!   assigns after allocating zeroed memory — the tail element type `T` must therefore be valid
//!   when zeroed (true for all primitive numeric types).
//!
//! # Errors
//!
//! Misuse (not a struct, missing `repr(C)`, sized tail, unsized non-tail field, missing default)
//! is reported at compile time with spanned diagnostics (via `proc_macro2_diagnostic`) and covered
//! by compile-fail tests in `tests/compilation/`. At runtime, an invalid `size` yields
//! `InvalidFoo::InvalidSize { requested, valid }` or `InvalidFoo::InvalidLayout(..)`.

use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToTokens, prelude::*};
use syn::DeriveInput;

mod parse;
use parse::DstStruct;

#[proc_macro_derive(DST)]
/// Derives an allocating constructor and a layout constructor for a `#[repr(C)]` DST struct.
///
/// See the [crate-level documentation](crate) for the full contract and generated code.
pub fn derive_dst(input: TokenStream1) -> TokenStream1 {
    impl_dst(input.into()).to_tokens()
}

/// Parses & validates the input, then generates the impls and error type.
fn impl_dst(input: TokenStream2) -> DiagnosticStream {
    let ast: DeriveInput = syn::parse2(input).expect("derive macro input");

    let dst = DstStruct::parse(&ast)?;

    Ok(dst.generate())
}
