#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
struct DynTraitNotLast {
    handler: dyn Fn(),
    DevicePath: [u16],
}

fn main() {}
