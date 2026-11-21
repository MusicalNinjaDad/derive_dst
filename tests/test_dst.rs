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

#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
#[expect(
    unevaluated_default_field_value,
    reason = "default is only evaluated during construction; we test it via `Generic::new`"
)]
struct Generic<T> {
    tag: u8 = 7,
    items: [T],
}

#[test]
fn new_allocates_a_layout_identical_buffer() {
    let shadow = DeviceDetailsShadow {
        cbSize: 4,
        DevicePath: [0; 1],
    };

    let custom = DeviceDetails::new(size_of_val(&shadow)).unwrap();

    assert_eq!(Layout::for_value(&shadow), Layout::for_value(&*custom));
    assert_eq!(
        offset_of!(DeviceDetailsShadow, cbSize),
        offset_of!(DeviceDetails, cbSize)
    );
    assert_eq!(offset_of!(DeviceDetailsShadow, DevicePath), 4);
    // the tail of a DST cannot be named in offset_of! without `offset_of_slice`;
    // layout_for reports its offset instead (checked in layout_for_reports_layout_offset_and_count)
    assert_eq!(custom.cbSize, 4);
    assert_eq!(custom.DevicePath.len(), 2);
}

#[test]
fn layout_for_reports_layout_offset_and_count() {
    let (layout, offset, count) = DeviceDetails::layout_for(8).unwrap();
    assert_eq!(layout, Layout::new::<DeviceDetailsShadow>());
    assert_eq!(offset, offset_of!(DeviceDetailsShadow, DevicePath));
    assert_eq!(count, 2);
}

#[test]
fn new_allows_an_empty_tail() {
    let details = DeviceDetails::new(4).unwrap();
    assert_eq!(details.cbSize, 4);
    assert!(details.DevicePath.is_empty());
}

#[test]
fn new_rejects_a_misaligned_size() {
    let err = DeviceDetails::new(6).unwrap_err();
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
fn new_rejects_a_size_smaller_than_the_head() {
    let err = DeviceDetails::new(2).unwrap_err();
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
    let err = DeviceDetails::new(usize::MAX).unwrap_err();
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
    let (layout, offset, count) = MultiHead::layout_for(12).unwrap();
    assert_eq!(layout.size(), 12);
    assert_eq!(layout.align(), 4);
    assert_eq!(offset, 8);
    assert_eq!(count, 4);
    assert_eq!(offset_of!(MultiHead, b), 4);
    assert_eq!(offset, 8); // tail offset, reported by layout_for

    let multi = MultiHead::new(12).unwrap();
    assert_eq!((multi.a, multi.b), (1, 2));
    assert_eq!(multi.tail.len(), 4);
}

#[test]
fn generic_tail() {
    let (layout, offset, count) = Generic::<u16>::layout_for(12).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 2, 2, 5)
    );

    let generic = Generic::<u16>::new(12).unwrap();
    assert_eq!(generic.tag, 7);
    assert_eq!(generic.items.len(), 5);

    let err = Generic::<u16>::new(11).unwrap_err();
    assert_eq!(
        err,
        InvalidGeneric::InvalidSize {
            requested: 11,
            valid: 12
        }
    );
}
