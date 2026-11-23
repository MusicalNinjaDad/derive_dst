#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

// rustc resolves `cfg_attr` *before* the derive runs, so the gate must be active (`all()`)
// for the derive to see the expanded `repr` — and it must be rejected: a cfg-gated `packed`
// changes the layout whenever the gate is enabled, so the generated allocation code would
// be unsound.
#[repr(C)]
#[cfg_attr(all(), repr(packed))]
#[derive(DST)]
struct CfgAttrPacked {
    cbSize: u32,
    DevicePath: [u16],
}

fn main() {}
