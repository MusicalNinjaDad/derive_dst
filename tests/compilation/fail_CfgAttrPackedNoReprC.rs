#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

// Without a plain `#[repr(C)]`, an active `cfg_attr` gate expanding to `repr(packed)` must
// still be rejected (the "change this to `#[repr(C)]`" error), not silently accepted.
#[cfg_attr(all(), repr(packed))]
#[derive(DST)]
struct CfgAttrPackedNoReprC {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
