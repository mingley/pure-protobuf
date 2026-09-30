//! Contract tests for the migration guide's Tonic behavior table.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration tests")]

use pbrs_grpc::Metadata;

#[test]
fn metadata_keys_follow_lowercase_http2_rules() {
    let mut metadata = Metadata::new();
    metadata
        .insert("x-request-id", "abc")
        .expect("lowercase metadata key");
    assert_eq!(metadata.get("x-request-id"), Some("abc"));

    metadata
        .insert("X-Request-ID", "normalized")
        .expect("valid uppercase metadata key is normalized");
    assert_eq!(
        metadata.get_all("x-request-id").collect::<Vec<_>>(),
        ["abc", "normalized"]
    );
}

#[test]
fn migration_guide_pins_every_branch_sensitive_behavior() {
    let guide = include_str!("../../docs/guides/migration.md");
    for behavior in [
        "Inbound message exceeds the decoding limit",
        "Outbound message exceeds the encoding/response decoding limit",
        "Client timeout",
        "Explicit client cancellation",
        "Empty or missing unary request message",
        "Trailers-only error",
        "Metadata key normalization",
        "`grpc-message` encoding",
        "Keepalive and GOAWAY",
    ] {
        assert!(guide.contains(behavior), "missing behavior row: {behavior}");
    }

    for evidence in [
        "server_oversize_decode_is_resource_exhausted",
        "server_oversize_encode_is_resource_exhausted",
        "channel_oversize_outbound_is_resource_exhausted",
        "request_send_window_stall_obeys_deadline_for_both_single_request_shapes",
        "request_send_window_stall_obeys_cancellation",
        "an_empty_body_decodes_to_a_default_message",
        "trailers_only_rejection_survives_early_request_body_reset",
        "metadata_keys_follow_lowercase_http2_rules",
        "h2c_keepalive_still_serves",
        "scenario_c_unary_response_headers_committed_no_retry_on_stream_error",
    ] {
        assert!(
            guide.contains(evidence),
            "missing test evidence: {evidence}"
        );
    }

    assert!(
        guide
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(5)
            .any(|w| { w == ["No", "compatibility", "switch", "is", "needed"] })
    );
}
