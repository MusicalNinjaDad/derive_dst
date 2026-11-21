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
//! let mut uninit = DeviceDetails::new_uninit(8).unwrap();
//! // `cbSize` already holds its declared default; initialise the tail:
//! for word in uninit.DevicePath.iter_mut() {
//!     word.write(0);
//! }
//! // SAFETY: every field (the defaulted `cbSize` and all tail words) is initialised.
//! let details = unsafe { uninit.assume_init() };
//! assert_eq!(details.cbSize, 4);
//! assert_eq!(details.DevicePath.len(), 2);
//! ```
//!
//! the derive generates (with the same visibility as the struct):
//!
//! - `UninitDeviceDetails` — a `#[repr(C)]` **mirror struct** in which every field is wrapped in
//!   [`MaybeUninit`] (`cbSize: MaybeUninit<u32>`, `DevicePath: [MaybeUninit<u16>]`). `MaybeUninit<T>`
//!   is guaranteed to have the same size, alignment and ABI as `T` but no validity invariant, so the
//!   mirror has a layout identical to `DeviceDetails` yet is a valid value for *any* bit pattern —
//!   including a fresh, uninitialised allocation.
//! - `DeviceDetails::new_uninit(size: usize) -> Result<Box<UninitDeviceDetails>, InvalidDeviceDetails>`
//!   — a **safe** allocating constructor: it validates the requested size via
//!   [`layout_for`][Self::layout_for], allocates a buffer of the correct layout, attaches the tail
//!   element count as the pointer metadata and returns it boxed. Any sized field that declares a
//!   default (`field: Type = expr`) is initialised with it (via [`MaybeUninit::write`]); everything
//!   else is left for the caller to write.
//! - `UninitDeviceDetails::assume_init(self: Box<Self>) -> Box<DeviceDetails>` — the single `unsafe`
//!   boundary: once the caller has initialised every field, converts the box into the initialised
//!   type. The typical pattern is a user-written `new` that calls `new_uninit`, performs any
//!   additional initialisation and then calls `assume_init`.
//! - `DeviceDetails::layout_for(size: usize) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), InvalidDeviceDetails>`
//!   — returns the [`Layout`], the byte offset of the unsized tail and the tail element count
//!   (typed as the tail slice's `Pointee::Metadata`, i.e. the fat-pointer metadata — a `usize`
//!   for `[T]`) for a buffer of `size` bytes (the mirror has the identical layout, so this
//!   serves both).
//! - `InvalidDeviceDetails` — a dedicated error enum with variants
//!   `InvalidLayout(::core::alloc::LayoutError)` and `InvalidSize { requested: usize, valid: usize }`,
//!   implementing `Debug`, `Clone`, `PartialEq`, `Eq`, `Display`, `From<LayoutError>` and
//!   `core::error::Error` — including `Error::source`.
//!
//! # Requirements
//!
//! - a **nightly** toolchain (pinned via `rust-toolchain.toml`); the crate *using* the derive needs
//!   - `#![feature(ptr_metadata)]` for the generated `ptr::from_raw_parts_mut` / `ptr::metadata` calls,
//!   - `#![feature(default_field_values)]` only if sized fields declare `= expr` defaults.
//!
//!   The proc-macro itself gets rich, spanned compiler errors from
//!   [`proc_macro2_diagnostic`](https://github.com/MusicalNinjaDad/proc_macro2_diagnostic), which
//!   uses `#![feature(proc_macro_diagnostic)]` internally when available (detected via
//!   `build_safely`).
//! - the annotated type must be a **struct with named fields** and **`#[repr(C)]`**;
//! - the **last field must be an unsized slice** `[T]`; only the last field may be unsized;
//! - sized fields *may* declare a default value (`cbSize: u32 = ...`); defaults are optional and are
//!   written by `new_uninit` when present.
//!
//! # Errors
//!
//! Misuse (not a struct, missing `repr(C)`, sized tail, unsized non-tail field) is reported at
//! compile time with spanned diagnostics (via `proc_macro2_diagnostic`) and covered by compile-fail
//! tests in `tests/compilation/`. At runtime, an invalid `size` yields
//! `InvalidFoo::InvalidSize { requested, valid }` or `InvalidFoo::InvalidLayout(..)`.

use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToDiagnostic, ToTokens, prelude::*};
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, Generics, Ident, Meta, Token, Type, Visibility,
    punctuated::Punctuated, spanned::Spanned,
};

#[proc_macro_derive(DST)]
/// Derives an `UninitFoo` mirror struct, a safe allocating constructor `Foo::new_uninit`, an
/// `unsafe` `UninitFoo::assume_init` and a layout constructor `Foo::layout_for` for a
/// `#[repr(C)]` DST struct.
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

/// A destructured, validated `#[repr(C)]` struct with named fields and an unsized slice tail.
pub(crate) struct DstStruct<'ast> {
    vis: &'ast Visibility,
    name: &'ast Ident,
    generics: &'ast Generics,
    /// The sized "head" fields (all but the last); a declared `= expr` default is written by
    /// `new_uninit`, but defaults are optional.
    head: Vec<&'ast Field>,
    /// The last field, of type `[T]`.
    tail: &'ast Field,
    /// `T` in the tail `[T]`.
    tail_elem: &'ast Type,
}

impl<'ast> DstStruct<'ast> {
    /// Handles all the invariant validation.
    pub(crate) fn parse(ast: &'ast DeriveInput) -> DiagnosticResult<Self> {
        let name = &ast.ident;

        // Fail fast: must be a struct with named fields
        let struct_data = match &ast.data {
            Data::Struct(struct_data) => struct_data,
            Data::Enum(enum_data) => {
                return error("DST can only be derived for a struct")
                    .add_help(enum_data.enum_token.span(), "not a struct");
            }
            Data::Union(union_data) => {
                return error("DST can only be derived for a struct")
                    .add_help(union_data.union_token.span(), "not a struct");
            }
        };
        let fields = match &struct_data.fields {
            Fields::Named(fields) => &fields.named,
            Fields::Unnamed(fields) => {
                return error("DST can only be derived for a struct with named fields").add_help(
                    fields.paren_token.span.span(),
                    "tuple structs are not supported",
                );
            }
            Fields::Unit => {
                return error("DST can only be derived for a struct with named fields")
                    .add_help(name.span(), "unit structs have no fields");
            }
        };

        // repr(C) is required so the generated layout matches a C structand can be generated with
        // Layout::extend
        let has_repr_c = ast.attrs.iter().any(|attr| {
            if !attr.meta.path().is_ident("repr") {
                return false;
            }
            match &attr.meta {
                Meta::List(list) => list
                    .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                    .is_ok_and(|args| {
                        args.iter()
                            .any(|meta| matches!(meta, Meta::Path(path) if path.is_ident("C")))
                    }),
                _ => false,
            }
        });
        if !has_repr_c {
            return error("DST requires #[repr(C)] so the generated layout matches a C struct")
                .add_help(name.span(), "add #[repr(C)] above this");
        }

        let tail = fields
            .last()
            .or_error("DST requires at least one field (an unsized slice tail)")
            .add_help(name.span(), "add a final field like `DevicePath: [u16]`")?;
        let tail_elem = match &tail.ty {
            Type::Slice(slice) => &slice.elem,
            _ => return error(
                "the last field of a DST struct must be an unsized slice, e.g. `DevicePath: [u16]`",
            )
            .add_help(tail.ty.span(), "this field is sized"),
        };

        let mut head = Vec::with_capacity(fields.len() - 1);
        for field in fields.iter().take(fields.len() - 1) {
            match &field.ty {
                Type::Slice(slice) => {
                    return error("only the last field of a DST struct may be unsized").add_help(
                        slice.bracket_token.span.span(),
                        "move this slice to the last position, or make it sized e.g. `[u16; 4]`",
                    );
                }
                Type::TraitObject(trait_object) => {
                    return error("only the last field of a DST struct may be unsized").add_note(
                        trait_object.span(),
                        "deriving DSTs with `dyn Trait` as a tail is not supported",
                    );
                }
                Type::Path(path) if path.path.is_ident("str") => {
                    return error("only the last field of a DST struct may be unsized").add_note(
                        path.span(),
                        "deriving DSTs with `str` as a tail is not supported",
                    );
                }
                _ => {}
            }
            head.push(field);
        }

        Ok(Self {
            vis: &ast.vis,
            name,
            generics: &ast.generics,
            head,
            tail,
            tail_elem,
        })
    }

    /// Generates the `UninitFoo` mirror struct, the `InvalidFoo` error type and the
    /// `new_uninit` / `assume_init` / `layout_for` methods.
    pub(crate) fn generate(&self) -> TokenStream2 {
        let vis = self.vis;
        let name = self.name;
        let uninit = format_ident!("Uninit{}", name);
        let err = format_ident!("Invalid{}", name);
        let generics = self.generics;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let head_vis = self.head.iter().map(|field| &field.vis);
        let head_idents = self
            .head
            .iter()
            .map(|field| field.ident.as_ref().expect("named fields"));
        let head_tys = self.head.iter().map(|field| &field.ty);
        // separate iterator for the second repetition site in `layout_for` (Map is not Copy)
        let head_tys_layout = self.head.iter().map(|field| &field.ty);
        // sized fields with a declared `= expr` default: `(ident, expr)` pairs written by new_uninit
        let defaulted: Vec<(&Ident, &Expr)> = self
            .head
            .iter()
            .filter_map(|field| {
                field
                    .default
                    .as_ref()
                    .map(|(_eq, expr)| (field.ident.as_ref().expect("named fields"), expr))
            })
            .collect();
        let d_idents = defaulted.iter().map(|(ident, _)| *ident);
        let d_exprs = defaulted.iter().map(|(_, expr)| *expr);
        let tail_vis = &self.tail.vis;
        let tail_ident = self.tail.ident.as_ref().expect("named fields");
        let elem = self.tail_elem;
        // The tail slice's pointer-metadata type: the type used for the tail element count
        // throughout the generated signatures & returns (`usize` for `[T]`, but spelled via
        // `Pointee` so the signatures mirror the fat pointer's metadata).
        let count_ty = quote! { <[#elem] as ::core::ptr::Pointee>::Metadata };

        quote! {
            #[derive(Debug, Clone, PartialEq, Eq)]
            #vis enum #err {
                #[doc = "The requested size cannot be laid out for this type (e.g. overflow computing the tail layout)."]
                InvalidLayout(::core::alloc::LayoutError),
                #[doc = "`requested` is not a valid total size for this type; `valid` is the correctly aligned and padded size for the requested tail length."]
                InvalidSize { requested: usize, valid: usize },
            }

            impl ::core::fmt::Display for #err {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::InvalidLayout(_) => ::core::write!(
                            f,
                            "invalid layout requested for a {} buffer",
                            ::core::stringify!(#name)
                        ),
                        Self::InvalidSize { requested, valid } => ::core::write!(
                            f,
                            "{requested} is not valid as a size for {}; when correctly aligned and padded the next valid size is {valid}",
                            ::core::stringify!(#name)
                        ),
                    }
                }
            }

            impl ::core::error::Error for #err {
                fn source(&self) -> ::core::option::Option<&(dyn ::core::error::Error + 'static)> {
                    match self {
                        Self::InvalidLayout(error) => ::core::option::Option::Some(error),
                        Self::InvalidSize { .. } => ::core::option::Option::None,
                    }
                }
            }

            impl ::core::convert::From<::core::alloc::LayoutError> for #err {
                fn from(error: ::core::alloc::LayoutError) -> Self {
                    Self::InvalidLayout(error)
                }
            }

            #[doc = concat!(
                "`MaybeUninit` mirror of [`", stringify!(#name), "`]: identical `repr(C)` layout, but every "
            )]
            #[doc = concat!(
                "field is wrapped in `MaybeUninit`, so any bit pattern — including a fresh, uninitialised ",
                "allocation — is a valid value. Constructed by `new_uninit`; converted back to the ",
                "initialised type with [`Self::assume_init`]."
            )]
            #[derive(Debug)]
            #[repr(C)]
            #vis struct #uninit #generics #where_clause {
                #(#head_vis #head_idents: ::core::mem::MaybeUninit<#head_tys>,)*
                #tail_vis #tail_ident: [::core::mem::MaybeUninit<#elem>],
            }

            impl #impl_generics #name #ty_generics #where_clause {
                #[doc = "Allocate a buffer of `size` bytes and return it boxed as the `Uninit` mirror of `Self`."]
                #[doc = ""]
                #[doc = "The allocation is **uninitialised**, except for any sized field that declares a default"]
                #[doc = "(`field: Type = expr`), which is written here. Initialise the remaining fields with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err` if `size` is not a valid, correctly aligned and padded total size"]
                #[doc = "for this type — see [`layout_for`][Self::layout_for]."]
                #[doc = ""]
                #[doc = concat!(
                    "# Implementation safety\n\n",
                    "The internal `unsafe` block is sound: `raw` was allocated by `Global` with the ",
                    "`layout` computed by [`layout_for`][Self::layout_for] for `size`, so it is valid for ",
                    "reads & writes of that layout; `count` is the number of tail elements used to build ",
                    "that layout, so `ptr::from_raw_parts_mut` attaches the correct metadata for the ",
                    "mirror (last field: `[MaybeUninit<",
                    stringify!(#elem),
                    ">]`). Every field of the mirror is `MaybeUninit<_>`, which is valid for ANY bit ",
                    "pattern, so the uninitialised allocation is a valid pointee; the pointer is moved ",
                    "into exactly one `Box` and not used afterwards."
                )]
                pub fn new_uninit(
                    size: usize,
                ) -> ::core::result::Result<::std::boxed::Box<#uninit #ty_generics>, #err> {
                    let (layout, _offset, count) = Self::layout_for(size)?;

                    let raw = match <::std::alloc::Global as ::core::alloc::Allocator>::allocate(
                        &::std::alloc::Global,
                        layout,
                    ) {
                        ::core::result::Result::Ok(pointer) => pointer,
                        ::core::result::Result::Err(_) => ::std::alloc::handle_alloc_error(layout),
                    };

                    let wide = ::core::ptr::from_raw_parts_mut::<#uninit #ty_generics>(
                        raw.cast::<()>().as_ptr(),
                        count,
                    );

                    // SAFETY:
                    // **No double-free risk**
                    // - this pointer will not be added to a second box, or used in any other way after this
                    // **Memory layout is correct**
                    // - Pointer was allocated in a single allocation above by Global with the layout
                    //   computed by layout_for for `size`
                    // **Metadata is correct**
                    // - `count` is the number of tail elements used to build that layout, provided by
                    //   layout_for and used in from_raw_parts_mut
                    // **The pointee is valid for any bytes**
                    // - every field of the mirror is `MaybeUninit<_>` (no validity invariant), so the
                    //   uninitialised allocation is a valid value of the mirror type
                    let mut boxed = unsafe { ::std::boxed::Box::from_raw(wide) };

                    // declared defaults (`field: Type = expr`) are written safely via MaybeUninit::write
                    #(boxed.#d_idents.write(#d_exprs);)*

                    ::core::result::Result::Ok(boxed)
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, tail element count)`"]
                #[doc = "for a `Self` buffer of `size` bytes. The count is typed as the tail slice's"]
                #[doc = "`Pointee::Metadata` (a `usize` for `[T]`), matching the fat-pointer metadata."]
                #[doc = "The `Uninit` mirror has the identical layout."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(InvalidLayout(..))` if the layout cannot be computed and"]
                #[doc = "`Err(InvalidSize { requested, valid })` if `size` is smaller than the sized head or"]
                #[doc = "does not match the correctly aligned and padded layout."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The correctness of this layout and the tail offset is relied upon by unsafe code"]
                #[doc = "in [`new_uninit`][Self::new_uninit]."]
                pub fn layout_for(
                    size: usize,
                ) -> ::core::result::Result<(::core::alloc::Layout, usize, #count_ty), #err> {
                    // sequential `extend` reproduces the repr(C) placement of the sized head
                    let mut prefix = ::core::alloc::Layout::new::<()>();
                    #(
                        prefix = prefix.extend(::core::alloc::Layout::new::<#head_tys_layout>())?.0;
                    )*

                    let elem_size = ::core::mem::size_of::<#elem>();
                    let Some(tail_bytes) = size.checked_sub(prefix.size()) else {
                        return ::core::result::Result::Err(#err::InvalidSize {
                            requested: size,
                            valid: prefix.pad_to_align().size(),
                        });
                    };
                    let count: #count_ty = if elem_size == 0 { 0 } else { tail_bytes / elem_size };

                    let tail = ::core::alloc::Layout::array::<#elem>(count)?;
                    let (extended, offset) = prefix.extend(tail)?;
                    let layout = extended.pad_to_align();

                    if layout.size() != size {
                        return ::core::result::Result::Err(#err::InvalidSize {
                            requested: size,
                            valid: layout.size(),
                        });
                    }

                    ::core::result::Result::Ok((layout, offset, count))
                }
            }

            impl #impl_generics #uninit #ty_generics #where_clause {
                #[doc = "Convert into the fully initialised struct, reusing the same allocation."]
                #[doc = ""]
                #[doc = "# Safety"]
                #[doc = "Every field of `self` must have been initialised: each sized field (a `MaybeUninit`)"]
                #[doc = "via [`MaybeUninit::write`], and every element of the unsized tail likewise."]
                #[doc = "Reading a field that was never written is undefined behaviour."]
                pub unsafe fn assume_init(
                    self: ::std::boxed::Box<Self>,
                ) -> ::std::boxed::Box<#name #ty_generics> {
                    let raw = ::std::boxed::Box::into_raw(self);
                    // The mirror and the original have identical layouts (both `repr(C)`, and
                    // `MaybeUninit<T>` is guaranteed to have the same size, alignment and ABI as `T`),
                    // and the tail metadata (element count) is carried over unchanged.
                    let wide = ::core::ptr::from_raw_parts_mut::<#name #ty_generics>(
                        raw.cast::<()>(),
                        ::core::ptr::metadata(raw),
                    );
                    // SAFETY:
                    // - the caller guarantees (see `# Safety` above) that every field is initialised,
                    //   so the pointee is a valid `#name` value
                    // - layout and metadata are identical to the mirror's, so the fat pointer is correct
                    // - the pointer is moved into exactly one `Box` and not used afterwards
                    unsafe { ::std::boxed::Box::from_raw(wide) }
                }
            }
        }
    }
}
