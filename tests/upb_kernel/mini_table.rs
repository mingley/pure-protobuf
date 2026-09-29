//! MiniTable schema decoding and linking (`src/runtime/mini_table.rs`).

use pbrs::runtime::{FieldType, build_enum_mini_table, build_mini_table, link_mini_table};

#[test]
fn address_table_has_one_string_field() {
    let mt = unsafe { build_mini_table("$M1P") };
    let table = unsafe { &*mt.0 };
    assert_eq!(table.fields.len(), 1);
    assert_eq!(table.fields[0].number, 1);
    assert_eq!(table.fields[0].ty, FieldType::String);
    // SAFETY: build_mini_table returns a fresh boxed table. This test owns it,
    // has installed no global references, and has finished all borrowed reads.
    unsafe { drop(Box::from_raw(mt.0.cast_mut())) };
}

#[test]
fn enum_table_build_returns_owned_validation_metadata() {
    let table = unsafe { build_enum_mini_table("!$") };
    assert!(!table.is_null());
    // SAFETY: the table is a fresh Box owned exclusively by this test.
    unsafe { drop(Box::from_raw(table.cast_mut())) };
}

#[test]
fn link_tolerates_empty_subtables() {
    let mt = unsafe { build_mini_table("$M1P") };
    unsafe { link_mini_table(mt, &[], &[]) };
    assert_eq!(unsafe { &*mt.0 }.fields.len(), 1);
    // SAFETY: this fresh test-owned table has no linked children or retained
    // references; all reads have finished, so reclaim its original Box.
    unsafe { drop(Box::from_raw(mt.0.cast_mut())) };
}
