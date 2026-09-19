//! Field-wise JSON and text for a generated proto3 real oneof.
//!
//! These checks fail on current main: `OneofHole` `to_json` / `to_text`
//! still serialize then `DynamicMessage`. After the cut they must not.
//! TAT and WKT other than Timestamp / Duration / Empty / wrappers /
//! FieldMask stay on `DynamicMessage`.
//! Remaining is not closed.

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
use pbrs::{
    Cardinality, DynamicMessage, FieldDescriptor, FieldType, MessageDescriptor, Presence, Value,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn oneof_hole_desc() -> Arc<MessageDescriptor> {
    let mut a = FieldDescriptor::new(
        "a",
        1,
        FieldType::String,
        Cardinality::Optional,
        Presence::Explicit,
    );
    a.json_name = "a".into();
    a.oneof_index = Some(0);
    let mut b = FieldDescriptor::new(
        "b",
        2,
        FieldType::Int64,
        Cardinality::Optional,
        Presence::Explicit,
    );
    b.json_name = "b".into();
    b.oneof_index = Some(0);
    let mut desc = MessageDescriptor::builder("example.OneofHole")
        .field(a)
        .field(b)
        .build();
    desc.oneofs = vec![vec![1, 2]];
    Arc::new(desc)
}

fn with_a(s: &str) -> DynamicMessage {
    let mut msg = DynamicMessage::new(oneof_hole_desc());
    msg.set(1, Value::String(s.into()));
    msg
}

fn with_b(n: i64) -> DynamicMessage {
    let mut msg = DynamicMessage::new(oneof_hole_desc());
    msg.set(2, Value::Int64(n));
    msg
}

/// Official DynamicMessage JSON goldens the generated OneofHole path must match.
#[test]
fn official_dynamic_json_goldens_for_oneof_hole() {
    assert_eq!(
        DynamicMessage::new(oneof_hole_desc()).to_json().unwrap(),
        "{}"
    );
    assert_eq!(with_a("x").to_json().unwrap(), "{\"a\":\"x\"}");
    assert_eq!(with_a("").to_json().unwrap(), "{\"a\":\"\"}");
    assert_eq!(with_b(99).to_json().unwrap(), "{\"b\":\"99\"}");
    assert_eq!(with_b(0).to_json().unwrap(), "{\"b\":\"0\"}");

    let parsed = DynamicMessage::from_json(oneof_hole_desc(), "{\"a\":\"x\"}").unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::String("x".into())));
    assert!(!parsed.has(2));

    let parsed = DynamicMessage::from_json(oneof_hole_desc(), "{\"b\":\"0\"}").unwrap();
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(0)));
    assert!(!parsed.has(1));

    let parsed = DynamicMessage::from_json(oneof_hole_desc(), "{\"b\":42}").unwrap();
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(42)));

    assert!(DynamicMessage::from_json(oneof_hole_desc(), "{\"a\":\"x\",\"b\":\"1\"}").is_err());
    let empty = DynamicMessage::from_json(oneof_hole_desc(), "{\"a\":null}").unwrap();
    assert!(!empty.has(1));
    assert!(!empty.has(2));
    assert!(DynamicMessage::from_json(oneof_hole_desc(), "{\"nope\":1}").is_err());
    let ign =
        DynamicMessage::from_json_ignore_unknown(oneof_hole_desc(), "{\"nope\":1,\"a\":\"x\"}")
            .unwrap();
    assert_eq!(ign.get_singular(1), Some(&Value::String("x".into())));
}

/// Official DynamicMessage text goldens the generated OneofHole path must match.
#[test]
fn official_dynamic_text_goldens_for_oneof_hole() {
    assert_eq!(
        DynamicMessage::new(oneof_hole_desc()).to_text().unwrap(),
        ""
    );
    assert_eq!(with_a("x").to_text().unwrap(), "a: \"x\"\n");
    assert_eq!(with_a("").to_text().unwrap(), "a: \"\"\n");
    assert_eq!(with_b(99).to_text().unwrap(), "b: 99\n");
    assert_eq!(with_b(0).to_text().unwrap(), "b: 0\n");

    let parsed = DynamicMessage::from_text(oneof_hole_desc(), "a: \"x\"").unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::String("x".into())));
    assert!(!parsed.has(2));

    let parsed = DynamicMessage::from_text(oneof_hole_desc(), "b: 0x2a").unwrap();
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(42)));

    // Text last-wins (same as DynamicMessage set).
    let parsed = DynamicMessage::from_text(oneof_hole_desc(), "a: \"x\" b: 1").unwrap();
    assert!(!parsed.has(1));
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(1)));

    assert!(DynamicMessage::from_text(oneof_hole_desc(), "nope: 1").is_err());
}

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pbrs") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/protoc-gen-pbrs")
}

fn json_method_block<'a>(src: &'a str, type_hint: &str) -> &'a str {
    let start = src
        .find(&format!("impl {type_hint} {{"))
        .expect("missing impl");
    let rest = &src[start..];
    let i = rest
        .find("pub fn to_json(")
        .expect("generated source must emit to_json");
    let chunk = &rest[i..];
    let end = chunk
        .find("pub fn to_text(")
        .expect("to_json without to_text in generated source");
    &chunk[..end]
}

fn text_method_block<'a>(src: &'a str, type_hint: &str) -> &'a str {
    let start = src
        .find(&format!("impl {type_hint} {{"))
        .expect("missing impl");
    let rest = &src[start..];
    let i = rest
        .find("pub fn to_text(")
        .expect("generated source must emit to_text");
    let chunk = &rest[i..];
    let end = chunk
        .find("pbrs::impl_typed_message")
        .expect("to_text without impl_typed_message in generated source");
    &chunk[..end]
}

fn write_consumer(dir: &std::path::Path, generated: &str, main_rs: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated-oneof-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), format!("{generated}\n{main_rs}")).unwrap();
}

fn run_consumer(dir: &std::path::Path) -> String {
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = Command::new("cargo");
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(dir);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = build.output().expect("cargo run consumer");
    assert!(
        run.status.success(),
        "consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

fn generate(out: &std::path::Path) -> String {
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/scalars.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", out.display()))
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed for scalars.proto");
    std::fs::read_to_string(out.join("scalars.rs")).expect("generated rs")
}

#[test]
fn generated_oneof_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-oneof");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate(&tmp);

    let hole_json = json_method_block(&generated, "OneofHole");
    assert!(
        hole_json.contains("to_json_value"),
        "generated OneofHole JSON must be field-wise:\n{hole_json}"
    );
    assert!(
        !hole_json.contains("DynamicMessage"),
        "generated OneofHole JSON must not allocate DynamicMessage:\n{hole_json}"
    );

    let official_a = with_a("x").to_json().expect("dm to_json a");
    let official_empty_a = with_a("").to_json().expect("dm to_json empty a");
    let official_b = with_b(99).to_json().expect("dm to_json b");
    let official_zero_b = with_b(0).to_json().expect("dm to_json zero b");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_a = {official_a:?};
    let official_empty_a = {official_empty_a:?};
    let official_b = {official_b:?};
    let official_zero_b = {official_zero_b:?};

    let empty = OneofHole::new();
    assert_eq!(empty.to_json().unwrap(), "{{}}");
    assert!(!empty.has_a());
    assert!(!empty.has_b());
    let parsed = OneofHole::from_json("{{}}").unwrap();
    assert!(!parsed.has_a());
    assert!(!parsed.has_b());

    let mut p = OneofHole::new();
    p.set_a("x");
    let json = p.to_json().expect("to_json a");
    assert_eq!(json, official_a, "generated to_json must match DynamicMessage");
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(p.has_a());
    assert!(!p.has_b());

    let q = OneofHole::from_json(&json).expect("from_json a");
    assert!(q.has_a());
    assert_eq!(q.a(), "x");
    assert!(!q.has_b());

    // Oneof presence is explicit: empty string / zero int64 are present.
    let mut z = OneofHole::new();
    z.set_a("");
    assert_eq!(z.to_json().unwrap(), official_empty_a);
    assert!(z.has_a());
    let z2 = OneofHole::from_json("{{\"a\":\"\"}}").unwrap();
    assert!(z2.has_a());
    assert_eq!(z2.a(), "");

    let mut n = OneofHole::new();
    n.set_b(99);
    assert_eq!(n.to_json().unwrap(), official_b);
    assert!(n.has_b());
    assert!(!n.has_a());
    let n2 = OneofHole::from_json("{{\"b\":\"99\"}}").unwrap();
    assert_eq!(n2.b(), 99);
    assert!(n2.has_b());

    let mut z = OneofHole::new();
    z.set_b(0);
    assert_eq!(z.to_json().unwrap(), official_zero_b);
    let z2 = OneofHole::from_json("{{\"b\":0}}").unwrap();
    assert_eq!(z2.b(), 0);
    assert!(z2.has_b());
    assert!(!z2.has_a());

    // Setter clears the sibling.
    let mut s = OneofHole::new();
    s.set_a("x");
    s.set_b(1);
    assert!(!s.has_a());
    assert!(s.has_b());
    assert_eq!(s.to_json().unwrap(), "{{\"b\":\"1\"}}");

    // Official proto3 JSON: two members of the same oneof is an error.
    assert!(OneofHole::from_json("{{\"a\":\"x\",\"b\":\"1\"}}").is_err());
    assert!(OneofHole::from_json_ignore("{{\"a\":\"x\",\"b\":\"1\"}}", true).is_err());

    // null is absent.
    let n = OneofHole::from_json("{{\"a\":null}}").unwrap();
    assert!(!n.has_a());
    assert!(!n.has_b());
    assert_eq!(n.to_json().unwrap(), "{{}}");

    // unknown rejected unless ignore.
    assert!(OneofHole::from_json("{{\"nope\":1}}").is_err());
    let ign = OneofHole::from_json_ignore("{{\"nope\":1,\"a\":\"x\"}}", true).unwrap();
    assert_eq!(ign.a(), "x");
    assert!(ign.has_a());

    println!("ok oneof json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok oneof json");
}

#[test]
fn generated_oneof_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-oneof");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate(&tmp);

    let hole_text = text_method_block(&generated, "OneofHole");
    assert!(
        hole_text.contains("write_text"),
        "generated OneofHole text must be field-wise:\n{hole_text}"
    );
    assert!(
        !hole_text.contains("DynamicMessage"),
        "generated OneofHole text must not allocate DynamicMessage:\n{hole_text}"
    );

    let official_a = with_a("x").to_text().expect("dm to_text a");
    let official_empty_a = with_a("").to_text().expect("dm to_text empty a");
    let official_b = with_b(99).to_text().expect("dm to_text b");
    let official_zero_b = with_b(0).to_text().expect("dm to_text zero b");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_a = {official_a:?};
    let official_empty_a = {official_empty_a:?};
    let official_b = {official_b:?};
    let official_zero_b = {official_zero_b:?};

    let empty = OneofHole::new();
    assert_eq!(empty.to_text().unwrap(), "");
    assert!(!empty.has_a());
    assert!(!empty.has_b());
    let parsed = OneofHole::from_text("").unwrap();
    assert!(!parsed.has_a());
    assert!(!parsed.has_b());

    let mut p = OneofHole::new();
    p.set_a("x");
    let text = p.to_text().expect("to_text a");
    assert_eq!(text, official_a, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    assert!(p.has_a());
    assert!(!p.has_b());

    let q = OneofHole::from_text(&text).expect("from_text a");
    assert!(q.has_a());
    assert_eq!(q.a(), "x");
    assert!(!q.has_b());

    let mut z = OneofHole::new();
    z.set_a("");
    assert_eq!(z.to_text().unwrap(), official_empty_a);
    let z2 = OneofHole::from_text("a: \"\"").unwrap();
    assert!(z2.has_a());
    assert_eq!(z2.a(), "");

    let mut n = OneofHole::new();
    n.set_b(99);
    assert_eq!(n.to_text().unwrap(), official_b);
    let n2 = OneofHole::from_text("b: 99").unwrap();
    assert_eq!(n2.b(), 99);
    assert!(n2.has_b());
    assert!(!n2.has_a());

    let mut z = OneofHole::new();
    z.set_b(0);
    assert_eq!(z.to_text().unwrap(), official_zero_b);
    let z2 = OneofHole::from_text("b: 0").unwrap();
    assert_eq!(z2.b(), 0);
    assert!(z2.has_b());

    let n = OneofHole::from_text("b: 0x2a").unwrap();
    assert_eq!(n.b(), 42);

    // Text last-wins (same as DynamicMessage).
    let m = OneofHole::from_text("a: \"x\" b: 1").unwrap();
    assert!(!m.has_a());
    assert!(m.has_b());
    assert_eq!(m.b(), 1);

    assert!(OneofHole::from_text("nope: 1").is_err());
    assert!(OneofHole::from_text("a: \"x\" leftover").is_err());

    println!("ok oneof text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok oneof text");
}
