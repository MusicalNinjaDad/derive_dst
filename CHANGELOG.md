# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The generated `InvalidFoo` error no longer implements `Error::provide`, so crates using the
  derive no longer need `#![feature(error_generic_member_access)]`.
- `new` is replaced by a **safe** `new_uninit(size) -> Result<Box<UninitFoo>, InvalidFoo>` plus an
  `unsafe` `UninitFoo::assume_init(Box<Self>) -> Box<Foo>`. The derive now generates `UninitFoo`,
  a `#[repr(C)]` mirror of the struct with every field wrapped in `MaybeUninit` (identical layout,
  no validity invariant), so the constructor no longer assumes zeroed memory is valid for the tail
  element type `T` and no longer zeroes the allocation.
- Sized-field defaults (`field: Type = expr`) are now **optional**; when present, `new_uninit`
  writes them via `MaybeUninit::write`. The compile error for a missing default is gone.
- `new_uninit` builds the `*mut UninitFoo` with `core::ptr::from_raw_parts_mut` (thin data pointer
  + `count` metadata) and `assume_init` carries the metadata across with `ptr::metadata`; crates
  using the derive need `#![feature(ptr_metadata)]`. `#![feature(default_field_values)]` is only
  needed when defaults are declared.
- The tail element count is now typed as `<[T] as core::ptr::Pointee>::Metadata` (a `usize` for
  `[T]`) throughout the generated signatures and return values, instead of a bare `usize`.
- The size-based methods are renamed to `new_uninit_for_size` / `layout_for_size`, and a
  count-based pair `new_uninit_for_count(count)` / `layout_for_count(count)` is added; the count
  is the tail slice's `Pointee::Metadata`. `new_uninit_for_size` validates the size via
  `layout_for_size` and delegates to `new_uninit_for_count`.
- `new_uninit_for_size` / `new_uninit_for_count` return
  `(Box<UninitFoo>, Layout, tail_offset, count)` — the `Layout`, byte offset of the tail and tail
  element count are returned alongside the box so initialisation routines do not need to call
  `layout_for_*` again (issue #8).
- Everything the derive generates (the `UninitFoo` mirror and all methods) is now **private to the
  module** the struct is declared in, instead of being `pub` / inheriting the struct's visibility:
  the caller owns the public API and writes their own `pub fn new` wrapper around the helpers.
- The custom `InvalidFoo` error type is removed: all generated functions return
  `core::alloc::LayoutError` directly (only produced by genuinely failing `Layout` computations,
  e.g. overflow). `new_uninit_for_size` / `layout_for_size` no longer reject a size that does not
  match the correctly aligned and padded layout — they return the computed `Layout`, whose
  `size()` is the valid size, so the caller can detect and handle a mismatch itself before
  `assume_init`.

## [0.1.0] - 2026-11-20

### Added

- `#[derive(DST)]` for `#[repr(C)]` structs whose last field is an unsized slice `[T]`.
- Generates `new(size) -> Result<Box<Self>, InvalidFoo>` (allocating constructor) and
  `layout_for(size) -> Result<(Layout, usize, usize), InvalidFoo>` (layout constructor).
- Generates a dedicated `InvalidFoo` error enum with `InvalidLayout(LayoutError)` and
  `InvalidSize { requested, valid }` variants, implementing `Display`, `Error::source`,
  `Error::provide` (provides the `LayoutError` and `io::ErrorKind::InvalidData`) and
  `From<LayoutError>`.
