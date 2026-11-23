#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

// A single `repr` inside an active `cfg_attr` gate carrying both `C` and `packed` is not a
// plain `repr(C)`: it must be rejected like a plain `#[repr(C, packed)]`.
#[repr(C)]
#[cfg_attr(all(), repr(C, packed))]
#[derive(DST)]
struct CfgAttrReprCAndPacked {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
