//! Provides `#[derive(DST)]` for `#[repr(C)]` structs whose **last field is an unsized slice**
//! (`[T]`) — the usual way to mirror a C/C++ "flexible array member" struct such as
//! `SP_DEVICE_INTERFACE_DETAIL_DATA_W`.
//!
//! # Why
//!
//! The nomicon finishes the [section on DSTs][nomicon-DSTs] with the honest statement
//!
//! > (Yes, custom DSTs are a largely half-baked feature for now.)
//!
//! and yet, various ffi functions make use of them.
//!
//! Often this is to provide a filepath or other
//! string as the final field (e.g. [`SP_DEVICE_INTERFACE_DETAIL_DATA_W`][Win-WPSIDDW] on Windows,
//! [`inotify_event`][linux-inotify] on linux). Even with null-terminated strings, you still need to
//! allocate a buffer in advance. Sometimes it's a dynamically-sized tail of structured data
//! (e.g. [`fiemap`][linux-fiemap] on linux).
//!
//! So do you:
//! - make a developer-time guess and hope your buffer is always big enough, wasting any oversizing
//!   on every call, or ...
//! - find a way to allocate a DST at runtime, when you know the exact size needed?
//!
//! # What you get
//!
//! The case which gave me the pattern for this macro:
//!
//! ```
//! #![feature(ptr_metadata)]
//! use std::io;
//!
//! use derive_dst::DST;
//!
//! #[repr(C)]
//! #[derive(Debug, DST)]
//! #[allow(nonstandard_style, reason = "mirroring FFI struct")]
//! /// A dynamically sized ffi-compatible version of `SP_DEVICE_INTERFACE_DETAIL_DATA_W`
//! pub struct DeviceDetails {
//!     cbSize: u32,
//!     DevicePath: [u16],
//! }
//!
//! impl DeviceDetails {
//!     /// Create a new DeviceDetails of the given size, as returned by an null-call to
//!     /// `SetupDiGetDeviceInterfaceDetailW`.
//!     ///
//!     /// Errors if the requested size does not produce a valid, well-aligned & padded struct
//!     pub fn new(size: usize) -> io::Result<Box<Self>> {
//!         // Create a new, boxed uninitialised DeviceDetails. This may return a LayoutError if the
//!         // overall struct is somehow invalid.
//!         let (mut raw, (layout, tail_offset, _tail_count)) =
//!             Self::new_uninit_for_size(size).map_err(io::Error::other)?;
//!
//!         // Check that the returned DST is of the exact size requested. DST will always be
//!         // well-aligned and padded.
//!         if layout.size() != size {
//!             return Err(io::Error::new(
//!                 io::ErrorKind::InvalidData,
//!                 "invalid buffer size",
//!             ));
//!         }
//!
//!         // API requires cbSize to be set to the size of the struct as if it were storing a single
//!         // `\0` as the DevicePath.
//!         let cbsize = (tail_offset + size_of::<u16>()).next_multiple_of(layout.align());
//!         raw.cbSize.write(cbsize as u32);
//!
//!         // SAFETY:
//!         // - cbSize is correctly set to size required for an empty path with a single null-terminator
//!         // - raw was initialized zeroed and `\0` is a valid value for u16
//!         let valid = unsafe { raw.assume_init() };
//!         Ok(valid)
//!     }
//! }
//! ```
//!
//! Behind the scenes this generates (an import-collision-safe version of).
//!
//! The doc-comments are included in the generated code, so you can read them locally and in IDE
//!
//! ```
//! # #![feature(ptr_metadata)]
//! # #![allow(dead_code)]
//! # #![allow(clippy::type_complexity)]
//! #
//! # use std::{
//! #     alloc::{self, Allocator, Global, Layout, LayoutError},
//! #     io,
//! #     mem::MaybeUninit,
//! #     ptr::{self, Pointee},
//! # };
//! #
//! # #[repr(C)]
//! # #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
//! # #[allow(nonstandard_style, reason = "mirroring FFI struct")]
//! # /// A dynamically sized ffi-compatible version of `SP_DEVICE_INTERFACE_DETAIL_DATA_W`
//! # pub struct DeviceDetails {
//! #     cbSize: u32,
//! #     DevicePath: [u16],
//! # }
//! #[repr(C)]
//! #[allow(nonstandard_style, reason = "mirroring FFI struct")]
//! /// `MaybeUninit` mirror of [`DeviceDetails`]: identical `repr(C)` layout, but every field is
//! /// wrapped in `MaybeUninit`. Constructed by `new_uninit_for_...` methods
//! /// converted back to the initialised type with [`Self::assume_init`].
//! #[derive(Debug)]
//! struct UninitDeviceDetails
//! where
//!     u16: Sized,
//!     [u16]: Pointee<Metadata = usize>,
//! {
//!     cbSize: MaybeUninit<u32>,
//!     DevicePath: [MaybeUninit<u16>],
//! }
//!
//! impl DeviceDetails
//! where
//!     u16: Sized,
//!     [u16]: Pointee<Metadata = usize>,
//! {
//!     /// Allocate a buffer for a `Self` value of `size` bytes and return it boxed as the
//!     /// `Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple.
//!     ///
//!     /// # SAFETY
//!     /// The allocation is entirely **uninitialised**: write every field with
//!     /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
//!     ///
//!     /// # Note
//!     /// a `size` that does not match the correctly aligned and padded layout for the derived
//!     /// tail count is **not** rejected — the returned `Layout` carries the valid size
//!     /// (`layout.size()`), so the caller can compare it with the requested `size` and decide how to
//!     /// handle a mismatch before calling `assume_init`.
//!     fn new_uninit_for_size(
//!         size: usize,
//!     ) -> Result<
//!         (
//!             Box<UninitDeviceDetails>,
//!             (Layout, usize, <[u16] as Pointee>::Metadata),
//!         ),
//!         LayoutError,
//!     > {
//!         // SAFETY: layout & count are directly calculated by layout_for_size and passed on
//!         unsafe { Result::Ok(Self::new_uninit_for(Self::layout_for_size(size)?)) }
//!     }
//!
//!     /// Allocate a buffer holding `count` tail elements and return it boxed as the `Uninit`
//!     /// mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple. `count` is
//!     /// typed as the tail slice's `Pointee::Metadata` (a `usize` for `[T]`).
//!     ///
//!     /// # SAFETY
//!     /// The allocation is entirely **uninitialised**: write every field with
//!     /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
//!     fn new_uninit_for_count(
//!         count: <[u16] as Pointee>::Metadata,
//!     ) -> Result<
//!         (
//!             Box<UninitDeviceDetails>,
//!             (Layout, usize, <[u16] as Pointee>::Metadata),
//!         ),
//!         LayoutError,
//!     > {
//!         // SAFETY: layout & count are directly calculated by layout_for_count and passed on
//!         unsafe { Result::Ok(Self::new_uninit_for(Self::layout_for_count(count)?)) }
//!     }
//!
//!     /// Allocate a buffer with the given `layout` and return it boxed as the
//!     /// `Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail
//!     /// count)` tuple it was built for.
//!     ///
//!     /// # SAFETY
//!     /// The allocation is entirely **uninitialised**: write every field with
//!     /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
//!     ///
//!     /// Extra care must be taken when using this function directly to ensure that
//!     /// `layout` and `count` are valid in combination. No validation is performed
//!     /// or possible in this function. Prefer the safe versions [`new_uninit_for_size`]
//!     /// and [`new_uninit_for_count] wherever possible.
//!     unsafe fn new_uninit_for(
//!         (layout, offset, count): (Layout, usize, <[u16] as Pointee>::Metadata),
//!     ) -> (
//!         Box<UninitDeviceDetails>,
//!         (Layout, usize, <[u16] as Pointee>::Metadata),
//!     ) {
//!         let raw = match Global::allocate_zeroed(&Global, layout) {
//!             Result::Ok(pointer) => pointer,
//!             Result::Err(_) => alloc::handle_alloc_error(layout),
//!         };
//!         let wide = ptr::from_raw_parts_mut::<UninitDeviceDetails>(raw.cast::<()>().as_ptr(), count);
//!
//!         // SAFETY:
//!         // **No double-free risk**
//!         // - this pointer will not be added to a second box, or used in any other way after this
//!         // **Memory layout is correct**
//!         // - Pointer was allocated in a single allocation above by Global with the given
//!         //   `layout` (as computed by `layout_for_size` / `layout_for_count` for `count`)
//!         // **Metadata is correct**
//!         // - `count` is the number of tail elements used to build that layout, provided by
//!         //   `layout_for_*` and used in from_raw_parts_mut
//!         // **The pointee is valid for any bytes**
//!         // - every field of the mirror is `MaybeUninit<_>` (no validity invariant), so the
//!         //   uninitialised allocation is a valid value of the mirror type
//!         let boxed = unsafe { Box::from_raw(wide) };
//!         (boxed, (layout, offset, count))
//!     }
//!
//!     /// Calculate the `(layout, offset of the unsized tail, tail element count)`
//!     /// for a `Self` buffer of `size` bytes.
//!     ///
//!     /// # SAFETY
//!     /// The correctness of this layout and the tail offset is relied upon by unsafe code
//!     /// in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_size`][Self::new_uninit_for_size]).
//!     fn layout_for_size(
//!         size: usize,
//!     ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
//!         let header = Self::layout_header()?;
//!         let elem_size = size_of::<u16>();
//!         let tail_bytes = size.saturating_sub(header.size());
//!         // `checked_div` returns `None` on a zero divisor (ZST tail element) — map that
//!         // to a count of 0 instead of panicking or special-casing
//!         let count: <[u16] as Pointee>::Metadata = tail_bytes.checked_div(elem_size).unwrap_or(0);
//!         Self::extend_with_tail(header, count)
//!     }
//!
//!     /// Calculate the `(layout, offset of the unsized tail, tail element count)`
//!     /// for a `Self` buffer holding `count` tail elements.
//!     ///
//!     /// # SAFETY
//!     /// The correctness of this layout and the tail offset is relied upon by unsafe code
//!     /// in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_count`][Self::new_uninit_for_count]).
//!     fn layout_for_count(
//!         count: <[u16] as Pointee>::Metadata,
//!     ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
//!         Self::extend_with_tail(Self::layout_header()?, count)
//!     }
//!
//!     /// The [`Layout`] of the sized head (everything before the unsized tail)
//!     fn layout_header() -> Result<Layout, LayoutError> {
//!         let mut prefix = Layout::new::<()>();
//!         prefix = prefix.extend(Layout::new::<u32>())?.0;
//!         Result::Ok(prefix)
//!     }
//!
//!     /// Extend a head [`Layout`] (from [`layout_header`][Self::layout_header]) with the tail of
//!     /// `count` elements and return the resulting `(layout, tail offset, count)` tuple, padded
//!     /// to the alignment.
//!     fn extend_with_tail(
//!         header: Layout,
//!         count: <[u16] as Pointee>::Metadata,
//!     ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
//!         let tail = Layout::array::<u16>(count)?;
//!         let (extended, offset) = header.extend(tail)?;
//!         Result::Ok((extended.pad_to_align(), offset, count))
//!     }
//! }
//!
//! impl UninitDeviceDetails
//! where
//!     u16: Sized,
//!     [u16]: Pointee<Metadata = usize>,
//! {
//!     /// Convert into the fully initialised struct, reusing the same allocation.
//!     ///
//!     /// # SAFETY
//!     /// Every head field & all tail elements must have been initialised to a valid
//!     /// value for the respective types. All fields will have been initialised zeroed, but zero
//!     /// is not a valid value for all types. Calling `assume_init` where some fields
//!     /// are not properly initialised will lead to UB. See [`core::mem::MaybeUninit::assume_init`]
//!     /// for more details.
//!     unsafe fn assume_init(self: Box<Self>) -> Box<DeviceDetails> {
//!         let raw = Box::into_raw(self);
//!         // The mirror and the original have identical layouts (both `repr(C)`, and
//!         // `MaybeUninit<T>` is guaranteed to have the same size, alignment and ABI as `T`),
//!         // and the tail metadata (element count) is carried over unchanged.
//!         let wide = ptr::from_raw_parts_mut::<DeviceDetails>(raw.cast::<()>(), ptr::metadata(raw));
//!         // SAFETY:
//!         // - the caller guarantees (see `# SAFETY` above) that every field is initialised,
//!         //   so the pointee is a valid `#name` value
//!         // - layout and metadata are identical to the mirror's, so the fat pointer is correct
//!         // - the pointer is moved into exactly one `Box` and not used afterwards
//!         unsafe { Box::from_raw(wide) }
//!     }
//! }
//! ```
//!
//! # Requirements
//!
//! - Nightly toolchain (and `feature(ptr_metadata)` not forbidden in config.toml)
//! - `#![feature(ptr_metadata)]`
//! - DST must:
//!     - be `#[repr(C)]`, no additional alignment or packing specification
//!     - have a *single*` unsized field as the *tail* (last field) as per the [nomicon][nomicon-DSTs]
//!     - have a slice `[T]` as the unsized tail
//!
//! # SAFETY
//!
//! We take extreme care to catch as many incorrect usages as possible at compile-time. Deliberately
//! or accidentally circumventing the above requirements will most likely lead to UB.
//!
//! We recommend you build the docs locally via `cargo doc --document-private-items` to review the
//! requirements for `assume_init`
//!
//! The *`UninitMirror`* type is zero-initialised and not guaranteed to be valid. All fields must
//! be suitably initialised, _including **all elements of the tail slice**_, before calling
//! `assume_init`.
//!
//! [nomicon-DSTs]: https://doc.rust-lang.org/nomicon/exotic-sizes.html#dynamically-sized-types-dsts
//! [Win-WPSIDDW]: https://learn.microsoft.com/en-us/windows/win32/api/setupapi/ns-setupapi-sp_device_interface_detail_data_w
//! [linux-inotify]: https://man7.org/linux/man-pages/man7/inotify.7.html
//! [linux-fiemap]: https://docs.kernel.org/filesystems/fiemap.html#c.fiemap

use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToTokens, prelude::*};
use quote::{ToTokens as _, format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, ExprAssign, Field, Fields, Generics, Ident, Meta, Token, Type,
    parse_quote, punctuated::Punctuated, spanned::Spanned,
};

#[proc_macro_derive(DST)]
/// Derives the required polyfill for this DST. See the [crate-level documentation](crate)
/// for the full contract and generated code.
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

fn is_repr_c(attrs: &[Attribute], name: &Ident) -> DiagnosticResult<()> {
    let repr_c: Attribute = parse_quote!(#[repr(C)]);
    let has_repr_c = attrs.iter().any(|attr| attr == &repr_c);
    let other_repr = attrs
        .iter()
        .find(|attr| attr.path().is_ident("repr") && *attr != &repr_c);

    match (has_repr_c, other_repr) {
        (true, None) => attrs.iter().try_for_each(|attr| check_for_cfg_attr_repr(&attr.meta)),
        (true, Some(repr)) => {
            error("DST requires exactly `#[repr(C)]`: `packed`, `align(N)` and other repr hints change the layout, so the generated allocation code would be unsound")
            .add_help(repr.span(), "remove this additional repr")
        },
        (false, None) => {
            attrs.iter().try_for_each(|attr| check_for_cfg_attr_repr(&attr.meta))?;
            error("DST requires #[repr(C)] so the layout can be generated for allocation")
            .add_help(name.span(), "add #[repr(C)] above this")
    },
        (false, Some(repr)) => {
            error("DST requires #[repr(C)] so the layout can be generated for allocation")
            .add_help(repr.span(), "change this to `#[repr(C)]`")
        },
    }
}

fn check_for_cfg_attr_repr(meta: &Meta) -> DiagnosticResult<()> {
    let Some(cfg_attr) = meta
        .require_list()
        .ok()
        .filter(|meta_list| meta_list.path.is_ident("cfg_attr"))
    else {
        return Ok(());
    };

    let repr_c: Attribute = parse_quote!(#[repr(C)]);
    let repr_c = repr_c.meta;
    let p = cfg_attr
        .parse_args_with(Punctuated::<Meta, Token![,]>::parse_separated_nonempty)
        .expect("cfg_attr takes comma separated list");
    let has_repr_c = p.iter().find(|meta| *meta == &repr_c);
    let other_repr = p
        .iter()
        .find(|meta| meta.path().is_ident("repr") && *meta != &repr_c);

    match (has_repr_c, other_repr) {
        (Some(repr_c), _) => {
            error("DST requires #[repr(C)] so the layout can be generated for allocation")
            .add_help(
                repr_c.span(),
                "this `repr(C)` is within a `#[cfg_attr(...)]` gate",
            )?
        },
        (None, Some(other_repr)) => {
            error("DST requires exactly `#[repr(C)]`: `packed`, `align(N)` and other repr hints change the layout, so the generated allocation code would be unsound")
            .add_help(other_repr.span(), "remove this additional repr")?
        },
        (None, None) => p.iter().try_for_each(check_for_cfg_attr_repr),
    }
}

fn remove_doc_and_derive(attrs: &mut Vec<Attribute>) {
    attrs.retain_mut(|attr| keep(&mut attr.meta))
}

fn keep(meta: &mut Meta) -> bool {
    if let Meta::List(meta_list) = meta
        && meta_list.path.is_ident("cfg_attr")
    {
        let mut p = meta_list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_separated_nonempty)
            .expect("cfg_attr takes comma separated list");
        p.retain(keep);
        if p.len() < 2 {
            // We stripped all the attributes and just have the condition left
            false
        } else {
            meta_list.tokens = p.into_token_stream();
            true
        }
    } else {
        !(meta.path().is_ident("derive") || meta.path().is_ident("doc"))
    }
}

fn parse_struct(
    data: Data,
    name: &Ident,
) -> DiagnosticResult<(Punctuated<Field, Token![,]>, Field)> {
    match data {
        Data::Struct(struct_data) => match struct_data.fields {
            Fields::Named(mut fields) => {
                let tail = fields.named.pop().expect("not a unit struct");
                fields.named.iter().try_for_each(is_valid_head_field)?;
                has_no_default(&tail)?;
                Ok((fields.named, tail))
            }
            Fields::Unnamed(fields) => {
                error("DST can only be derived for a struct with named fields").add_help(
                    fields.paren_token.span.span(),
                    "tuple structs are not supported",
                )
            }
            Fields::Unit => error("DST requires at least one field (an unsized slice tail)")
                .add_help(name.span(), "add a tail field like `DevicePath: [u16]`"),
        },
        Data::Enum(enum_data) => error("DST can only be derived for a struct")
            .add_help(enum_data.enum_token.span(), "not a struct"),
        Data::Union(union_data) => error("DST can only be derived for a struct")
            .add_help(union_data.union_token.span(), "not a struct"),
    }
}

fn is_valid_head_field(field: &Field) -> DiagnosticResult<()> {
    match &field.ty {
        Type::Slice(slice) => error("only the last field of a DST struct may be unsized").add_help(
            slice.bracket_token.span.span(),
            "move this slice to the last position, or make it sized e.g. `[u16; 4]`",
        ),
        Type::TraitObject(trait_object) => {
            error("only the last field of a DST struct may be unsized").add_note(
                trait_object.span(),
                "deriving DSTs with `dyn Trait` as a tail is not supported",
            )
        }
        Type::Path(path) if path.path.is_ident("str") => {
            error("only the last field of a DST struct may be unsized").add_note(
                path.span(),
                "deriving DSTs with `str` as a tail is not supported",
            )
        }
        _ => has_no_default(field),
    }
}

fn has_no_default(field: &Field) -> DiagnosticResult<()> {
    if let Some((_eq, expr)) = &field.default {
        error("DST does not support default field values (`field: Type = expr`)").add_help(
            expr.span(),
            "write the field in your constructor (e.g. `new`) before `assume_init`",
        )
    } else {
        Ok(())
    }
}

impl DstStruct {
    /// Handles all the invariant validation.
    pub(crate) fn parse(mut ast: DeriveInput) -> DiagnosticResult<Self> {
        let name = ast.ident;
        let mirror_name = format_ident!("Uninit{}", name);

        is_repr_c(&ast.attrs, &name)?;
        remove_doc_and_derive(&mut ast.attrs);

        let (fields, tail) = parse_struct(ast.data, &name)?;

        let tail_elem = match &tail.ty {
            Type::Slice(slice) => *slice.elem.clone(),
            _ => return error(
                "the last field of a DST struct must be an unsized slice, e.g. `DevicePath: [u16]`",
            )
            .add_help(tail.ty.span(), "this field is sized"),
        };

        let mut mirror_tail = tail.clone();
        mirror_tail.ty = parse_quote!([::core::mem::MaybeUninit<#tail_elem>]);

        let count_ty = parse_quote!(<[#tail_elem] as ::core::ptr::Pointee>::Metadata);

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
            #(#attrs)*
            #[doc = concat!("`MaybeUninit` mirror of [`", stringify!(#name), "}`]: identical `repr(C)` layout, but every field is")]
            #[doc = "zero-initialised and wrapped in `MaybeUninit`. Constructed by `new_uninit_for_...` methods"]
            #[doc = "converted back to the initialised type with [`Self::assume_init`]."]
            #[derive(Debug)]
            struct #mirror_name #generics #where_clause {
                #(#mirror_head_fields)*
                #mirror_tail
            }

            impl #impl_generics #name #ty_generics #where_clause {
                #[doc = "Allocate a buffer for a `Self` value of `size` bytes and return it boxed as the"]
                #[doc = "`Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The allocation is entirely **uninitialised**: write every field with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "# Note"]
                #[doc = "a `size` that does not match the correctly aligned and padded layout for the derived"]
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
                    // SAFETY: layout & count are directly calculated by layout_for_size and passed on
                    unsafe {
                        ::core::result::Result::Ok(Self::new_uninit_for(Self::layout_for_size(size)?))
                    }
                }

                #[doc = "Allocate a buffer holding `count` tail elements and return it boxed as the `Uninit`"]
                #[doc = "mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple. `count` is"]
                #[doc = "typed as the tail slice's `Pointee::Metadata` (a `usize` for `[T]`)."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The allocation is entirely **uninitialised**: write every field with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                fn new_uninit_for_count(
                    count: #count_ty,
                ) -> ::core::result::Result<
                    (
                        ::std::boxed::Box<#mirror_name #ty_generics>,
                        (::core::alloc::Layout, usize, #count_ty),
                    ),
                    ::core::alloc::LayoutError,
                > {
                    unsafe {
                        // SAFETY: layout & count are directly calculated by layout_for_count and passed on
                        ::core::result::Result::Ok(Self::new_uninit_for(Self::layout_for_count(count)?))
                    }
                }

                #[doc = "Allocate a buffer with the given `layout` and return it boxed as the"]
                #[doc = "`Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail"]
                #[doc = "count)` tuple it was built for."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The allocation is entirely **uninitialised**: write every field with"]
                #[doc = "[`MaybeUninit::write`], then convert with `assume_init` on the returned box."]
                #[doc = ""]
                #[doc = "Extra care must be taken when using this function directly to ensure that"]
                #[doc = "`layout` and `count` are valid in combination. No validation is performed"]
                #[doc = "or possible in this function. Prefer the safe versions [`new_uninit_for_size`]"]
                #[doc = "and [`new_uninit_for_count] wherever possible."]
                unsafe fn new_uninit_for(
                    (layout, offset, count): (::core::alloc::Layout, usize, #count_ty),
                ) -> (
                    ::std::boxed::Box<#mirror_name #ty_generics>,
                    (::core::alloc::Layout, usize, #count_ty),
                ) {
                    let raw = match <::std::alloc::Global as ::core::alloc::Allocator>::allocate_zeroed(
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
                    let boxed = unsafe { ::std::boxed::Box::from_raw(wide) };

                    (boxed, (layout, offset, count))
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, tail element count)`"]
                #[doc = "for a `Self` buffer of `size` bytes."]
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
                    let tail_bytes = size.saturating_sub(header.size());
                    // `checked_div` returns `None` on a zero divisor (ZST tail element) — map that
                    // to a count of 0 instead of panicking or special-casing
                    let count: #count_ty = tail_bytes.checked_div(elem_size).unwrap_or(0);

                    Self::extend_with_tail(header, count)
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, tail element count)`"]
                #[doc = "for a `Self` buffer holding `count` tail elements."]
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

                #[doc = "The [`Layout`] of the sized head (everything before the unsized tail)"]
                fn layout_header() -> ::core::result::Result<::core::alloc::Layout, ::core::alloc::LayoutError> {
                    // sequential `extend` reproduces the repr(C) placement of the sized head
                    let mut prefix = ::core::alloc::Layout::new::<()>();
                    #(#header_extends)*;
                    ::core::result::Result::Ok(prefix)
                }

                #[doc = "Extend a head [`Layout`] (from [`layout_header`][Self::layout_header]) with the tail of"]
                #[doc = "`count` elements and return the resulting `(layout, tail offset, count)` tuple, padded"]
                #[doc = "to the alignment."]
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
                #[doc = "# SAFETY"]
                #[doc = "Every head field & all tail elements must have been initialised to a valid"]
                #[doc = "value for the respective types. All fields will have been initialised zeroed, but zero"]
                #[doc = "is not a valid value for all types. Calling `assume_init` where some fields"]
                #[doc = "are not properly initialised will lead to UB. See [`core::mem::MaybeUninit::assume_init`]"]
                #[doc = "for more details."]
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
                    // - the caller guarantees (see `# SAFETY` above) that every field is initialised,
                    //   so the pointee is a valid `#name` value
                    // - layout and metadata are identical to the mirror's, so the fat pointer is correct
                    // - the pointer is moved into exactly one `Box` and not used afterwards
                    unsafe { ::std::boxed::Box::from_raw(wide) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    #[test]
    fn remove_derive() {
        let mut foo: Attribute = parse_quote! {#[derive(Debug)]};
        assert!(!keep(&mut foo.meta));
    }

    #[test]
    fn remove_doc() {
        let mut foo: Attribute = parse_quote! {#[doc(inline)]};
        assert!(!keep(&mut foo.meta));
    }

    #[test]
    fn remove_derive_from_cfg_attr() {
        let mut foo: Attribute =
            parse_quote! {#[cfg_attr(test, derive(Debug), deprecated, doc = "foo")]};
        let expected: Attribute = parse_quote! {#[cfg_attr(test, deprecated)]};
        assert!(keep(&mut foo.meta));
        assert_eq!(foo, expected);
    }

    #[test]
    fn remove_empty_cfg_attr() {
        let mut foo: Attribute = parse_quote! {#[cfg_attr(test, derive(Debug), doc = "foo")]};
        assert!(!keep(&mut foo.meta));
    }

    #[test]
    fn nested_cfg_attr() {
        let mut foo: Attribute = parse_quote! {#[cfg_attr(test, cfg_attr(any(), derive(Debug)), cfg_attr(any(), deprecated, doc = "foo"))]};
        let expected: Attribute = parse_quote! {#[cfg_attr(test, cfg_attr(any(), deprecated))]};
        assert!(keep(&mut foo.meta));
        assert_eq!(foo, expected);
    }

    #[test]
    fn repr_c_alone_is_repr_c() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn no_repr_is_an_error() {
        let ast: DeriveInput = parse_quote! {
            #[derive(Debug)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn packed_on_a_separate_repr_line_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[repr(packed)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn packed_in_the_same_repr_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C, packed)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn align_in_repr_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C, align(16))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    // --- `repr` hidden behind a (possibly nested) `#[cfg_attr(...)]` gate -----------
    // rustc resolves `cfg_attr` before the derive runs, but the derive must still reject
    // raw `cfg_attr` tokens (e.g. when another macro hands it unexpanded tokens): a
    // cfg-gated `repr` would change the layout whenever the gate is enabled, so the
    // generated allocation code would be unsound.

    #[test]
    fn cfg_attr_gated_repr_c_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(C))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_repr_c_without_plain_repr_c_is_rejected() {
        // the cfg-gate error propagates (via `?`) before the plain missing-repr(C) error
        let ast: DeriveInput = parse_quote! {
            #[cfg_attr(feature = "extra", repr(C))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_packed_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(packed))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_align_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(all(), repr(align(16)))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_transparent_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(transparent))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_bare_repr_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gated_repr_c_and_packed_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(C, packed))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_gating_repr_alongside_other_attrs_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", derive(Debug), repr(packed))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_with_multiple_reprs_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(C), repr(packed))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_with_complex_condition_gating_repr_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(all(unix, feature = "extra"), repr(C))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn nested_cfg_attr_gated_repr_c_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "a", cfg_attr(feature = "b", repr(C)))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn nested_cfg_attr_gated_packed_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "a", cfg_attr(all(), repr(packed)))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn triple_nested_cfg_attr_gated_align_is_rejected() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(a, cfg_attr(b, cfg_attr(c, repr(align(8)))))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_error());
    }

    #[test]
    fn cfg_attr_with_non_repr_attrs_is_ok() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", allow(dead_code), deprecated)]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn cfg_attr_with_derive_is_ok() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(test, derive(Clone))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn cfg_attr_doc_mentioning_repr_is_ok() {
        // a doc string is a name-value meta, not a `repr` attribute — no false positive
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", doc = "use `repr(C)`, never `repr(packed)`")]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn cfg_attr_macro_attr_containing_repr_tokens_is_ok() {
        // `my_macro(repr(C))` is a list with path `my_macro`, not a `repr` attribute
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", my_macro(repr(C)))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn nested_cfg_attr_without_repr_is_ok() {
        let ast: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "a", cfg_attr(feature = "b", allow(dead_code)))]
            struct Foo { tail: [u8] }
        };
        assert!(is_repr_c(&ast.attrs, &ast.ident).is_ok());
    }

    #[test]
    fn check_for_cfg_attr_repr_ignores_non_cfg_attr() {
        let attr: Attribute = parse_quote!(#[allow(dead_code)]);
        assert!(check_for_cfg_attr_repr(&attr.meta).is_ok());
        let attr: Attribute = parse_quote!(#[repr(C)]);
        assert!(check_for_cfg_attr_repr(&attr.meta).is_ok());
    }

    #[test]
    fn check_for_cfg_attr_repr_flags_gated_repr_c() {
        let attr: Attribute = parse_quote!(#[cfg_attr(feature = "extra", repr(C))]);
        assert!(check_for_cfg_attr_repr(&attr.meta).is_error());
    }

    #[test]
    fn check_for_cfg_attr_repr_flags_nested_gated_repr() {
        let attr: Attribute = parse_quote!(#[cfg_attr(a, cfg_attr(b, repr(align(8))))]);
        assert!(check_for_cfg_attr_repr(&attr.meta).is_error());
    }

    #[test]
    fn check_for_cfg_attr_repr_ok_without_repr() {
        let attr: Attribute =
            parse_quote!(#[cfg_attr(feature = "extra", allow(dead_code), deprecated)]);
        assert!(check_for_cfg_attr_repr(&attr.meta).is_ok());
    }

    #[test]
    fn parse_rejects_cfg_attr_gated_repr() {
        let input: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "extra", repr(packed))]
            #[derive(DST)]
            struct Foo { tail: [u8] }
        };
        assert!(DstStruct::parse(input).is_error());
    }

    #[test]
    fn parse_rejects_nested_cfg_attr_gated_repr_c() {
        let input: DeriveInput = parse_quote! {
            #[repr(C)]
            #[cfg_attr(feature = "a", cfg_attr(feature = "b", repr(C)))]
            #[derive(DST)]
            struct Foo { tail: [u8] }
        };
        assert!(DstStruct::parse(input).is_error());
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
