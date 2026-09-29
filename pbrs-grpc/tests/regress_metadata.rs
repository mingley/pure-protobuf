//! Regression coverage for public metadata validation.

use pbrs_grpc::{Code, Metadata};

#[test]
fn ascii_metadata_rejects_non_ascii_values() {
    let mut md = Metadata::new();

    let err = md
        .insert("x-name", "\u{0572}")
        .expect_err("non-ASCII metadata value must be rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(md.get("x-name"), None);

    let err = md
        .set("x-name", "\u{0572}")
        .expect_err("non-ASCII replacement must be rejected");
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(md.get("x-name"), None);
}
