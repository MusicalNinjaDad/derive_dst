#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
struct DefaultFieldValue {
    cbSize: u32 = 4,
    DevicePath: [u16],
}

fn main() {}
