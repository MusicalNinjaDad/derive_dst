#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
struct StrNotLast {
    name: str,
    DevicePath: [u16],
}

fn main() {}
