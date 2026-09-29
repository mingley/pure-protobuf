//! Regression coverage for peer-controlled timeout parsing inputs.

use pbrs_grpc::timeout::parse_timeout;

#[test]
fn grpc_timeout_rejects_non_ascii_unit_without_panicking() {
    assert_eq!(parse_timeout("<\0\u{0261}"), None);
}
