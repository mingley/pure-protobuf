//! JSON and text format differential fixtures verification suite.
//!
//! Validates deterministic pinned-reference fixtures for:
//! - WKT JSON mappings: Timestamp RFC 3339, Duration `s` suffix, Empty `{}`,
//!   Wrappers (direct value), FieldMask camelCase, Value/Struct/ListValue
//! - Enums: string name vs integer number; open enum unknown integers;
//!   unknown enum string rejection or skip when ignore_unknown is enabled
//! - Numbers: signed zero (`-0.0`), subnormals, large exponents, float32 range bounds,
//!   NaN and Infinity
//! - Maps: string, integer, boolean keys; duplicate key handling in JSON and textproto
//! - DynamicMessage vs generated message format parity
//! - Round-trip: text -> parse -> print -> verify exact or canonical equality
//! - Round-trip: json -> parse -> print -> verify exact or canonical equality

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

use pbrs::gencode::{
    conformance_pool, BoolValue, BytesValue, DoubleValue, Duration, Empty, FieldMask, FloatValue,
    Int32Value, Int64Value, ListValue, PbValue, StringValue, Struct, TestAllTypesProto3, Timestamp,
    UInt32Value, UInt64Value,
};
use pbrs::{DescriptorPool, DynamicMessage, MapKeyValue, MessageDescriptor, Value};
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

// ---------------------------------------------------------------------------
// 1. WKT JSON Mappings
// ---------------------------------------------------------------------------

#[test]
fn test_wkt_timestamp_rfc3339_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("google.protobuf.Timestamp")
        .expect("timestamp desc");

    // Valid pinned timestamp fixture: RFC 3339 UTC with 9 nanosecond digits
    let json_valid = include_str!("fixtures/differential/wkt_timestamp.json").trim();
    let text_pinned = include_str!("fixtures/differential/wkt_timestamp.textproto");

    let gen_ts = Timestamp::from_json(json_valid).expect("parse gen Timestamp from json");
    assert_eq!(gen_ts.seconds(), 1789744348);
    assert_eq!(gen_ts.nanos(), 123456789);

    let dyn_ts =
        DynamicMessage::from_json(desc.clone(), json_valid).expect("parse dyn Timestamp from json");
    assert_eq!(dyn_ts.get_singular(1), Some(&Value::Int64(1789744348)));
    assert_eq!(dyn_ts.get_singular(2), Some(&Value::Int32(123456789)));

    // Parity: both generated and dynamic serialize to exact pinned JSON
    let gen_json = gen_ts.to_json().expect("gen Timestamp to_json");
    let dyn_json = dyn_ts.to_json().expect("dyn Timestamp to_json");
    assert_eq!(gen_json, json_valid);
    assert_eq!(dyn_json, json_valid);

    // Parity: text format
    let gen_text = gen_ts.to_text().expect("gen Timestamp to_text");
    let dyn_text = dyn_ts.to_text().expect("dyn Timestamp to_text");
    assert_eq!(gen_text, text_pinned);
    assert_eq!(dyn_text, text_pinned);

    // Timezone offset fixture: +05:00 parses to identical UTC seconds
    let json_offset = include_str!("fixtures/differential/wkt_timestamp_offset.json").trim();
    let gen_offset = Timestamp::from_json(json_offset).expect("parse offset gen Timestamp");
    let dyn_offset =
        DynamicMessage::from_json(desc.clone(), json_offset).expect("parse offset dyn Timestamp");
    assert_eq!(gen_offset.seconds(), 1789744348);
    assert_eq!(gen_offset.nanos(), 123456789);
    assert_eq!(dyn_offset.get_singular(1), Some(&Value::Int64(1789744348)));
    assert_eq!(dyn_offset.get_singular(2), Some(&Value::Int32(123456789)));

    // Epoch fixture: 1970-01-01T00:00:00Z -> seconds 0, nanos 0
    let json_epoch = include_str!("fixtures/differential/wkt_timestamp_epoch.json").trim();
    let gen_epoch = Timestamp::from_json(json_epoch).expect("parse epoch gen Timestamp");
    let dyn_epoch =
        DynamicMessage::from_json(desc.clone(), json_epoch).expect("parse epoch dyn Timestamp");
    assert_eq!(gen_epoch.seconds(), 0);
    assert_eq!(gen_epoch.nanos(), 0);
    assert_eq!(gen_epoch.to_json().unwrap(), json_epoch);
    assert_eq!(dyn_epoch.to_json().unwrap(), json_epoch);

    // Rejection of invalid timestamp JSON
    for invalid in ["\"not-rfc3339\"", "{}", "12345", "\"2026/09/18 15:12:28\""] {
        assert!(
            Timestamp::from_json(invalid).is_err(),
            "Timestamp::from_json should reject {invalid}"
        );
        assert!(
            DynamicMessage::from_json(desc.clone(), invalid).is_err(),
            "DynamicMessage::from_json should reject {invalid}"
        );
    }
}

#[test]
fn test_wkt_duration_s_suffix_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("google.protobuf.Duration")
        .expect("duration desc");

    // Positive duration fixture: "123.456789s"
    let json_pos = include_str!("fixtures/differential/wkt_duration.json").trim();
    let text_pinned = include_str!("fixtures/differential/wkt_duration.textproto");

    let gen_d = Duration::from_json(json_pos).expect("parse gen Duration");
    assert_eq!(gen_d.seconds(), 123);
    assert_eq!(gen_d.nanos(), 456789000);

    let dyn_d = DynamicMessage::from_json(desc.clone(), json_pos).expect("parse dyn Duration");
    assert_eq!(dyn_d.get_singular(1), Some(&Value::Int64(123)));
    assert_eq!(dyn_d.get_singular(2), Some(&Value::Int32(456789000)));

    // Parity: to_json and to_text
    assert_eq!(gen_d.to_json().unwrap(), json_pos);
    assert_eq!(dyn_d.to_json().unwrap(), json_pos);
    assert_eq!(gen_d.to_text().unwrap(), text_pinned);
    assert_eq!(dyn_d.to_text().unwrap(), text_pinned);

    // Negative duration fixture: "-0.500s"
    let json_neg = include_str!("fixtures/differential/wkt_duration_neg.json").trim();
    let gen_neg = Duration::from_json(json_neg).expect("parse neg Duration");
    let dyn_neg =
        DynamicMessage::from_json(desc.clone(), json_neg).expect("parse neg dyn Duration");
    assert_eq!(gen_neg.seconds(), 0);
    assert_eq!(gen_neg.nanos(), -500_000_000);
    assert_eq!(dyn_neg.get_singular(2), Some(&Value::Int32(-500_000_000)));
    assert_eq!(gen_neg.to_json().unwrap(), json_neg);
    assert_eq!(dyn_neg.to_json().unwrap(), json_neg);

    // Zero duration fixture: "0s"
    let json_zero = include_str!("fixtures/differential/wkt_duration_zero.json").trim();
    let gen_zero = Duration::from_json(json_zero).expect("parse zero Duration");
    let dyn_zero =
        DynamicMessage::from_json(desc.clone(), json_zero).expect("parse zero dyn Duration");
    assert_eq!(gen_zero.seconds(), 0);
    assert_eq!(gen_zero.nanos(), 0);
    assert_eq!(gen_zero.to_json().unwrap(), json_zero);
    assert_eq!(dyn_zero.to_json().unwrap(), json_zero);

    // Rejection of invalid duration strings (missing 's', objects, plain numbers)
    for invalid in ["\"123\"", "123", "{}", "\"123sec\"", "\"s\""] {
        assert!(
            Duration::from_json(invalid).is_err(),
            "Duration::from_json should reject {invalid}"
        );
        assert!(
            DynamicMessage::from_json(desc.clone(), invalid).is_err(),
            "DynamicMessage::from_json should reject {invalid}"
        );
    }
}

#[test]
fn test_wkt_empty_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("google.protobuf.Empty")
        .expect("empty desc");

    let json_empty = include_str!("fixtures/differential/wkt_empty.json").trim();
    let text_empty = include_str!("fixtures/differential/wkt_empty.textproto");

    let gen_e = Empty::from_json(json_empty).expect("parse gen Empty");
    let dyn_e = DynamicMessage::from_json(desc.clone(), json_empty).expect("parse dyn Empty");

    assert_eq!(gen_e.to_json().unwrap(), "{}");
    assert_eq!(dyn_e.to_json().unwrap(), "{}");
    assert_eq!(gen_e.to_text().unwrap(), text_empty);
    assert_eq!(dyn_e.to_text().unwrap(), text_empty);

    // Empty message rejecting unknown fields when ignore_unknown is false
    let unknown_json = r#"{"extra": 42}"#;
    assert!(
        Empty::from_json(unknown_json).is_err(),
        "strict Empty must reject unknown fields"
    );

    // Empty message accepting unknown fields when ignore_unknown is true
    assert!(
        Empty::from_json_ignore(unknown_json, true).is_ok(),
        "Empty with ignore_unknown must succeed"
    );
    assert!(
        DynamicMessage::from_json_ignore_unknown(desc.clone(), unknown_json).is_ok(),
        "dyn Empty with ignore_unknown must succeed"
    );

    // Reject non-object JSON for Empty
    for non_obj in ["[]", "\"\"", "null", "42"] {
        assert!(
            Empty::from_json(non_obj).is_err(),
            "Empty::from_json should reject non-object {non_obj}"
        );
    }
}

#[test]
fn test_wkt_wrappers_direct_value_differential() {
    let pool = conformance_pool();

    macro_rules! check_wrapper {
        ($gen_ty:ident, $desc_name:expr, $json_file:expr, $text_file:expr, $val_match:pat => $assert_expr:expr) => {{
            let desc = pool.get_message($desc_name).expect($desc_name);
            let json_pinned = include_str!(concat!("fixtures/differential/", $json_file)).trim();
            let text_pinned = include_str!(concat!("fixtures/differential/", $text_file));

            let gen_msg = $gen_ty::from_json(json_pinned).expect(concat!("parse gen ", $desc_name));
            let dyn_msg = DynamicMessage::from_json(desc.clone(), json_pinned)
                .expect(concat!("parse dyn ", $desc_name));

            // Verify direct value (not an object {"value": ...})
            assert_eq!(gen_msg.to_json().unwrap(), json_pinned);
            assert_eq!(dyn_msg.to_json().unwrap(), json_pinned);

            // Verify text format
            assert_eq!(gen_msg.to_text().unwrap(), text_pinned);
            assert_eq!(dyn_msg.to_text().unwrap(), text_pinned);

            // Verify value matches
            match dyn_msg.get_singular(1) {
                Some($val_match) => $assert_expr,
                other => panic!(
                    "unexpected dynamic wrapper value for {}: {:?}",
                    $desc_name, other
                ),
            }
        }};
    }

    check_wrapper!(
        BoolValue,
        "google.protobuf.BoolValue",
        "wkt_bool_wrapper.json",
        "wkt_bool_wrapper.textproto",
        Value::Bool(b) => assert_eq!(*b, true)
    );
    check_wrapper!(
        Int32Value,
        "google.protobuf.Int32Value",
        "wkt_int32_wrapper.json",
        "wkt_int32_wrapper.textproto",
        Value::Int32(n) => assert_eq!(*n, 42)
    );
    check_wrapper!(
        Int64Value,
        "google.protobuf.Int64Value",
        "wkt_int64_wrapper.json",
        "wkt_int64_wrapper.textproto",
        Value::Int64(n) => assert_eq!(*n, 1234567890123)
    );
    check_wrapper!(
        UInt32Value,
        "google.protobuf.UInt32Value",
        "wkt_uint32_wrapper.json",
        "wkt_uint32_wrapper.textproto",
        Value::Uint32(n) => assert_eq!(*n, 4294967295)
    );
    check_wrapper!(
        UInt64Value,
        "google.protobuf.UInt64Value",
        "wkt_uint64_wrapper.json",
        "wkt_uint64_wrapper.textproto",
        Value::Uint64(n) => assert_eq!(*n, 18446744073709551615)
    );
    check_wrapper!(
        FloatValue,
        "google.protobuf.FloatValue",
        "wkt_float_wrapper.json",
        "wkt_float_wrapper.textproto",
        Value::Float(f) => assert_eq!(*f, 1.5)
    );
    check_wrapper!(
        DoubleValue,
        "google.protobuf.DoubleValue",
        "wkt_double_wrapper.json",
        "wkt_double_wrapper.textproto",
        Value::Double(d) => assert_eq!(*d, 2.718281828)
    );
    check_wrapper!(
        StringValue,
        "google.protobuf.StringValue",
        "wkt_string_wrapper.json",
        "wkt_string_wrapper.textproto",
        Value::String(s) => assert_eq!(s.as_view(), "hello pure-protobuf")
    );
    check_wrapper!(
        BytesValue,
        "google.protobuf.BytesValue",
        "wkt_bytes_wrapper.json",
        "wkt_bytes_wrapper.textproto",
        Value::Bytes(b) => assert_eq!(b.as_bytes(), b"hello world")
    );
}

#[test]
fn test_wkt_fieldmask_camel_case_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("google.protobuf.FieldMask")
        .expect("field mask desc");

    let json_pinned = include_str!("fixtures/differential/wkt_fieldmask.json").trim();
    let text_pinned = include_str!("fixtures/differential/wkt_fieldmask.textproto");

    // JSON contains camelCase: "fooBar,bazQux.subField"
    let gen_mask = FieldMask::from_json(json_pinned).expect("parse gen FieldMask");
    let dyn_mask =
        DynamicMessage::from_json(desc.clone(), json_pinned).expect("parse dyn FieldMask");

    // Internal proto representation uses snake_case: ["foo_bar", "baz_qux.sub_field"]
    let gen_paths: Vec<String> = gen_mask
        .paths()
        .iter()
        .map(|s| s.to_str().unwrap().to_string())
        .collect();
    assert_eq!(gen_paths, vec!["foo_bar", "baz_qux.sub_field"]);

    let dyn_paths: Vec<String> = dyn_mask
        .get_repeated(1)
        .unwrap()
        .iter()
        .map(|v| match v {
            Value::String(s) => s.as_view().to_str().unwrap().to_string(),
            _ => panic!("expected string path"),
        })
        .collect();
    assert_eq!(dyn_paths, vec!["foo_bar", "baz_qux.sub_field"]);

    // Serialization to JSON produces comma-separated camelCase string
    assert_eq!(gen_mask.to_json().unwrap(), json_pinned);
    assert_eq!(dyn_mask.to_json().unwrap(), json_pinned);

    // Serialization to text format produces snake_case paths
    assert_eq!(gen_mask.to_text().unwrap(), text_pinned);
    assert_eq!(dyn_mask.to_text().unwrap(), text_pinned);

    // Rejection of underscore in JSON path (must be camelCase)
    for invalid in [
        "\"foo_bar\"",
        "\"fooBar,baz_qux\"",
        "\"_leading\"",
        "\"trailing_\"",
    ] {
        assert!(
            FieldMask::from_json(invalid).is_err(),
            "FieldMask should reject non-camelCase {invalid}"
        );
        assert!(
            DynamicMessage::from_json(desc.clone(), invalid).is_err(),
            "dyn FieldMask should reject non-camelCase {invalid}"
        );
    }
}

#[test]
fn test_wkt_struct_value_listvalue_differential() {
    let pool = conformance_pool();
    let struct_desc = pool
        .get_message("google.protobuf.Struct")
        .expect("struct desc");
    let list_desc = pool
        .get_message("google.protobuf.ListValue")
        .expect("listvalue desc");
    let value_desc = pool
        .get_message("google.protobuf.Value")
        .expect("value desc");

    // 1. Struct mapping
    let struct_json = include_str!("fixtures/differential/wkt_struct.json");
    let gen_st = Struct::from_json(struct_json).expect("parse gen Struct");
    let dyn_st = DynamicMessage::from_json_with_pool(
        struct_desc.clone(),
        Some(pool.clone()),
        struct_json,
        false,
    )
    .expect("parse dyn Struct");

    let gen_st_json = gen_st.to_json().expect("gen Struct to_json");
    let dyn_st_json = dyn_st.to_json().expect("dyn Struct to_json");
    assert_eq!(
        pbrs::json::parse(&gen_st_json).unwrap(),
        pbrs::json::parse(&dyn_st_json).unwrap()
    );
    assert_eq!(
        pbrs::json::parse(&gen_st_json).unwrap(),
        pbrs::json::parse(struct_json).unwrap()
    );

    // 2. ListValue mapping
    let list_json = include_str!("fixtures/differential/wkt_listvalue.json");
    let gen_lv = ListValue::from_json(list_json).expect("parse gen ListValue");
    let dyn_lv = DynamicMessage::from_json_with_pool(
        list_desc.clone(),
        Some(pool.clone()),
        list_json,
        false,
    )
    .expect("parse dyn ListValue");

    let gen_lv_json = gen_lv.to_json().expect("gen ListValue to_json");
    let dyn_lv_json = dyn_lv.to_json().expect("dyn ListValue to_json");
    assert_eq!(
        pbrs::json::parse(&gen_lv_json).unwrap(),
        pbrs::json::parse(&dyn_lv_json).unwrap()
    );
    assert_eq!(
        pbrs::json::parse(&gen_lv_json).unwrap(),
        pbrs::json::parse(list_json).unwrap()
    );

    // 3. Value individual types
    let json_str = include_str!("fixtures/differential/wkt_value_str.json").trim();
    let gen_v_str = PbValue::from_json(json_str).expect("parse gen PbValue str");
    let dyn_v_str = DynamicMessage::from_json_with_pool(
        value_desc.clone(),
        Some(pool.clone()),
        json_str,
        false,
    )
    .expect("parse dyn PbValue str");
    assert!(matches!(dyn_v_str.get_singular(3), Some(Value::String(_))));
    assert_eq!(
        pbrs::json::parse(&gen_v_str.to_json().unwrap()).unwrap(),
        pbrs::json::parse(&dyn_v_str.to_json().unwrap()).unwrap()
    );

    let json_num = include_str!("fixtures/differential/wkt_value_num.json").trim();
    let gen_v_num = PbValue::from_json(json_num).expect("parse gen PbValue num");
    let dyn_v_num = DynamicMessage::from_json_with_pool(
        value_desc.clone(),
        Some(pool.clone()),
        json_num,
        false,
    )
    .expect("parse dyn PbValue num");
    assert_eq!(dyn_v_num.get_singular(2), Some(&Value::Double(42.5)));
    assert_eq!(
        pbrs::json::parse(&gen_v_num.to_json().unwrap()).unwrap(),
        pbrs::json::parse(&dyn_v_num.to_json().unwrap()).unwrap()
    );

    let json_bool = include_str!("fixtures/differential/wkt_value_bool.json").trim();
    let gen_v_bool = PbValue::from_json(json_bool).expect("parse gen PbValue bool");
    let dyn_v_bool = DynamicMessage::from_json_with_pool(
        value_desc.clone(),
        Some(pool.clone()),
        json_bool,
        false,
    )
    .expect("parse dyn PbValue bool");
    assert_eq!(dyn_v_bool.get_singular(4), Some(&Value::Bool(true)));
    assert_eq!(
        pbrs::json::parse(&gen_v_bool.to_json().unwrap()).unwrap(),
        pbrs::json::parse(&dyn_v_bool.to_json().unwrap()).unwrap()
    );

    let json_null = include_str!("fixtures/differential/wkt_value_null.json").trim();
    let gen_v_null = PbValue::from_json(json_null).expect("parse gen PbValue null");
    let dyn_v_null = DynamicMessage::from_json_with_pool(
        value_desc.clone(),
        Some(pool.clone()),
        json_null,
        false,
    )
    .expect("parse dyn PbValue null");
    assert_eq!(dyn_v_null.get_singular(1), Some(&Value::Enum(0)));
    assert_eq!(
        pbrs::json::parse(&gen_v_null.to_json().unwrap()).unwrap(),
        pbrs::json::parse(&dyn_v_null.to_json().unwrap()).unwrap()
    );
}

// ---------------------------------------------------------------------------
// 2. Enums
// ---------------------------------------------------------------------------

#[test]
fn test_enums_string_name_vs_integer_number_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.OpenEnumMessage");

    let json_name = include_str!("fixtures/differential/enum_string_name.json").trim();
    let json_num = include_str!("fixtures/differential/enum_integer_number.json").trim();
    let text_name = include_str!("fixtures/differential/enum_string_name.textproto");
    let text_num = include_str!("fixtures/differential/enum_integer_number.textproto");

    // JSON: both string name "OPEN_TWO" and integer number 2 parse to enum value 2
    let msg_from_name = DynamicMessage::from_json(desc.clone(), json_name).unwrap();
    let msg_from_num = DynamicMessage::from_json(desc.clone(), json_num).unwrap();
    assert_eq!(msg_from_name.get_singular(1), Some(&Value::Enum(2)));
    assert_eq!(msg_from_num.get_singular(1), Some(&Value::Enum(2)));

    // Serializing to JSON emits canonical enum string name
    assert_eq!(msg_from_name.to_json().unwrap(), json_name);
    assert_eq!(msg_from_num.to_json().unwrap(), json_name);

    // Text format: both string name and integer number parse
    let msg_from_text_name = DynamicMessage::from_text(desc.clone(), text_name).unwrap();
    let msg_from_text_num = DynamicMessage::from_text(desc.clone(), text_num).unwrap();
    assert_eq!(msg_from_text_name.get_singular(1), Some(&Value::Enum(2)));
    assert_eq!(msg_from_text_num.get_singular(1), Some(&Value::Enum(2)));

    // Serializing to text emits enum name
    assert_eq!(msg_from_text_name.to_text().unwrap(), text_name);
    assert_eq!(msg_from_text_num.to_text().unwrap(), text_name);
}

#[test]
fn test_open_enum_unknown_integers_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.OpenEnumMessage");

    let json_unknown = include_str!("fixtures/differential/enum_open_unknown.json").trim();
    let text_unknown = include_str!("fixtures/differential/enum_open_unknown.textproto");

    // Open enums preserve unknown integer values
    let msg = DynamicMessage::from_json(desc.clone(), json_unknown).unwrap();
    assert_eq!(msg.get_singular(1), Some(&Value::Enum(999)));

    // In JSON, unknown enum value MUST format as a number, not string
    assert_eq!(
        msg.to_json().unwrap(),
        json_unknown,
        "unknown open enum must serialize as integer number"
    );

    // In text format, unknown enum value serializes as number
    assert_eq!(msg.to_text().unwrap(), text_unknown);

    // TestAllTypesProto3 parity for open enum unknown value
    let mut tat = TestAllTypesProto3::new();
    tat.set_optional_nested_enum(999);
    assert_eq!(i32::from(tat.optional_nested_enum()), 999);

    let tat_json = tat.to_json().unwrap();
    assert!(
        tat_json.contains("\"optionalNestedEnum\":999"),
        "TestAllTypesProto3 must serialize unknown enum as number: {tat_json}"
    );

    let reparsed = TestAllTypesProto3::from_json(&tat_json).unwrap();
    assert_eq!(i32::from(reparsed.optional_nested_enum()), 999);
}

#[test]
fn test_unknown_enum_string_rejection_and_skip_differential() {
    let pool = test_pool();
    let desc = message_desc(&pool, "differential.OpenEnumMessage");
    let json_unk_str = include_str!("fixtures/differential/enum_unknown_string.json").trim();

    // Strict mode: unknown enum string must be rejected
    assert!(
        DynamicMessage::from_json(desc.clone(), json_unk_str).is_err(),
        "strict from_json must reject unknown enum string"
    );

    // Ignore unknown mode: unknown enum string must be skipped (field remains unset)
    let ignored = DynamicMessage::from_json_ignore_unknown(desc.clone(), json_unk_str).unwrap();
    assert!(!ignored.has(1), "unknown enum string must be skipped");

    // TestAllTypesProto3 parity
    let tat_unk_json = r#"{"optionalNestedEnum": "UNKNOWN_CUSTOM_ENUM"}"#;
    assert!(
        TestAllTypesProto3::from_json(tat_unk_json).is_err(),
        "strict TestAllTypesProto3 must reject unknown enum string"
    );
    let tat_ignored = TestAllTypesProto3::from_json_ignore(tat_unk_json, true).unwrap();
    assert_eq!(
        i32::from(tat_ignored.optional_nested_enum()),
        0,
        "ignored unknown enum field stays default"
    );
}

// ---------------------------------------------------------------------------
// 3. Numbers
// ---------------------------------------------------------------------------

#[test]
fn test_numeric_signed_zero_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let json = include_str!("fixtures/differential/numeric_signed_zero.json").trim();
    let json_str = include_str!("fixtures/differential/numeric_signed_zero_str.json").trim();
    let text = include_str!("fixtures/differential/numeric_signed_zero.textproto");

    // 1. Parse from JSON number: -0.0
    let gen = TestAllTypesProto3::from_json(json).unwrap();
    assert!(gen.optional_float().is_sign_negative());
    assert!(gen.optional_double().is_sign_negative());
    assert_eq!(gen.optional_float().to_bits(), (-0.0f32).to_bits());
    assert_eq!(gen.optional_double().to_bits(), (-0.0f64).to_bits());

    let dyn_msg = DynamicMessage::from_json(desc.clone(), json).unwrap();
    match dyn_msg.get_singular(11) {
        Some(Value::Float(f)) => {
            assert!(f.is_sign_negative());
            assert_eq!(f.to_bits(), (-0.0f32).to_bits());
        }
        other => panic!("expected float, got {other:?}"),
    }
    match dyn_msg.get_singular(12) {
        Some(Value::Double(d)) => {
            assert!(d.is_sign_negative());
            assert_eq!(d.to_bits(), (-0.0f64).to_bits());
        }
        other => panic!("expected double, got {other:?}"),
    }

    // 2. Parse from JSON string: "-0.0"
    let gen_s = TestAllTypesProto3::from_json(json_str).unwrap();
    assert!(gen_s.optional_float().is_sign_negative());
    assert!(gen_s.optional_double().is_sign_negative());

    let dyn_s = DynamicMessage::from_json(desc.clone(), json_str).unwrap();
    match dyn_s.get_singular(11) {
        Some(Value::Float(f)) => assert!(f.is_sign_negative()),
        other => panic!("expected float, got {other:?}"),
    }

    // 3. Parse from textproto: optional_float: -0
    let gen_txt = TestAllTypesProto3::from_text(text).unwrap();
    assert!(gen_txt.optional_float().is_sign_negative());
    assert!(gen_txt.optional_double().is_sign_negative());

    let dyn_txt = DynamicMessage::from_text(desc.clone(), text).unwrap();
    match dyn_txt.get_singular(11) {
        Some(Value::Float(f)) => assert!(f.is_sign_negative()),
        other => panic!("expected float, got {other:?}"),
    }

    // Text serialization prints -0 for negative zero
    let text_out = dyn_txt.to_text().unwrap();
    assert!(text_out.contains("optional_float: -0"), "{text_out}");
    assert!(text_out.contains("optional_double: -0"), "{text_out}");
}

#[test]
fn test_numeric_subnormals_and_large_exponents_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    // 1. Subnormals positive
    let json_sub = include_str!("fixtures/differential/numeric_subnormals.json").trim();
    let gen = TestAllTypesProto3::from_json(json_sub).unwrap();
    assert_eq!(gen.optional_float().to_bits(), 1); // min positive subnormal float32
    assert_eq!(gen.optional_double().to_bits(), 1); // min positive subnormal float64

    let dyn_msg = DynamicMessage::from_json(desc.clone(), json_sub).unwrap();
    match dyn_msg.get_singular(11) {
        Some(Value::Float(f)) => assert_eq!(f.to_bits(), 1),
        other => panic!("expected float, got {other:?}"),
    }
    match dyn_msg.get_singular(12) {
        Some(Value::Double(d)) => assert_eq!(d.to_bits(), 1),
        other => panic!("expected double, got {other:?}"),
    }

    // 2. Subnormals negative
    let json_sub_neg = include_str!("fixtures/differential/numeric_subnormals_neg.json").trim();
    let gen_neg = TestAllTypesProto3::from_json(json_sub_neg).unwrap();
    assert_eq!(gen_neg.optional_float().to_bits(), 0x8000_0001);
    assert_eq!(gen_neg.optional_double().to_bits(), 0x8000_0000_0000_0001);

    // 3. Large exponents: "1.5e+2" -> 150.0, "2.5E-3" -> 0.0025
    let json_exp = include_str!("fixtures/differential/numeric_large_exponents.json").trim();
    let gen_exp = TestAllTypesProto3::from_json(json_exp).unwrap();
    assert_eq!(gen_exp.optional_float(), 150.0);
    assert_eq!(gen_exp.optional_double(), 0.0025);

    let dyn_exp = DynamicMessage::from_json(desc.clone(), json_exp).unwrap();
    assert_eq!(dyn_exp.get_singular(11), Some(&Value::Float(150.0)));
    assert_eq!(dyn_exp.get_singular(12), Some(&Value::Double(0.0025)));
}

#[test]
fn test_numeric_float32_bounds_and_overflow_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    // Float32 max bound: 3.402823e+38 is valid
    let json_max = include_str!("fixtures/differential/numeric_float32_max.json").trim();
    let gen = TestAllTypesProto3::from_json(json_max).unwrap();
    assert!((gen.optional_float() - 3.402823e+38_f32).abs() < 1e32);

    let dyn_msg = DynamicMessage::from_json(desc.clone(), json_max).unwrap();
    match dyn_msg.get_singular(11) {
        Some(Value::Float(f)) => assert!((f - 3.402823e+38_f32).abs() < 1e32),
        other => panic!("expected float, got {other:?}"),
    }

    // Float32 overflow: 3.502823e+38 must be rejected
    let json_f32_overflow =
        include_str!("fixtures/differential/numeric_float32_overflow.json").trim();
    assert!(
        TestAllTypesProto3::from_json(json_f32_overflow).is_err(),
        "must reject float32 overflow"
    );
    assert!(
        DynamicMessage::from_json(desc.clone(), json_f32_overflow).is_err(),
        "must reject dyn float32 overflow"
    );

    // Float64 overflow: 1.89769e+308 must be rejected
    let json_f64_overflow =
        include_str!("fixtures/differential/numeric_float64_overflow.json").trim();
    assert!(
        TestAllTypesProto3::from_json(json_f64_overflow).is_err(),
        "must reject float64 overflow"
    );
    assert!(
        DynamicMessage::from_json(desc.clone(), json_f64_overflow).is_err(),
        "must reject dyn float64 overflow"
    );
}

#[test]
fn test_numeric_nan_and_infinity_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let json_specials = include_str!("fixtures/differential/numeric_special_strings.json").trim();
    let json_neg_inf = include_str!("fixtures/differential/numeric_neg_infinity.json").trim();
    let text_specials = include_str!("fixtures/differential/numeric_specials.textproto");
    let text_neg_inf = include_str!("fixtures/differential/numeric_neg_inf.textproto");

    // JSON NaN and Infinity
    let gen = TestAllTypesProto3::from_json(json_specials).unwrap();
    assert!(gen.optional_float().is_nan());
    assert_eq!(gen.optional_double(), f64::INFINITY);

    let dyn_msg = DynamicMessage::from_json(desc.clone(), json_specials).unwrap();
    match dyn_msg.get_singular(11) {
        Some(Value::Float(f)) => assert!(f.is_nan()),
        other => panic!("expected NaN float, got {other:?}"),
    }
    match dyn_msg.get_singular(12) {
        Some(Value::Double(d)) => assert_eq!(*d, f64::INFINITY),
        other => panic!("expected Inf double, got {other:?}"),
    }

    // JSON -Infinity
    let gen_neg = TestAllTypesProto3::from_json(json_neg_inf).unwrap();
    assert_eq!(gen_neg.optional_float(), f32::NEG_INFINITY);
    assert_eq!(gen_neg.optional_double(), f64::NEG_INFINITY);

    let dyn_neg = DynamicMessage::from_json(desc.clone(), json_neg_inf).unwrap();
    assert_eq!(
        dyn_neg.get_singular(11),
        Some(&Value::Float(f32::NEG_INFINITY))
    );
    assert_eq!(
        dyn_neg.get_singular(12),
        Some(&Value::Double(f64::NEG_INFINITY))
    );

    // Serialization to JSON produces exact special strings
    let json_out = dyn_neg.to_json().unwrap();
    assert!(
        json_out.contains("\"optionalFloat\":\"-Infinity\""),
        "{json_out}"
    );
    assert!(
        json_out.contains("\"optionalDouble\":\"-Infinity\""),
        "{json_out}"
    );

    // Text format: nan, inf, -inf
    let gen_txt = TestAllTypesProto3::from_text(text_specials).unwrap();
    assert!(gen_txt.optional_float().is_nan());
    assert_eq!(gen_txt.optional_double(), f64::INFINITY);

    let dyn_txt = DynamicMessage::from_text(desc.clone(), text_specials).unwrap();
    assert_eq!(
        dyn_txt.get_singular(12),
        Some(&Value::Double(f64::INFINITY))
    );

    let gen_txt_neg = TestAllTypesProto3::from_text(text_neg_inf).unwrap();
    assert_eq!(gen_txt_neg.optional_float(), f32::NEG_INFINITY);
    assert_eq!(gen_txt_neg.optional_double(), f64::NEG_INFINITY);

    let dyn_txt_neg = DynamicMessage::from_text(desc.clone(), text_neg_inf).unwrap();
    assert_eq!(dyn_txt_neg.to_text().unwrap(), text_neg_inf);
}

// ---------------------------------------------------------------------------
// 4. Maps
// ---------------------------------------------------------------------------

#[test]
fn test_maps_key_types_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    // 1. String keys: map_string_string
    let json_str = include_str!("fixtures/differential/map_string_key.json").trim();
    let gen_str = TestAllTypesProto3::from_json(json_str).unwrap();
    assert_eq!(
        gen_str
            .map_string_string()
            .get("greeting")
            .unwrap()
            .to_str()
            .unwrap(),
        "hello"
    );
    assert_eq!(
        gen_str
            .map_string_string()
            .get("farewell")
            .unwrap()
            .to_str()
            .unwrap(),
        "world"
    );

    let dyn_str =
        DynamicMessage::from_json_with_pool(desc.clone(), Some(pool.clone()), json_str, false)
            .unwrap();
    let m = dyn_str.get_map(69).expect("map_string_string map field");
    assert_eq!(
        m.get(&MapKeyValue::String("greeting".into())),
        Some(&Value::String("hello".into()))
    );

    // 2. Integer keys: map_int32_int32 ("-1": 200, "0": 0, "42": 100)
    let json_int = include_str!("fixtures/differential/map_integer_key.json").trim();
    let gen_int = TestAllTypesProto3::from_json(json_int).unwrap();
    assert_eq!(gen_int.map_int32_int32().get(&42), Some(100));
    assert_eq!(gen_int.map_int32_int32().get(&-1), Some(200));
    assert_eq!(gen_int.map_int32_int32().get(&0), Some(0));

    let dyn_int =
        DynamicMessage::from_json_with_pool(desc.clone(), Some(pool.clone()), json_int, false)
            .unwrap();
    let m_int = dyn_int.get_map(56).expect("map_int32_int32 map field");
    assert_eq!(m_int.get(&MapKeyValue::I32(42)), Some(&Value::Int32(100)));
    assert_eq!(m_int.get(&MapKeyValue::I32(-1)), Some(&Value::Int32(200)));

    // 3. Boolean keys: map_bool_bool ("false": false, "true": true)
    let json_bool = include_str!("fixtures/differential/map_bool_key.json").trim();
    let gen_bool = TestAllTypesProto3::from_json(json_bool).unwrap();
    assert_eq!(gen_bool.map_bool_bool().get(&true), Some(true));
    assert_eq!(gen_bool.map_bool_bool().get(&false), Some(false));

    let dyn_bool =
        DynamicMessage::from_json_with_pool(desc.clone(), Some(pool.clone()), json_bool, false)
            .unwrap();
    let m_bool = dyn_bool.get_map(68).expect("map_bool_bool map field");
    assert_eq!(
        m_bool.get(&MapKeyValue::Bool(true)),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        m_bool.get(&MapKeyValue::Bool(false)),
        Some(&Value::Bool(false))
    );

    // 4. Text format parsing for maps
    let text_maps = include_str!("fixtures/differential/map_cases.textproto");
    let gen_txt = TestAllTypesProto3::from_text(text_maps).unwrap();
    assert_eq!(gen_txt.map_int32_int32().get(&42), Some(100));
    assert_eq!(gen_txt.map_bool_bool().get(&true), Some(true));
    assert_eq!(
        gen_txt
            .map_string_string()
            .get("greeting")
            .unwrap()
            .to_str()
            .unwrap(),
        "hello"
    );

    let dyn_txt =
        DynamicMessage::from_text_with_pool(desc.clone(), Some(pool.clone()), text_maps).unwrap();
    assert_eq!(
        dyn_txt.get_map(56).unwrap().get(&MapKeyValue::I32(42)),
        Some(&Value::Int32(100))
    );
}

#[test]
fn test_maps_duplicate_keys_json_rejection_vs_text_last_wins() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    // In JSON, duplicate keys in an object MUST BE REJECTED by parse_json_no_dup
    let json_dup = include_str!("fixtures/differential/map_duplicate_keys.json").trim();
    assert!(
        TestAllTypesProto3::from_json(json_dup).is_err(),
        "TestAllTypesProto3::from_json must reject duplicate map keys"
    );
    assert!(
        DynamicMessage::from_json(desc.clone(), json_dup).is_err(),
        "DynamicMessage::from_json must reject duplicate map keys"
    );

    // In text format, duplicate map entries are allowed and follow last-wins semantics
    let text_dup = include_str!("fixtures/differential/map_duplicate_keys.textproto");
    let gen = TestAllTypesProto3::from_text(text_dup).expect("parse gen text with dup keys");
    assert_eq!(
        gen.map_string_string()
            .get("dupKey")
            .unwrap()
            .to_str()
            .unwrap(),
        "second",
        "text format map entry must follow last-wins"
    );

    let dyn_msg = DynamicMessage::from_text_with_pool(desc.clone(), Some(pool), text_dup)
        .expect("parse dyn text with dup keys");
    let m = dyn_msg.get_map(69).unwrap();
    assert_eq!(
        m.get(&MapKeyValue::String("dupKey".into())),
        Some(&Value::String("second".into())),
        "dyn text format map entry must follow last-wins"
    );
}

// ---------------------------------------------------------------------------
// 5. Casing and Duplicate Fields
// ---------------------------------------------------------------------------

#[test]
fn test_casing_and_duplicate_fields_differential() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let json_camel = include_str!("fixtures/differential/casing_camel.json").trim();
    let json_snake = include_str!("fixtures/differential/casing_snake.json").trim();
    let json_dup = include_str!("fixtures/differential/casing_duplicate.json").trim();

    // Both camelCase and snake_case parse into the same field
    let gen_c = TestAllTypesProto3::from_json(json_camel).unwrap();
    let gen_s = TestAllTypesProto3::from_json(json_snake).unwrap();
    assert_eq!(gen_c.optional_int32(), 42);
    assert_eq!(gen_s.optional_int32(), 42);

    let dyn_c = DynamicMessage::from_json(desc.clone(), json_camel).unwrap();
    let dyn_s = DynamicMessage::from_json(desc.clone(), json_snake).unwrap();
    assert_eq!(dyn_c.get_singular(1), Some(&Value::Int32(42)));
    assert_eq!(dyn_s.get_singular(1), Some(&Value::Int32(42)));

    // When both camelCase and snake_case appear in the same JSON object for the same field,
    // it must be rejected as a duplicate field!
    assert!(
        TestAllTypesProto3::from_json(json_dup).is_err(),
        "TestAllTypesProto3 must reject duplicate field aliases in JSON"
    );
    assert!(
        DynamicMessage::from_json(desc.clone(), json_dup).is_err(),
        "DynamicMessage must reject duplicate field aliases in JSON"
    );
}

// ---------------------------------------------------------------------------
// 6. DynamicMessage vs Generated Message Format Parity
// ---------------------------------------------------------------------------

#[test]
fn test_dynamic_vs_generated_format_parity() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let mut tat = TestAllTypesProto3::new();
    tat.set_optional_int32(42);
    tat.set_optional_int64(9999999999);
    tat.set_optional_uint32(100);
    tat.set_optional_uint64(20000000000);
    tat.set_optional_sint32(-15);
    tat.set_optional_sint64(-300);
    tat.set_optional_fixed32(123);
    tat.set_optional_fixed64(456);
    tat.set_optional_sfixed32(-789);
    tat.set_optional_sfixed64(-1011);
    tat.set_optional_float(1.5);
    tat.set_optional_double(-2.5);
    tat.set_optional_bool(true);
    tat.set_optional_string("hello format parity");
    tat.set_optional_bytes(b"bytes parity".to_vec());
    tat.set_optional_nested_enum(1); // BAR

    // Serialize to wire then parse into DynamicMessage
    let wire = pbrs::Serialize::serialize(&tat).unwrap();
    let dyn_msg = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), &wire).unwrap();

    // JSON format parity
    let gen_json = tat.to_json().unwrap();
    let dyn_json = dyn_msg.to_json().unwrap();
    assert_eq!(
        pbrs::json::parse(&gen_json).unwrap(),
        pbrs::json::parse(&dyn_json).unwrap(),
        "DynamicMessage and generated message to_json must have canonical parity"
    );

    // Text format parity
    let gen_text = tat.to_text().unwrap();
    let dyn_text = dyn_msg.to_text().unwrap();
    assert_eq!(
        gen_text, dyn_text,
        "DynamicMessage and generated message to_text must be identical"
    );

    // Reparse from generated JSON into DynamicMessage
    let reparsed_dyn = DynamicMessage::from_json(desc.clone(), &gen_json).unwrap();
    assert_eq!(reparsed_dyn.get_singular(1), Some(&Value::Int32(42)));
    assert_eq!(
        reparsed_dyn.get_singular(2),
        Some(&Value::Int64(9999999999))
    );
    assert_eq!(reparsed_dyn.get_singular(11), Some(&Value::Float(1.5)));
    assert_eq!(reparsed_dyn.get_singular(12), Some(&Value::Double(-2.5)));
    assert_eq!(reparsed_dyn.get_singular(13), Some(&Value::Bool(true)));

    // Reparse from generated text into DynamicMessage
    let reparsed_dyn_text = DynamicMessage::from_text(desc.clone(), &gen_text).unwrap();
    assert_eq!(reparsed_dyn_text.get_singular(1), Some(&Value::Int32(42)));
}

// ---------------------------------------------------------------------------
// 7. Full Round-Trip Tests
// ---------------------------------------------------------------------------

#[test]
fn test_roundtrip_json_canonical_equality() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let json_pinned = include_str!("fixtures/differential/format_roundtrip.json");
    let pinned_tree = pbrs::json::parse(json_pinned).expect("parse pinned json tree");

    // 1. Generated message round-trip: json -> parse -> to_json -> verify canonical equality
    let gen = TestAllTypesProto3::from_json(json_pinned).expect("parse gen TestAllTypesProto3");
    let gen_json = gen.to_json().expect("gen to_json");
    let gen_tree = pbrs::json::parse(&gen_json).expect("parse gen json tree");
    assert_eq!(
        gen_tree, pinned_tree,
        "gen JSON tree must match pinned tree"
    );

    let gen_reparsed = TestAllTypesProto3::from_json(&gen_json).expect("reparse gen");
    assert_eq!(gen, gen_reparsed, "gen message must round-trip exactly");

    // 2. Dynamic message round-trip: json -> parse -> to_json -> verify canonical equality
    let dyn_msg = DynamicMessage::from_json(desc.clone(), json_pinned).expect("parse dyn message");
    let dyn_json = dyn_msg.to_json().expect("dyn to_json");
    let dyn_tree = pbrs::json::parse(&dyn_json).expect("parse dyn json tree");
    assert_eq!(
        dyn_tree, pinned_tree,
        "dyn JSON tree must match pinned tree"
    );

    let dyn_reparsed = DynamicMessage::from_json(desc.clone(), &dyn_json).expect("reparse dyn");
    assert_eq!(
        dyn_msg.to_json().unwrap(),
        dyn_reparsed.to_json().unwrap(),
        "dyn message must round-trip exactly"
    );
}

#[test]
fn test_roundtrip_text_equality() {
    let pool = conformance_pool();
    let desc = pool
        .get_message("protobuf_test_messages.proto3.TestAllTypesProto3")
        .expect("tat desc");

    let text_pinned = include_str!("fixtures/differential/format_roundtrip.textproto");

    // 1. Generated message round-trip: text -> parse -> to_text -> verify exact equality
    let gen = TestAllTypesProto3::from_text(text_pinned).expect("parse gen text");
    let gen_text = gen.to_text().expect("gen to_text");
    assert_eq!(gen_text, text_pinned, "gen text must match pinned text");

    let gen_reparsed = TestAllTypesProto3::from_text(&gen_text).expect("reparse gen text");
    assert_eq!(
        gen, gen_reparsed,
        "gen message must round-trip text exactly"
    );
    assert_eq!(gen_reparsed.to_text().unwrap(), gen_text);

    // 2. Dynamic message round-trip: text -> parse -> to_text -> verify exact equality
    let dyn_msg = DynamicMessage::from_text(desc.clone(), text_pinned).expect("parse dyn text");
    let dyn_text = dyn_msg.to_text().expect("dyn to_text");
    assert_eq!(dyn_text, text_pinned, "dyn text must match pinned text");

    let dyn_reparsed =
        DynamicMessage::from_text(desc.clone(), &dyn_text).expect("reparse dyn text");
    assert_eq!(dyn_reparsed.to_text().unwrap(), dyn_text);

    // 3. Parity between generated and dynamic text output
    assert_eq!(gen_text, dyn_text);
}
