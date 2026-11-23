//! Tests that `#[derive(DST)]` correctly handles attributes on the struct and its fields.
//!
//! # What is verified
//!
//! - **Struct-level attributes** (except `derive`/`doc`) are mirrored onto the generated
//!   `Uninit` mirror struct. This file uses `#![deny(non_snake_case)]`, so if an `#[allow]`
//!   on the original struct were *not* copied to the mirror, the mirror's non-snake-case
//!   fields would trip the `deny` and this file would not compile. The file compiling is
//!   itself the first test; behavioural assertions follow.
//! - **Field-level attributes** (except `derive`/`doc`) are mirrored onto the mirror's fields,
//!   verified the same way with a field-level `#[allow]`.
//! - **`cfg` gates on fields** are consistent across the original struct, the mirror, and the
//!   generated code: `#[cfg(any())]` (always false) is stripped by the compiler *before* the
//!   derive runs, so the field is absent from the struct, the mirror, and `layout_header` alike
//!   — the layout tests prove the generated code does not reference the cfg'd-out field.
//! - **`cfg_attr`** is resolved by the compiler *before* the derive runs, so the derive sees
//!   only the resolved attributes — e.g. `#[cfg_attr(all(), allow(...))]` becomes
//!   `#[allow(...)]`, which is then mirrored verbatim onto the mirror struct / mirror field.
//!
//! All structs use snake_case fields unless they carry an explicit `#[allow(non_snake_case)]`
//! (struct- or field-level), which is what makes the lint-based tests meaningful.

#![feature(ptr_metadata)]
// Deny the lint that the `#[allow(non_snake_case)]` attributes below are meant to suppress:
// if an allow is NOT mirrored onto the `Uninit` mirror, compilation of this file fails.
#![deny(non_snake_case)]

use std::alloc::Layout;
use std::mem::offset_of;

use derive_dst::DST;

// ===========================================================================
// 1. Struct-level `#[allow]` is mirrored onto the `Uninit` mirror struct
// ===========================================================================

/// The struct-level `#[allow(non_snake_case)]` must be copied to `UninitCamelStruct`;
/// otherwise the mirror's `HeadSize` / `TailWords` fields would trip `#![deny(non_snake_case)]`.
#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug, PartialEq, Eq, DST)]
struct CamelStruct {
    HeadSize: u32,
    TailWords: [u16],
}

#[test]
fn struct_level_allow_is_mirrored_onto_mirror() {
    // Compilation of this file under `#![deny(non_snake_case)]` already proves the allow
    // was mirrored. Behavioural checks:

    // repr(C): HeadSize at 0, TailWords at 4; size 8 → 2 × u16 tail
    let (layout, offset, count) = CamelStruct::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 2));

    // field offsets match between the original and the mirror
    assert_eq!(
        offset_of!(CamelStruct, HeadSize),
        offset_of!(UninitCamelStruct, HeadSize)
    );
    assert_eq!(offset_of!(CamelStruct, HeadSize), 0);

    // round-trip: allocate, write every field, assume_init
    let (mut uninit, (layout, _offset, _count)) = CamelStruct::new_uninit_for_size(8).unwrap();
    assert_eq!(Layout::for_value(&*uninit), layout);
    uninit.HeadSize.write(42);
    for (i, w) in uninit.TailWords.iter_mut().enumerate() {
        w.write(0xC000 + i as u16);
    }
    // SAFETY: HeadSize and both TailWords are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.HeadSize, 42);
    assert_eq!(&value.TailWords[..], &[0xC000, 0xC001]);
}

// ===========================================================================
// 2. Field-level `#[allow]` is mirrored onto the mirror's field
// ===========================================================================

/// Only `WeirdName` carries the allow; `tail` is snake_case. If the field-level allow were
/// not mirrored, `UninitFieldAllow::WeirdName` would trip `#![deny(non_snake_case)]`.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct FieldAllow {
    #[allow(non_snake_case)]
    WeirdName: u32,
    tail: [u8],
}

#[test]
fn field_level_allow_is_mirrored_onto_mirror_field() {
    // repr(C): WeirdName at 0, tail at 4; size 8 → 4 × u8 tail
    let (layout, offset, count) = FieldAllow::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    assert_eq!(
        offset_of!(FieldAllow, WeirdName),
        offset_of!(UninitFieldAllow, WeirdName)
    );

    let (mut uninit, _) = FieldAllow::new_uninit_for_size(8).unwrap();
    uninit.WeirdName.write(7);
    for b in uninit.tail.iter_mut() {
        b.write(0xFF);
    }
    // SAFETY: WeirdName and all tail bytes are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.WeirdName, 7);
    assert_eq!(&value.tail[..], &[0xFF; 4]);
}

// ===========================================================================
// 3. `#[cfg(any())]` on a head field — field absent everywhere
// ===========================================================================

/// The `#[cfg(any())]` field is stripped by the compiler before the derive runs, so it is
/// absent from `CfgAbsent`, `UninitCfgAbsent`, and the generated `layout_header` alike.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct CfgAbsent {
    a: u32,
    #[cfg(any())] // never enabled: stripped before the derive runs
    ghost: u64,
    tail: [u8],
}

#[test]
fn cfg_any_field_is_absent_from_layout_and_mirror() {
    // repr(C) without `ghost`: a at 0, tail at 4; size 8 → 4 × u8 tail
    let (layout, offset, count) = CfgAbsent::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    // the mirror has the same field offsets as the original (minus the cfg'd-out field)
    assert_eq!(offset_of!(CfgAbsent, a), offset_of!(UninitCfgAbsent, a));
    assert_eq!(offset_of!(CfgAbsent, a), 0);

    // layout_for_count agrees: count 3 → align_up(4 + 3, 4) = 8, offset 4
    let (layout, offset, count) = CfgAbsent::layout_for_count(3).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 3));

    // round-trip
    let (mut uninit, (_layout, _offset, _count)) = CfgAbsent::new_uninit_for_size(8).unwrap();
    uninit.a.write(1);
    for b in uninit.tail.iter_mut() {
        b.write(b'x');
    }
    // SAFETY: `a` and all tail bytes are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.a, 1);
    assert_eq!(value.tail.len(), 4);
}

// ===========================================================================
// 4. Multiple `#[cfg(any())]` fields at various positions
// ===========================================================================

/// Two `#[cfg(any())]` fields — one at the start of the head, one in the middle — are both
/// stripped before the derive runs. Only `a`, `b`, and `tail` contribute to the layout.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct MultiCfg {
    #[cfg(any())]
    x: u32,
    a: u8,
    #[cfg(any())]
    y: u64,
    b: u16,
    tail: [u8],
}

#[test]
fn multiple_cfg_any_fields_are_all_absent() {
    // repr(C) without x and y: a at 0, b at 2 (u16 align 2), tail at 4; header size 4 align 2.
    // size 8 → tail_bytes 4 → count 4; extend → offset 4, size 8, align 2 → padded 8
    let (layout, offset, count) = MultiCfg::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 2, 4, 4));

    assert_eq!(offset_of!(MultiCfg, a), 0);
    assert_eq!(offset_of!(MultiCfg, b), 2);
    assert_eq!(offset_of!(UninitMultiCfg, a), offset_of!(MultiCfg, a));
    assert_eq!(offset_of!(UninitMultiCfg, b), offset_of!(MultiCfg, b));

    // layout_for_count agrees
    let (layout2, offset2, count2) = MultiCfg::layout_for_count(4).unwrap();
    assert_eq!((layout2.size(), offset2, count2), (8, 4, 4));

    // round-trip
    let (mut uninit, _) = MultiCfg::new_uninit_for_size(8).unwrap();
    uninit.a.write(1);
    uninit.b.write(2);
    for byte in uninit.tail.iter_mut() {
        byte.write(3);
    }
    // SAFETY: a, b and all tail bytes are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!((value.a, value.b), (1, 2));
    assert_eq!(&value.tail[..], &[3, 3, 3, 3]);
}

// ===========================================================================
// 5. cfg'd-out field with an attribute (cfg_attr) — both stripped together
// ===========================================================================

/// A field that is both cfg'd out *and* carries a `cfg_attr`: the `#[cfg(any())]` strips the
/// entire field (including its `cfg_attr`) before the derive runs.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct CfgWithAttr {
    a: u32,
    #[cfg(any())]
    #[cfg_attr(all(), allow(dead_code))]
    gone: u16,
    tail: [u8],
}

#[test]
fn cfg_any_field_with_cfg_attr_is_fully_absent() {
    // identical layout to CfgAbsent: a at 0, tail at 4
    let (layout, offset, count) = CfgWithAttr::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));
    assert_eq!(offset_of!(CfgWithAttr, a), offset_of!(UninitCfgWithAttr, a));
}

// ===========================================================================
// 6. `cfg_attr` resolved before derive — the resolved attribute is mirrored
// ===========================================================================

/// `#[cfg_attr(all(), allow(non_snake_case))]` is resolved by the compiler to
/// `#[allow(non_snake_case)]` before the derive runs. The derive sees and mirrors the
/// `#[allow]`, so `UninitCfgAttr::AttrName` does not trip `#![deny(non_snake_case)]`.
#[repr(C)]
#[derive(Debug, PartialEq, Eq, DST)]
struct CfgAttrField {
    #[cfg_attr(all(), allow(non_snake_case))]
    AttrName: u32,
    tail: [u8],
}

#[test]
fn cfg_attr_resolved_allow_is_mirrored_onto_mirror_field() {
    // Same layout as CfgAbsent: AttrName at 0, tail at 4
    let (layout, offset, count) = CfgAttrField::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    assert_eq!(
        offset_of!(CfgAttrField, AttrName),
        offset_of!(UninitCfgAttrField, AttrName)
    );

    let (mut uninit, _) = CfgAttrField::new_uninit_for_size(8).unwrap();
    uninit.AttrName.write(99);
    for b in uninit.tail.iter_mut() {
        b.write(0);
    }
    // SAFETY: AttrName and all tail bytes are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.AttrName, 99);
    assert_eq!(value.tail.len(), 4);
}

// ===========================================================================
// 7. Struct-level `cfg_attr` resolved before derive is mirrored onto the mirror
// ===========================================================================

/// `#[cfg_attr(all(), allow(non_snake_case))]` on the struct resolves to
/// `#[allow(non_snake_case)]` before the derive runs; the derive mirrors it onto
/// `UninitStructCfgAttr`.
#[repr(C)]
#[cfg_attr(all(), allow(non_snake_case))]
#[derive(Debug, PartialEq, Eq, DST)]
struct StructCfgAttr {
    SomeField: u32,
    tail: [u8],
}

#[test]
fn struct_level_cfg_attr_resolved_allow_is_mirrored() {
    let (layout, offset, count) = StructCfgAttr::layout_for_size(8).unwrap();
    assert_eq!((layout.size(), layout.align(), offset, count), (8, 4, 4, 4));

    assert_eq!(
        offset_of!(StructCfgAttr, SomeField),
        offset_of!(UninitStructCfgAttr, SomeField)
    );

    let (mut uninit, _) = StructCfgAttr::new_uninit_for_size(8).unwrap();
    uninit.SomeField.write(5);
    for b in uninit.tail.iter_mut() {
        b.write(0);
    }
    // SAFETY: SomeField and all tail bytes are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!(value.SomeField, 5);
}

// ===========================================================================
// 8. Mirror layout is identical to the original for an attributed struct
// ===========================================================================

#[test]
fn mirror_layout_matches_original_for_attributed_struct() {
    let (uninit, (layout, _offset, _count)) = CamelStruct::new_uninit_for_size(8).unwrap();
    // the mirror's layout is identical to the layout computed for the original
    assert_eq!(Layout::for_value(&*uninit), layout);

    // and after assume_init, the initialised value has the same layout
    let mut uninit2 = uninit;
    uninit2.HeadSize.write(1);
    for w in uninit2.TailWords.iter_mut() {
        w.write(0);
    }
    // SAFETY: all fields initialised
    let value = unsafe { uninit2.assume_init() };
    assert_eq!(Layout::for_value(&*value), layout);
}

// ===========================================================================
// 9. Combined: struct-level allow + field-level allow + cfg'd-out field
// ===========================================================================

/// A struct with a struct-level `#[allow]`, a field-level `#[allow]` on one field, and a
/// `#[cfg(any())]` field. All three mechanisms must work together: the allows are mirrored
/// (file compiles under `deny(non_snake_case)`), and the cfg'd-out field is absent from the
/// layout.
#[repr(C)]
#[allow(non_snake_case)]
#[derive(Debug, PartialEq, Eq, DST)]
struct Combined {
    FirstField: u32,
    #[allow(non_snake_case)]
    SecondField: u16,
    #[cfg(any())]
    GhostField: u64,
    tail: [u8],
}

#[test]
fn combined_attributes_and_cfg_are_handled_together() {
    // repr(C): FirstField at 0, SecondField at 4, tail at 6; header size 6 align 4.
    // size 10 → tail_bytes 4 → count 4; extend → offset 6, size 10, align 4 → padded 12
    let (layout, offset, count) = Combined::layout_for_size(10).unwrap();
    assert_eq!(
        (layout.size(), layout.align(), offset, count),
        (12, 4, 6, 4)
    );

    assert_eq!(offset_of!(Combined, FirstField), 0);
    assert_eq!(offset_of!(Combined, SecondField), 4);
    assert_eq!(
        offset_of!(UninitCombined, FirstField),
        offset_of!(Combined, FirstField)
    );
    assert_eq!(
        offset_of!(UninitCombined, SecondField),
        offset_of!(Combined, SecondField)
    );

    // round-trip
    let (mut uninit, _) = Combined::new_uninit_for_size(10).unwrap();
    uninit.FirstField.write(1);
    uninit.SecondField.write(2);
    for b in uninit.tail.iter_mut() {
        b.write(0);
    }
    // SAFETY: all (remaining) fields are initialised
    let value = unsafe { uninit.assume_init() };
    assert_eq!((value.FirstField, value.SecondField), (1, 2));
    assert_eq!(value.tail.len(), 4);
}
