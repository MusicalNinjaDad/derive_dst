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
//! let (mut uninit, (layout, tail_offset, count)) = DeviceDetails::new_uninit_for_size(8).unwrap();
//! assert_eq!(layout.size(), 8); // 8 is a valid size: 4-byte head + 2 x u16, correctly padded
//! assert_eq!(tail_offset, 4);
//! assert_eq!(count, 2);
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
//! the derive generates **private items in the same module** (the caller owns the public API —
//! e.g. a hand-written `pub fn new` that wraps these helpers — and no implementation details leak
//! into it):
//!
//! - `UninitDeviceDetails` — a `#[repr(C)]` **mirror struct** in which every field is wrapped in
//!   [`MaybeUninit`] (`cbSize: MaybeUninit<u32>`, `DevicePath: [MaybeUninit<u16>]`). `MaybeUninit<T>`
//!   is guaranteed to have the same size, alignment and ABI as `T` but no validity invariant, so the
//!   mirror has a layout identical to `DeviceDetails` yet is a valid value for *any* bit pattern —
//!   including a fresh, uninitialised allocation.
//! - `DeviceDetails::new_uninit_for_size(size: usize) -> Result<(Box<UninitDeviceDetails>, (Layout, usize, <[u16] as Pointee>::Metadata)), LayoutError>`
//!   and `DeviceDetails::new_uninit_for_count(count: <[u16] as Pointee>::Metadata) -> Result<(Box<UninitDeviceDetails>, (Layout, usize, <[u16] as Pointee>::Metadata)), LayoutError>`
//!   — **safe** allocating constructors, taking the total buffer size or the tail element count
//!   respectively: they compute the layout via [`layout_for_size`][Self::layout_for_size] /
//!   [`layout_for_count`][Self::layout_for_count], allocate a buffer of that layout, attach the
//!   tail element count as the pointer metadata and return it boxed **together with the
//!   `(Layout, tail offset, tail count)` tuple** — the same tuple the `layout_for_*` constructors
//!   return — so initialisation routines do not need to call `layout_for_*` again. Any sized field that declares a default
//!   (`field: Type = expr`) is initialised with it (via [`MaybeUninit::write`]); everything
//!   else is left for the caller to write. A `size` that does not match the correctly aligned and
//!   padded layout is **not** rejected: the returned `Layout` carries the valid size
//!   (`layout.size()`), so the caller can compare it with the requested `size` and decide how to
//!   handle a mismatch before calling `assume_init`.
//! - `DeviceDetails::new_uninit_for((layout, tail_offset, count): (Layout, usize, <[u16] as Pointee>::Metadata)) -> (Box<UninitDeviceDetails>, (Layout, usize, <[u16] as Pointee>::Metadata))`
//!   — the **single allocation site** shared by `new_uninit_for_size` / `new_uninit_for_count` (they
//!   just feed it the tuple their `layout_for_*` counterpart returns), so the `unsafe`
//!   allocate → `from_raw_parts_mut` → `Box::from_raw` sequence exists exactly once and the layout is
//!   computed exactly once per call.
//! - `UninitDeviceDetails::assume_init(self: Box<Self>) -> Box<DeviceDetails>` — the single `unsafe`
//!   boundary: once the caller has initialised every field, converts the box into the initialised
//!   type. The typical pattern is a user-written `new` that calls `new_uninit_for_size` (or
//!   `new_uninit_for_count`), performs any additional initialisation and then calls `assume_init`.
//! - `DeviceDetails::layout_for_size(size: usize) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError>`
//!   and `DeviceDetails::layout_for_count(count: <[u16] as Pointee>::Metadata) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError>`
//!   — return the `(Layout, tail offset, tail count)` tuple: the [`Layout`], the byte offset of the unsized tail and the tail element count
//!   (typed as the tail slice's `Pointee::Metadata`, i.e. the fat-pointer metadata — a `usize`
//!   for `[T]`) for a buffer of `size` bytes or of `count` tail elements (the mirror has the
//!   identical layout, so this serves both). They are built from two shared helpers:
//!   `DeviceDetails::layout_header() -> Result<Layout, LayoutError>` (the sized head's layout,
//!   computed once) and `DeviceDetails::extend_with_tail(header, count) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError>`
//!   (head + tail of `count` elements, padded to alignment).
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
//! - the **last field must be an unsized slice** `[T]`; only the last field may be unsized. The
//!   generated impl headers carry the explicit bounds `where [T]: Pointee<Metadata = usize>, T: Sized`
//!   — pinning the tail's fat-pointer metadata to `usize` (what the unsafe code relies on) and making
//!   `size_of::<T>()` / `Layout::array::<T>()` well-defined. Zero-sized elements (`tail: [()]`) are
//!   allowed: `layout_for_size` then maps any `size` to a tail count of 0 (the divide is
//!   `checked_div(..).unwrap_or(0)`, which cannot panic), while `layout_for_count` accepts any count;
//! - sized fields *may* declare a default value (`cbSize: u32 = ...`); defaults are optional and are
//!   written by the `new_uninit_for_*` constructors when present.
//!
//! # Errors
//!
//! Misuse (not a struct, missing `repr(C)`, sized tail, unsized non-tail field) is reported at
//! compile time with spanned diagnostics (via `proc_macro2_diagnostic`) and covered by compile-fail
//! tests in `tests/compilation/`. At runtime the only failure mode is an uncomputable layout (e.g.
//! the tail array overflows `isize::MAX`): every generated function then returns
//! `Err(::core::alloc::LayoutError)`. A `size` that does not match the correctly aligned and
//! padded layout for the derived tail count is deliberately **not** an error — the returned
//! `Layout` carries the valid size, so the caller can detect and handle the mismatch itself.

use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToDiagnostic, ToTokens, prelude::*};
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, Generics, Ident, Meta, Token, Type,
    punctuated::Punctuated, spanned::Spanned,
};

#[proc_macro_derive(DST)]
/// Derives an `UninitFoo` mirror struct, safe allocating constructors `Foo::new_uninit_for_size`
/// and `Foo::new_uninit_for_count`, an `unsafe` `UninitFoo::assume_init` and layout constructors
/// `Foo::layout_for_size` / `Foo::layout_for_count` for a `#[repr(C)]` DST struct.
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
            name,
            generics: &ast.generics,
            head,
            tail,
            tail_elem,
        })
    }

    /// Generates the `UninitFoo` mirror struct and the `new_uninit_for_size` /
    /// `new_uninit_for_count` / `assume_init` / `layout_for_size` / `layout_for_count` methods
    /// (all returning `core::alloc::LayoutError` on failure).
    pub(crate) fn generate(&self) -> TokenStream2 {
        let name = self.name;
        let uninit = format_ident!("Uninit{}", name);
        let generics = self.generics;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let head_vis = self.head.iter().map(|field| &field.vis);
        let head_idents = self
            .head
            .iter()
            .map(|field| field.ident.as_ref().expect("named fields"));
        let head_tys = self.head.iter().map(|field| &field.ty);
        // repetition site in `layout_header` (Map is not Copy)
        let head_tys_header = self.head.iter().map(|field| &field.ty);
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
        // Both generated impl headers carry the same explicit bounds, in a single place each:
        // - `[#elem]: Pointee<Metadata = usize>` pins the tail slice's fat-pointer metadata to
        //   `usize`, so the count type in the signatures is known to be `usize` *by the bound*
        //   (not just by projection normalization) — this is what the unsafe code in
        //   `new_uninit_for` / `assume_init` relies on when attaching/carrying the metadata;
        // - `#elem: Sized` makes `size_of::<#elem>()` / `Layout::array::<#elem>()` well-defined
        //   (ZST elements like `()` are fine — they just make the element size 0).
        let mut impl_where: syn::WhereClause = match &self.generics.where_clause {
            Some(where_clause) => where_clause.clone(),
            None => syn::WhereClause {
                where_token: Default::default(),
                predicates: Default::default(),
            },
        };
        impl_where
            .predicates
            .push(syn::parse_quote! { [#elem]: ::core::ptr::Pointee<Metadata = usize> });
        impl_where
            .predicates
            .push(syn::parse_quote! { #elem: ::core::marker::Sized });

        quote! {
            #[doc = concat!(
                "`MaybeUninit` mirror of [`", stringify!(#name), "`]: identical `repr(C)` layout, but every "
            )]
            #[doc = concat!(
                "field is wrapped in `MaybeUninit`, so any bit pattern — including a fresh, uninitialised ",
                "allocation — is a valid value. Constructed by `new_uninit_for_size` / `new_uninit_for_count`; ",
                "converted back to the initialised type with [`Self::assume_init`]."
            )]
            #[derive(Debug)]
            #[repr(C)]
            struct #uninit #generics #where_clause {
                #(#head_vis #head_idents: ::core::mem::MaybeUninit<#head_tys>,)*
                #tail_vis #tail_ident: [::core::mem::MaybeUninit<#elem>],
            }

            impl #impl_generics #name #ty_generics #impl_where {
                #[doc = "Allocate a buffer for a `Self` value of `size` bytes and return it boxed as the"]
                #[doc = "`Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple."]
                #[doc = ""]
                #[doc = "The tail element count is derived from `size` by [`layout_for_size`][Self::layout_for_size];"]
                #[doc = "the allocation then proceeds exactly as for [`new_uninit_for`][Self::new_uninit_for]."]
                #[doc = ""]
                #[doc = "The allocation is **uninitialised**, except for any sized field that declares a default"]
                #[doc = "(`field: Type = expr`), which is written here. Initialise the remaining fields with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if the layout cannot be computed (e.g. the tail array"]
                #[doc = "overflows) — see [`layout_for_size`][Self::layout_for_size]."]
                #[doc = ""]
                #[doc = "# Returns"]
                #[doc = "The boxed mirror together with the `(Layout, tail offset, tail count)` tuple — the same"]
                #[doc = "tuple [`layout_for_size`][Self::layout_for_size] returns (the count typed as the tail"]
                #[doc = "slice's `Pointee::Metadata`) — so initialisation routines do not need to call"]
                #[doc = "[`layout_for_size`][Self::layout_for_size] again."]
                #[doc = ""]
                #[doc = "Note: a `size` that does not match the correctly aligned and padded layout for the derived"]
                #[doc = "tail count is **not** rejected — the returned `Layout` carries the valid size"]
                #[doc = "(`layout.size()`), so the caller can compare it with the requested `size` and decide how to"]
                #[doc = "handle a mismatch before calling `assume_init`."]
                fn new_uninit_for_size(
                    size: usize,
                ) -> ::core::result::Result<
                    (
                        ::std::boxed::Box<#uninit #ty_generics>,
                        (::core::alloc::Layout, usize, #count_ty),
                    ),
                    ::core::alloc::LayoutError,
                > {
                    ::core::result::Result::Ok(Self::new_uninit_for(Self::layout_for_size(size)?))
                }

                #[doc = "Allocate a buffer holding `count` tail elements and return it boxed as the `Uninit`"]
                #[doc = "mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple. `count` is"]
                #[doc = "typed as the tail slice's `Pointee::Metadata` (a `usize` for `[T]`), matching the"]
                #[doc = "fat-pointer metadata."]
                #[doc = ""]
                #[doc = "This is [`new_uninit_for`][Self::new_uninit_for] fed with the tuple from"]
                #[doc = "[`layout_for_count`][Self::layout_for_count] — the layout is computed exactly once."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if the layout for `count` tail elements cannot be computed"]
                #[doc = "(e.g. overflow) — see [`layout_for_count`][Self::layout_for_count]."]
                #[doc = ""]
                #[doc = "# Returns"]
                #[doc = "The boxed mirror together with the `(Layout, tail offset, tail count)` tuple — the same"]
                #[doc = "tuple [`layout_for_count`][Self::layout_for_count] returns — so initialisation routines"]
                #[doc = "do not need to call [`layout_for_count`][Self::layout_for_count] again."]
                fn new_uninit_for_count(
                    count: #count_ty,
                ) -> ::core::result::Result<
                    (
                        ::std::boxed::Box<#uninit #ty_generics>,
                        (::core::alloc::Layout, usize, #count_ty),
                    ),
                    ::core::alloc::LayoutError,
                > {
                    ::core::result::Result::Ok(Self::new_uninit_for(Self::layout_for_count(count)?))
                }

                #[doc = "Allocate a buffer of `layout` bytes, attach `count` as the tail metadata and return it"]
                #[doc = "boxed as the `Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail"]
                #[doc = "count)` tuple it was built for. This is the **single allocation site** shared by"]
                #[doc = "[`new_uninit_for_size`][Self::new_uninit_for_size] and"]
                #[doc = "[`new_uninit_for_count`][Self::new_uninit_for_count]: pass it the tuple returned by"]
                #[doc = "[`layout_for_size`][Self::layout_for_size] / [`layout_for_count`][Self::layout_for_count]."]
                #[doc = ""]
                #[doc = "The allocation is **uninitialised**, except for any sized field that declares a default"]
                #[doc = "(`field: Type = expr`), which is written here. Initialise the remaining fields with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "# Aborts"]
                #[doc = "Aborts via `handle_alloc_error` if the allocation fails; the `layout` is taken as"]
                #[doc = "given (computed by `layout_for_*`), so it cannot fail here."]
                #[doc = ""]
                #[doc = "# Returns"]
                #[doc = "The boxed mirror together with the `(Layout, tail offset, tail count)` tuple passed in."]
                #[doc = ""]
                #[doc = concat!(
                    "# Implementation safety\n\n",
                    "The internal `unsafe` block is sound: `raw` was allocated by `Global` with the ",
                    "given `layout`, so it is valid for reads & writes of that layout; `count` is the ",
                    "number of tail elements that layout was built for, so `ptr::from_raw_parts_mut` ",
                    "attaches the correct metadata for the mirror (last field: `[MaybeUninit<",
                    stringify!(#elem),
                    ">]`). Every field of the mirror is `MaybeUninit<_>`, which is valid for ANY bit ",
                    "pattern, so the uninitialised allocation is a valid pointee; the pointer is moved ",
                    "into exactly one `Box` and not used afterwards."
                )]
                fn new_uninit_for(
                    (layout, offset, count): (::core::alloc::Layout, usize, #count_ty),
                ) -> (
                    ::std::boxed::Box<#uninit #ty_generics>,
                    (::core::alloc::Layout, usize, #count_ty),
                ) {
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
                    // - Pointer was allocated in a single allocation above by Global with the given
                    //   `layout` (as computed by `layout_for_size` / `layout_for_count` for `count`)
                    // **Metadata is correct**
                    // - `count` is the number of tail elements used to build that layout, provided by
                    //   `layout_for_*` and used in from_raw_parts_mut
                    // **The pointee is valid for any bytes**
                    // - every field of the mirror is `MaybeUninit<_>` (no validity invariant), so the
                    //   uninitialised allocation is a valid value of the mirror type
                    let mut boxed = unsafe { ::std::boxed::Box::from_raw(wide) };

                    // declared defaults (`field: Type = expr`) are written safely via MaybeUninit::write
                    #(boxed.#d_idents.write(#d_exprs);)*

                    (boxed, (layout, offset, count))
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, tail element count)`"]
                #[doc = "for a `Self` buffer of `size` bytes. The count is typed as the tail slice's"]
                #[doc = "`Pointee::Metadata` (a `usize` for `[T]`), matching the fat-pointer metadata."]
                #[doc = "The `Uninit` mirror has the identical layout."]
                #[doc = ""]
                #[doc = "The tail count is derived from `size` (the bytes beyond the sized head, rounded down"]
                #[doc = "to whole elements; a `size` smaller than the head maps to an empty tail). A `size`"]
                #[doc = "that does not equal the resulting `layout.size()` is **not** an error: compare the"]
                #[doc = "returned [`Layout`] with the requested `size` yourself to validate it."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if the layout cannot be computed (e.g. the tail array"]
                #[doc = "overflows)."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The correctness of this layout and the tail offset is relied upon by unsafe code"]
                #[doc = "in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_size`][Self::new_uninit_for_size])."]
                fn layout_for_size(
                    size: usize,
                ) -> ::core::result::Result<
                    (::core::alloc::Layout, usize, #count_ty),
                    ::core::alloc::LayoutError,
                > {
                    let header = Self::layout_header()?;

                    let elem_size = ::core::mem::size_of::<#elem>();
                    // named fns only (no `/`): a size smaller than the head saturates to an empty
                    // tail; the caller validates the result by comparing `layout.size()` with the
                    // requested `size`
                    let tail_bytes = size.saturating_sub(header.size());
                    // `checked_div` returns `None` on a zero divisor (ZST tail element) — map that
                    // to a count of 0 instead of panicking or special-casing
                    let count: #count_ty = tail_bytes.checked_div(elem_size).unwrap_or(0);

                    Self::extend_with_tail(header, count)
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, tail element count)`"]
                #[doc = "for a `Self` buffer holding `count` tail elements. `count` is typed as the tail"]
                #[doc = "slice's `Pointee::Metadata` (a `usize` for `[T]`), matching the fat-pointer metadata."]
                #[doc = "The `Uninit` mirror has the identical layout."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if the layout cannot be computed (e.g. the tail array"]
                #[doc = "overflows)."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The correctness of this layout and the tail offset is relied upon by unsafe code"]
                #[doc = "in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_count`][Self::new_uninit_for_count])."]
                fn layout_for_count(
                    count: #count_ty,
                ) -> ::core::result::Result<
                    (::core::alloc::Layout, usize, #count_ty),
                    ::core::alloc::LayoutError,
                > {
                    Self::extend_with_tail(Self::layout_header()?, count)
                }

                #[doc = "The [`Layout`] of the sized head (everything before the unsized tail), computed once"]
                #[doc = "and shared by [`layout_for_size`][Self::layout_for_size] and"]
                #[doc = "[`layout_for_count`][Self::layout_for_count]."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if a head field's layout cannot be extended (overflow)."]
                fn layout_header() -> ::core::result::Result<::core::alloc::Layout, ::core::alloc::LayoutError> {
                    // sequential `extend` reproduces the repr(C) placement of the sized head
                    let mut prefix = ::core::alloc::Layout::new::<()>();
                    #(
                        prefix = prefix.extend(::core::alloc::Layout::new::<#head_tys_header>())?.0;
                    )*
                    ::core::result::Result::Ok(prefix)
                }

                #[doc = "Extend a head [`Layout`] (from [`layout_header`][Self::layout_header]) with the tail of"]
                #[doc = "`count` elements and return the resulting `(layout, tail offset, count)` tuple, padded"]
                #[doc = "to the alignment. The `Uninit` mirror has the identical layout."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(LayoutError)` if the tail array overflows or the layouts cannot be"]
                #[doc = "extended."]
                fn extend_with_tail(
                    header: ::core::alloc::Layout,
                    count: #count_ty,
                ) -> ::core::result::Result<
                    (::core::alloc::Layout, usize, #count_ty),
                    ::core::alloc::LayoutError,
                > {
                    let tail = ::core::alloc::Layout::array::<#elem>(count)?;
                    let (extended, offset) = header.extend(tail)?;
                    ::core::result::Result::Ok((extended.pad_to_align(), offset, count))
                }
            }

            impl #impl_generics #uninit #ty_generics #impl_where {
                #[doc = "Convert into the fully initialised struct, reusing the same allocation."]
                #[doc = ""]
                #[doc = "# Safety"]
                #[doc = "Every field of `self` must have been initialised: each sized field (a `MaybeUninit`)"]
                #[doc = "via [`MaybeUninit::write`], and every element of the unsized tail likewise."]
                #[doc = "Reading a field that was never written is undefined behaviour."]
                unsafe fn assume_init(
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
