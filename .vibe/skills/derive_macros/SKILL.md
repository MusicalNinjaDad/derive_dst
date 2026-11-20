---
name: derive_macros
description: Load this skill when creating or modifying a proc-macro derive crate in this repo (or a sibling repo by the same author). Covers error handling with proc_macro2_diagnostic, implementation structure, tests, build_safely nightly gating and documentation style, distilled from try_v2_derive (try_v2), exit_safely and proc_macro2_diagnostic.
user-invocable: true
---

# Derive Macros Skill

Patterns for writing `#[derive(...)]` proc-macro crates, distilled from the author's
[`try_v2_derive`](https://github.com/MusicalNinjaDad/try_v2) (`try_v2` repo),
[`exit_safely`](https://github.com/MusicalNinjaDad/exit_safely) and
[`proc_macro2_diagnostic`](https://github.com/MusicalNinjaDad/proc_macro2_diagnostic).

## 1. Error handling with `proc_macro2_diagnostic`

Depend on `proc-macro2`, `proc_macro2_diagnostic`, `quote`, `syn` (features `full`, `extra-traits`):

```toml
[dependencies]
syn = { version = "3", features = ["full", "extra-traits"] }
proc-macro2 = "1"
proc_macro2_diagnostic = "0.8"
quote = "1"
```

Structure the entry point so the whole body returns a `DiagnosticStream` and is converted once:

```rust
use proc_macro::TokenStream as TokenStream1;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro2_diagnostic::{ToTokens, prelude::*};

#[proc_macro_derive(DST)]
pub fn derive_dst(input: TokenStream1) -> TokenStream1 {
    impl_dst(input.into()).to_tokens() // DiagnosticResult<TokenStream2> -> TokenStream1
}

fn impl_dst(input: TokenStream2) -> DiagnosticStream {
    let ast: syn::DeriveInput = syn::parse2(input).expect("derive macro input");
    // ... validate with `?` on DiagnosticResult ...
    Ok(quote! { /* generated items */ })
}
```

- Build errors with `error("...")`, `error_spanned(span, "...")`, `warn_spanned((), span, "...")` and
  chain context with `.add_help(span, "...")` / `.add_note(...)`; convert `Option`/`Result` with
  `.or_error("...")` — all from `proc_macro2_diagnostic::prelude::*`.
- Return `DiagnosticResult<T>` / `DiagnosticStream` from helper (parse) functions and use `?`.
- On nightly the diagnostics are emitted natively (spans, help, notes); on stable
  `proc_macro2_diagnostic` falls back to plain `compile_error!`-style output, so the crate still
  works everywhere.
- Validate invariants *fail fast* at the top of the parse function (see `TryEnum::parse`), pointing
  at the offending token with `add_help` (e.g. `struct_data.struct_token.span()`).

## 2. Implementation structure

- Single-purpose proc-macro crate: `[lib] proc-macro = true`, edition 2024.
- `src/lib.rs`: crate-level docs (with runnable doctests), the `#[proc_macro_derive(...)]` entry
  point (thin: parse → validate → `quote!` → `.to_tokens()`), and the top-level `impl_*` function.
- `src/parse.rs`: a `pub(crate) struct XxxStruct<'ast>` holding the destructured, *validated*
  `DeriveInput` (mirrors `TryEnum`), with `parse(&DeriveInput) -> DiagnosticResult<Self>` and a
  `generate(&self) -> TokenStream2` (or arm-generating helpers like `generate_arms`).
- Re-export nothing; the derive is the only public item. Use fully-qualified paths
  (`::core::...`, `::std::...`) in generated code so downstream crates need no imports.
- Respect downstream lint setups: e.g. emit `#[expect(unsafe_code, reason = "...")]` on generated
  `unsafe` statements so crates with `#![deny(unsafe_code)]` keep compiling.
- Honor generics via `ast.generics.split_for_impl()`.

## 3. Integration tests & compile-fail tests

- `tests/test_usage.rs` (integration): real usage of the derive, gated on the build-safely cfgs:

  ```rust
  #![cfg_attr(unstable_default_field_values, feature(default_field_values))]
  #![cfg(all(has_default_field_values, /* ... */))]
  ```

- `tests/test_compilation.rs` + `tests/compilation/fail_*.rs` / `fail_*.stderr` via
  [`trybuild`](https://crates.io/crates/trybuild):

  ```rust
  use trybuild::TestCases;

  #[test]
  fn errors() {
      TestCases::new().compile_fail("tests/compilation/fail_*.rs");
  }
  ```

  - Gate the test on `#[cfg(has_proc_macro_diagnostic)]` when the `.stderr` files were recorded
    with native diagnostics (nightly), because the stable fallback renders differently.
  - Record `.stderr` with `TRYBUILD=overwrite cargo +nightly test`; they are nightly-version
    sensitive, re-record when bumping the toolchain.
  - Fail test files start with `#![allow(stable_features)]` + the `#![feature(...)]` attrs needed to
    parse the snippet, then a `fn main() {}`.
- Unit tests inside `src/lib.rs` (`#[cfg(test)] mod tests`) can snapshot the generated tokens:
  `assert_eq!(expected.to_string(), impl_xxx(input).unwrap().to_string())` (see `exit_safely`).

## 4. build_safely for nightly feature availability

`build.rs` (build-dependency `build_safely = "0.6"`):

```rust
use build_safely::prelude::*;

fn main() -> Result<()> {
    let mut ac = AutoCfg::new()?;
    let allowed_features = cargo_allowed_features()?;
    ac.emit_unstable_feature(proc_macro_diagnostic, &allowed_features); // dedicated variant
    ac.emit_unstable_feature(default_field_values, &allowed_features);  // dedicated variant
    ac.emit_unstable_feature(OtherFeature("error_generic_member_access".to_string()), &allowed_features);
    Ok(())
}
```

- This emits `cargo::rustc-cfg=unstable_<feature>` / `has_<feature>` (and check-cfg) for every
  toolchain, so `#![cfg_attr(unstable_proc_macro_diagnostic, feature(proc_macro_diagnostic))]` in
  `lib.rs` enables the feature only where it is still unstable, and tests can use
  `#[cfg(has_proc_macro_diagnostic)]`.
- Features without a dedicated variant use `OtherFeature("name".to_string())`.
- If the crate *requires* nightly (as `derive_dst` does, because the generated code needs
  `default_field_values` + `error_generic_member_access`), pin it in `rust-toolchain.toml`:

  ```toml
  [toolchain]
  channel = "nightly"
  profile = "default"
  ```

  and keep the CI matrix nightly-only (`.github/workflows/.config/matrix.yml`).
- `.cargo/config.toml` keeps the author's aliases (`bld = "ninja build"`, `stage = "ninja stage"`);
  see the `cargo-stage` skill for `cargo stage --strict --json` as the single verification command.

## 5. Documentation style

- Crate-level `//!` docs in `src/lib.rs`: short purpose statement, then a runnable doctest showing
  the derive on a minimal example (doctests are separate crates: enable needed nightly features
  directly with `#![feature(...)]` inside the doctest).
- On the `#[proc_macro_derive]` fn: document the **contract** (what the input must look like), the
  **derived code** (show an `ignore`d expansion sketch), and notable design decisions
  (see `try_v2_derive`'s "Things to note" / poka-yoke sections).
- Generated items get doc comments too (they appear in downstream rustdoc): document `# Errors`,
  and `# SAFETY` on any generated `unsafe`.
- A `README.md` with the motivation, a before/after example and the requirements; plus
  `CHANGELOG.md` (Keep a Changelog). Markdown is linted in the devcontainer (markdownlint with
  MD013/MD046 disabled — let the editor wrap lines).
- Repo foundation (`.gitignore`, `.devcontainer/`, `.github/workflows/` incl. `rust.yml`,
  `rust-stability.yml`, `deploy-rust.yml`, dependabot) is lifted from `try_v2` / `exit_safely`;
  keep the dependabot auto-merge workflow pointed at the right `owner/repo`.
