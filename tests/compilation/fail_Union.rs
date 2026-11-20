#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[repr(C)]
#[derive(DST)]
union NotAStruct {
    a: u32,
}

fn main() {}
