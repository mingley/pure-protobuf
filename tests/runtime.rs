//! MessageSet, generated WKT accessors, and view reads.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]
use pbrs::gencode::TestAllTypesProto2;
use pbrs::gencode::TestAllTypesProto3;
use pbrs::gencode::{
    Any, BoolValue, Duration, Empty, FieldMask, ListValue, PbValue, Struct, Timestamp,
};
use pbrs::prelude::*;
use pbrs::{Parse, Serialize};

#[test]
fn tat_default_is_empty_and_zeroed() {
    let a = TestAllTypesProto3::new();
    let b = TestAllTypesProto3::default();
    assert_eq!(a, b);
    assert!(Serialize::serialize(&a).unwrap().is_empty());
    assert!(std::mem::size_of::<TestAllTypesProto3>() < 4096);
}

#[test]
fn generated_wkt_timestamp_roundtrip() {
    let mut t = Timestamp::new();
    t.set_seconds(1);
    t.set_nanos(2);
    let b = Serialize::serialize(&t).unwrap();
    let q = Timestamp::parse(&b).unwrap();
    assert_eq!(q.seconds(), 1);
    assert_eq!(q.nanos(), 2);
}

#[test]
fn generated_wkt_duration_any_empty_mask() {
    let mut d = Duration::new();
    d.set_seconds(3);
    d.set_nanos(4);
    assert_eq!(
        Duration::parse(&Serialize::serialize(&d).unwrap())
            .unwrap()
            .seconds(),
        3
    );

    let mut a = Any::new();
    a.set_type_url("type.googleapis.com/google.protobuf.Empty");
    assert_eq!(
        Any::parse(&Serialize::serialize(&a).unwrap())
            .unwrap()
            .type_url(),
        "type.googleapis.com/google.protobuf.Empty"
    );

    let e = Empty::new();
    assert!(Serialize::serialize(&e).unwrap().is_empty());

    let mut m = FieldMask::new();
    m.paths_mut().push("foo");
    assert_eq!(
        FieldMask::parse(&Serialize::serialize(&m).unwrap())
            .unwrap()
            .paths()
            .len(),
        1
    );

    let mut w = BoolValue::new();
    w.set_value(true);
    assert!(BoolValue::parse(&Serialize::serialize(&w).unwrap())
        .unwrap()
        .value());

    let s = Struct::new();
    let _ = ListValue::new();
    let _ = PbValue::new();
    assert_eq!(s.fields().len(), 0);
}

#[test]
fn packed_truncated_is_err() {
    // packed int32 field 31: tag 0xFA 0x01, length 2, one-byte overlong/truncated varint 0x80
    let buf = vec![0xFA, 0x01, 0x02, 0x80, 0x80];
    assert!(pbrs::gencode::TestAllTypesProto3::parse(&buf).is_err());
}

#[test]
fn packed_parse_roundtrip_without_touching_getters() {
    let mut m = pbrs::gencode::TestAllTypesProto3::new();
    for i in 0..8 {
        m.repeated_int32_mut().push(i);
        m.packed_int32_mut().push(i * 3);
    }
    let bytes = Serialize::serialize(&m).unwrap();
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    let again = Serialize::serialize(&parsed).unwrap();
    assert_eq!(bytes, again);
    assert_eq!(
        parsed.repeated_int32().iter().collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
}

#[test]
fn nested_merge_second_empty_does_not_wipe() {
    let mut nested = pbrs::gencode::NestedMessage::new();
    nested.set_a(9);
    let mut m = pbrs::gencode::TestAllTypesProto3::new();
    m.set_optional_nested_message(nested);
    let mut bytes = Serialize::serialize(&m).unwrap();
    // field 18 empty LEN
    bytes.extend_from_slice(&[0x92, 0x01, 0x00]);
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    assert_eq!(parsed.optional_nested_message().a(), 9);
}

#[test]
fn view_reads_string_and_nested_without_owned_child() {
    let mut nested = pbrs::gencode::TestAllTypesProto3::new();
    // NestedMessage lives in the proto3 module; use optional_string + recursive via generated TAT.
    nested.set_optional_string("ada");
    nested.set_optional_int32(7);
    let bytes = Serialize::serialize(&nested).unwrap();
    let parsed = pbrs::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    let s: &pbrs::ProtoStr = parsed.optional_string();
    assert_eq!(s, "ada");
    let view = parsed.as_view();
    assert_eq!(view.0.optional_int32(), 7);
}

#[test]
fn message_set_item_roundtrip() {
    // type_id 1547769, message bytes for MessageSetCorrectExtension1 { str = "hi" }
    // Extension1 field 25 is string. Encode "hi" as field 25: tag 0xca 0x01, len 2, hi
    let mut inner = Vec::new();
    pbrs::rt::encode_len_field(&mut inner, 25, b"hi");
    let mut item = Vec::new();
    pbrs::rt::encode_tag(&mut item, 1, pbrs::rt::WIRE_SGROUP);
    pbrs::rt::encode_tag(&mut item, 2, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut item, 1_547_769);
    pbrs::rt::encode_len_field(&mut item, 3, &inner);
    pbrs::rt::encode_tag(&mut item, 1, pbrs::rt::WIRE_EGROUP);

    let parsed = TestAllTypesProto2::parse(&{
        let mut wrap = Vec::new();
        pbrs::rt::encode_len_field(&mut wrap, 500, &item);
        wrap
    })
    .unwrap();
    assert!(parsed.has_message_set_correct());
    let ms = parsed.message_set_correct();
    let out = Serialize::serialize(&parsed).unwrap();
    let again = TestAllTypesProto2::parse(&out).unwrap();
    assert_eq!(
        Serialize::serialize(again.message_set_correct()).unwrap(),
        Serialize::serialize(ms).unwrap()
    );
}
