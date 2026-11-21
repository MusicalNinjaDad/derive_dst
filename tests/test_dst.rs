#![cfg_attr(unstable_default_field_values, feature(default_field_values))]
#![cfg_attr(unstable_ptr_metadata, feature(ptr_metadata))]
// `has_*` is only emitted for build_safely's known features; `ptr_metadata` goes via
// `OtherFeature`, which only ever emits `unstable_*`
#![cfg(all(has_default_field_values, unstable_ptr_metadata))]
// field names mimic the C++ structs these DSTs mirror
#![allow(non_snake_case)]

use std::alloc::Layout;
use std::error::Error;
use std::mem::{offset_of, size_of, size_of_val};

use derive_dst::DST;

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct DeviceDetails {
    cbSize: u32 = size_of::<u32>() as u32,
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
    a: u8 = 1,
    b: u32 = 2,
    tail: [u8],
}

/// Defaults are optional: no sized field is required to declare a `= expr`.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct NoDefaults {
    a: u32,
    tail: [u8],
}

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
#[expect(
    unevaluated_default_field_value,
    reason = "default is only evaluated during construction; we test it via `Generic::new_uninit_for_size`"
)]
struct Generic<T> {
    tag: u8 = 7,
    items: [T],
}

/// `new_uninit_for_size` + write the tail + `assume_init`.
fn init_details(size: usize) -> Box<DeviceDetails> {
    let mut uninit = DeviceDetails::new_uninit_for_size(size).unwrap();
    // the declared default was already written by `new_uninit`
    // SAFETY: `new_uninit` wrote the default into `cbSize`
    assert_eq!(unsafe { uninit.cbSize.assume_init_read() }, 4);
    for (i, word) in uninit.DevicePath.iter_mut().enumerate() {
        word.write(0xA000 + i as u16);
    }
    // SAFETY: `cbSize` (default) and every `DevicePath` word are initialised
    unsafe { uninit.assume_init() }
}

#[test]
fn new_uninit_allocates_a_layout_identical_buffer() {
    let shadow = DeviceDetailsShadow {
        cbSize: 4,
        DevicePath: [0; 1],
    };

    // the Uninit-mirror has the same layout as the C shadow
    let uninit = DeviceDetails::new_uninit_for_size(size_of_val(&shadow)).unwrap();
    assert_eq!(Layout::for_value(&shadow), Layout::for_value(&*uninit));
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
    // layout_for reports its offset instead (checked in layout_for_reports_layout_offset_and_count)
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
fn new_uninit_rejects_a_misaligned_size() {
    let err = DeviceDetails::new_uninit_for_size(6).err().unwrap();
    assert_eq!(
        err,
        InvalidDeviceDetails::InvalidSize {
            requested: 6,
            valid: 8
        }
    );
    assert!(err.source().is_none());
    assert_eq!(
        err.to_string(),
        "6 is not valid as a size for DeviceDetails; when correctly aligned and padded the next valid size is 8"
    );
}

#[test]
fn new_uninit_rejects_a_size_smaller_than_the_head() {
    let err = DeviceDetails::new_uninit_for_size(2).err().unwrap();
    assert_eq!(
        err,
        InvalidDeviceDetails::InvalidSize {
            requested: 2,
            valid: 4
        }
    );
}

#[test]
fn layout_errors_are_sourced() {
    let err = DeviceDetails::new_uninit_for_size(usize::MAX)
        .err()
        .unwrap();
    assert!(matches!(err, InvalidDeviceDetails::InvalidLayout(_)));

    // Error::source
    assert!(err.source().is_some());
    assert_eq!(
        err.to_string(),
        "invalid layout requested for a DeviceDetails buffer"
    );
}

#[test]
fn from_layout_error() {
    let layout_error = Layout::array::<u8>(usize::MAX).unwrap_err();
    let err = InvalidDeviceDetails::from(layout_error);
    assert!(matches!(err, InvalidDeviceDetails::InvalidLayout(_)));
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
    assert_eq!(offset, 8); // tail offset, reported by layout_for

    let mut uninit = MultiHead::new_uninit_for_size(12).unwrap();
    // SAFETY: `new_uninit` wrote the declared defaults
    assert_eq!(unsafe { uninit.a.assume_init_read() }, 1);
    assert_eq!(unsafe { uninit.b.assume_init_read() }, 2);
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

    let mut uninit = Generic::<u16>::new_uninit_for_size(12).unwrap();
    // SAFETY: `new_uninit` wrote the declared default
    assert_eq!(unsafe { uninit.tag.assume_init_read() }, 7);
    for item in uninit.items.iter_mut() {
        item.write(0);
    }
    // SAFETY: all fields are initialised
    let generic = unsafe { uninit.assume_init() };
    assert_eq!(generic.tag, 7);
    assert_eq!(generic.items.len(), 5);

    let err = Generic::<u16>::new_uninit_for_size(11).err().unwrap();
    assert_eq!(
        err,
        InvalidGeneric::InvalidSize {
            requested: 11,
            valid: 12
        }
    );
}

#[test]
fn defaults_are_optional() {
    // repr(C): a at 0, tail [u8] at 4; total for count n = align_up(4 + n, 4)
    let (layout, offset, count) = NoDefaults::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    let mut uninit = NoDefaults::new_uninit_for_size(8).unwrap();
    uninit.a.write(42);
    for byte in uninit.tail.iter_mut() {
        byte.write(1);
    }
    // SAFETY: all fields are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.a, 42);
    assert_eq!(&value.tail[..], &[1, 1, 1, 1]);
}

#[test]
fn dropping_a_partially_initialised_mirror_is_safe() {
    let mut uninit = DeviceDetails::new_uninit_for_size(8).unwrap();
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
    let mut uninit = DeviceDetails::new_uninit_for_count(2).unwrap();
    // SAFETY: `new_uninit_for_count` wrote the declared default
    assert_eq!(unsafe { uninit.cbSize.assume_init_read() }, 4);
    assert_eq!(uninit.DevicePath.len(), 2);
    for (i, word) in uninit.DevicePath.iter_mut().enumerate() {
        word.write(0xB000 + i as u16);
    }
    // SAFETY: `cbSize` (default) and both `DevicePath` words are initialised
    let details = unsafe { uninit.assume_init() };
    assert_eq!(details.cbSize, 4);
    assert_eq!(&details.DevicePath[..], &[0xB000, 0xB001]);
    // the allocation matches layout_for_count(2)
    assert_eq!(
        Layout::for_value(&*details),
        DeviceDetails::layout_for_count(2).unwrap().0
    );
}

#[test]
fn new_uninit_for_count_rejects_an_overflowing_count() {
    let err = DeviceDetails::new_uninit_for_count(usize::MAX)
        .err()
        .unwrap();
    assert!(matches!(err, InvalidDeviceDetails::InvalidLayout(_)));
}
