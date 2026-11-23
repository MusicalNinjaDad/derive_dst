#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[repr(packed)]
#[derive(DST)]
struct MultipleReprs {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
