//! `derive_dst` provides `#[derive(DST)]` for `#[repr(C)]` structs whose **last field is an
//! unsized slice** (`[T]`) — the usual way to mirror a C/C++ "flexible array member" struct such
//! as `SP_DEVICE_INTERFACE_DETAIL_DATA_W`.
//!
//! # What is generated
//!
//! For
//!
//! ```
//! #![feature(ptr_metadata)]
//! use derive_dst::DST;
//!
//! #[repr(C)]
//! #[derive(Debug, PartialEq, Eq, DST)]
//! struct DeviceDetails {
//!     cbSize: u32,
//!     DevicePath: [u16],
//! }
//!
//! let (mut uninit, (layout, tail_offset, count)) = DeviceDetails::new_uninit_for_size(8).unwrap();
//! assert_eq!(layout.size(), 8); // 8 is a valid size: 4-byte head + 2 x u16, correctly padded
//! assert_eq!(tail_offset, 4);
//! assert_eq!(count, 2);
//! // the allocation is uninitialised: write every field...
//! uninit.cbSize.write(4);
//! for word in uninit.DevicePath.iter_mut() {
//!     word.write(0);
//! }
//! // SAFETY: every field (`cbSize` and all tail words) is initialised.
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
//!   including a fresh, uninitialised allocation. The mirror carries the **same attributes** as
//!   `DeviceDetails` — copied verbatim, so lint levels (`allow`/`expect`/...), `cfg`/`cfg_attr`
//!   gates, `repr` hints, ... all apply to the mirror too — except `derive` and `doc` (the mirror
//!   gets its own `#[derive(Debug)]` and docs), and each mirrored field likewise carries its
//!   original field's attributes except `derive` and `doc`.
//! - `DeviceDetails::new_uninit_for_size(size: usize) -> Result<(Box<UninitDeviceDetails>, (Layout, usize, <[u16] as Pointee>::Metadata)), LayoutError>`
//!   and `DeviceDetails::new_uninit_for_count(count: <[u16] as Pointee>::Metadata) -> Result<(Box<UninitDeviceDetails>, (Layout, usize, <[u16] as Pointee>::Metadata)), LayoutError>`
//!   — **safe** allocating constructors, taking the total buffer size or the tail element count
//!   respectively: they compute the layout via [`layout_for_size`][Self::layout_for_size] /
//!   [`layout_for_count`][Self::layout_for_count], allocate a buffer of that layout, attach the
//!   tail element count as the pointer metadata and return it boxed **together with the
//!   `(Layout, tail offset, tail count)` tuple** — the same tuple the `layout_for_*` constructors
//!   return — so initialisation routines do not need to call `layout_for_*` again. The allocation
//!   is entirely **uninitialised**: the caller writes every field (via [`MaybeUninit::write`])
//!   before `assume_init`. A `size` that does not match the correctly aligned and
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
//!   `#![feature(ptr_metadata)]` for the generated `ptr::from_raw_parts_mut` / `ptr::metadata` calls.
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
//! - default field values (`field: Type = expr`) are **not** supported and are rejected with a
//!   spanned error: the `Uninit` mirror could not carry them, so the caller's own constructor
//!   (e.g. `new`) writes every field before `assume_init`.
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
use quote::{ToTokens as _, format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, ExprAssign, Field, Fields, Generics, Ident, Meta, Token, Type,
    parse_quote, punctuated::Punctuated, spanned::Spanned,
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

    let dst = DstStruct::parse(ast)?;

    Ok(dst.generate())
}

/// A destructured, validated `#[repr(C)]` struct with named fields and an unsized slice tail.
#[derive(Debug)]
pub(crate) struct DstStruct {
    name: Ident,
    mirror_name: Ident,
    /// Extended with `where T: Sized, [T]: Pointee<Metadata = usize>`
    generics: Generics,
    /// Attributes after stripping doc & derive
    attrs: Vec<Attribute>,
    /// The extend assignments to `prefix` required to build a Layout for the head fields
    layout_head: Punctuated<ExprAssign, Token![;]>,
    /// MaybeUninit versions of the head fields
    mirror_head: Punctuated<Field, Token![,]>,
    /// MaybeUninit version of the tail field
    mirror_tail: Field,
    /// `T` in the tail `[T]`.
    tail_elem: Type,
    /// `<T as Pointee>::Metadata`
    count_ty: Type,
}

impl DstStruct {
    /// Handles all the invariant validation.
    pub(crate) fn parse(mut ast: DeriveInput) -> DiagnosticResult<Self> {
        let name = ast.ident;
        let mirror_name = format_ident!("Uninit{}", name);

        let mut fields = match ast.data {
            Data::Struct(struct_data) => match struct_data.fields {
                Fields::Named(fields) => fields.named,
                Fields::Unnamed(fields) => {
                    return error("DST can only be derived for a struct with named fields")
                        .add_help(
                            fields.paren_token.span.span(),
                            "tuple structs are not supported",
                        );
                }
                Fields::Unit => {
                    return error("DST can only be derived for a struct with named fields")
                        .add_help(name.span(), "unit structs have no fields");
                }
            },
            Data::Enum(enum_data) => {
                return error("DST can only be derived for a struct")
                    .add_help(enum_data.enum_token.span(), "not a struct");
            }
            Data::Union(union_data) => {
                return error("DST can only be derived for a struct")
                    .add_help(union_data.union_token.span(), "not a struct");
            }
        };

        remove_doc_and_derive(&mut ast.attrs);

        if ast.attrs.iter().find(repr_c).is_none() {
            return error("DST requires #[repr(C)] so the layout can be generated for allocation")
                .add_help(name.span(), "add #[repr(C)] above this");
        };

        let tail = fields
            .pop()
            .or_error("DST requires at least one field (an unsized slice tail)")
            .add_help(name.span(), "add a final field like `DevicePath: [u16]`")?;
        let tail_elem = match &tail.ty {
            Type::Slice(slice) => *slice.elem.clone(),
            _ => return error(
                "the last field of a DST struct must be an unsized slice, e.g. `DevicePath: [u16]`",
            )
            .add_help(tail.ty.span(), "this field is sized"),
        };
        if let Some((_eq, expr)) = &tail.default {
            return error("DST does not support default field values (`field: Type = expr`)")
                .add_help(
                    expr.span(),
                    "write the field in your constructor (e.g. `new`) before `assume_init`",
                );
        };

        let mut mirror_tail = tail.clone();
        mirror_tail.ty = parse_quote!([::core::mem::MaybeUninit<#tail_elem>]);

        let count_ty = parse_quote!(<[#tail_elem] as ::core::ptr::Pointee>::Metadata);

        for field in fields.iter_mut() {
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
            if let Some((_eq, expr)) = &field.default {
                return error("DST does not support default field values (`field: Type = expr`)")
                    .add_help(
                        expr.span(),
                        "write the field in your constructor (e.g. `new`) before `assume_init`",
                    );
            };
            remove_doc_and_derive(&mut field.attrs);
        }

        let mut mirror_head = fields.clone();
        for field in mirror_head.iter_mut() {
            let ty = field.ty.clone();
            field.ty = parse_quote!(::core::mem::MaybeUninit<#ty>)
        }

        let layout_head: Punctuated<ExprAssign, syn::token::Semi> = fields
            .iter()
            .map(|field| -> ExprAssign {
                let gates = field
                    .attrs
                    .iter()
                    .filter(|attr| attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr"));
                let ty = &field.ty;

                parse_quote! {
                    #(#gates)*
                    prefix = prefix.extend(::core::alloc::Layout::new::<#ty>())?.0
                }
            })
            .collect();

        let where_clause = match &mut ast.generics.where_clause {
            Some(where_clause) => where_clause,
            None => ast.generics.make_where_clause(),
        };

        where_clause
            .predicates
            .push(parse_quote!(#tail_elem: ::core::marker::Sized));
        where_clause
            .predicates
            .push(parse_quote!([#tail_elem]: ::core::ptr::Pointee<Metadata = usize>));

        Ok(Self {
            name,
            mirror_name,
            generics: ast.generics,
            attrs: ast.attrs,
            layout_head,
            mirror_head,
            mirror_tail,
            tail_elem,
            count_ty,
        })
    }

    /// Generates the `UninitFoo` mirror struct — carrying the original struct's attributes
    /// (except `derive` & `doc`) and, per field, the field's attributes (except `derive` & `doc`)
    /// — and the `new_uninit_for_size` / `new_uninit_for_count` / `assume_init` /
    /// `layout_for_size` / `layout_for_count` methods (all returning `core::alloc::LayoutError`
    /// on failure).
    pub(crate) fn generate(self) -> TokenStream2 {
        let DstStruct {
            name,
            mirror_name,
            generics,
            attrs,
            layout_head,
            mirror_head,
            mirror_tail,
            tail_elem,
            count_ty,
        } = self;

        let mirror_head_fields = mirror_head.pairs();
        let header_extends = layout_head.pairs();

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        quote! {
            // the original struct's attributes (incl. `repr(C)` — required by `parse`), except
            // `derive` & `doc`; the generated docs & `#[derive(Debug)]` below are the mirror's own
            #(#attrs)*
            #[doc = concat!("`MaybeUninit` mirror of [`", stringify!(#name), "}`]: identical `repr(C)` layout, but every field is")]
            #[doc = "wrapped in `MaybeUninit`, so any bit pattern — including a fresh, uninitialised"]
            #[doc = "allocation — is a valid value. Constructed by `new_uninit_for_size` /"]
            #[doc = "`new_uninit_for_count`; converted back to the initialised type with"]
            #[doc = "[`Self::assume_init`]."]
            #[derive(Debug)]
            struct #mirror_name #generics #where_clause {
                #(#mirror_head_fields)*
                #mirror_tail
            }

            impl #impl_generics #name #ty_generics #where_clause {
                #[doc = "Allocate a buffer for a `Self` value of `size` bytes and return it boxed as the"]
                #[doc = "`Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple."]
                #[doc = ""]
                #[doc = "The tail element count is derived from `size` by [`layout_for_size`][Self::layout_for_size];"]
                #[doc = "the allocation then proceeds exactly as for [`new_uninit_for`][Self::new_uninit_for]."]
                #[doc = ""]
                #[doc = "The allocation is entirely **uninitialised**: write every field with"]
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
                        ::std::boxed::Box<#mirror_name #ty_generics>,
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
                        ::std::boxed::Box<#mirror_name #ty_generics>,
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
                #[doc = "The allocation is entirely **uninitialised**: write every field with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "# Aborts"]
                #[doc = "Aborts via `handle_alloc_error` if the allocation fails; the `layout` is taken as"]
                #[doc = "given (computed by `layout_for_*`), so it cannot fail here."]
                #[doc = ""]
                #[doc = "# Returns"]
                #[doc = "The boxed mirror together with the `(Layout, tail offset, tail count)` tuple passed in."]
                #[doc = ""]
                #[doc = "# Implementation safety"]
                #[doc = ""]
                #[doc = "The internal `unsafe` block is sound: `raw` was allocated by `Global` with the"]
                #[doc = "given `layout`, so it is valid for reads & writes of that layout; `count` is the"]
                #[doc = "number of tail elements that layout was built for, so `ptr::from_raw_parts_mut`"]
                #[doc = concat!("attaches the correct metadata for the mirror (last field: `[MaybeUninit<", stringify!(#tail_elem), ">]`).")]
                #[doc = "Every field of the mirror is `MaybeUninit<_>`, which is valid for ANY bit pattern,"]
                #[doc = "so the uninitialised allocation is a valid pointee; the pointer is moved into"]
                #[doc = "exactly one `Box` and not used afterwards."]
                fn new_uninit_for(
                    (layout, offset, count): (::core::alloc::Layout, usize, #count_ty),
                ) -> (
                    ::std::boxed::Box<#mirror_name #ty_generics>,
                    (::core::alloc::Layout, usize, #count_ty),
                ) {
                    let raw = match <::std::alloc::Global as ::core::alloc::Allocator>::allocate(
                        &::std::alloc::Global,
                        layout,
                    ) {
                        ::core::result::Result::Ok(pointer) => pointer,
                        ::core::result::Result::Err(_) => ::std::alloc::handle_alloc_error(layout),
                    };

                    let wide = ::core::ptr::from_raw_parts_mut::<#mirror_name #ty_generics>(
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

                    let elem_size = ::core::mem::size_of::<#tail_elem>();
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
                    #(#header_extends)*;
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
                    let tail = ::core::alloc::Layout::array::<#tail_elem>(count)?;
                    let (extended, offset) = header.extend(tail)?;
                    ::core::result::Result::Ok((extended.pad_to_align(), offset, count))
                }
            }

            impl #impl_generics #mirror_name #ty_generics #where_clause {
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

fn is_wanted(meta: &mut Meta) -> bool {
    !(meta.path().is_ident("derive") || meta.path().is_ident("doc"))
}

fn keep(attr: &mut Attribute) -> Option<&mut Attribute> {
    (is_wanted(&mut attr.meta))
        .then_some(attr)
        .and_then(|attr| {
            if let Meta::List(meta_list) = &mut attr.meta
                && meta_list.path.is_ident("cfg_attr")
            {
                let mut p = meta_list
                    .parse_args_with(Punctuated::<Meta, Token![,]>::parse_separated_nonempty)
                    .expect("cfg_attr takes comma separated list");
                p.retain(is_wanted);
                if p.len() < 2 {
                    // We stripped all the attributes and just have the condition left
                    return None;
                }
                meta_list.tokens = p.into_token_stream();
            }
            Some(attr)
        })
}

fn remove_doc_and_derive(attrs: &mut Vec<Attribute>) {
    attrs.retain_mut(|attr| keep(attr).is_some())
}

fn repr_c(attr: &&Attribute) -> bool {
    if let Meta::List(meta_list) = &attr.meta
        && meta_list.path.is_ident("repr")
    {
        meta_list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_separated_nonempty)
            .is_ok_and(|args| {
                args.iter()
                    .any(|meta| matches!(meta, Meta::Path(path) if path.is_ident("C")))
            })
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    #[test]
    fn remove_derive() {
        let mut foo: Attribute = parse_quote! {#[derive(Debug)]};
        assert_eq!(keep(&mut foo), None);
    }

    #[test]
    fn remove_doc() {
        let mut foo: Attribute = parse_quote! {#[doc(inline)]};
        assert_eq!(keep(&mut foo), None);
    }

    #[test]
    fn remove_derive_from_cfg_attr() {
        let mut foo: Attribute =
            parse_quote! {#[cfg_attr(test, derive(Debug), deprecated, doc = "foo")]};
        let mut expected: Attribute = parse_quote! {#[cfg_attr(test, deprecated)]};
        assert_eq!(keep(&mut foo), Some(&mut expected));
    }

    #[test]
    fn remove_empty_cfg_attr() {
        let mut foo: Attribute = parse_quote! {#[cfg_attr(test, derive(Debug), doc = "foo")]};
        assert_eq!(keep(&mut foo), None);
    }

    #[test]
    fn parse_cstruct() {
        let input: DeriveInput = parse_quote! {
            #[repr(C)]
            #[doc(ignore)]
            #[derive(Debug, DST)]
            #[cfg_attr(test, deprecated, derive(Clone))]
            struct DeviceDetails {
                cbSize: u32,
                DevicePath: [u16]
            }
        };
        let device_info = DstStruct::parse(input).unwrap();
        assert_eq!(device_info.name, format_ident!("DeviceDetails"));
        assert_eq!(
            device_info.mirror_tail,
            parse_quote!(DevicePath: [::core::mem::MaybeUninit<u16>])
        );
        assert_eq!(device_info.tail_elem, parse_quote!(u16));
    }
}
