#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

// An active `cfg_attr` gate expanding to `repr(align(16))` must be rejected like a plain
// `#[repr(align(16))]`: the alignment changes the layout the generated code computes.
#[repr(C)]
#[cfg_attr(all(), repr(align(16)))]
#[derive(DST)]
struct CfgAttrAlign {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
