//! GN-11 imported google.protobuf ownership and external mapping contracts.
#![cfg(feature = "codegen")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "synchronous generated-consumer regression tests"
)]

use std::path::PathBuf;

#[path = "support/wkt_consumer.rs"]
mod wkt_consumer;
use wkt_consumer::{compile, generate, root};

#[test]
fn imported_descriptor_types_are_emitted_once_in_multi_file_output() {
    let output = generate(
        "descriptor",
        &["opt_a.proto", "opt_b.proto", "opt_c.proto"],
        None,
    )
    .unwrap();
    for name in ["opt_a.rs", "opt_b.rs", "opt_c.rs"] {
        let text = std::fs::read_to_string(output.join(name)).unwrap();
        assert!(
            !text.contains("pub struct DescriptorProto {"),
            "private descriptors remain in {name}"
        );
    }
    let descriptors =
        std::fs::read_to_string(output.join("google/protobuf/descriptor.rs")).unwrap();
    assert_eq!(
        descriptors.matches("pub struct DescriptorProto {").count(),
        1
    );
    assert!(
        !output.join("descriptor.rs").exists(),
        "synthetic owners must not duplicate compatibility files"
    );
    compile(
        &output,
        "descriptor",
        r#"
include!("../../mod.rs");
fn main() {
    let mut b = wktsharing::OptB::new();
    b.set_id(31);
    let wire = pbrs::Serialize::serialize(&b).unwrap();
    let decoded = <wktsharing::OptB as pbrs::Parse>::parse(&wire).unwrap();
    assert_eq!(decoded.id(), 31);
    let _: google::protobuf::DescriptorProto = Default::default();
}
"#,
    );
}

#[test]
fn multi_file_wkt_fields_share_one_type_and_single_file_stays_standalone() {
    let multiple = generate("multi-wkt", &["wkt_a.proto", "wkt_b.proto"], None).unwrap();
    assert!(
        !std::fs::read_to_string(multiple.join("wkt_a.rs"))
            .unwrap()
            .contains("pub struct Timestamp {")
    );
    compile(
        &multiple,
        "multi-wkt",
        r#"
include!("../../mod.rs");
fn main() {
    let mut a = shared::a::EventA::new();
    a.time_mut().set_seconds(23);
    let mut b = shared::b::EventB::new();
    pbrs::CopyFrom::copy_from(b.time_mut(), a.time());
    assert_eq!(b.time().seconds(), 23);
    let _: google::protobuf::Timestamp = Default::default();
}
"#,
    );
    let single = generate("single-wkt", &["wkt_a.proto"], None).unwrap();
    assert!(
        std::fs::read_to_string(single.join("wkt_a.rs"))
            .unwrap()
            .contains("pub struct Timestamp {")
    );
    compile(
        &single,
        "single-wkt",
        r#"
mod nested { include!("../../wkt_a.rs"); }
fn main() {
    let mut a = nested::EventA::new();
    a.time_mut().set_seconds(29);
    assert_eq!(a.time().seconds(), 29);
    let _: nested::Timestamp = Default::default();
}
"#,
    );
}

#[test]
fn explicitly_requested_mapped_google_file_is_rejected() {
    let output = root().join("target/gn11-tests/mapped-target");
    let mut config = pbrs::codegen::Config::new();
    config
        .out_dir(output)
        .extern_path(".google.protobuf", "::pbrs::wkt");
    let error = config
        .compile_descriptor_set(
            root().join("tests/fixtures/codegen-wkt-sharing/timestamp.fds"),
            &[PathBuf::from("google/protobuf/timestamp.proto")],
            &[root().join("tests/fixtures/codegen-wkt-sharing")],
        )
        .expect_err("mapped explicit target must not silently become empty");
    assert!(error.to_string().contains("extern_path"), "{error}");
    assert!(error.to_string().contains("timestamp.proto"), "{error}");
}

#[test]
fn bundled_wkt_mapping_rejects_unsupported_descriptor_types() {
    let error = generate("unsupported-facade", &["opt_b.proto"], Some("::pbrs::wkt"))
        .expect_err("bundled facade has no descriptor types");
    assert!(error.to_string().contains("wkt"), "{error}");
    assert!(error.to_string().contains("google.protobuf"), "{error}");
}

#[test]
fn imported_owner_does_not_remove_an_explicit_unique_stem_alias() {
    let output = generate("alias", &["local/descriptor.proto", "wkt_b.proto"], None).unwrap();
    let alias = std::fs::read_to_string(output.join("descriptor.rs")).unwrap();
    assert!(alias.contains("pub struct LocalMarker {"));
    assert!(!alias.contains("pub struct DescriptorProto {"));
    compile(
        &output,
        "alias",
        r#"
include!("../../mod.rs");
fn main() {
    let mut marker = shared::alias::LocalMarker::new();
    marker.descriptor_mut().set_name("schema");
    assert_eq!(marker.descriptor().name(), "schema");
}
"#,
    );
}
