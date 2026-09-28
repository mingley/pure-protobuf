//! Reflective kernel access (`src/runtime/reflect.rs`).

use pbrs::runtime::InnerProtoString;

#[test]
fn inner_proto_string_round_trips_bytes() {
    let inner = InnerProtoString::from(b"beta".as_slice());
    let (view, _arena) = inner.into_raw_parts();
    assert_eq!(unsafe { view.as_ref() }, b"beta");
}
