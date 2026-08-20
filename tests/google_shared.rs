//! Google `rust/test/shared` serialization + proto3 accessor tests, compiled
//! against this crate's plugin (not googletest / upb gencode).

#[path = "google_gen/unittest.rs"]
mod unittest;
#[path = "google_gen/unittest_proto3.rs"]
mod unittest_proto3;
#[path = "google_gen/unittest_proto3_optional.rs"]
mod unittest_proto3_optional;

use protobuf::prelude::*;
use unittest::TestRequired;
use unittest_proto3::TestAllTypes;
use unittest_proto3_optional::TestProto3Optional;

#[test]
fn serialization_zero_length_proto3() {
    let msg = TestAllTypes::new();
    assert_eq!(msg.serialize().unwrap().len(), 0);
    assert_eq!(msg.as_view().serialize().unwrap().len(), 0);
}

#[test]
fn serialize_deserialize_message_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bool(true);
    msg.set_optional_bytes(b"serialize deserialize test");
    let serialized = msg.serialize().unwrap();
    let msg2 = TestAllTypes::parse(&serialized).unwrap();
    assert_eq!(msg.optional_int64(), msg2.optional_int64());
    assert_eq!(msg.optional_bool(), msg2.optional_bool());
    assert_eq!(msg.optional_bytes(), msg2.optional_bytes());
}

#[test]
fn deserialize_empty_proto3() {
    assert!(TestAllTypes::parse(&[]).is_ok());
    assert!(TestProto3Optional::parse(&[]).is_ok());
}

#[test]
fn deserialize_error_proto3() {
    assert!(TestAllTypes::parse(b"not a serialized proto").is_err());
}

#[test]
fn set_bytes_with_serialized_data_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bool(true);
    let mut msg2 = TestAllTypes::new();
    msg2.set_optional_bytes(msg.serialize().unwrap());
    assert_eq!(msg2.optional_bytes(), msg.serialize().unwrap());
}

#[test]
fn deserialize_on_previously_allocated_message_proto3() {
    let mut msg = TestAllTypes::new();
    msg.set_optional_int64(42);
    msg.set_optional_bytes(b"serialize deserialize test");
    let serialized = msg.serialize().unwrap();
    let mut msg2 = TestAllTypes::new();
    msg2.set_optional_bool(true);
    assert!(msg2.clear_and_parse(&serialized).is_ok());
    assert_eq!(msg2.optional_int64(), msg.optional_int64());
    assert_eq!(msg2.optional_bytes(), msg.optional_bytes());
    assert!(!msg2.optional_bool());
}

#[test]
fn proto3_optional_roundtrip() {
    let mut msg = TestProto3Optional::new();
    msg.set_optional_int64(7);
    msg.set_optional_bytes(b"opt");
    let again = TestProto3Optional::parse(&msg.serialize().unwrap()).unwrap();
    assert_eq!(again.optional_int64(), 7);
    assert_eq!(again.optional_bytes(), b"opt");
}

#[test]
fn test_required_field_enforced() {
    assert!(TestRequired::parse(&[]).is_err());
    let mut msg = TestRequired::new();
    assert!(msg.clear_and_parse(&[]).is_err());
}

#[test]
fn test_required_field_not_enforced() {
    let mut msg = TestRequired::parse_dont_enforce_required(&[]).unwrap();
    assert!(!msg.has_a());
    msg.set_a(1);
    msg.clear_and_parse_dont_enforce_required(&[]).unwrap();
    assert!(!msg.has_a());
}

#[test]
fn test_int32_byte_size_proto3_optional() {
    for (value, expected_value_size) in [(0, 1), (127, 1), (128, 2), (-1, 10)] {
        let mut msg = TestProto3Optional::new();
        msg.set_optional_int32(value);
        let serialized = msg.serialize().unwrap();
        assert_eq!(serialized.len(), expected_value_size + 1, "value={value}");
    }
}

#[test]
fn test_fixed32_accessors() {
    let mut msg = TestAllTypes::new();
    assert_eq!(msg.optional_fixed32(), 0);
    msg.set_optional_fixed32(42);
    assert_eq!(msg.optional_fixed32(), 42);
    msg.set_optional_fixed32(u32::default());
    assert_eq!(msg.optional_fixed32(), 0);
    msg.set_optional_fixed32(43);
    assert_eq!(msg.optional_fixed32(), 43);
}

#[test]
fn test_bool_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(!msg.optional_bool());
    msg.set_optional_bool(true);
    assert!(msg.optional_bool());
    msg.set_optional_bool(bool::default());
    assert!(!msg.optional_bool());
}

#[test]
fn test_bytes_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(msg.optional_bytes().is_empty());
    msg.set_optional_bytes(b"accessors_test");
    assert_eq!(msg.optional_bytes(), b"accessors_test");
    {
        let s = Vec::from(&b"hello world"[..]);
        msg.set_optional_bytes(&s[..]);
    }
    assert_eq!(msg.optional_bytes(), b"hello world");
    msg.set_optional_bytes(b"");
    assert!(msg.optional_bytes().is_empty());
}

#[test]
fn test_optional_bytes_accessors() {
    let mut msg = TestProto3Optional::new();
    assert!(msg.optional_bytes().is_empty());
    {
        let s = Vec::from(&b"hello world"[..]);
        msg.set_optional_bytes(&s[..]);
    }
    assert_eq!(msg.optional_bytes(), b"hello world");
    msg.set_optional_bytes(b"");
    assert!(msg.optional_bytes().is_empty());
    msg.set_optional_bytes(b"\xffbinary\x85non-utf8");
    assert_eq!(msg.optional_bytes(), b"\xffbinary\x85non-utf8");
}

#[test]
fn test_string_accessors() {
    let mut msg = TestAllTypes::new();
    assert!(msg.optional_string().as_bytes().is_empty());
    msg.set_optional_string("accessors_test");
    assert_eq!(msg.optional_string(), "accessors_test");
}
