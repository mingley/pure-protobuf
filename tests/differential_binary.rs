//! Binary differential schema fixtures verification suite.
//!
//! Validates deterministic pinned-reference fixtures for:
//! - Presence vs absent scalar fields (proto2 vs proto3)
//! - Oneof field setting and overwriting
//! - Unknown fields (varint, fixed32, fixed64, length-delimited) preserved through round-trip
//! - Open vs closed enums with known and unknown enum values
//! - Map entries (last-wins key semantics)
//! - Extensions (proto2 dynamic extensions)
//! - Packed vs unpacked repeated fields (and unpacked wire data parsed into packed fields)
//! - Truncated varint / truncated length-delimited payload
//! - Recursion depth limit (100 levels)
//! - Wire merge semantics (submessage merge, repeated append, scalar overwrite)

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
    clippy::disallowed_types,
    unreachable_pub,
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]

use pbrs::rt::UnknownField;
use pbrs::{
    DescriptorPool, DynamicMessage, MapKeyValue, MergeFrom, MessageDescriptor, Serialize, Value,
};
use std::sync::Arc;

const DIFFERENTIAL_FDS: &[u8] = include_bytes!("fixtures/differential/differential.fds");

fn test_pool() -> Arc<DescriptorPool> {
    let pool = DescriptorPool::from_file_descriptor_set(DIFFERENTIAL_FDS)
        .expect("must load differential.fds");
    Arc::new(pool)
}

fn message_desc(pool: &Arc<DescriptorPool>, name: &str) -> Arc<MessageDescriptor> {
    pool.get_message(name)
        .unwrap_or_else(|| panic!("message descriptor not found: {name}"))
}

#[test]
fn test_presence_proto3_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.Proto3Presence");

    // 1. Absent fixture: 0 bytes
    let absent_bytes = include_bytes!("fixtures/differential/presence_proto3_absent.bin");
    assert_eq!(absent_bytes.len(), 0);
    let msg_absent =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), absent_bytes)
            .expect("parse absent proto3");
    assert!(!msg_absent.has(1), "implicit int32 not present on wire");
    assert!(!msg_absent.has(2), "implicit string not present on wire");
    assert!(
        !msg_absent.has(3),
        "explicit optional int32 not present on wire"
    );
    let serialized_absent = msg_absent.serialize().expect("serialize absent");
    assert_eq!(serialized_absent, absent_bytes);

    // 2. Default fixture: in proto3, setting default values (0, "") omits them on wire
    let default_bytes = include_bytes!("fixtures/differential/presence_proto3_default.bin");
    assert_eq!(default_bytes.len(), 0);
    let mut msg_default = DynamicMessage::new(desc.clone());
    msg_default.set_pool(pool.clone());
    msg_default.set(1, Value::Int32(0));
    msg_default.set(2, Value::String("".into()));
    let serialized_default = msg_default.serialize().expect("serialize default");
    assert_eq!(
        serialized_default, default_bytes,
        "proto3 default values must be omitted on wire"
    );

    // 3. Set fixture: non-default values must be present on wire
    let set_bytes = include_bytes!("fixtures/differential/presence_proto3_set.bin");
    let msg_set =
        DynamicMessage::parse_with_pool(desc, Some(pool), set_bytes).expect("parse set proto3");
    assert_eq!(msg_set.get_singular(1), Some(&Value::Int32(42)));
    match msg_set.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "hello"),
        other => panic!("expected string, got {other:?}"),
    }
    assert_eq!(msg_set.get_singular(3), Some(&Value::Int32(100)));
    let serialized_set = msg_set.serialize().expect("serialize set");
    assert_eq!(serialized_set, set_bytes);
}

#[test]
fn test_presence_proto2_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.Proto2Presence");

    // 1. Absent fixture: 0 bytes
    let absent_bytes = include_bytes!("fixtures/differential/presence_proto2_absent.bin");
    let msg_absent =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), absent_bytes)
            .expect("parse absent proto2");
    assert!(!msg_absent.has(1), "optional int32 not present on wire");
    assert!(!msg_absent.has(2), "optional string not present on wire");
    assert_eq!(
        msg_absent.serialize().expect("serialize absent"),
        absent_bytes
    );

    // 2. Default set fixture: in proto2, explicitly setting 0 DOES emit tag + 0 on wire!
    let default_set_bytes = include_bytes!("fixtures/differential/presence_proto2_default_set.bin");
    assert_eq!(default_set_bytes, &[0x08, 0x00]);
    let msg_default_set =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), default_set_bytes)
            .expect("parse proto2 default set");
    assert!(msg_default_set.has(1), "proto2 explicit presence tracks 0");
    assert_eq!(msg_default_set.get_singular(1), Some(&Value::Int32(0)));
    assert!(!msg_default_set.has(2));
    assert_eq!(
        msg_default_set
            .serialize()
            .expect("serialize proto2 default set"),
        default_set_bytes,
        "proto2 must serialize explicit default 0"
    );

    // 3. Set fixture: non-default values
    let set_bytes = include_bytes!("fixtures/differential/presence_proto2_set.bin");
    let msg_set =
        DynamicMessage::parse_with_pool(desc, Some(pool), set_bytes).expect("parse proto2 set");
    assert!(msg_set.has(1));
    assert_eq!(msg_set.get_singular(1), Some(&Value::Int32(42)));
    assert!(msg_set.has(2));
    match msg_set.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "world"),
        other => panic!("expected string, got {other:?}"),
    }
    assert_eq!(
        msg_set.serialize().expect("serialize proto2 set"),
        set_bytes
    );
}

#[test]
fn test_oneof_field_setting_and_overwriting() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.OneofMessage");

    // 1. Oneof first field set
    let first_bytes = include_bytes!("fixtures/differential/oneof_first.bin");
    let msg_first = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), first_bytes)
        .expect("parse oneof first");
    assert!(msg_first.has(1));
    assert!(!msg_first.has(2));
    match msg_first.get_singular(1) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "alpha"),
        other => panic!("expected string, got {other:?}"),
    }
    assert_eq!(
        msg_first.serialize().expect("serialize oneof first"),
        first_bytes
    );

    // 2. Oneof second field set
    let second_bytes = include_bytes!("fixtures/differential/oneof_second.bin");
    let msg_second =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), second_bytes)
            .expect("parse oneof second");
    assert!(!msg_second.has(1));
    assert!(msg_second.has(2));
    assert_eq!(msg_second.get_singular(2), Some(&Value::Int32(456)));
    assert_eq!(
        msg_second.serialize().expect("serialize oneof second"),
        second_bytes
    );

    // 3. Wire overwriting: wire contains field 1 followed by field 2
    let overwritten_bytes = include_bytes!("fixtures/differential/oneof_overwritten.bin");
    let msg_overwritten =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), overwritten_bytes)
            .expect("parse oneof overwritten");
    assert!(
        !msg_overwritten.has(1),
        "field 1 must be cleared when field 2 overwrites in oneof"
    );
    assert!(
        msg_overwritten.has(2),
        "field 2 must be set as the last seen oneof field"
    );
    assert_eq!(msg_overwritten.get_singular(2), Some(&Value::Int32(456)));
    // When serialized back out, ONLY field 2 is written
    let serialized_overwritten = msg_overwritten.serialize().expect("serialize overwritten");
    assert_eq!(serialized_overwritten, second_bytes);

    // 4. Programmatic overwriting
    let mut msg_prog = DynamicMessage::new(desc);
    msg_prog.set_pool(pool);
    msg_prog.set(1, Value::String("alpha".into()));
    assert!(msg_prog.has(1));
    assert!(!msg_prog.has(2));
    msg_prog.set(2, Value::Int32(456));
    assert!(!msg_prog.has(1));
    assert!(msg_prog.has(2));
    assert_eq!(
        msg_prog.serialize().expect("serialize prog oneof"),
        second_bytes
    );
}

#[test]
fn test_unknown_fields_roundtrip() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.UnknownFieldsHost");

    let unknown_bytes = include_bytes!("fixtures/differential/unknown_fields_all_types.bin");
    let msg = DynamicMessage::parse_with_pool(desc, Some(pool), unknown_bytes)
        .expect("parse unknown fields");

    // Known field is parsed
    assert_eq!(msg.get_singular(1), Some(&Value::Int32(42)));

    // Unknown fields contains all 4 wire types
    let unknowns = msg.unknown_fields();
    assert_eq!(
        unknowns.fields.as_slice().len(),
        4,
        "must preserve all 4 unknown fields"
    );

    // Field 10: Varint
    assert!(unknowns.fields.iter().any(|f| matches!(
        f,
        UnknownField::Varint {
            number: 10,
            value: 99999
        }
    )));

    // Field 11: Fixed64
    assert!(unknowns.fields.iter().any(|f| matches!(
        f,
        UnknownField::Fixed64 {
            number: 11,
            value: 0x0102030405060708
        }
    )));

    // Field 12: Length-delimited
    assert!(unknowns.fields.iter().any(|f| matches!(f, UnknownField::LengthDelimited { number: 12, value } if value == b"unknown payload")));

    // Field 13: Fixed32
    assert!(unknowns.fields.iter().any(|f| matches!(
        f,
        UnknownField::Fixed32 {
            number: 13,
            value: 0xdeadbeef
        }
    )));

    // Exact byte-for-byte round-trip preservation
    let serialized = msg.serialize().expect("serialize unknown fields");
    assert_eq!(
        serialized, unknown_bytes,
        "unknown fields must round-trip exactly"
    );
}

#[test]
fn test_open_vs_closed_enums() {
    let pool = test_pool();

    // 1. Open enum: known value
    let open_desc = message_desc(&pool, "differential.OpenEnumMessage");
    let open_known_bytes = include_bytes!("fixtures/differential/enum_open_known.bin");
    let msg_open_known =
        DynamicMessage::parse_with_pool(open_desc.clone(), Some(pool.clone()), open_known_bytes)
            .expect("parse open enum known");
    assert_eq!(msg_open_known.get_singular(1), Some(&Value::Enum(2)));
    assert!(msg_open_known.unknown_fields().fields.is_empty());
    assert_eq!(
        msg_open_known.serialize().expect("serialize"),
        open_known_bytes
    );

    // 2. Open enum: unknown value (999) is kept in field
    let open_unknown_bytes = include_bytes!("fixtures/differential/enum_open_unknown.bin");
    let msg_open_unknown =
        DynamicMessage::parse_with_pool(open_desc, Some(pool.clone()), open_unknown_bytes)
            .expect("parse open enum unknown");
    assert_eq!(
        msg_open_unknown.get_singular(1),
        Some(&Value::Enum(999)),
        "open enum keeps unknown numeric value in field"
    );
    assert!(msg_open_unknown.unknown_fields().fields.is_empty());
    assert_eq!(
        msg_open_unknown.serialize().expect("serialize"),
        open_unknown_bytes
    );

    // 3. Closed enum: known value
    let closed_desc = message_desc(&pool, "differential.ClosedEnumMessage");
    let closed_known_bytes = include_bytes!("fixtures/differential/enum_closed_known.bin");
    let msg_closed_known = DynamicMessage::parse_with_pool(
        closed_desc.clone(),
        Some(pool.clone()),
        closed_known_bytes,
    )
    .expect("parse closed enum known");
    assert_eq!(msg_closed_known.get_singular(1), Some(&Value::Enum(2)));
    assert!(msg_closed_known.unknown_fields().fields.is_empty());
    assert_eq!(
        msg_closed_known.serialize().expect("serialize"),
        closed_known_bytes
    );

    // 4. Closed enum: unknown value (999) must be placed in unknown_fields
    let closed_unknown_bytes = include_bytes!("fixtures/differential/enum_closed_unknown.bin");
    let msg_closed_unknown =
        DynamicMessage::parse_with_pool(closed_desc, Some(pool), closed_unknown_bytes)
            .expect("parse closed enum unknown");
    assert!(
        !msg_closed_unknown.has(1),
        "closed enum must NOT keep unknown enum value in field"
    );
    assert_eq!(
        msg_closed_unknown.unknown_fields().fields.as_slice().len(),
        1,
        "closed enum must move unknown value to unknown_fields"
    );
    assert!(matches!(
        msg_closed_unknown
            .unknown_fields()
            .fields
            .as_slice()
            .first(),
        Some(UnknownField::Varint {
            number: 1,
            value: 999
        })
    ));
    assert_eq!(
        msg_closed_unknown.serialize().expect("serialize"),
        closed_unknown_bytes,
        "closed enum must preserve wire roundtrip through unknown_fields"
    );
}

#[test]
fn test_map_entries_last_wins() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.MapMessage");

    // 1. Single entry
    let single_bytes = include_bytes!("fixtures/differential/map_single_entry.bin");
    let msg_single =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), single_bytes)
            .expect("parse single map entry");
    let map = msg_single.get_map(1).expect("map exists");
    assert_eq!(map.len(), 1);
    assert_eq!(
        map.get(&MapKeyValue::String("key1".into())),
        Some(&Value::Int32(10))
    );

    // 2. Duplicate keys: last-wins semantics
    let dup_bytes = include_bytes!("fixtures/differential/map_duplicate_keys.bin");
    let msg_dup = DynamicMessage::parse_with_pool(desc, Some(pool), dup_bytes)
        .expect("parse duplicate map keys");
    let map_dup = msg_dup.get_map(1).expect("map exists");
    assert_eq!(map_dup.len(), 1, "duplicate keys must be deduplicated");
    assert_eq!(
        map_dup.get(&MapKeyValue::String("dup_key".into())),
        Some(&Value::Int32(200)),
        "last-seen entry for duplicate map key wins"
    );
}

#[test]
fn test_extensions_proto2_dynamic() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.Proto2ExtensionsHost");

    // 1. Empty extension
    let empty_bytes = include_bytes!("fixtures/differential/extension_empty.bin");
    let msg_empty = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), empty_bytes)
        .expect("parse empty extension host");
    assert_eq!(msg_empty.get_singular(1), Some(&Value::Int32(1)));
    assert!(!msg_empty.has_extension(101));
    assert!(!msg_empty.has_extension(102));
    assert_eq!(msg_empty.serialize().expect("serialize"), empty_bytes);

    // 2. Set extension with pool
    let set_bytes = include_bytes!("fixtures/differential/extension_set.bin");
    let msg_set = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), set_bytes)
        .expect("parse set extension host");
    assert_eq!(msg_set.get_singular(1), Some(&Value::Int32(1)));
    assert!(msg_set.has_extension(101));
    assert_eq!(msg_set.get_extension(101), Some(&Value::Int32(42)));
    assert!(msg_set.has_extension(102));
    match msg_set.get_extension(102) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "extended"),
        other => panic!("expected string, got {other:?}"),
    }
    assert_eq!(msg_set.serialize().expect("serialize"), set_bytes);

    // 3. Clear extension
    let mut msg_mut = msg_set;
    msg_mut.clear_extension(101);
    assert!(!msg_mut.has_extension(101));
    assert!(msg_mut.has_extension(102));
}

#[test]
fn test_packed_vs_unpacked_repeated() {
    let pool = test_pool();
    let packed_desc = message_desc(&pool, "differential.PackedRepeated");
    let unpacked_desc = message_desc(&pool, "differential.UnpackedRepeated");

    let packed_bytes = include_bytes!("fixtures/differential/packed_repeated.bin");
    let unpacked_bytes = include_bytes!("fixtures/differential/unpacked_repeated.bin");

    let expected_values = vec![
        Value::Int32(10),
        Value::Int32(20),
        Value::Int32(30),
        Value::Int32(40),
    ];

    // 1. Packed wire into packed descriptor
    let m1 = DynamicMessage::parse_with_pool(packed_desc.clone(), Some(pool.clone()), packed_bytes)
        .expect("parse packed wire into packed descriptor");
    assert_eq!(m1.get_repeated(1), Some(expected_values.as_slice()));

    // 2. Unpacked wire into packed descriptor (wire data parsed into packed field)
    let m2 = DynamicMessage::parse_with_pool(packed_desc, Some(pool.clone()), unpacked_bytes)
        .expect("parse unpacked wire into packed descriptor");
    assert_eq!(
        m2.get_repeated(1),
        Some(expected_values.as_slice()),
        "unpacked wire format must decode into packed repeated field"
    );

    // 3. Unpacked wire into unpacked descriptor
    let m3 =
        DynamicMessage::parse_with_pool(unpacked_desc.clone(), Some(pool.clone()), unpacked_bytes)
            .expect("parse unpacked wire into unpacked descriptor");
    assert_eq!(m3.get_repeated(1), Some(expected_values.as_slice()));

    // 4. Packed wire into unpacked descriptor (packed wire data parsed into unpacked field)
    let m4 = DynamicMessage::parse_with_pool(unpacked_desc, Some(pool), packed_bytes)
        .expect("parse packed wire into unpacked descriptor");
    assert_eq!(
        m4.get_repeated(1),
        Some(expected_values.as_slice()),
        "packed wire format must decode into unpacked repeated field"
    );
}

#[test]
fn test_truncated_payload_boundaries() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.UnknownFieldsHost");

    // 1. Truncated varint
    let truncated_varint = include_bytes!("fixtures/differential/truncated_varint.bin");
    let err_varint =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), truncated_varint);
    assert!(err_varint.is_err(), "truncated varint must fail parse");

    // 2. Truncated length-delimited payload
    let truncated_len = include_bytes!("fixtures/differential/truncated_length_delimited.bin");
    let err_len = DynamicMessage::parse_with_pool(desc, Some(pool), truncated_len);
    assert!(
        err_len.is_err(),
        "truncated length-delimited payload must fail parse"
    );
}

#[test]
fn test_recursion_depth_limit() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.RecursiveNode");

    // 1. Depth 100: at RECURSION_LIMIT, must parse successfully
    let depth_100 = include_bytes!("fixtures/differential/recursion_depth_100.bin");
    let ok = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), depth_100);
    assert!(ok.is_ok(), "depth == 100 must parse successfully");

    // 2. Depth 101: over RECURSION_LIMIT, must fail
    let depth_101 = include_bytes!("fixtures/differential/recursion_depth_101.bin");
    let err = DynamicMessage::parse_with_pool(desc, Some(pool), depth_101);
    assert!(
        err.is_err(),
        "depth > 100 must fail with recursion limit error"
    );
}

#[test]
fn test_merge_semantics_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.MergeTarget");

    let part1 = include_bytes!("fixtures/differential/merge_part1.bin");
    let part2 = include_bytes!("fixtures/differential/merge_part2.bin");
    let concatenated = include_bytes!("fixtures/differential/merge_concatenated.bin");

    // Parse part 1 alone
    let msg1 = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), part1)
        .expect("parse part 1");
    assert_eq!(msg1.get_singular(1), Some(&Value::Int32(1)));
    let nested1 = match msg1.get_singular(2) {
        Some(Value::Message(m)) => m,
        other => panic!("expected nested message, got {other:?}"),
    };
    assert_eq!(nested1.get_singular(1), Some(&Value::Int32(10)));
    assert!(!nested1.has(2));
    assert_eq!(msg1.get_repeated(3), Some([Value::Int32(100)].as_slice()));

    // Parse part 2 alone
    let msg2 = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), part2)
        .expect("parse part 2");
    assert_eq!(msg2.get_singular(1), Some(&Value::Int32(2)));
    let nested2 = match msg2.get_singular(2) {
        Some(Value::Message(m)) => m,
        other => panic!("expected nested message, got {other:?}"),
    };
    assert!(!nested2.has(1));
    match nested2.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "merged"),
        other => panic!("expected string, got {other:?}"),
    }
    assert_eq!(msg2.get_repeated(3), Some([Value::Int32(200)].as_slice()));

    // Parse concatenated wire bytes: wire merge semantics
    let msg_concat =
        DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), concatenated)
            .expect("parse concatenated");
    // 1. Scalar field 1 is overwritten by part 2
    assert_eq!(msg_concat.get_singular(1), Some(&Value::Int32(2)));
    // 2. Nested message 2 has fields from both part 1 and part 2 merged!
    let nested_concat = match msg_concat.get_singular(2) {
        Some(Value::Message(m)) => m,
        other => panic!("expected nested message, got {other:?}"),
    };
    assert_eq!(nested_concat.get_singular(1), Some(&Value::Int32(10)));
    match nested_concat.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "merged"),
        other => panic!("expected string, got {other:?}"),
    }
    // 3. Repeated field 3 has items from part 1 and part 2 concatenated!
    assert_eq!(
        msg_concat.get_repeated(3),
        Some([Value::Int32(100), Value::Int32(200)].as_slice())
    );

    // Also verify programmatic merge_from behaves identically to wire concatenation
    let mut msg_merged = msg1;
    msg_merged.merge_from(&msg2);
    assert_eq!(msg_merged, msg_concat);
}
