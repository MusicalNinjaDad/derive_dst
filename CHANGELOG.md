# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- The generated `InvalidFoo` error no longer implements `Error::provide`, so crates using the
  derive no longer need `#![feature(error_generic_member_access)]`.
- `new` now builds the `*mut Self` with `core::ptr::from_raw_parts_mut` (thin data pointer +
  `count` metadata) instead of casting a `*mut [T]`; crates using the derive now also need
  `#![feature(ptr_metadata)]`.

## [0.1.0] - 2026-11-20

### Added

- `#[derive(DST)]` for `#[repr(C)]` structs whose last field is an unsized slice `[T]`.
- Generates `new(size) -> Result<Box<Self>, InvalidFoo>` (allocating constructor) and
  `layout_for(size) -> Result<(Layout, usize, usize), InvalidFoo>` (layout constructor).
- Generates a dedicated `InvalidFoo` error enum with `InvalidLayout(LayoutError)` and
  `InvalidSize { requested, valid }` variants, implementing `Display`, `Error::source`,
  `Error::provide` (provides the `LayoutError` and `io::ErrorKind::InvalidData`) and
  `From<LayoutError>`.
