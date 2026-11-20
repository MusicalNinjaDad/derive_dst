#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[derive(DST)]
struct NoReprC {
    cbSize: u32 = 4,
    DevicePath: [u16],
}

fn main() {}
