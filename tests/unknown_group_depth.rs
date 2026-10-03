//! QG-18 bounded regression oracles. The test-only baseline is expected to
//! reject neither 101 unknown groups nor mixed known/unknown depth above 100.
//! The over-limit rejection assertions are semantic reds until hardening is applied.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    unreachable_pub,
    reason = "bounded source-pinned integration oracles and synchronous consumer subprocess"
)]

#[path = "support/unknown_group_depth.rs"]
#[allow(dead_code, reason = "shared vectors also serve feature-gated and generated consumer oracles")]
mod vectors;
use pbrs::rt::{UnknownField, WIRE_EGROUP, WIRE_I32, WIRE_I64, WIRE_LEN, WIRE_VARINT, capture_unknown, decode_tag, skip_field};
use pbrs::{ParseError, UnknownFields};

fn skip_first(bytes: &[u8]) -> Result<usize, ParseError> {
    let mut pos = 0;
    let (_, wire) = decode_tag(bytes, &mut pos)?;
    skip_field(bytes, &mut pos, wire)?;
    Ok(pos)
}

fn capture_first(bytes: &[u8]) -> Result<(UnknownField, usize), ParseError> {
    let mut pos = 0;
    let (number, wire) = decode_tag(bytes, &mut pos)?;
    let field = capture_unknown(bytes, &mut pos, number, wire)?;
    Ok((field, pos))
}

#[test]
fn public_skip_accepts_100_unknown_groups() {
    let wire = vectors::unknown_groups(100, 99);
    assert_eq!(skip_first(&wire).unwrap(), wire.len());
}

#[test]
fn public_skip_rejects_101_unknown_groups() {
    assert!(skip_first(&vectors::unknown_groups(101, 99)).is_err());
}

#[test]
fn public_capture_accepts_100_unknown_groups_and_preserves_wire() {
    let wire = vectors::unknown_groups(100, 99);
    let (field, pos) = capture_first(&wire).unwrap();
    assert_eq!(pos, wire.len());
    let mut fields = UnknownFields::default();
    fields.fields.push(field);
    let mut encoded = Vec::new();
    fields.encode(&mut encoded);
    assert_eq!(encoded, wire);
}

#[test]
fn public_capture_rejects_101_unknown_groups() {
    assert!(capture_first(&vectors::unknown_groups(101, 99)).is_err());
}

#[test]
fn legacy_skip_end_tag_and_truncation_behavior_is_explicit() {
    // skip_field has no expected outer number: its historical behavior accepts
    // mismatched end-group numbers. QG-18 changes depth, not this public contract.
    assert_eq!(skip_first(&[0x0b, 0x14]).unwrap(), 2);
    assert_eq!(skip_first(&[0x0b, 0x13, 0x1c, 0x0c]).unwrap(), 4);
    for bytes in [&[0x0b][..], &[0x0b, 0x08, 0x80][..], &[0x0c][..], &[0x0f][..], &[0][..]] {
        assert!(skip_first(bytes).is_err());
    }
}

#[test]
fn capture_retains_matching_end_validation_and_malformed_errors() {
    for bytes in [&[0x0b][..], &[0x0b, 0x14][..], &[0x0b, 0x13, 0x1c, 0x0c][..], &[0x0b, 0x08, 0x80][..], &[0x0c][..], &[0x0f][..], &[0][..]] {
        assert!(capture_first(bytes).is_err());
    }
    assert!(capture_first(&[0x0b, 0x0c]).is_ok());
}

#[test]
fn non_group_paths_and_public_function_types_remain_compatible() {
    let _: fn(&[u8], &mut usize, u32) -> Result<(), ParseError> = skip_field;
    let _: fn(&[u8], &mut usize, u32, u32) -> Result<UnknownField, ParseError> = capture_unknown;
    for (wire, bytes) in [
        (WIRE_VARINT, vec![0xac, 2]),
        (WIRE_I32, vec![1, 2, 3, 4]),
        (WIRE_I64, vec![1, 2, 3, 4, 5, 6, 7, 8]),
        (WIRE_LEN, vec![2, 7, 8]),
    ] {
        let mut pos = 0;
        skip_field(&bytes, &mut pos, wire).unwrap();
        assert_eq!(pos, bytes.len());
        let mut captured = 0;
        capture_unknown(&bytes, &mut captured, 7, wire).unwrap();
        assert_eq!(captured, pos);
    }
    for wire in [WIRE_EGROUP, 6, 7] {
        assert!(skip_field(&[], &mut 0, wire).is_err());
        assert!(capture_unknown(&[], &mut 0, 7, wire).is_err());
    }
}

#[test]
fn group_consumption_stops_before_a_following_sibling() {
    let group = vectors::unknown_groups(2, 99);
    let mut wire = group.clone();
    wire.extend_from_slice(&[0x08, 9]);
    assert_eq!(skip_first(&wire).unwrap(), group.len());
    let (_, pos) = capture_first(&wire).unwrap();
    assert_eq!(pos, group.len());
    let mut pos = pos;
    assert_eq!(decode_tag(&wire, &mut pos).unwrap(), (1, WIRE_VARINT));
    skip_field(&wire, &mut pos, WIRE_VARINT).unwrap();
    assert_eq!(pos, wire.len());
}

#[cfg(feature = "reflect")]
fn dynamic_parse(bytes: &[u8]) -> Result<pbrs::DynamicMessage, ParseError> {
    let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(&vectors::descriptor_set()).unwrap());
    let descriptor = pool.get_message("qg18.Node").unwrap();
    pbrs::DynamicMessage::parse_with_pool(descriptor, Some(pool), bytes)
}

#[cfg(feature = "reflect")]
#[test]
fn dynamic_unknown_group_boundary_accepts_100() {
    use pbrs::Serialize;
    let wire = vectors::unknown_groups(100, 99);
    assert_eq!(dynamic_parse(&wire).unwrap().serialize().unwrap(), wire);
}

#[cfg(feature = "reflect")]
#[test]
fn dynamic_unknown_group_boundary_rejects_101() {
    assert!(dynamic_parse(&vectors::unknown_groups(101, 99)).is_err());
}

#[cfg(feature = "reflect")]
#[test]
fn dynamic_known_and_unknown_share_the_100_depth_budget() {
    for child in [1, 3] {
        for number in [99, 2] {
            assert!(dynamic_parse(&vectors::known_then_unknown(99, child, 1, number)).is_ok());
            assert!(dynamic_parse(&vectors::known_then_unknown(99, child, 2, number)).is_err());
            assert!(dynamic_parse(&vectors::known_then_unknown(100, child, 1, number)).is_err());
        }
    }
    assert!(dynamic_parse(&vectors::known_group_pairs_then_unknown(49, 2)).is_ok());
    assert!(dynamic_parse(&vectors::known_group_pairs_then_unknown(50, 1)).is_err());
}

#[cfg(feature = "conformance")]
#[test]
fn checked_bundled_merge_composes_its_supplied_depth_with_unknown_groups() {
    let mut message = pbrs::gencode::Empty::new();
    assert!(message.merge_bytes(&vectors::unknown_groups(1, 99), 99).is_ok());
    let mut message = pbrs::gencode::Empty::new();
    assert!(message.merge_bytes(&vectors::unknown_groups(2, 99), 99).is_err());
}

#[cfg(feature = "conformance")]
#[test]
fn checked_bundled_validation_composes_its_supplied_depth_with_unknown_groups() {
    for (groups, valid) in [(1, true), (2, false)] {
        let bytes = vectors::unknown_groups(groups, 99);
        let wire = pbrs::rt::Wire::from_slice(&bytes);
        assert_eq!(pbrs::gencode::Empty::validate_inner(&wire, &mut 0, 99).is_ok(), valid);
    }
}

#[cfg(feature = "codegen")]
#[test]
fn fresh_generated_unknown_group_depth_consumer() {
    use std::path::PathBuf;
    use std::process::Command;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let retained = std::env::var_os("PBRS_QG18_CONSUMER_DIR");
    let consumer = retained.as_ref().map_or_else(
        || root.join("target").join(format!("qg18-consumer-{}", std::process::id())),
        PathBuf::from,
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let files = pbrs::codegen::generate_from_file_descriptor_set(&vectors::descriptor_set(), &["qg18.proto".into()]).unwrap();
    let (_, source) = files.into_iter().find(|(name, _)| name == "qg18.rs").unwrap();
    std::fs::write(consumer.join("src/qg18.rs"), source).unwrap();
    std::fs::write(consumer.join("src/vectors.rs"), include_str!("support/unknown_group_depth.rs")).unwrap();
    std::fs::write(consumer.join("src/lib.rs"), include_str!("fixtures/unknown_group_depth_consumer.rs")).unwrap();
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname=\"qg18-unknown-group-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\npbrs={{path={:?}}}\n", root.to_str().unwrap()
    )).unwrap();
    // Seed every existing registry pin; this adds only the new local consumer
    // root. Its locked acceptance must be qualified before calling baseline
    // assertion failures semantic reds.
    let mut lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    lock.push_str("\n[[package]]\nname = \"qg18-unknown-group-consumer\"\nversion = \"0.0.0\"\ndependencies = [\n \"pbrs\",\n]\n");
    std::fs::write(consumer.join("Cargo.lock"), lock).unwrap();
    let mut command = Command::new(std::env::var_os("PBRS_QG18_CONSUMER_CARGO").unwrap_or_else(|| "cargo".into()));
    command.env("CARGO_BUILD_JOBS", "1").env("CARGO_INCREMENTAL", "0")
        .args(["test", "--offline", "--locked", "--quiet", "--lib", "--", "--test-threads=1"])
        .current_dir(&consumer);
    // Root's ordinary gate supplies the exact owned CARGO_TARGET_DIR. A normal
    // standalone test run gets its own cache, never plugin.rs's shared cache.
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        command.env("CARGO_TARGET_DIR", root.join("target/qg18-consumer-cache"));
    }
    let result = command.output().unwrap();
    std::fs::write(consumer.join("stdout"), &result.stdout).unwrap();
    std::fs::write(consumer.join("stderr"), &result.stderr).unwrap();
    assert!(result.status.success(), "fresh generated consumer failed: {}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).contains("6 passed; 0 failed"));
    if retained.is_none() {
        std::fs::remove_dir_all(consumer).unwrap();
    }
}
