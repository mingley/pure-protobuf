//! Generated MessageSet items follow the protobuf C++ item-selection rules.
#![cfg(feature = "codegen")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "isolated generated-code consumer and captured compiler output"
)]

use std::path::PathBuf;
use std::process::Command;

fn field(name: &str, number: u64, kind: u64, required: bool) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 1, name.as_bytes());
    for (tag, value) in [(3, number), (4, if required { 2 } else { 1 }), (5, kind)] {
        pbrs::rt::encode_tag(&mut out, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut out, value);
    }
    out
}

fn request() -> Vec<u8> {
    let mut child = Vec::new();
    pbrs::rt::encode_len_field(&mut child, 1, b"Child");
    for (name, number) in [("a", 1), ("b", 2)] {
        pbrs::rt::encode_len_field(&mut child, 2, &field(name, number, 5, true));
    }
    let mut host = Vec::new();
    pbrs::rt::encode_len_field(&mut host, 1, b"Host");
    pbrs::rt::encode_len_field(&mut host, 7, &[8, 1]);
    let mut range = vec![8, 4, 16];
    pbrs::rt::encode_varint(&mut range, 536_870_912);
    pbrs::rt::encode_len_field(&mut host, 5, &range);
    let mut extension = field("extension", 100, 11, false);
    pbrs::rt::encode_len_field(&mut extension, 2, b".Host");
    pbrs::rt::encode_len_field(&mut extension, 6, b".Child");
    let mut file = Vec::new();
    pbrs::rt::encode_len_field(&mut file, 1, b"messageset_payload.proto");
    pbrs::rt::encode_len_field(&mut file, 4, &child);
    pbrs::rt::encode_len_field(&mut file, 4, &host);
    pbrs::rt::encode_len_field(&mut file, 7, &extension);
    pbrs::rt::encode_len_field(&mut file, 12, b"proto2");
    let mut request = Vec::new();
    pbrs::rt::encode_len_field(&mut request, 1, b"messageset_payload.proto");
    pbrs::rt::encode_len_field(
        &mut request,
        2,
        b"stubs=none,emit_json=false,emit_text=false",
    );
    pbrs::rt::encode_len_field(&mut request, 15, &file);
    request
}

#[test]
fn generated_messageset_item_selection_matches_cpp_oracle() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let consumer = std::env::var_os("PBRS_MESSAGESET_PAYLOAD_CONSUMER_DIR").map_or_else(
        || {
            repo.join("target")
                .join(format!("codegen-messageset-payload-{}", std::process::id()))
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let request = request();
    std::fs::write(consumer.join("request.bin"), &request).unwrap();
    for (name, source) in pbrs::codegen::generate_from_code_generator_request(&request).unwrap() {
        std::fs::write(consumer.join(name), source).unwrap();
    }
    std::fs::write(consumer.join("src/lib.rs"), CONSUMER).unwrap();
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname=\"codegen-messageset-payload-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\npbrs={{path={repo:?},default-features=false,features=[\"reflect\"]}}\n"
    )).unwrap();
    std::fs::copy(repo.join("Cargo.lock"), consumer.join("Cargo.lock")).unwrap();
    let prepared = Command::new("cargo")
        .args(["metadata", "--offline", "--format-version=1"])
        .current_dir(&consumer)
        .output()
        .unwrap();
    std::fs::write(consumer.join("prepare.stderr"), &prepared.stderr).unwrap();
    assert!(
        prepared.status.success(),
        "offline consumer lock preparation"
    );
    let lock = std::fs::read(consumer.join("Cargo.lock")).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--offline", "--locked", "--lib"])
        .env("CARGO_BUILD_JOBS", "1")
        .current_dir(&consumer)
        .output()
        .unwrap();
    std::fs::write(consumer.join("test.stdout"), &output.stdout).unwrap();
    std::fs::write(consumer.join("test.stderr"), &output.stderr).unwrap();
    assert_eq!(std::fs::read(consumer.join("Cargo.lock")).unwrap(), lock);
    assert!(
        output.status.success(),
        "generated MessageSet controls failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(feature = "conformance")]
#[test]
fn bundled_messageset_handlers_keep_the_first_payload() {
    use pbrs::{ClearAndParse, Serialize};

    let mut first = Vec::new();
    pbrs::rt::encode_len_field(&mut first, 25, b"first");
    let mut second = Vec::new();
    pbrs::rt::encode_len_field(&mut second, 25, b"second");
    let mut prefix = vec![11, 16];
    pbrs::rt::encode_varint(&mut prefix, 1_547_769);
    let mut expected = prefix.clone();
    pbrs::rt::encode_len_field(&mut expected, 3, &first);
    expected.push(12);
    let mut duplicated = prefix.clone();
    pbrs::rt::encode_len_field(&mut duplicated, 3, &first);
    pbrs::rt::encode_len_field(&mut duplicated, 3, &second);
    duplicated.push(12);
    let mut malformed = prefix.clone();
    pbrs::rt::encode_len_field(&mut malformed, 3, &[8, 128]);
    pbrs::rt::encode_len_field(&mut malformed, 3, &first);
    malformed.push(12);
    prefix.push(12);

    macro_rules! check {
        ($root:ty) => {{
            let root = <$root>::new();
            let mut parsed = root.message_set_correct().clone();
            parsed.clear_and_parse(&duplicated).unwrap();
            assert_eq!(parsed.serialize().unwrap(), expected);
            assert!(parsed.clear_and_parse(&malformed).is_err());
            parsed.clear_and_parse(&prefix).unwrap();
            assert!(parsed.serialize().unwrap().is_empty());
        }};
    }
    check!(pbrs::gencode::TestAllTypesProto2);
    check!(pbrs::gencode::EditionsTestAllTypesProto2);
}

const CONSUMER: &str = r#"
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/messageset_payload.rs"));

#[cfg(test)]
mod controls {
    use super::Host;
    use pbrs::{ClearAndParse, Parse, Serialize};

    fn item(ids: &[u64], payloads: &[&[u8]], payload_first: bool, delimited: bool) -> Vec<u8> {
        let mut body = Vec::new();
        let ids = |out: &mut Vec<u8>| for id in ids {
            pbrs::rt::encode_tag(out, 2, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(out, *id);
        };
        if !payload_first { ids(&mut body); }
        for payload in payloads { pbrs::rt::encode_len_field(&mut body, 3, payload); }
        if payload_first { ids(&mut body); }
        let mut out = Vec::new();
        if delimited { pbrs::rt::encode_len_field(&mut out, 1, &body); }
        else { out.push(11); out.extend(body); out.push(12); }
        out
    }

    #[test]
    fn first_payload_wins_for_both_item_forms_and_orders() {
        for delimited in [false, true] {
            for payload_first in [false, true] {
                let wire = item(&[100], &[&[8,1,16,2], &[8,3,16,4]], payload_first, delimited);
                let parsed = Host::parse(&wire).unwrap();
                assert_eq!(parsed.serialize().unwrap(), item(&[100], &[&[8,1,16,2]], false, false));
                assert!(Host::parse(&item(&[100], &[&[], &[8,1,16,2]], payload_first, delimited)).is_err());
                assert!(Host::parse(&item(&[100], &[&[8,128], &[8,1,16,2]], payload_first, delimited)).is_err());
                assert!(Host::parse(&item(&[100], &[&[8,1,16,2], &[8,128]], payload_first, delimited)).is_ok());
            }
        }
    }

    #[test]
    fn unknown_items_keep_the_first_opaque_payload() {
        for delimited in [false, true] {
            let wire = item(&[102], &[&[8,128], &[8,1,16,2]], false, delimited);
            let parsed = Host::parse(&wire).unwrap();
            assert_eq!(parsed.serialize().unwrap(), item(&[102], &[&[8,128]], false, false));
        }
    }

    #[test]
    fn separate_items_merge_and_missing_payload_does_not_create_an_extension() {
        for delimited in [false, true] {
            let mut wire = item(&[100], &[&[8,1]], false, delimited);
            wire.extend(item(&[100], &[&[16,2]], false, delimited));
            assert!(Host::parse(&wire).is_ok());
            assert_eq!(Host::parse(&item(&[100], &[], false, delimited)).unwrap().serialize().unwrap(), []);
            assert_eq!(Host::parse(&item(&[], &[&[8,1,16,2]], false, delimited)).unwrap().serialize().unwrap(), []);
        }
    }

    #[test]
    fn first_type_id_wins_and_explicit_zero_is_always_invalid() {
        for delimited in [false, true] {
            for payload_first in [false, true] {
                let wire = item(&[100,102], &[&[8,1,16,2]], payload_first, delimited);
                assert_eq!(Host::parse(&wire).unwrap().serialize().unwrap(), item(&[100], &[&[8,1,16,2]], false, false));
                for ids in [&[0][..], &[0,100][..], &[100,0][..]] {
                    let wire = item(ids, &[&[8,1,16,2]], payload_first, delimited);
                    assert!(Host::parse(&wire).is_err());
                    let mut partial = Host::new();
                    assert!(partial.merge_from_bytes_dont_enforce_required(&wire).is_err());
                }
            }
        }
    }
}
"#;
