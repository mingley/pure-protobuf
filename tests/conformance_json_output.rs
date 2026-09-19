//! Locks the empty-FDS failure class: without TestAllTypes in
//! `conformance_pool()`, every official JsonOutput case returns
//! `serialize_error: "missing desc"`.

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
use pbrs::gencode::{
    conformance_pool, TestAllTypesEdition2023, TestAllTypesProto2, TestAllTypesProto3,
};

#[test]
fn build_rs_does_not_write_empty_conformance_fds() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"));
    assert!(
        !src.contains("write(&fds, [])"),
        "build.rs must fail if vendor/google/conformance_fds.bin is missing; \
         never write an empty descriptor set"
    );
    assert!(
        src.contains("vendor/google/conformance_fds.bin"),
        "build.rs must name vendor/google/conformance_fds.bin on the hard error path"
    );
}

#[test]
fn conformance_pool_has_test_all_types() {
    let pool = conformance_pool();
    for name in [
        "protobuf_test_messages.proto3.TestAllTypesProto3",
        "protobuf_test_messages.proto2.TestAllTypesProto2",
        "protobuf_test_messages.editions.TestAllTypesEdition2023",
    ] {
        assert!(
            pool.get_message(name).is_some(),
            "missing descriptor {name} (empty conformance FDS)"
        );
    }
}

#[test]
fn proto3_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesProto3::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}

#[test]
fn proto2_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesProto2::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}

#[test]
fn edition2023_valid_data_scalar_int32_json_output() {
    let mut msg = TestAllTypesEdition2023::new();
    msg.set_optional_int32(1);
    let json = msg
        .to_json()
        .expect("ProtobufInput.ValidDataScalar JsonOutput");
    assert!(
        json.contains("\"optionalInt32\":1"),
        "unexpected JsonOutput: {json}"
    );
}

#[test]
fn proto3_float_double_json_signed_zero() {
    // Both -0.0 and 0.0 must be parsed properly preserving sign bit.
    let neg_zero_float = TestAllTypesProto3::from_json(r#"{"optionalFloat": -0.0}"#).unwrap();
    assert!(neg_zero_float.optional_float().is_sign_negative());
    assert_eq!(
        neg_zero_float.optional_float().to_bits(),
        (-0.0f32).to_bits()
    );

    let neg_zero_float_str = TestAllTypesProto3::from_json(r#"{"optionalFloat": "-0.0"}"#).unwrap();
    assert!(neg_zero_float_str.optional_float().is_sign_negative());
    assert_eq!(
        neg_zero_float_str.optional_float().to_bits(),
        (-0.0f32).to_bits()
    );

    let pos_zero_float = TestAllTypesProto3::from_json(r#"{"optionalFloat": 0.0}"#).unwrap();
    assert!(pos_zero_float.optional_float().is_sign_positive());
    assert_eq!(pos_zero_float.optional_float().to_bits(), 0.0f32.to_bits());

    let neg_zero_double = TestAllTypesProto3::from_json(r#"{"optionalDouble": -0.0}"#).unwrap();
    assert!(neg_zero_double.optional_double().is_sign_negative());
    assert_eq!(
        neg_zero_double.optional_double().to_bits(),
        (-0.0f64).to_bits()
    );

    let neg_zero_double_str =
        TestAllTypesProto3::from_json(r#"{"optionalDouble": "-0.0"}"#).unwrap();
    assert!(neg_zero_double_str.optional_double().is_sign_negative());
    assert_eq!(
        neg_zero_double_str.optional_double().to_bits(),
        (-0.0f64).to_bits()
    );

    let pos_zero_double = TestAllTypesProto3::from_json(r#"{"optionalDouble": 0.0}"#).unwrap();
    assert!(pos_zero_double.optional_double().is_sign_positive());
    assert_eq!(
        pos_zero_double.optional_double().to_bits(),
        0.0f64.to_bits()
    );
}

#[test]
fn proto3_float_double_json_subnormals_and_exponents() {
    // Subnormal float32: 1.40129846e-45 (min positive subnormal)
    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": 1.40129846e-45}"#).unwrap();
    assert_eq!(msg.optional_float().to_bits(), 1); // min subnormal bit pattern

    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": "-1.40129846e-45"}"#).unwrap();
    assert_eq!(msg.optional_float().to_bits(), 0x8000_0001);

    // Subnormal float64: 4.9406564584124654e-324 (min positive subnormal)
    let msg =
        TestAllTypesProto3::from_json(r#"{"optionalDouble": 4.9406564584124654e-324}"#).unwrap();
    assert_eq!(msg.optional_double().to_bits(), 1);

    let msg =
        TestAllTypesProto3::from_json(r#"{"optionalDouble": "-4.9406564584124654e-324"}"#).unwrap();
    assert_eq!(msg.optional_double().to_bits(), 0x8000_0000_0000_0001);

    // Official conformance min positive normals
    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": 1.175494e-38}"#).unwrap();
    assert!((msg.optional_float() - 1.175494e-38_f32).abs() < 1e-44);

    let msg = TestAllTypesProto3::from_json(r#"{"optionalDouble": 2.22507e-308}"#).unwrap();
    assert!((msg.optional_double() - 2.22507e-308_f64).abs() < 1e-315);

    // Exponents with positive sign and various forms
    let msg =
        TestAllTypesProto3::from_json(r#"{"optionalFloat": "1.5e+2", "optionalDouble": "2.5E-3"}"#)
            .unwrap();
    assert_eq!(msg.optional_float(), 150.0);
    assert_eq!(msg.optional_double(), 0.0025);
}

#[test]
fn proto3_float_double_json_boundary_rounding() {
    // Official conformance float32 max: 3.402823e+38
    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": 3.402823e+38}"#).unwrap();
    assert!((msg.optional_float() - 3.402823e+38_f32).abs() < 1e32);

    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": -3.402823e+38}"#).unwrap();
    assert!((msg.optional_float() - (-3.402823e+38_f32)).abs() < 1e32);

    // Exact float32 max as f64
    let exact_f32_max_json = format!(r#"{{"optionalFloat": {}}}"#, f32::MAX as f64);
    let msg = TestAllTypesProto3::from_json(&exact_f32_max_json).unwrap();
    assert_eq!(msg.optional_float(), f32::MAX);

    // float64 max: 1.7976931348623157e+308
    let msg =
        TestAllTypesProto3::from_json(r#"{"optionalDouble": 1.7976931348623157e+308}"#).unwrap();
    assert_eq!(msg.optional_double(), f64::MAX);

    let msg =
        TestAllTypesProto3::from_json(r#"{"optionalDouble": -1.7976931348623157e+308}"#).unwrap();
    assert_eq!(msg.optional_double(), f64::MIN);
}

#[test]
fn proto3_float_range_rejection() {
    // Values exceeding f32::MAX must be rejected as float out of range
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": 3.502823e+38}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": -3.502823e+38}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": "3.502823e+38"}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": "-3.502823e+38"}"#).is_err());
}

#[test]
fn proto3_double_overflow_rejection() {
    // Values exceeding f64::MAX must be rejected as float overflow
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": 1.89769e+308}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": -1.89769e+308}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": "1.89769e+308"}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": "-1.89769e+308"}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": "1e999"}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalDouble": "-1e999"}"#).is_err());
}

#[test]
fn proto3_float_double_nan_infinity_spellings() {
    // Valid exact spellings
    let msg = TestAllTypesProto3::from_json(r#"{"optionalFloat": "NaN", "optionalDouble": "NaN"}"#)
        .unwrap();
    assert!(msg.optional_float().is_nan());
    assert!(msg.optional_double().is_nan());

    let msg = TestAllTypesProto3::from_json(
        r#"{"optionalFloat": "Infinity", "optionalDouble": "Infinity"}"#,
    )
    .unwrap();
    assert_eq!(msg.optional_float(), f32::INFINITY);
    assert_eq!(msg.optional_double(), f64::INFINITY);

    let msg = TestAllTypesProto3::from_json(
        r#"{"optionalFloat": "-Infinity", "optionalDouble": "-Infinity"}"#,
    )
    .unwrap();
    assert_eq!(msg.optional_float(), f32::NEG_INFINITY);
    assert_eq!(msg.optional_double(), f64::NEG_INFINITY);

    // Invalid spellings of NaN and Infinity must be rejected
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
        "INFINITY",
    ] {
        let json_f = format!(r#"{{"optionalFloat": "{invalid}"}}"#);
        assert!(
            TestAllTypesProto3::from_json(&json_f).is_err(),
            "should reject float {invalid}"
        );

        let json_d = format!(r#"{{"optionalDouble": "{invalid}"}}"#);
        assert!(
            TestAllTypesProto3::from_json(&json_d).is_err(),
            "should reject double {invalid}"
        );
    }

    // Unquoted NaN / Infinity is not valid JSON
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": NaN}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": Infinity}"#).is_err());
    assert!(TestAllTypesProto3::from_json(r#"{"optionalFloat": -Infinity}"#).is_err());
}

#[test]
fn proto3_float_double_reject_invalid_and_trailing() {
    for invalid in [
        "",
        "12abc",
        "1.5foo",
        "12 34",
        "12,34",
        "12谷歌34",
        "0x1.0",
        "0x10",
        " 1.5",
        "1.5 ",
        "1.2.3",
        "e10",
        "1e",
        "1e+",
    ] {
        let json_f = format!(r#"{{"optionalFloat": "{invalid}"}}"#);
        assert!(
            TestAllTypesProto3::from_json(&json_f).is_err(),
            "should reject float {invalid:?}"
        );

        let json_d = format!(r#"{{"optionalDouble": "{invalid}"}}"#);
        assert!(
            TestAllTypesProto3::from_json(&json_d).is_err(),
            "should reject double {invalid:?}"
        );
    }
}

#[test]
fn proto2_and_edition2023_float_double_json_parity() {
    let json = r#"{"optionalFloat": 1.5, "optionalDouble": -2.5}"#;
    let p2 = TestAllTypesProto2::from_json(json).unwrap();
    assert_eq!(p2.optional_float(), 1.5);
    assert_eq!(p2.optional_double(), -2.5);

    let ed = TestAllTypesEdition2023::from_json(json).unwrap();
    assert_eq!(ed.optional_float(), 1.5);
    assert_eq!(ed.optional_double(), -2.5);
}
