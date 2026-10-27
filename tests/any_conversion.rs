//! Typed Any conversion and the shared well-known type metadata.

#![cfg(feature = "conformance")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "assertions report conversion failures in well-known message tests"
)]

use pbrs::{MessageName, Name, Parse, Serialize, wkt};

#[test]
fn named_any_roundtrips_known_wire_and_shared_well_known_names() {
    let mut message = wkt::Timestamp::new();
    message.set_seconds(42);
    let any = wkt::Any::from_msg(&message).unwrap();
    assert_eq!(
        any.type_url().to_str().unwrap(),
        "type.googleapis.com/google.protobuf.Timestamp"
    );
    assert_eq!(any.value(), &[8, 42]);
    let wire = any.serialize().unwrap();
    let decoded = wkt::Any::parse(&wire).unwrap();
    assert_eq!(decoded.to_msg::<wkt::Timestamp>().unwrap(), message);
    assert!(decoded.to_msg::<wkt::Duration>().is_err());
}

#[test]
fn named_any_matches_full_type_name_with_any_url_prefix() {
    let mut message = wkt::Timestamp::new();
    message.set_seconds(-1);
    let mut any = wkt::Any::from_msg(&message).unwrap();
    for prefix in ["", "type.example.test", "https://example.test/a/b"] {
        any.set_type_url(format!("{prefix}/google.protobuf.Timestamp"));
        assert_eq!(any.to_msg::<wkt::Timestamp>().unwrap(), message);
    }
    for invalid in [
        "",
        "google.protobuf.Timestamp",
        "/.google.protobuf.Timestamp",
        "/Timestamp",
        "/google.protobuf.Duration",
        "/google.protobuf.Timestamp/",
    ] {
        any.set_type_url(invalid.to_owned());
        assert!(any.to_msg::<wkt::Timestamp>().is_err(), "{invalid}");
    }
    any.set_type_url(wkt::Timestamp::type_url());
    any.set_value(vec![8]);
    assert!(any.to_msg::<wkt::Timestamp>().is_err());
}

#[test]
fn every_shared_well_known_message_has_canonical_metadata() {
    fn check<M: Name>() {
        assert_eq!(M::PACKAGE, "google.protobuf");
        assert_eq!(M::full_name(), M::FULL_NAME);
        assert_eq!(
            M::type_url(),
            format!("type.googleapis.com/{}", M::FULL_NAME)
        );
    }
    check::<wkt::Any>();
    check::<wkt::BoolValue>();
    check::<wkt::BytesValue>();
    check::<wkt::DoubleValue>();
    check::<wkt::Duration>();
    check::<wkt::Empty>();
    check::<wkt::FieldMask>();
    check::<wkt::FloatValue>();
    check::<wkt::Int32Value>();
    check::<wkt::Int64Value>();
    check::<wkt::ListValue>();
    check::<wkt::PbValue>();
    check::<wkt::StringValue>();
    check::<wkt::Struct>();
    check::<wkt::Timestamp>();
    check::<wkt::UInt32Value>();
    check::<wkt::UInt64Value>();
    assert_eq!(<wkt::PbValue as Name>::NAME, "Value");
    assert_eq!(
        <wkt::PbValue as MessageName>::FULL_NAME,
        "google.protobuf.Value"
    );
}
