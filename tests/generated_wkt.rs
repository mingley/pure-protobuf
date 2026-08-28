//! Field-wise JSON and text for generated Timestamp / Duration.
//!
//! These checks fail on current main: generated `Timestamp` / `Duration`
//! `to_json` / `to_text` still serialize then `DynamicMessage`. After the
//! cut they must not. Other WKT and TAT stay on `DynamicMessage`.
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
use pbrs::gencode::{Duration as GenDuration, Timestamp as GenTimestamp};
use pbrs::{DynamicMessage, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn timestamp_desc() -> Arc<pbrs::MessageDescriptor> {
    pbrs::gencode::conformance_pool()
        .get_message("google.protobuf.Timestamp")
        .expect("timestamp desc")
}

fn duration_desc() -> Arc<pbrs::MessageDescriptor> {
    pbrs::gencode::conformance_pool()
        .get_message("google.protobuf.Duration")
        .expect("duration desc")
}

fn dm_timestamp(seconds: i64, nanos: i32) -> DynamicMessage {
    let mut msg = DynamicMessage::new(timestamp_desc());
    if seconds != 0 {
        msg.set(1, Value::Int64(seconds));
    }
    if nanos != 0 {
        msg.set(2, Value::Int32(nanos));
    }
    msg
}

fn dm_duration(seconds: i64, nanos: i32) -> DynamicMessage {
    let mut msg = DynamicMessage::new(duration_desc());
    if seconds != 0 {
        msg.set(1, Value::Int64(seconds));
    }
    if nanos != 0 {
        msg.set(2, Value::Int32(nanos));
    }
    msg
}

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc_gen_pbrs") {
        return PathBuf::from(p);
    }
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

fn assert_wkt_json_field_wise(src: &str, type_hint: &str, helper: &str) {
    let block = json_method_block(src, type_hint);
    assert!(
        block.contains("to_json_value"),
        "generated {type_hint} JSON must be field-wise:\n{block}"
    );
    assert!(
        block.contains(&format!("pbrs::json::{helper}")),
        "generated {type_hint} JSON must use official string helper:\n{block}"
    );
    assert!(
        !block.contains("DynamicMessage"),
        "generated {type_hint} JSON must not allocate DynamicMessage:\n{block}"
    );
}

fn assert_wkt_text_field_wise(src: &str, type_hint: &str) {
    let block = text_method_block(src, type_hint);
    assert!(
        block.contains("write_text"),
        "generated {type_hint} text must be field-wise:\n{block}"
    );
    assert!(
        !block.contains("DynamicMessage"),
        "generated {type_hint} text must not allocate DynamicMessage:\n{block}"
    );
    assert!(
        block.contains("pbrs::text::parse"),
        "from_text must use pbrs::text::parse:\n{block}"
    );
}

fn write_consumer(dir: &Path, generated: &str, main_rs: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated-wkt-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), format!("{generated}\n{main_rs}")).unwrap();
}

fn run_consumer(dir: &Path) -> String {
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

fn generate(proto: &str, out: &Path) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto = root.join(proto);
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", out.display()))
        .arg("-I")
        .arg(root.join("proto"))
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(
        status.success(),
        "protoc plugin failed for {}",
        proto.display()
    );
    let stem = proto.file_stem().unwrap().to_str().unwrap();
    std::fs::read_to_string(out.join(format!("{stem}.rs"))).expect("generated rs")
}

fn checked_in(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/generated")
            .join(name),
    )
    .expect("checked-in generated WKT")
}

/// Fails on current main: checked-in Timestamp `to_json` still mentions
/// `DynamicMessage`.
#[test]
fn checked_in_timestamp_json_text_is_field_wise() {
    let src = checked_in("timestamp.rs");
    assert_wkt_json_field_wise(&src, "Timestamp", "timestamp");
    assert_wkt_text_field_wise(&src, "Timestamp");
}

/// Fails on current main: checked-in Duration `to_json` still mentions
/// `DynamicMessage`.
#[test]
fn checked_in_duration_json_text_is_field_wise() {
    let src = checked_in("duration.rs");
    assert_wkt_json_field_wise(&src, "Duration", "duration");
    assert_wkt_text_field_wise(&src, "Duration");
}

#[test]
fn generated_timestamp_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-timestamp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/timestamp.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "Timestamp", "timestamp");

    let official_empty = dm_timestamp(0, 0).to_json().unwrap();
    let official = dm_timestamp(1, 2).to_json().unwrap();
    let official_offset =
        DynamicMessage::from_json(timestamp_desc(), "\"1970-01-01T00:00:01+00:00\"")
            .unwrap()
            .to_json()
            .unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};
    let official_offset = {official_offset:?};

    assert_eq!(Timestamp::new().to_json().unwrap(), official_empty);
    let epoch = Timestamp::from_json(&official_empty).unwrap();
    assert_eq!(epoch.seconds(), 0);
    assert_eq!(epoch.nanos(), 0);

    let mut t = Timestamp::new();
    t.set_seconds(1);
    t.set_nanos(2);
    let json = t.to_json().expect("to_json");
    assert_eq!(json, official, "generated to_json must match DynamicMessage");
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    let q = Timestamp::from_json(&json).expect("from_json");
    assert_eq!(q.seconds(), 1);
    assert_eq!(q.nanos(), 2);

    let off = Timestamp::from_json("\"1970-01-01T00:00:01+00:00\"").unwrap();
    assert_eq!(off.seconds(), 1);
    assert_eq!(off.to_json().unwrap(), official_offset);

    assert!(Timestamp::from_json("{{}}").is_err());
    assert!(Timestamp::from_json("\"not-rfc3339\"").is_err());
    println!("ok timestamp json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok timestamp json");
}

#[test]
fn generated_duration_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-duration");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/duration.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "Duration", "duration");

    let official_empty = dm_duration(0, 0).to_json().unwrap();
    let official = dm_duration(3, 4).to_json().unwrap();
    let official_neg = dm_duration(0, -500_000_000).to_json().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};
    let official_neg = {official_neg:?};

    assert_eq!(Duration::new().to_json().unwrap(), official_empty);
    let z = Duration::from_json(&official_empty).unwrap();
    assert_eq!(z.seconds(), 0);
    assert_eq!(z.nanos(), 0);

    let mut d = Duration::new();
    d.set_seconds(3);
    d.set_nanos(4);
    let json = d.to_json().expect("to_json");
    assert_eq!(json, official, "generated to_json must match DynamicMessage");
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    let q = Duration::from_json(&json).expect("from_json");
    assert_eq!(q.seconds(), 3);
    assert_eq!(q.nanos(), 4);

    let n = Duration::from_json("\"-0.500s\"").unwrap();
    assert_eq!(n.seconds(), 0);
    assert_eq!(n.nanos(), -500_000_000);
    assert_eq!(n.to_json().unwrap(), official_neg);

    assert!(Duration::from_json("{{}}").is_err());
    assert!(Duration::from_json("\"1\"").is_err());
    println!("ok duration json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok duration json");
}

#[test]
fn generated_timestamp_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-timestamp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/timestamp.proto", &tmp);
    assert_wkt_text_field_wise(&generated, "Timestamp");

    let official_empty = dm_timestamp(0, 0).to_text().unwrap();
    let official = dm_timestamp(1, 2).to_text().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};

    assert_eq!(Timestamp::new().to_text().unwrap(), official_empty);
    let parsed = Timestamp::from_text("").unwrap();
    assert_eq!(parsed.seconds(), 0);
    assert_eq!(parsed.nanos(), 0);

    let mut t = Timestamp::new();
    t.set_seconds(1);
    t.set_nanos(2);
    let text = t.to_text().expect("to_text");
    assert_eq!(text, official, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    let q = Timestamp::from_text(&text).expect("from_text");
    assert_eq!(q.seconds(), 1);
    assert_eq!(q.nanos(), 2);

    let hex = Timestamp::from_text("seconds: 0x2a nanos: 7").unwrap();
    assert_eq!(hex.seconds(), 42);
    assert_eq!(hex.nanos(), 7);
    println!("ok timestamp text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok timestamp text");
}

#[test]
fn generated_duration_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-duration");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/duration.proto", &tmp);
    assert_wkt_text_field_wise(&generated, "Duration");

    let official_empty = dm_duration(0, 0).to_text().unwrap();
    let official = dm_duration(3, 4).to_text().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};

    assert_eq!(Duration::new().to_text().unwrap(), official_empty);
    let parsed = Duration::from_text("").unwrap();
    assert_eq!(parsed.seconds(), 0);
    assert_eq!(parsed.nanos(), 0);

    let mut d = Duration::new();
    d.set_seconds(3);
    d.set_nanos(4);
    let text = d.to_text().expect("to_text");
    assert_eq!(text, official, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    let q = Duration::from_text(&text).expect("from_text");
    assert_eq!(q.seconds(), 3);
    assert_eq!(q.nanos(), 4);
    println!("ok duration text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok duration text");
}

#[test]
fn generated_has_wkt_parent_is_field_wise() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-has-wkt");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/wkt.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "Timestamp", "timestamp");
    assert_wkt_json_field_wise(&generated, "Duration", "duration");
    let has = json_method_block(&generated, "HasWkt");
    assert!(
        has.contains("to_json_value"),
        "HasWkt JSON must be field-wise:\n{has}"
    );
    assert!(
        !has.contains("DynamicMessage"),
        "HasWkt JSON must not allocate DynamicMessage:\n{has}"
    );
    let official_ts = dm_timestamp(1, 2).to_json().unwrap();
    let official_dur = dm_duration(3, 4).to_json().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_ts = {official_ts:?};
    let official_dur = {official_dur:?};
    assert_eq!(HasWkt::new().to_json().unwrap(), "{{}}");

    let mut h = HasWkt::new();
    let mut ts = Timestamp::new();
    ts.set_seconds(1);
    ts.set_nanos(2);
    h.set_ts(ts);
    let mut dur = Duration::new();
    dur.set_seconds(3);
    dur.set_nanos(4);
    h.set_dur(dur);

    let json = h.to_json().unwrap();
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(json.contains(&official_ts.trim_matches('"')), "{{json}}");
    assert!(json.contains(&official_dur.trim_matches('"')), "{{json}}");

    let q = HasWkt::from_json(&json).unwrap();
    assert_eq!(q.ts().seconds(), 1);
    assert_eq!(q.ts().nanos(), 2);
    assert_eq!(q.dur().seconds(), 3);
    assert_eq!(q.dur().nanos(), 4);
    println!("ok has wkt");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok has wkt");
}

#[test]
fn gencode_timestamp_duration_match_dynamic_message() {
    let official_ts = dm_timestamp(1, 2).to_json().unwrap();
    let official_dur = dm_duration(3, 4).to_json().unwrap();
    let official_ts_text = dm_timestamp(1, 2).to_text().unwrap();
    let official_dur_text = dm_duration(3, 4).to_text().unwrap();

    let mut t = GenTimestamp::new();
    t.set_seconds(1);
    t.set_nanos(2);
    let json = t.to_json().unwrap();
    assert_eq!(json, official_ts);
    assert!(!json.contains("DynamicMessage"));
    let q = GenTimestamp::from_json(&json).unwrap();
    assert_eq!(q.seconds(), 1);
    assert_eq!(q.nanos(), 2);
    assert_eq!(t.to_text().unwrap(), official_ts_text);
    let qt = GenTimestamp::from_text(&official_ts_text).unwrap();
    assert_eq!(qt.seconds(), 1);
    assert_eq!(qt.nanos(), 2);

    let mut d = GenDuration::new();
    d.set_seconds(3);
    d.set_nanos(4);
    let json = d.to_json().unwrap();
    assert_eq!(json, official_dur);
    assert!(!json.contains("DynamicMessage"));
    let q = GenDuration::from_json(&json).unwrap();
    assert_eq!(q.seconds(), 3);
    assert_eq!(q.nanos(), 4);
    assert_eq!(d.to_text().unwrap(), official_dur_text);
    let qd = GenDuration::from_text(&official_dur_text).unwrap();
    assert_eq!(qd.seconds(), 3);
    assert_eq!(qd.nanos(), 4);

    assert_eq!(
        GenTimestamp::new().to_json().unwrap(),
        dm_timestamp(0, 0).to_json().unwrap()
    );
    assert_eq!(
        GenDuration::new().to_json().unwrap(),
        dm_duration(0, 0).to_json().unwrap()
    );
}
