#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
struct SizedTail {
    cbSize: u32 = 4,
    DevicePath: [u16; 4],
}

fn main() {}
