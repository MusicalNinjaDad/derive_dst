#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

// Nested `cfg_attr` gates are expanded one after another before the derive runs; with both
// gates active the derive sees the expanded `repr(packed)`, which must be rejected.
#[repr(C)]
#[cfg_attr(all(), cfg_attr(all(), repr(packed)))]
#[derive(DST)]
struct NestedCfgAttrPacked {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
