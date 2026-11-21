# derive_dst

`#[derive(DST)]` for `#[repr(C)]` structs whose **last field is an unsized slice** (`[T]`),
typically to mirror a C/C++ "flexible array member" struct such as
[`SP_DEVICE_INTERFACE_DETAIL_DATA_W`](https://learn.microsoft.com/en-us/windows/win32/api/setupapi/ns-setupapi-sp_device_interface_detail_data_w).

The derive generates (all as **private items in the same module** — you own the public API, e.g. by
writing your own `pub fn new` that wraps these helpers, and no implementation details leak into it):

- **`UninitFoo`** — a `#[repr(C)]` mirror struct (named `Uninit` + the struct name) in which every
  field is wrapped in `MaybeUninit`. `MaybeUninit<T>` has the same size, alignment and ABI as `T`
  but no validity invariant, so the mirror has a layout identical to `Foo` yet is valid for *any*
  bit pattern, including a fresh uninitialised allocation. The mirror carries the **same
  attributes** as `Foo` (copied verbatim — lint levels, `cfg`/`cfg_attr` gates, extra `repr` hints,
  ...) except `derive` and `doc`, and each mirrored field likewise carries its original field's
  attributes except `derive` and `doc`.
- **`Foo::new_uninit_for_size(size: usize)` / `Foo::new_uninit_for_count(count: <[T] as Pointee>::Metadata)`**
  → `Result<(Box<UninitFoo>, (Layout, usize, <[T] as Pointee>::Metadata)), LayoutError>` — **safe**
  allocating constructors taking the total buffer size or the tail element count: they compute the
  layout, allocate a buffer of that layout, attach the tail element count as pointer metadata and
  return it boxed **together with the `(Layout, tail offset, tail count)` tuple** — the same tuple
  the `layout_for_*` constructors return — so initialisation routines do not need to call
  `layout_for_*` again. The allocation is entirely **uninitialised**: write every field (via
  `MaybeUninit::write`, e.g. in your own `new`) before `assume_init`. A `size` that does not match
  the correctly aligned and padded layout is *not* an error —
  compare the returned `layout.size()` with the requested `size` yourself and decide how to handle
  it before `assume_init`.
- **`Foo::new_uninit_for((layout, tail_offset, count)) -> (Box<UninitFoo>, (Layout, usize, count))`** —
  the **single allocation site** shared by `new_uninit_for_size` / `new_uninit_for_count` (they just
  feed it the tuple their `layout_for_*` counterpart returns): allocate → `ptr::from_raw_parts_mut`
  → `Box::from_raw`, exactly once, so the layout is computed exactly once per call and the `unsafe`
  sequence exists in one place only.
- **`UninitFoo::assume_init(self: Box<Self>) -> Box<Foo>`** — the single `unsafe` boundary: converts
  the box to the initialised type once every field has been written (typically inside a user-written
  `new` that calls `new_uninit_for_size` / `new_uninit_for_count`, does any extra initialisation,
  then calls `assume_init`).
- **`Foo::layout_for_size(size: usize)` / `Foo::layout_for_count(count: <[T] as Pointee>::Metadata)`**
  → `Result<(Layout, usize, <[T] as Pointee>::Metadata), LayoutError>` — a layout constructor pair
  returning the `(Layout, tail offset, tail count)` tuple: the [`Layout`], the byte offset of the
  unsized tail and the tail element count (the tail slice's `Pointee::Metadata`, i.e. the
  fat-pointer metadata — a `usize` for `[T]`). They are built from two shared helpers:
  `Foo::layout_header() -> Result<Layout, LayoutError>` (the sized head's layout, computed once) and
  `Foo::extend_with_tail(header, count) -> Result<(Layout, usize, count), LayoutError>` (head +
  tail of `count` elements, padded to alignment).
- On failure all of the above return **`core::alloc::LayoutError`** — only ever produced by a
  genuinely failing `Layout` computation (e.g. the tail array overflowing `isize::MAX`).

## Example

### Before (hand-written, e.g. in `redbook`)

```rust,ignore
#[repr(C)]
pub(crate) struct DeviceDetails {
    cbSize: u32,
    DevicePath: [u16],
}

impl DeviceDetails {
    pub fn new(size: usize) -> Result<Box<Self>, InvalidBufferSize> {
        let (layout, offset, num_words) = Self::layout_for(size)?;
        let rawu8 = match Global.allocate_zeroed(layout) {
            Ok(pointer) => pointer,
            Err(error) => handle_alloc_error(layout),
        };
        let opaque = rawu8.as_mut_ptr() as *mut ();
        let raw = ptr::from_raw_parts_mut::<Self>(opaque, num_words);
        let mut boxed = unsafe { Box::from_raw(raw) };
        boxed.cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        Ok(boxed)
    }

    pub fn layout_for(
        size: usize,
    ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), InvalidBufferSize> {
        let cbsize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        let layout_cbsize = Layout::for_value(&cbsize);
        let num_words = size.strict_sub(layout_cbsize.size()).strict_div(size_of::<u16>());
        let layout_devicepath = Layout::array::<u16>(num_words)?;
        let (layout_extended, offset) = layout_cbsize.extend(layout_devicepath)?;
        let layout = layout_extended.pad_to_align();
        (layout.size() == size)
            .ok_or_else(|| InvalidBufferSize::InvalidSize { requested: size, valid: layout.size() })?;
        Ok((layout, offset, num_words))
    }
}
```

### After (with `derive_dst`)

```rust
#![feature(ptr_metadata)]
use derive_dst::DST;

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
pub struct DeviceDetails {
    cbSize: u32,
    DevicePath: [u16],
}

impl DeviceDetails {
    // Your own public constructor — everything the derive generates stays private
    // to this module, so no implementation details are exposed.
    pub fn new(size: usize) -> Result<Box<Self>, std::alloc::LayoutError> {
        let (mut uninit, (layout, _tail_offset, _count)) = Self::new_uninit_for_size(size)?;
        // A mismatched size is not an error from the derive; validate it yourself:
        assert_eq!(layout.size(), size, "size must match the padded layout");
        uninit.cbSize.write(size_of::<u32>() as u32);
        for word in uninit.DevicePath.iter_mut() {
            word.write(0);
        }
        // SAFETY: `cbSize` and every `DevicePath` word are initialised
        Ok(unsafe { uninit.assume_init() })
    }
}

// Generated by the derive (all private to this module):
// - `UninitDeviceDetails` — the MaybeUninit mirror struct
// - `DeviceDetails::new_uninit_for_size(size)` / `new_uninit_for_count(count)`
//   -> `Result<(Box<UninitDeviceDetails>, (Layout, usize, count)), LayoutError>`
// - `DeviceDetails::new_uninit_for((layout, offset, count))` — the single allocation site
//   -> `(Box<UninitDeviceDetails>, (Layout, usize, count))`
// - `DeviceDetails::layout_header()` / `extend_with_tail(header, count)` — the head/tail
//   layout helpers shared by `layout_for_size` / `layout_for_count`
// - `UninitDeviceDetails::assume_init(Box<Self>) -> Box<DeviceDetails>` (unsafe)
// - `DeviceDetails::layout_for_size(size)` / `layout_for_count(count)`
```

### Or allocate by tail count

```rust
let (mut uninit, (layout, tail_offset, count)) = DeviceDetails::new_uninit_for_count(2)?;
assert_eq!((layout.size(), tail_offset, count), (8, 4, 2));
```

## Requirements & contract

- **Nightly toolchain** (pinned via `rust-toolchain.toml`). The only nightly feature required in
  the crate *using* the derive is `ptr_metadata` — for the generated `ptr::from_raw_parts_mut` /
  `ptr::metadata` calls.

  (The proc-macro crate itself additionally uses `proc_macro_diagnostic`, detected via
  [`build_safely`](https://crates.io/crates/build_safely), for rich compiler errors.)
- The annotated type must be a **struct with named fields** and **`#[repr(C)]`** (the whole point
  is a predictable, C-compatible layout).
- The **last field must be an unsized slice** `[T]`; only the last field may be unsized. The
  generated impl headers carry the explicit bounds `where [T]: Pointee<Metadata = usize>, T: Sized`
  — pinning the tail's fat-pointer metadata to `usize` (what the unsafe code relies on) and making
  `size_of::<T>()` / `Layout::array::<T>()` well-defined. Zero-sized elements (`tail: [()]`) are
  allowed: `layout_for_size` then maps any `size` to a tail count of 0 (a `checked_div` that yields
  0 on a zero divisor, so it cannot panic), while `layout_for_count` accepts any count.
- Default field values (`field: Type = expr`) are **not** supported and are rejected with a
  spanned compile error (the `UninitFoo` mirror could not carry them); write every field in your
  own constructor (e.g. `new`) before `assume_init`.
- Everything generated (the `UninitFoo` mirror and all methods) is **private to the module** the
  struct is declared in — write your own `pub` wrappers (e.g. `pub fn new`) around them.

Misuse (wrong item kind, missing `repr(C)`, sized tail, unsized non-tail field, ...) is reported
with rich, spanned compiler diagnostics via
[`proc_macro2_diagnostic`](https://github.com/MusicalNinjaDad/proc_macro2_diagnostic), and covered
by compile-fail tests (`tests/compilation/`).

## Safety

`new_uninit_for_size` / `new_uninit_for_count` allocate with `Global.allocate` using the computed
layout, form the fat pointer with `ptr::from_raw_parts_mut` (thin data pointer + element count as
metadata) and box it as `Box<UninitFoo>`. This is sound for any field types because every field of
the mirror is `MaybeUninit<_>`, which is valid for any bit pattern — no zeroed-validity assumption
is needed anywhere. The allocation always matches the `Layout` returned alongside the box; a
requested `size` that does not match the correctly aligned and padded layout is surfaced through
that `Layout` (`layout.size() != size`), not through an error, so the caller decides how to handle
it. The only `Err` is `LayoutError` from a genuinely failing layout computation (e.g. overflow).

The only `unsafe` the *user* writes is `UninitFoo::assume_init`, whose contract is that every field
has been initialised. Dropping a `Box<UninitFoo>` without `assume_init` is safe (and simply frees
the allocation): `MaybeUninit` fields have no drop glue.
