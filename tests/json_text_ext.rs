//! JSON/text round-trip on the shipped API.

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
use pbrs::{
    Cardinality, DynamicMessage, FieldDescriptor, FieldType, MessageDescriptor, Presence,
    Serialize, Value,
};
use std::sync::Arc;

fn person_desc() -> Arc<MessageDescriptor> {
    Arc::new(
        MessageDescriptor::builder("example.Person")
            .field(FieldDescriptor::new(
                "id",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .field({
                let mut f = FieldDescriptor::new(
                    "name",
                    2,
                    FieldType::String,
                    Cardinality::Optional,
                    Presence::Implicit,
                );
                f.json_name = "name".into();
                f
            })
            .field({
                let mut f = FieldDescriptor::new(
                    "email",
                    3,
                    FieldType::String,
                    Cardinality::Optional,
                    Presence::Explicit,
                );
                f.json_name = "email".into();
                f
            })
            .build(),
    )
}

fn ext_host_desc() -> Arc<MessageDescriptor> {
    let mut d = MessageDescriptor::builder("example.Host")
        .field(FieldDescriptor::new(
            "id",
            1,
            FieldType::Int32,
            Cardinality::Optional,
            Presence::Implicit,
        ))
        .build();
    d.extension_ranges.push((100, 200));
    Arc::new(d)
}

#[test]
fn json_roundtrip_shipped_api() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.set(1, Value::Int32(7));
    msg.set(2, Value::String("ada".into()));
    msg.set(3, Value::String("a@b".into()));
    let json = msg.to_json().expect("json encode");
    assert!(json.contains("\"id\":7"), "{json}");
    assert!(json.contains("\"name\":\"ada\""), "{json}");
    let parsed = DynamicMessage::from_json(person_desc(), &json).expect("json decode");
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(7)));
    match parsed.get_singular(2) {
        Some(Value::String(s)) => assert_eq!(s.as_view(), "ada"),
        other => panic!("{other:?}"),
    }
    assert_eq!(parsed.serialize().unwrap(), msg.serialize().unwrap());
}

#[test]
fn text_roundtrip_shipped_api() {
    let mut msg = DynamicMessage::new(person_desc());
    msg.set(1, Value::Int32(3));
    msg.set(2, Value::String("x".into()));
    let text = msg.to_text().expect("text encode");
    assert!(text.contains("id: 3"), "{text}");
    assert!(text.contains("name:"), "{text}");
    let parsed = DynamicMessage::from_text(person_desc(), &text).expect("text decode");
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(3)));
    assert_eq!(parsed.serialize().unwrap(), msg.serialize().unwrap());
}

#[test]
fn extension_get_set_roundtrip() {
    let mut msg = DynamicMessage::new(ext_host_desc());
    msg.set(1, Value::Int32(1));
    assert!(!msg.has_extension(101));
    msg.set_extension(101, Value::Int32(99));
    assert!(msg.has_extension(101));
    assert_eq!(msg.get_extension(101), Some(&Value::Int32(99)));
    let bytes = msg.serialize().unwrap();
    let parsed = DynamicMessage::parse_with(ext_host_desc(), &bytes).unwrap();
    // unregistered extension number 101 is preserved as unknown and re-encoded
    assert_eq!(parsed.serialize().unwrap(), bytes);
    msg.clear_extension(101);
    assert!(!msg.has_extension(101));
}

#[test]
fn editions_explicit_presence_zero() {
    // editions 2023 default: explicit presence. 0 is serialized.
    let mut f = FieldDescriptor::new(
        "count",
        1,
        FieldType::Int32,
        Cardinality::Optional,
        Presence::Explicit,
    );
    f.json_name = "count".into();
    let desc = Arc::new(MessageDescriptor::builder("ed.Msg").field(f).build());
    let mut msg = DynamicMessage::new(desc.clone());
    msg.set(1, Value::Int32(0));
    assert_eq!(msg.serialize().unwrap(), vec![0x08, 0x00]);
    let json = msg.to_json().unwrap();
    assert!(json.contains("\"count\":0"), "{json}");
}

#[test]
fn json_float_double_shipped_helpers_pure_rust() {
    // Verify float/double encoding
    assert_eq!(
        pbrs::json::float(f32::NAN),
        pbrs::json::Json::String("NaN".into())
    );
    assert_eq!(
        pbrs::json::float(f32::INFINITY),
        pbrs::json::Json::String("Infinity".into())
    );
    assert_eq!(
        pbrs::json::float(f32::NEG_INFINITY),
        pbrs::json::Json::String("-Infinity".into())
    );
    assert_eq!(
        pbrs::json::double(f64::NAN),
        pbrs::json::Json::String("NaN".into())
    );
    assert_eq!(
        pbrs::json::double(f64::INFINITY),
        pbrs::json::Json::String("Infinity".into())
    );
    assert_eq!(
        pbrs::json::double(f64::NEG_INFINITY),
        pbrs::json::Json::String("-Infinity".into())
    );

    // Verify signed zero
    let neg_zero_json = pbrs::json::parse("-0.0").unwrap();
    let val_f64 = pbrs::json::as_f64(&neg_zero_json).unwrap();
    assert!(val_f64.is_sign_negative());
    assert_eq!(val_f64.to_bits(), (-0.0f64).to_bits());

    let val_f32 = pbrs::json::as_f32(&neg_zero_json).unwrap();
    assert!(val_f32.is_sign_negative());
    assert_eq!(val_f32.to_bits(), (-0.0f32).to_bits());

    let neg_zero_str = pbrs::json::parse(r#""-0.0""#).unwrap();
    assert!(pbrs::json::as_f64(&neg_zero_str)
        .unwrap()
        .is_sign_negative());
    assert!(pbrs::json::as_f32(&neg_zero_str)
        .unwrap()
        .is_sign_negative());

    // Valid special values
    let nan_val = pbrs::json::as_f64(&pbrs::json::parse(r#""NaN""#).unwrap()).unwrap();
    assert!(nan_val.is_nan());
    let inf_val = pbrs::json::as_f64(&pbrs::json::parse(r#""Infinity""#).unwrap()).unwrap();
    assert_eq!(inf_val, f64::INFINITY);
    let neg_inf_val = pbrs::json::as_f64(&pbrs::json::parse(r#""-Infinity""#).unwrap()).unwrap();
    assert_eq!(neg_inf_val, f64::NEG_INFINITY);

    // Invalid NaN / Infinity spellings must be rejected
    for invalid in [
        "nan",
        "NAN",
        "Nan",
        "+NaN",
        "-NaN",
        "inf",
        "INF",
        "-inf",
        "+inf",
        "+Infinity",
        "infinity",
        "-infinity",
    ] {
        let v = pbrs::json::parse(&format!(r#""{invalid}""#)).unwrap();
        assert!(
            pbrs::json::as_f64(&v).is_err(),
            "should reject double {invalid}"
        );
        assert!(
            pbrs::json::as_f32(&v).is_err(),
            "should reject float {invalid}"
        );
    }

    // Trailing/invalid data rejection
    for invalid in [
        "1.5foo",
        "12abc",
        "12 34",
        "12,34",
        "12谷歌34",
        "0x1.0",
        "0x10",
        "",
    ] {
        let v = pbrs::json::parse(&format!(r#""{invalid}""#)).unwrap();
        assert!(
            pbrs::json::as_f64(&v).is_err(),
            "should reject double {invalid:?}"
        );
        assert!(
            pbrs::json::as_f32(&v).is_err(),
            "should reject float {invalid:?}"
        );
    }

    // Subnormals
    let subnormal_f64 = pbrs::json::parse("4.9406564584124654e-324").unwrap();
    assert_eq!(pbrs::json::as_f64(&subnormal_f64).unwrap().to_bits(), 1);

    let subnormal_f32 = pbrs::json::parse("1.40129846e-45").unwrap();
    assert_eq!(pbrs::json::as_f32(&subnormal_f32).unwrap().to_bits(), 1);

    // Range checks
    let float_too_large = pbrs::json::parse("3.502823e+38").unwrap();
    assert!(pbrs::json::as_f32(&float_too_large).is_err());
    let float_too_small = pbrs::json::parse("-3.502823e+38").unwrap();
    assert!(pbrs::json::as_f32(&float_too_small).is_err());

    let double_too_large = pbrs::json::parse("1.89769e+308").unwrap();
    assert!(pbrs::json::as_f64(&double_too_large).is_err());
    let double_str_overflow = pbrs::json::parse(r#""1e999""#).unwrap();
    assert!(pbrs::json::as_f64(&double_str_overflow).is_err());
}

#[test]
fn dynamic_message_float_double_json_roundtrip() {
    let desc = Arc::new(
        MessageDescriptor::builder("example.FloatDouble")
            .field(FieldDescriptor::new(
                "flt",
                1,
                FieldType::Float,
                Cardinality::Optional,
                Presence::Explicit,
            ))
            .field(FieldDescriptor::new(
                "dbl",
                2,
                FieldType::Double,
                Cardinality::Optional,
                Presence::Explicit,
            ))
            .build(),
    );

    let mut msg = DynamicMessage::new(desc.clone());
    msg.set(1, Value::Float(-0.0));
    msg.set(2, Value::Double(-0.0));
    let json = msg.to_json().unwrap();
    let parsed = DynamicMessage::from_json(desc.clone(), &json).unwrap();
    match parsed.get_singular(1) {
        Some(Value::Float(f)) => {
            assert!(f.is_sign_negative());
            assert_eq!(f.to_bits(), (-0.0f32).to_bits());
        }
        other => panic!("expected float, got {other:?}"),
    }
    match parsed.get_singular(2) {
        Some(Value::Double(d)) => {
            assert!(d.is_sign_negative());
            assert_eq!(d.to_bits(), (-0.0f64).to_bits());
        }
        other => panic!("expected double, got {other:?}"),
    }

    // Special floats roundtrip
    let mut msg_spec = DynamicMessage::new(desc.clone());
    msg_spec.set(1, Value::Float(f32::NAN));
    msg_spec.set(2, Value::Double(f64::INFINITY));
    let json_spec = msg_spec.to_json().unwrap();
    assert!(json_spec.contains(r#""flt":"NaN""#));
    assert!(json_spec.contains(r#""dbl":"Infinity""#));
    let parsed_spec = DynamicMessage::from_json(desc.clone(), &json_spec).unwrap();
    match parsed_spec.get_singular(1) {
        Some(Value::Float(f)) => assert!(f.is_nan()),
        other => panic!("expected NaN float, got {other:?}"),
    }
    match parsed_spec.get_singular(2) {
        Some(Value::Double(d)) => assert_eq!(*d, f64::INFINITY),
        other => panic!("expected Infinity double, got {other:?}"),
    }
}
