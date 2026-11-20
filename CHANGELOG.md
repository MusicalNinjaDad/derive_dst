# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-20

### Added

- `#[derive(DST)]` for `#[repr(C)]` structs whose last field is an unsized slice `[T]`.
- Generates `new(size) -> Result<Box<Self>, InvalidFoo>` (allocating constructor) and
  `layout_for(size) -> Result<(Layout, usize, usize), InvalidFoo>` (layout constructor).
- Generates a dedicated `InvalidFoo` error enum with `InvalidLayout(LayoutError)` and
  `InvalidSize { requested, valid }` variants, implementing `Display`, `Error::source`,
  `Error::provide` (provides the `LayoutError` and `io::ErrorKind::InvalidData`) and
  `From<LayoutError>`.
