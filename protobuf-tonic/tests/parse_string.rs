//! Short proto3 strings parse without a parent-frame Arc; long ones still work.

use pbrs::{Parse, Serialize};
use protobuf_tonic::hello::HelloRequest;

#[test]
fn hello_short_name_parses() {
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let wire = Serialize::serialize(&req).expect("serialize");
    assert_eq!(wire, [0x0a, 0x03, b'a', b'd', b'a']);
    let parsed = HelloRequest::parse(&wire).expect("parse short");
    assert_eq!(parsed.name(), "ada");
}

#[test]
fn hello_long_name_parses_and_roundtrips() {
    let name = "x".repeat(24);
    let mut req = HelloRequest::new();
    req.set_name(name.as_str());
    let wire = Serialize::serialize(&req).expect("serialize");
    let parsed = HelloRequest::parse(&wire).expect("parse long");
    assert_eq!(parsed.name(), name.as_str());
    assert_eq!(Serialize::serialize(&parsed).expect("re-encode"), wire);
}

#[test]
fn hello_proto3_rejects_invalid_utf8() {
    // field 1, len 3, invalid UTF-8
    let wire = [0x0a, 0x03, 0xff, 0xfe, 0xfd];
    assert!(HelloRequest::parse(&wire).is_err());
}
