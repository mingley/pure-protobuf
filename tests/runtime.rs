//! MessageSet, generated WKT accessors, and view reads.

use protobuf::gencode::TestAllTypesProto2;
use protobuf::gencode::{
    Any, BoolValue, Duration, Empty, FieldMask, ListValue, PbValue, Struct, Timestamp,
};
use protobuf::prelude::*;
use protobuf::{Parse, Serialize};

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
    m.paths_mut().push("foo".into());
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
fn view_reads_string_and_nested_without_owned_child() {
    let mut nested = protobuf::gencode::TestAllTypesProto3::new();
    // NestedMessage lives in the proto3 module; use optional_string + recursive via generated TAT.
    nested.set_optional_string("ada");
    nested.set_optional_int32(7);
    let bytes = Serialize::serialize(&nested).unwrap();
    let parsed = protobuf::gencode::TestAllTypesProto3::parse(&bytes).unwrap();
    let s: &protobuf::ProtoStr = parsed.optional_string();
    assert_eq!(s, "ada");
    let view = parsed.as_view();
    assert_eq!(view.0.optional_int32(), 7);
}

#[test]
fn message_set_item_roundtrip() {
    // type_id 1547769, message bytes for MessageSetCorrectExtension1 { str = "hi" }
    // Extension1 field 25 is string. Encode "hi" as field 25: tag 0xca 0x01, len 2, hi
    let mut inner = Vec::new();
    protobuf::rt::encode_len_field(&mut inner, 25, b"hi");
    let mut item = Vec::new();
    protobuf::rt::encode_tag(&mut item, 1, protobuf::rt::WIRE_SGROUP);
    protobuf::rt::encode_tag(&mut item, 2, protobuf::rt::WIRE_VARINT);
    protobuf::rt::encode_varint(&mut item, 1_547_769);
    protobuf::rt::encode_len_field(&mut item, 3, &inner);
    protobuf::rt::encode_tag(&mut item, 1, protobuf::rt::WIRE_EGROUP);

    let parsed = TestAllTypesProto2::parse(&{
        let mut wrap = Vec::new();
        protobuf::rt::encode_len_field(&mut wrap, 500, &item);
        wrap
    })
    .unwrap();
    assert!(parsed.has_message_set_correct());
    let ms = parsed.message_set_correct().unwrap();
    let out = Serialize::serialize(&parsed).unwrap();
    let again = TestAllTypesProto2::parse(&out).unwrap();
    assert_eq!(
        Serialize::serialize(again.message_set_correct().unwrap()).unwrap(),
        Serialize::serialize(ms).unwrap()
    );
}
