//! MiniTable schema decoding and linking (`src/runtime/mini_table.rs`).

use pbrs::runtime::{
    FieldType, MiniTablePtr, build_enum_mini_table, build_mini_table, link_mini_table,
};

#[test]
fn address_table_has_one_string_field() {
    let mt = unsafe { build_mini_table("$M1P") };
    let table = unsafe { &*mt.0 };
    assert_eq!(table.fields.len(), 1);
    assert_eq!(table.fields[0].number, 1);
    assert_eq!(table.fields[0].ty, FieldType::String);
}

#[test]
fn enum_table_build_is_a_null_pointer() {
    assert!(unsafe { build_enum_mini_table("$E0") }.is_null());
}

#[test]
fn link_tolerates_empty_subtables() {
    let mt = unsafe { build_mini_table("$M1P") };
    unsafe { link_mini_table(mt, &[], &[]) };
    assert_eq!(unsafe { &*mt.0 }.fields.len(), 1);
}
