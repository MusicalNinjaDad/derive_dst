# derive_dst

Provides `#[derive(DST)]` for `#[repr(C)]` structs whose **last field is an unsized slice**
(`[T]`) — the usual way to mirror a C/C++ "flexible array member" struct such as
`SP_DEVICE_INTERFACE_DETAIL_DATA_W`.

## Why

The nomicon finishes the [section on DSTs][nomicon-DSTs] with the honest statement

> (Yes, custom DSTs are a largely half-baked feature for now.)

and yet, various ffi functions make use of them.

Often this is to provide a filepath or other
string as the final field (e.g. [`SP_DEVICE_INTERFACE_DETAIL_DATA_W`][Win-WPSIDDW] on Windows,
[`inotify_event`][linux-inotify] on linux). Even with null-terminated strings, you still need to
allocate a buffer in advance. Sometimes it's a dynamically-sized tail of structured data
(e.g. [`fiemap`][linux-fiemap] on linux).

So do you:

- make a developer-time guess and hope your buffer is always big enough, wasting any oversizing
  on every call, or ...
- find a way to allocate a DST at runtime, when you know the exact size needed?

## What you get

The case which gave me the pattern for this macro:

```rust
#![feature(ptr_metadata)]
use std::io;

use derive_dst::DST;

#[repr(C)]
#[derive(Debug, DST)]
#[allow(nonstandard_style, reason = "mirroring FFI struct")]
/// A dynamically sized ffi-compatible version of `SP_DEVICE_INTERFACE_DETAIL_DATA_W`
pub struct DeviceDetails {
    cbSize: u32,
    DevicePath: [u16],
}

impl DeviceDetails {
    /// Create a new DeviceDetails of the given size, as returned by an null-call to
    /// `SetupDiGetDeviceInterfaceDetailW`.
    ///
    /// Errors if the requested size does not produce a valid, well-aligned & padded struct
    pub fn new(size: usize) -> io::Result<Box<Self>> {
        // Create a new, boxed uninitialised DeviceDetails. This may return a LayoutError if the
        // overall struct is somehow invalid.
        let (mut raw, (layout, tail_offset, _tail_count)) =
            Self::new_uninit_for_size(size).map_err(io::Error::other)?;

        // Check that the returned DST is of the exact size requested. DST will always be
        // well-aligned and padded.
        if layout.size() != size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid buffer size",
            ));
        }

        // API requires cbSize to be set to the size of the struct as if it were storing a single
        // `\0` as the DevicePath.
        let cbsize = (tail_offset + size_of::<u16>()).next_multiple_of(layout.align());
        raw.cbSize.write(cbsize as u32);

        // SAFETY:
        // - cbSize is correctly set to size required for an empty path with a single null-terminator
        // - raw was initialized zeroed and `\0` is a valid value for u16
        let valid = unsafe { raw.assume_init() };
        Ok(valid)
    }
}
```

Behind the scenes this generates (an import-collision-safe version of).

The doc-comments are included in the generated code, so you can read them locally and in IDE

```rust
#[repr(C)]
#[allow(nonstandard_style, reason = "mirroring FFI struct")]
/// `MaybeUninit` mirror of [`DeviceDetails`]: identical `repr(C)` layout, but every field is
/// wrapped in `MaybeUninit`. Constructed by `new_uninit_for_...` methods
/// converted back to the initialised type with [`Self::assume_init`].
#[derive(Debug)]
struct UninitDeviceDetails
where
    u16: Sized,
    [u16]: Pointee<Metadata = usize>,
{
    cbSize: MaybeUninit<u32>,
    DevicePath: [MaybeUninit<u16>],
}

impl DeviceDetails
where
    u16: Sized,
    [u16]: Pointee<Metadata = usize>,
{
    /// Allocate a buffer for a `Self` value of `size` bytes and return it boxed as the
    /// `Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple.
    ///
    /// # SAFETY
    /// The allocation is entirely **uninitialised**: write every field with
    /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
    ///
    /// # Note
    /// a `size` that does not match the correctly aligned and padded layout for the derived
    /// tail count is **not** rejected — the returned `Layout` carries the valid size
    /// (`layout.size()`), so the caller can compare it with the requested `size` and decide how to
    /// handle a mismatch before calling `assume_init`.
    fn new_uninit_for_size(
        size: usize,
    ) -> Result<
        (
            Box<UninitDeviceDetails>,
            (Layout, usize, <[u16] as Pointee>::Metadata),
        ),
        LayoutError,
    > {
        // SAFETY: layout & count are directly calculated by layout_for_size and passed on
        unsafe { Result::Ok(Self::new_uninit_for(Self::layout_for_size(size)?)) }
    }

    /// Allocate a buffer holding `count` tail elements and return it boxed as the `Uninit`
    /// mirror of `Self`, together with the `(Layout, tail offset, tail count)` tuple. `count` is
    /// typed as the tail slice's `Pointee::Metadata` (a `usize` for `[T]`).
    ///
    /// # SAFETY
    /// The allocation is entirely **uninitialised**: write every field with
    /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
    fn new_uninit_for_count(
        count: <[u16] as Pointee>::Metadata,
    ) -> Result<
        (
            Box<UninitDeviceDetails>,
            (Layout, usize, <[u16] as Pointee>::Metadata),
        ),
        LayoutError,
    > {
        // SAFETY: layout & count are directly calculated by layout_for_count and passed on
        unsafe { Result::Ok(Self::new_uninit_for(Self::layout_for_count(count)?)) }
    }

    /// Allocate a buffer with the given `layout` and return it boxed as the
    /// `Uninit` mirror of `Self`, together with the `(Layout, tail offset, tail
    /// count)` tuple it was built for.
    ///
    /// # SAFETY
    /// The allocation is entirely **uninitialised**: write every field with
    /// [`MaybeUninit::write`], then convert with `assume_init` on the returned box.
    ///
    /// Extra care must be taken when using this function directly to ensure that
    /// `layout` and `count` are valid in combination. No validation is performed
    /// or possible in this function. Prefer the safe versions [`new_uninit_for_size`]
    /// and [`new_uninit_for_count] wherever possible.
    unsafe fn new_uninit_for(
        (layout, offset, count): (Layout, usize, <[u16] as Pointee>::Metadata),
    ) -> (
        Box<UninitDeviceDetails>,
        (Layout, usize, <[u16] as Pointee>::Metadata),
    ) {
        let raw = match Global::allocate_zeroed(&Global, layout) {
            Result::Ok(pointer) => pointer,
            Result::Err(_) => alloc::handle_alloc_error(layout),
        };
        let wide = ptr::from_raw_parts_mut::<UninitDeviceDetails>(raw.cast::<()>().as_ptr(), count);

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
        let boxed = unsafe { Box::from_raw(wide) };
        (boxed, (layout, offset, count))
    }

    /// Calculate the `(layout, offset of the unsized tail, tail element count)`
    /// for a `Self` buffer of `size` bytes.
    ///
    /// # SAFETY
    /// The correctness of this layout and the tail offset is relied upon by unsafe code
    /// in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_size`][Self::new_uninit_for_size]).
    fn layout_for_size(
        size: usize,
    ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
        let header = Self::layout_header()?;
        let elem_size = size_of::<u16>();
        let tail_bytes = size.saturating_sub(header.size());
        // `checked_div` returns `None` on a zero divisor (ZST tail element) — map that
        // to a count of 0 instead of panicking or special-casing
        let count: <[u16] as Pointee>::Metadata = tail_bytes.checked_div(elem_size).unwrap_or(0);
        Self::extend_with_tail(header, count)
    }

    /// Calculate the `(layout, offset of the unsized tail, tail element count)`
    /// for a `Self` buffer holding `count` tail elements.
    ///
    /// # SAFETY
    /// The correctness of this layout and the tail offset is relied upon by unsafe code
    /// in [`new_uninit_for`][Self::new_uninit_for] (via [`new_uninit_for_count`][Self::new_uninit_for_count]).
    fn layout_for_count(
        count: <[u16] as Pointee>::Metadata,
    ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
        Self::extend_with_tail(Self::layout_header()?, count)
    }

    /// The [`Layout`] of the sized head (everything before the unsized tail)
    fn layout_header() -> Result<Layout, LayoutError> {
        let mut prefix = Layout::new::<()>();
        prefix = prefix.extend(Layout::new::<u32>())?.0;
        Result::Ok(prefix)
    }

    /// Extend a head [`Layout`] (from [`layout_header`][Self::layout_header]) with the tail of
    /// `count` elements and return the resulting `(layout, tail offset, count)` tuple, padded
    /// to the alignment.
    fn extend_with_tail(
        header: Layout,
        count: <[u16] as Pointee>::Metadata,
    ) -> Result<(Layout, usize, <[u16] as Pointee>::Metadata), LayoutError> {
        let tail = Layout::array::<u16>(count)?;
        let (extended, offset) = header.extend(tail)?;
        Result::Ok((extended.pad_to_align(), offset, count))
    }
}

impl UninitDeviceDetails
where
    u16: Sized,
    [u16]: Pointee<Metadata = usize>,
{
    /// Convert into the fully initialised struct, reusing the same allocation.
    ///
    /// # SAFETY
    /// Every head field & all tail elements must have been initialised to a valid
    /// value for the respective types. All fields will have been initialised zeroed, but zero
    /// is not a valid value for all types. Calling `assume_init` where some fields
    /// are not properly initialised will lead to UB. See [`core::mem::MaybeUninit::assume_init`]
    /// for more details.
    unsafe fn assume_init(self: Box<Self>) -> Box<DeviceDetails> {
        let raw = Box::into_raw(self);
        // The mirror and the original have identical layouts (both `repr(C)`, and
        // `MaybeUninit<T>` is guaranteed to have the same size, alignment and ABI as `T`),
        // and the tail metadata (element count) is carried over unchanged.
        let wide = ptr::from_raw_parts_mut::<DeviceDetails>(raw.cast::<()>(), ptr::metadata(raw));
        // SAFETY:
        // - the caller guarantees (see `# SAFETY` above) that every field is initialised,
        //   so the pointee is a valid `#name` value
        // - layout and metadata are identical to the mirror's, so the fat pointer is correct
        // - the pointer is moved into exactly one `Box` and not used afterwards
        unsafe { Box::from_raw(wide) }
    }
}
```

## Requirements

- Nightly toolchain (and `feature(ptr_metadata)` not forbidden in config.toml)
- `#![feature(ptr_metadata)]`
- DST must:
  - be `#[repr(C)]`, no additional alignment or packing specification
  - have a *single*` unsized field as the *tail* (last field) as per the [nomicon][nomicon-DSTs]
  - have a slice `[T]` as the unsized tail

## SAFETY

We take extreme care to catch as many incorrect usages as possible at compile-time. Deliberately
or accidentally circumventing the above requirements will most likely lead to UB.

We recommend you build the docs locally via `cargo doc --document-private-items` to review the
requirements for `assume_init`

The *`UninitMirror`* type is zero-initialised and not guaranteed to be valid. All fields must
be suitably initialised, _including **all elements of the tail slice**_, before calling
`assume_init`.

## Alternatives

- Hand-roll your own DSTs using the same pattern (if you do, please star the repo on github so I know it's useful)
- If you don't have an API that can report the required size at runtime and need to trial-and-error [`grob`][https://docs.rs/grob/] handles growable buffers
- Create a fixed, hopefully oversized, buffer type
- If you are staying in rust *just `Box` the unsized type within the struct* and move on with all your hair intact!

[nomicon-DSTs]: https://doc.rust-lang.org/nomicon/exotic-sizes.html#dynamically-sized-types-dsts
[Win-WPSIDDW]: https://learn.microsoft.com/en-us/windows/win32/api/setupapi/ns-setupapi-sp_device_interface_detail_data_w
[linux-inotify]: https://man7.org/linux/man-pages/man7/inotify.7.html
[linux-fiemap]: https://docs.kernel.org/filesystems/fiemap.html#c.fiemap