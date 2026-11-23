#![allow(stable_features)]
#![feature(default_field_values)]

use derive_dst::DST;

#[derive(DST)]
enum NotAStruct {
    A,
    B,
}

fn main() {}
