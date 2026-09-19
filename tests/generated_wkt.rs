//! Field-wise JSON and text for generated Timestamp / Duration / Empty /
//! proto3 wrappers / FieldMask.
//!
//! These checks fail on current main: generated `FieldMask` `to_json` /
//! `to_text` still serialize then `DynamicMessage`. After the cut they
//! must not. Struct / Value / ListValue / Any and TAT stay on
//! `DynamicMessage`. Remaining is not closed.

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
use pbrs::gencode::{
    BoolValue as GenBoolValue, BytesValue as GenBytesValue, DoubleValue as GenDoubleValue,
    Duration as GenDuration, Empty as GenEmpty, FieldMask as GenFieldMask,
    FloatValue as GenFloatValue, Int32Value as GenInt32Value, Int64Value as GenInt64Value,
    StringValue as GenStringValue, Timestamp as GenTimestamp, UInt32Value as GenUInt32Value,
    UInt64Value as GenUInt64Value,
};
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

fn wkt_desc(name: &str) -> Arc<pbrs::MessageDescriptor> {
    pbrs::gencode::conformance_pool()
        .get_message(name)
        .unwrap_or_else(|| panic!("{name} desc"))
}

fn dm_empty() -> DynamicMessage {
    DynamicMessage::new(wkt_desc("google.protobuf.Empty"))
}

fn dm_wrapper(name: &str, value: Option<Value>) -> DynamicMessage {
    let mut msg = DynamicMessage::new(wkt_desc(name));
    if let Some(v) = value {
        msg.set(1, v);
    }
    msg
}

fn dm_field_mask(paths: &[&str]) -> DynamicMessage {
    let mut msg = DynamicMessage::new(wkt_desc("google.protobuf.FieldMask"));
    for p in paths {
        msg.push(1, Value::String(pbrs::ProtoString::from(*p)));
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

#[test]
fn checked_in_timestamp_json_text_is_field_wise() {
    let src = checked_in("timestamp.rs");
    assert_wkt_json_field_wise(&src, "Timestamp", "timestamp");
    assert_wkt_text_field_wise(&src, "Timestamp");
}

#[test]
fn checked_in_duration_json_text_is_field_wise() {
    let src = checked_in("duration.rs");
    assert_wkt_json_field_wise(&src, "Duration", "duration");
    assert_wkt_text_field_wise(&src, "Duration");
}

#[test]
fn checked_in_empty_json_text_is_field_wise() {
    let src = checked_in("empty.rs");
    assert_wkt_json_field_wise(&src, "Empty", "empty");
    assert_wkt_text_field_wise(&src, "Empty");
}

#[test]
fn checked_in_wrappers_json_text_is_field_wise() {
    let src = checked_in("wrappers.rs");
    for (ty, helper) in [
        ("BoolValue", "boolean"),
        ("Int32Value", "int32"),
        ("Int64Value", "int64"),
        ("UInt32Value", "uint32"),
        ("UInt64Value", "uint64"),
        ("FloatValue", "float"),
        ("DoubleValue", "double"),
        ("StringValue", "string"),
        ("BytesValue", "bytes"),
    ] {
        assert_wkt_json_field_wise(&src, ty, helper);
        assert_wkt_text_field_wise(&src, ty);
    }
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

#[test]
fn generated_empty_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-empty");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/empty.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "Empty", "empty");

    let official = dm_empty().to_json().unwrap();
    assert_eq!(official, "{}");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official = {official:?};
    assert_eq!(Empty::new().to_json().unwrap(), official);
    assert_eq!(Empty::from_json("{{}}").unwrap(), Empty::new());
    assert!(Empty::from_json("[]").is_err());
    assert!(Empty::from_json("null").is_err());
    assert!(Empty::from_json("{{\"nope\":1}}").is_err());
    assert_eq!(Empty::from_json_ignore("{{\"nope\":1}}", true).unwrap(), Empty::new());
    let json = Empty::new().to_json().expect("to_json");
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    println!("ok empty json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok empty json");
}

#[test]
fn generated_empty_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-empty");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/empty.proto", &tmp);
    assert_wkt_text_field_wise(&generated, "Empty");

    let official = dm_empty().to_text().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official = {official:?};
    assert_eq!(Empty::new().to_text().unwrap(), official);
    assert_eq!(Empty::from_text("").unwrap(), Empty::new());
    assert!(Empty::from_text("nope: 1").is_err());
    let text = Empty::new().to_text().expect("to_text");
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    println!("ok empty text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok empty text");
}

#[test]
fn generated_wrappers_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-wrappers");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/wrappers.proto", &tmp);
    for (ty, helper) in [
        ("BoolValue", "boolean"),
        ("Int32Value", "int32"),
        ("Int64Value", "int64"),
        ("UInt32Value", "uint32"),
        ("UInt64Value", "uint64"),
        ("FloatValue", "float"),
        ("DoubleValue", "double"),
        ("StringValue", "string"),
        ("BytesValue", "bytes"),
    ] {
        assert_wkt_json_field_wise(&generated, ty, helper);
    }

    let official_bool = dm_wrapper("google.protobuf.BoolValue", Some(Value::Bool(true)))
        .to_json()
        .unwrap();
    let official_bool_empty = dm_wrapper("google.protobuf.BoolValue", None)
        .to_json()
        .unwrap();
    let official_i32 = dm_wrapper("google.protobuf.Int32Value", Some(Value::Int32(-7)))
        .to_json()
        .unwrap();
    let official_i64 = dm_wrapper("google.protobuf.Int64Value", Some(Value::Int64(42)))
        .to_json()
        .unwrap();
    let official_u32 = dm_wrapper("google.protobuf.UInt32Value", Some(Value::Uint32(9)))
        .to_json()
        .unwrap();
    let official_u64 = dm_wrapper("google.protobuf.UInt64Value", Some(Value::Uint64(11)))
        .to_json()
        .unwrap();
    let official_f32 = dm_wrapper("google.protobuf.FloatValue", Some(Value::Float(1.5)))
        .to_json()
        .unwrap();
    let official_f64 = dm_wrapper("google.protobuf.DoubleValue", Some(Value::Double(2.5)))
        .to_json()
        .unwrap();
    let official_str = dm_wrapper(
        "google.protobuf.StringValue",
        Some(Value::String(pbrs::ProtoString::from("hi"))),
    )
    .to_json()
    .unwrap();
    let official_bytes = dm_wrapper(
        "google.protobuf.BytesValue",
        Some(Value::Bytes(pbrs::ProtoBytes::from(vec![1, 2, 3]))),
    )
    .to_json()
    .unwrap();
    let official_i64_empty = dm_wrapper("google.protobuf.Int64Value", None)
        .to_json()
        .unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    assert_eq!(BoolValue::new().to_json().unwrap(), {official_bool_empty:?});
    let mut b = BoolValue::new();
    b.set_value(true);
    let json = b.to_json().expect("to_json");
    assert_eq!(json, {official_bool:?});
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(BoolValue::from_json("{{}}").is_err());
    assert!(BoolValue::from_json(&json).unwrap().value());

    let mut i = Int32Value::new();
    i.set_value(-7);
    assert_eq!(i.to_json().unwrap(), {official_i32:?});
    assert_eq!(Int32Value::from_json({official_i32:?}).unwrap().value(), -7);

    let mut i64 = Int64Value::new();
    i64.set_value(42);
    assert_eq!(i64.to_json().unwrap(), {official_i64:?});
    assert_eq!(Int64Value::new().to_json().unwrap(), {official_i64_empty:?});
    assert_eq!(Int64Value::from_json({official_i64:?}).unwrap().value(), 42);

    let mut u = UInt32Value::new();
    u.set_value(9);
    assert_eq!(u.to_json().unwrap(), {official_u32:?});
    let mut u64 = UInt64Value::new();
    u64.set_value(11);
    assert_eq!(u64.to_json().unwrap(), {official_u64:?});

    let mut f = FloatValue::new();
    f.set_value(1.5);
    assert_eq!(f.to_json().unwrap(), {official_f32:?});
    let mut d = DoubleValue::new();
    d.set_value(2.5);
    assert_eq!(d.to_json().unwrap(), {official_f64:?});

    let mut s = StringValue::new();
    s.set_value("hi");
    assert_eq!(s.to_json().unwrap(), {official_str:?});
    assert_eq!(StringValue::from_json({official_str:?}).unwrap().value(), "hi");

    let mut bytes = BytesValue::new();
    bytes.set_value(vec![1, 2, 3]);
    assert_eq!(bytes.to_json().unwrap(), {official_bytes:?});
    assert_eq!(BytesValue::from_json({official_bytes:?}).unwrap().value(), &[1, 2, 3]);
    println!("ok wrappers json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok wrappers json");
}

#[test]
fn generated_wrappers_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-wrappers");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/wrappers.proto", &tmp);
    for ty in [
        "BoolValue",
        "Int32Value",
        "Int64Value",
        "StringValue",
        "BytesValue",
    ] {
        assert_wkt_text_field_wise(&generated, ty);
    }

    let official_empty = dm_wrapper("google.protobuf.BoolValue", None)
        .to_text()
        .unwrap();
    let official_bool = dm_wrapper("google.protobuf.BoolValue", Some(Value::Bool(true)))
        .to_text()
        .unwrap();
    let official_i32 = dm_wrapper("google.protobuf.Int32Value", Some(Value::Int32(42)))
        .to_text()
        .unwrap();
    let official_str = dm_wrapper(
        "google.protobuf.StringValue",
        Some(Value::String(pbrs::ProtoString::from("hi"))),
    )
    .to_text()
    .unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    assert_eq!(BoolValue::new().to_text().unwrap(), {official_empty:?});
    let mut b = BoolValue::new();
    b.set_value(true);
    let text = b.to_text().expect("to_text");
    assert_eq!(text, {official_bool:?});
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    assert!(BoolValue::from_text(&text).unwrap().value());

    let mut i = Int32Value::new();
    i.set_value(42);
    assert_eq!(i.to_text().unwrap(), {official_i32:?});
    assert_eq!(Int32Value::from_text("value: 0x2a").unwrap().value(), 42);

    let mut s = StringValue::new();
    s.set_value("hi");
    assert_eq!(s.to_text().unwrap(), {official_str:?});
    println!("ok wrappers text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok wrappers text");
}

#[test]
fn generated_has_empty_wrappers_parent_is_field_wise() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-has-empty-wrappers");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/wkt.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "Empty", "empty");
    assert_wkt_json_field_wise(&generated, "BoolValue", "boolean");
    let has = json_method_block(&generated, "HasEmptyWrappers");
    assert!(
        has.contains("to_json_value"),
        "HasEmptyWrappers JSON must be field-wise:\n{has}"
    );
    assert!(
        !has.contains("DynamicMessage"),
        "HasEmptyWrappers JSON must not allocate DynamicMessage:\n{has}"
    );
    let official_empty = dm_empty().to_json().unwrap();
    let official_bool = dm_wrapper("google.protobuf.BoolValue", Some(Value::Bool(true)))
        .to_json()
        .unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    assert_eq!(HasEmptyWrappers::new().to_json().unwrap(), "{{}}");

    let mut h = HasEmptyWrappers::new();
    h.set_empty(Empty::new());
    let mut flag = BoolValue::new();
    flag.set_value(true);
    h.set_flag(flag);
    let mut i = Int32Value::new();
    i.set_value(3);
    h.set_num(i);
    let mut name = StringValue::new();
    name.set_value("ada");
    h.set_name(name);

    let json = h.to_json().unwrap();
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(json.contains({official_empty:?}), "{{json}}");
    assert!(json.contains({official_bool:?}), "{{json}}");

    let q = HasEmptyWrappers::from_json(&json).unwrap();
    assert!(q.has_empty());
    assert!(q.flag().value());
    assert_eq!(q.num().value(), 3);
    assert_eq!(q.name().value(), "ada");
    println!("ok has empty wrappers");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok has empty wrappers");
}

#[test]
fn gencode_empty_wrappers_match_dynamic_message() {
    assert_eq!(
        GenEmpty::new().to_json().unwrap(),
        dm_empty().to_json().unwrap()
    );
    assert_eq!(
        GenEmpty::new().to_text().unwrap(),
        dm_empty().to_text().unwrap()
    );
    assert_eq!(GenEmpty::from_json("{}").unwrap(), GenEmpty::new());
    assert_eq!(GenEmpty::from_text("").unwrap(), GenEmpty::new());

    let mut b = GenBoolValue::new();
    b.set_value(true);
    let official = dm_wrapper("google.protobuf.BoolValue", Some(Value::Bool(true)))
        .to_json()
        .unwrap();
    let json = b.to_json().unwrap();
    assert_eq!(json, official);
    assert!(!json.contains("DynamicMessage"));
    assert!(GenBoolValue::from_json(&json).unwrap().value());
    assert_eq!(
        b.to_text().unwrap(),
        dm_wrapper("google.protobuf.BoolValue", Some(Value::Bool(true)))
            .to_text()
            .unwrap()
    );

    let mut i = GenInt32Value::new();
    i.set_value(-7);
    assert_eq!(
        i.to_json().unwrap(),
        dm_wrapper("google.protobuf.Int32Value", Some(Value::Int32(-7)))
            .to_json()
            .unwrap()
    );

    let mut i64 = GenInt64Value::new();
    i64.set_value(42);
    assert_eq!(
        i64.to_json().unwrap(),
        dm_wrapper("google.protobuf.Int64Value", Some(Value::Int64(42)))
            .to_json()
            .unwrap()
    );
    assert_eq!(
        GenInt64Value::new().to_json().unwrap(),
        dm_wrapper("google.protobuf.Int64Value", None)
            .to_json()
            .unwrap()
    );

    let mut u = GenUInt32Value::new();
    u.set_value(9);
    assert_eq!(
        u.to_json().unwrap(),
        dm_wrapper("google.protobuf.UInt32Value", Some(Value::Uint32(9)))
            .to_json()
            .unwrap()
    );
    let mut u64 = GenUInt64Value::new();
    u64.set_value(11);
    assert_eq!(
        u64.to_json().unwrap(),
        dm_wrapper("google.protobuf.UInt64Value", Some(Value::Uint64(11)))
            .to_json()
            .unwrap()
    );

    let mut f = GenFloatValue::new();
    f.set_value(1.5);
    assert_eq!(
        f.to_json().unwrap(),
        dm_wrapper("google.protobuf.FloatValue", Some(Value::Float(1.5)))
            .to_json()
            .unwrap()
    );
    let mut d = GenDoubleValue::new();
    d.set_value(2.5);
    assert_eq!(
        d.to_json().unwrap(),
        dm_wrapper("google.protobuf.DoubleValue", Some(Value::Double(2.5)))
            .to_json()
            .unwrap()
    );

    let mut s = GenStringValue::new();
    s.set_value("hi");
    assert_eq!(
        s.to_json().unwrap(),
        dm_wrapper(
            "google.protobuf.StringValue",
            Some(Value::String(pbrs::ProtoString::from("hi")))
        )
        .to_json()
        .unwrap()
    );

    let mut bytes = GenBytesValue::new();
    bytes.set_value(vec![1, 2, 3]);
    assert_eq!(
        bytes.to_json().unwrap(),
        dm_wrapper(
            "google.protobuf.BytesValue",
            Some(Value::Bytes(pbrs::ProtoBytes::from(vec![1, 2, 3])))
        )
        .to_json()
        .unwrap()
    );
    assert_eq!(
        GenBoolValue::new().to_json().unwrap(),
        dm_wrapper("google.protobuf.BoolValue", None)
            .to_json()
            .unwrap()
    );
}

/// Fails on current main: checked-in FieldMask `to_json` still mentions
/// `DynamicMessage`.
#[test]
fn checked_in_field_mask_json_text_is_field_wise() {
    let src = checked_in("field_mask.rs");
    assert_wkt_json_field_wise(&src, "FieldMask", "field_mask");
    assert_wkt_text_field_wise(&src, "FieldMask");
}

#[test]
fn generated_field_mask_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-field-mask");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/field_mask.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "FieldMask", "field_mask");

    let official_empty = dm_field_mask(&[]).to_json().unwrap();
    let official = dm_field_mask(&["a", "b.c"]).to_json().unwrap();
    let official_camel = dm_field_mask(&["foo_bar"]).to_json().unwrap();
    assert_eq!(official_empty, "\"\"");
    assert_eq!(official, "\"a,b.c\"");
    assert_eq!(official_camel, "\"fooBar\"");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};
    let official_camel = {official_camel:?};

    assert_eq!(FieldMask::new().to_json().unwrap(), official_empty);
    let z = FieldMask::from_json(&official_empty).unwrap();
    assert_eq!(z.paths().len(), 0);

    let mut m = FieldMask::new();
    m.paths_mut().push("a");
    m.paths_mut().push("b.c");
    let json = m.to_json().expect("to_json");
    assert_eq!(json, official, "generated to_json must match DynamicMessage");
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(!json.contains("{{"), "FieldMask JSON must be a string, not an object: {{json}}");
    let q = FieldMask::from_json(&json).expect("from_json");
    assert_eq!(q.paths().len(), 2);
    assert_eq!(q.paths().get(0).unwrap(), "a");
    assert_eq!(q.paths().get(1).unwrap(), "b.c");

    let mut camel = FieldMask::new();
    camel.paths_mut().push("foo_bar");
    assert_eq!(camel.to_json().unwrap(), official_camel);
    let parsed = FieldMask::from_json("\"fooBar\"").unwrap();
    assert_eq!(parsed.paths().get(0).unwrap(), "foo_bar");

    assert!(FieldMask::from_json("{{}}").is_err());
    assert!(FieldMask::from_json("\"foo_bar\"").is_err());
    assert!(FieldMask::from_json("[]").is_err());
    println!("ok field mask json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok field mask json");
}

#[test]
fn generated_field_mask_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-field-mask");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/google/protobuf/field_mask.proto", &tmp);
    assert_wkt_text_field_wise(&generated, "FieldMask");

    let official_empty = dm_field_mask(&[]).to_text().unwrap();
    let official = dm_field_mask(&["a", "b.c"]).to_text().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official_empty = {official_empty:?};
    let official = {official:?};

    assert_eq!(FieldMask::new().to_text().unwrap(), official_empty);
    let parsed = FieldMask::from_text("").unwrap();
    assert_eq!(parsed.paths().len(), 0);

    let mut m = FieldMask::new();
    m.paths_mut().push("a");
    m.paths_mut().push("b.c");
    let text = m.to_text().expect("to_text");
    assert_eq!(text, official, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");
    let q = FieldMask::from_text(&text).expect("from_text");
    assert_eq!(q.paths().len(), 2);
    assert_eq!(q.paths().get(0).unwrap(), "a");
    assert_eq!(q.paths().get(1).unwrap(), "b.c");

    let listed = FieldMask::from_text("paths: [\"a\", \"b.c\"]").unwrap();
    assert_eq!(listed.paths().len(), 2);
    assert_eq!(listed.paths().get(0).unwrap(), "a");
    println!("ok field mask text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok field mask text");
}

#[test]
fn generated_has_field_mask_parent_is_field_wise() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-has-field-mask");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/wkt.proto", &tmp);
    assert_wkt_json_field_wise(&generated, "FieldMask", "field_mask");
    let has = json_method_block(&generated, "HasFieldMask");
    assert!(
        has.contains("to_json_value"),
        "HasFieldMask JSON must be field-wise:\n{has}"
    );
    assert!(
        !has.contains("DynamicMessage"),
        "HasFieldMask JSON must not allocate DynamicMessage:\n{has}"
    );
    let official = dm_field_mask(&["a", "b.c"]).to_json().unwrap();
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    assert_eq!(HasFieldMask::new().to_json().unwrap(), "{{}}");

    let mut h = HasFieldMask::new();
    let mut mask = FieldMask::new();
    mask.paths_mut().push("a");
    mask.paths_mut().push("b.c");
    h.set_mask(mask);

    let json = h.to_json().unwrap();
    assert!(!json.contains("DynamicMessage"), "{{json}}");
    assert!(json.contains({official:?}), "{{json}}");

    let q = HasFieldMask::from_json(&json).unwrap();
    assert_eq!(q.mask().paths().len(), 2);
    assert_eq!(q.mask().paths().get(0).unwrap(), "a");
    assert_eq!(q.mask().paths().get(1).unwrap(), "b.c");
    println!("ok has field mask");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok has field mask");
}

#[test]
fn gencode_field_mask_match_dynamic_message() {
    let official_empty = dm_field_mask(&[]).to_json().unwrap();
    let official = dm_field_mask(&["a", "b.c"]).to_json().unwrap();
    let official_camel = dm_field_mask(&["foo_bar"]).to_json().unwrap();
    let official_empty_text = dm_field_mask(&[]).to_text().unwrap();
    let official_text = dm_field_mask(&["a", "b.c"]).to_text().unwrap();

    assert_eq!(GenFieldMask::new().to_json().unwrap(), official_empty);
    assert_eq!(GenFieldMask::new().to_text().unwrap(), official_empty_text);
    assert_eq!(
        GenFieldMask::from_json(&official_empty).unwrap(),
        GenFieldMask::new()
    );
    assert_eq!(GenFieldMask::from_text("").unwrap(), GenFieldMask::new());

    let mut m = GenFieldMask::new();
    m.paths_mut().push("a");
    m.paths_mut().push("b.c");
    let json = m.to_json().unwrap();
    assert_eq!(json, official);
    assert!(!json.contains("DynamicMessage"));
    assert!(!json.contains('{'));
    let q = GenFieldMask::from_json(&json).unwrap();
    assert_eq!(q.paths().len(), 2);
    assert_eq!(q.paths().get(0).unwrap(), "a");
    assert_eq!(q.paths().get(1).unwrap(), "b.c");
    assert_eq!(m.to_text().unwrap(), official_text);
    let qt = GenFieldMask::from_text(&official_text).unwrap();
    assert_eq!(qt.paths().len(), 2);
    assert_eq!(qt.paths().get(0).unwrap(), "a");

    let mut camel = GenFieldMask::new();
    camel.paths_mut().push("foo_bar");
    assert_eq!(camel.to_json().unwrap(), official_camel);
    assert_eq!(
        GenFieldMask::from_json("\"fooBar\"")
            .unwrap()
            .paths()
            .get(0)
            .unwrap(),
        "foo_bar"
    );

    assert!(GenFieldMask::from_json("{}").is_err());
    assert!(GenFieldMask::from_json("\"foo_bar\"").is_err());
}

/// TAT still has Struct / Value / ListValue / Any, so it stays on
/// `DynamicMessage`. Remaining is not closed.
#[test]
fn tat_json_still_uses_dynamic_message() {
    let src = checked_in("test_messages_proto3.rs");
    let block = json_method_block(&src, "TestAllTypesProto3");
    assert!(
        block.contains("DynamicMessage"),
        "TAT must stay on DynamicMessage until Struct / Value / ListValue / Any are field-wise:\n{block}"
    );
}
