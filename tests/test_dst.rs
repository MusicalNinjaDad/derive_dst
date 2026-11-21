#![cfg_attr(unstable_ptr_metadata, feature(ptr_metadata))]
// `has_*` is only emitted for build_safely's known features; `ptr_metadata` goes via
// `OtherFeature`, which only ever emits `unstable_*`
#![cfg(unstable_ptr_metadata)]
// field names mimic the C++ structs these DSTs mirror
#![allow(non_snake_case)]

use std::alloc::{Layout, LayoutError};
use std::error::Error;
use std::mem::{offset_of, size_of_val};

use derive_dst::DST;

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct DeviceDetails {
    cbSize: u32,
    DevicePath: [u16],
}

/// Sized shadow of the Windows struct `SP_DEVICE_INTERFACE_DETAIL_DATA_W`.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
struct DeviceDetailsShadow {
    cbSize: u32,
    DevicePath: [u16; 1],
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct MultiHead {
    a: u8,
    b: u32,
    tail: [u8],
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct Generic<T> {
    tag: u8,
    items: [T],
}

/// `new_uninit_for_size` + write every field + `assume_init`.
fn init_details(size: usize) -> Box<DeviceDetails> {
    let (mut uninit, (_layout, _offset, _count)) =
        DeviceDetails::new_uninit_for_size(size).unwrap();
    uninit.cbSize.write(4);
    for (i, word) in uninit.DevicePath.iter_mut().enumerate() {
        word.write(0xA000 + i as u16);
    }
    // SAFETY: `cbSize` and every `DevicePath` word are initialised
    unsafe { uninit.assume_init() }
}

#[test]
fn new_uninit_allocates_a_layout_identical_buffer() {
    let shadow = DeviceDetailsShadow {
        cbSize: 4,
        DevicePath: [0; 1],
    };

    // the Uninit-mirror has the same layout as the C shadow
    let (uninit, (layout, _offset, _count)) =
        DeviceDetails::new_uninit_for_size(size_of_val(&shadow)).unwrap();
    assert_eq!(Layout::for_value(&shadow), Layout::for_value(&*uninit));
    assert_eq!(layout, Layout::for_value(&shadow));
    assert_eq!(
        offset_of!(DeviceDetailsShadow, cbSize),
        offset_of!(UninitDeviceDetails, cbSize)
    );
    assert_eq!(offset_of!(DeviceDetailsShadow, DevicePath), 4);

    let custom = init_details(size_of_val(&shadow));
    assert_eq!(Layout::for_value(&shadow), Layout::for_value(&*custom));
    assert_eq!(
        offset_of!(DeviceDetailsShadow, cbSize),
        offset_of!(DeviceDetails, cbSize)
    );
    // the tail of a DST cannot be named in offset_of! without `offset_of_slice`;
    // layout_for_size reports its offset instead (checked in layout_for_reports_layout_offset_and_count)
    assert_eq!(custom.cbSize, 4);
    assert_eq!(custom.DevicePath.len(), 2);
    assert_eq!(&custom.DevicePath[..], &[0xA000, 0xA001]);
}

#[test]
fn layout_for_reports_layout_offset_and_count() {
    let (layout, offset, count) = DeviceDetails::layout_for_size(8).unwrap();
    assert_eq!(layout, Layout::new::<DeviceDetailsShadow>());
    assert_eq!(offset, offset_of!(DeviceDetailsShadow, DevicePath));
    assert_eq!(count, 2);
}

#[test]
fn new_uninit_allows_an_empty_tail() {
    let details = init_details(4);
    assert_eq!(details.cbSize, 4);
    assert!(details.DevicePath.is_empty());
}

#[test]
fn new_uninit_for_size_does_not_reject_a_misaligned_size_but_returns_the_layout() {
    // 6 bytes maps to a 1-word tail; the correctly aligned & padded layout is 8 bytes.
    // That mismatch is not an error: the returned `Layout` carries the valid size, so the
    // caller can detect it (6 != layout.size()) and decide how to handle it.
    let (uninit, (layout, offset, count)) = DeviceDetails::new_uninit_for_size(6).unwrap();
    assert_eq!(layout.size(), 8);
    assert_ne!(layout.size(), 6);
    assert_eq!((offset, count), (4, 1));
    assert_eq!(uninit.DevicePath.len(), 1);
}

#[test]
fn new_uninit_for_size_maps_a_size_smaller_than_the_head_to_an_empty_tail() {
    let (uninit, (layout, offset, count)) = DeviceDetails::new_uninit_for_size(2).unwrap();
    assert_eq!((layout.size(), offset, count), (4, 4, 0));
    assert!(uninit.DevicePath.is_empty());
}

#[test]
fn layout_errors_are_plain_layout_errors() {
    // usize::MAX overflows the tail array layout -> Err(LayoutError)
    let err = DeviceDetails::new_uninit_for_size(usize::MAX)
        .err()
        .unwrap();
    assert_eq!(err, Layout::array::<u16>(usize::MAX).unwrap_err());

    // LayoutError is a std unit error: no source, fixed Display
    assert!(err.source().is_none());
    assert_eq!(
        err.to_string(),
        "invalid parameters to Layout::from_size_align"
    );

    let err = DeviceDetails::layout_for_count(usize::MAX).err().unwrap();
    assert_eq!(err, Layout::array::<u8>(usize::MAX).unwrap_err());
}

#[test]
fn multi_head_layout_matches_repr_c() {
    // repr(C): a at 0, b at 4, tail at 8; total for count n = align_up(8 + n, 4)
    let (layout, offset, count) = MultiHead::layout_for_size(12).unwrap();
    assert_eq!(layout.size(), 12);
    assert_eq!(layout.align(), 4);
    assert_eq!(offset, 8);
    assert_eq!(count, 4);
    assert_eq!(offset_of!(MultiHead, b), 4);
    assert_eq!(offset, 8); // tail offset, reported by layout_for_size

    let (mut uninit, (_layout, _offset, _count)) = MultiHead::new_uninit_for_size(12).unwrap();
    uninit.a.write(1);
    uninit.b.write(2);
    for byte in uninit.tail.iter_mut() {
        byte.write(0);
    }
    // SAFETY: all fields are initialised
    let multi = unsafe { uninit.assume_init() };
    assert_eq!((multi.a, multi.b), (1, 2));
    assert_eq!(multi.tail.len(), 4);
}

#[test]
fn generic_tail() {
    let (layout, offset, count) = Generic::<u16>::layout_for_size(12).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 2, 2, 5)
    );

    let (mut uninit, (_layout, _offset, _count)) = Generic::<u16>::new_uninit_for_size(12).unwrap();
    uninit.tag.write(7);
    for item in uninit.items.iter_mut() {
        item.write(0);
    }
    // SAFETY: all fields are initialised
    let generic = unsafe { uninit.assume_init() };
    assert_eq!(generic.tag, 7);
    assert_eq!(generic.items.len(), 5);

    // 11 does not match the padded layout (12); not an error, the Layout says so
    let (_uninit, (layout, _offset, _count)) = Generic::<u16>::new_uninit_for_size(11).unwrap();
    assert_eq!(layout.size(), 12);
}

#[test]
fn dropping_a_partially_initialised_mirror_is_safe() {
    let (mut uninit, (_layout, _offset, _count)) = DeviceDetails::new_uninit_for_size(8).unwrap();
    uninit.DevicePath[0].write(1); // leave the rest uninitialised
    // MaybeUninit fields have no drop glue, so dropping here runs no destructors on
    // uninitialised bytes (no UB) and still frees the allocation
    drop(uninit);
}

#[test]
fn layout_for_count_reports_layout_offset_and_count() {
    // DeviceDetails: cbSize (u32) at 0, tail [u16] at 4; count 3 -> align_up(4 + 6, 4) = 12
    let (layout, offset, count) = DeviceDetails::layout_for_count(3).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 4, 4, 3)
    );

    let (layout, offset, count) = MultiHead::layout_for_count(4).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 4, 8, 4)
    );

    let (layout, offset, count) = Generic::<u16>::layout_for_count(5).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 2, 2, 5)
    );
}

#[test]
fn new_uninit_for_count_allocates_for_that_count() {
    let (mut uninit, (layout, offset, count)) = DeviceDetails::new_uninit_for_count(2).unwrap();
    assert_eq!((layout.size(), offset, count), (8, 4, 2));
    uninit.cbSize.write(4);
    assert_eq!(uninit.DevicePath.len(), 2);
    for (i, word) in uninit.DevicePath.iter_mut().enumerate() {
        word.write(0xB000 + i as u16);
    }
    // SAFETY: `cbSize` and both `DevicePath` words are initialised
    let details = unsafe { uninit.assume_init() };
    assert_eq!(details.cbSize, 4);
    assert_eq!(&details.DevicePath[..], &[0xB000, 0xB001]);
    // the allocation matches layout_for_count(2)
    assert_eq!(Layout::for_value(&*details), layout);
    assert_eq!(layout, DeviceDetails::layout_for_count(2).unwrap().0);
}

#[test]
fn new_uninit_for_count_rejects_an_overflowing_count() {
    let err: LayoutError = DeviceDetails::new_uninit_for_count(usize::MAX)
        .err()
        .unwrap();
    assert_eq!(err, Layout::array::<u16>(usize::MAX).unwrap_err());
}

#[test]
fn new_uninit_returns_layout_offset_and_count_alongside_the_box() {
    // issue #8: the nested tuple carries everything an initialisation routine needs,
    // so it does not have to call `layout_for_*` again — and it is exactly the tuple
    // `layout_for_size` returns
    let (uninit, spec) = DeviceDetails::new_uninit_for_size(8).unwrap();
    let expected = DeviceDetails::layout_for_size(8).unwrap();
    assert_eq!(spec, expected);
    assert_eq!((spec.1, spec.2), (4, 2));
    assert_eq!(Layout::for_value(&*uninit), spec.0);
    assert_eq!(uninit.DevicePath.len(), 2);

    let (uninit, (_layout, offset, count)) = MultiHead::new_uninit_for_count(4).unwrap();
    assert_eq!((offset, count), (8, 4));
    assert_eq!(uninit.tail.len(), 4);
}

/// A zero-sized tail element is allowed: `size_of::<()>()` is 0, and the generated
/// `checked_div(..).unwrap_or(0)` yields count 0 (no divide-by-zero panic).
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct ZstTail {
    tag: u8,
    tail: [()],
}

#[test]
fn zst_tail_element_is_supported() {
    // repr(C): tag (u8) at 0; the ZST tail occupies 0 bytes at offset 1
    let (layout, offset, count) = ZstTail::layout_for_size(100).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (1, 1, 1, 0));

    // count-based layout accepts any count for a ZST tail (0 bytes either way)
    let (layout, offset, count) = ZstTail::layout_for_count(5).unwrap();
    assert_eq!((layout.size(), offset, count), (1, 1, 5));

    let (mut uninit, (layout, offset, count)) = ZstTail::new_uninit_for_size(4).unwrap();
    assert_eq!((layout.size(), offset, count), (1, 1, 0));
    assert!(uninit.tail.is_empty());
    uninit.tag.write(1);
    // SAFETY: `tag` written; the tail has no elements to initialise
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.tag, 1);
    assert!(value.tail.is_empty());

    let (mut uninit, (_layout, _offset, count)) = ZstTail::new_uninit_for_count(3).unwrap();
    assert_eq!(count, 3);
    assert_eq!(uninit.tail.len(), 3);
    uninit.tag.write(1);
    for unit in uninit.tail.iter_mut() {
        unit.write(());
    }
    // SAFETY: `tag` + every (zero-sized) tail element written
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.tag, 1);
    assert_eq!(value.tail.len(), 3);
}

/// Attributes on the struct and on the fields are mirrored onto the `Uninit` mirror and its
/// fields (except `derive` & `doc`) — including `#[cfg]`, which also gates the generated
/// per-field statements, so a cfg'd-out field is consistently absent from the original struct,
/// the mirror and the generated code alike.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct CfgGatedField {
    a: u32,
    #[cfg(any())] // never enabled: dropped from the struct, the mirror and the generated code
    b: u32,
    tail: [u8],
}

#[test]
fn cfg_gated_field_is_consistent_between_struct_mirror_and_generated_code() {
    // repr(C): a at 0, tail at 4 — the cfg'd-out `b` contributes nothing anywhere
    let (layout, offset, count) = CfgGatedField::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    let (mut uninit, (_layout, _offset, _count)) = CfgGatedField::new_uninit_for_size(8).unwrap();
    uninit.a.write(9);
    for byte in uninit.tail.iter_mut() {
        byte.write(1);
    }
    // SAFETY: all (remaining) fields are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.a, 9);
    assert_eq!(&value.tail[..], &[1, 1, 1, 1]);
}
