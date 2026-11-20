#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
struct UnsizedNotLast {
    DevicePath: [u16],
    cbSize: u32 = 4,
}

fn main() {}
