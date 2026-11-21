use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToDiagnostic, prelude::*};
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, Generics, Ident, Meta, Token, Type, Visibility,
    punctuated::Punctuated, spanned::Spanned,
};

/// A destructured, validated `#[repr(C)]` struct with named fields and an unsized slice tail.
pub(crate) struct DstStruct<'ast> {
    vis: &'ast Visibility,
    name: &'ast Ident,
    generics: &'ast Generics,
    /// The sized "head" fields (all but the last); each is validated to have a default `= expr`.
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

        // repr(C) is required so the generated layout matches a C struct
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
                    return error("only the last field of a DST struct may be unsized").add_help(
                        trait_object.span(),
                        "move this trait object to the last position",
                    );
                }
                Type::Path(path) if path.path.is_ident("str") => {
                    return error("only the last field of a DST struct may be unsized")
                        .add_help(path.span(), "move this `str` to the last position");
                }
                _ => {}
            }
            if field.default.is_none() {
                return error("DST requires a default value (`= expr`) for every sized field")
                    .add_help(
                        field.span(),
                        "add `= <expr>` after the type, e.g. `cbSize: u32 = size_of::<u32>() as u32`",
                    );
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

    /// Generates the `InvalidFoo` error type and the `new` / `layout_for` inherent methods.
    pub(crate) fn generate(&self) -> TokenStream2 {
        let vis = self.vis;
        let name = self.name;
        let err = format_ident!("Invalid{}", name);
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let head_idents: Vec<&Ident> = self
            .head
            .iter()
            .map(|field| field.ident.as_ref().expect("named fields"))
            .collect();
        let head_tys: Vec<&Type> = self.head.iter().map(|field| &field.ty).collect();
        let defaults: Vec<&Expr> = self
            .head
            .iter()
            .map(|field| &field.default.as_ref().expect("validated default").1)
            .collect();
        let tail_ty = &self.tail.ty;
        let elem = self.tail_elem;

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

            impl ::std::error::Error for #err {
                fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> {
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

            impl #impl_generics #name #ty_generics #where_clause {
                #[doc = "Construct a new `Self` in a `Box`, allocating a buffer of `size` bytes."]
                #[doc = ""]
                #[doc = "Every sized field is initialised with its declared default (`field: Type = expr`);"]
                #[doc = "the tail is zeroed (the tail element type must be valid when zeroed)."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err` if `size` is not a valid, correctly aligned and padded total size"]
                #[doc = "for this type — see [`layout_for`][Self::layout_for]."]
                #[doc = ""]
                #[doc = concat!(
                    "# Implementation safety\n\n",
                    "The internal `unsafe` block is sound: `raw` was allocated by `Global` with the ",
                    "`layout` computed by [`layout_for`][Self::layout_for] for `size` and zeroed, so it is ",
                    "valid for reads & writes of that layout; `count` is the number of `",
                    stringify!(#elem),
                    "` tail elements used to build that layout, so `ptr::from_raw_parts_mut` ",
                    "attaches `count` as the metadata of `raw`, correct for `Self` (last field: `",
                    stringify!(#tail_ty),
                    "`). Zero is a valid bit pattern for the tail and every head field is overwritten ",
                    "with its default; the pointer is moved into exactly one `Box` and not used afterwards."
                )]
                pub fn new(size: usize) -> ::core::result::Result<::std::boxed::Box<Self>, #err> {
                    let (layout, _offset, count) = Self::layout_for(size)?;

                    let raw = match <::std::alloc::Global as ::std::alloc::Allocator>::allocate_zeroed(
                        &::std::alloc::Global,
                        layout,
                    ) {
                        ::core::result::Result::Ok(pointer) => pointer,
                        ::core::result::Result::Err(_) => ::std::alloc::handle_alloc_error(layout),
                    };

                    let raw = ::core::ptr::from_raw_parts_mut::<Self>(
                        raw.cast::<()>().as_ptr(),
                        count,
                    );

                    #[expect(
                        unsafe_code,
                        reason = "allocation validated by layout_for; tail metadata matches the unsized last field"
                    )]
                    let mut boxed = unsafe { ::std::boxed::Box::from_raw(raw) };
                    #(boxed.#head_idents = #defaults;)*

                    ::core::result::Result::Ok(boxed)
                }

                #[doc = "Calculate the `(layout, offset of the unsized tail, number of tail elements)`"]
                #[doc = "for a `Self` buffer of `size` bytes."]
                #[doc = ""]
                #[doc = "# Errors"]
                #[doc = "Returns `Err(InvalidLayout(..))` if the layout cannot be computed and"]
                #[doc = "`Err(InvalidSize { requested, valid })` if `size` is smaller than the sized head or"]
                #[doc = "does not match the correctly aligned and padded layout."]
                #[doc = ""]
                #[doc = "# SAFETY"]
                #[doc = "The correctness of this layout and the tail offset is relied upon by unsafe code"]
                #[doc = "in [`new`][Self::new]."]
                pub fn layout_for(
                    size: usize,
                ) -> ::core::result::Result<(::core::alloc::Layout, usize, usize), #err> {
                    // sequential `extend` reproduces the repr(C) placement of the sized head
                    let mut prefix = ::core::alloc::Layout::new::<()>();
                    #(
                        prefix = prefix.extend(::core::alloc::Layout::new::<#head_tys>())?.0;
                    )*

                    let elem_size = ::core::mem::size_of::<#elem>();
                    let Some(tail_bytes) = size.checked_sub(prefix.size()) else {
                        return ::core::result::Result::Err(#err::InvalidSize {
                            requested: size,
                            valid: prefix.pad_to_align().size(),
                        });
                    };
                    let count = if elem_size == 0 { 0 } else { tail_bytes / elem_size };

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
        }
    }
}
