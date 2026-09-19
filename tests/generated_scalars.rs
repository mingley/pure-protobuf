//! Field-wise JSON and text for extra proto3 scalars.
//!
//! These checks fail on current main: a generated message that uses bool /
//! int64 / uint / sint / fixed / float / double / bytes / proto3 enums still
//! serializes then `DynamicMessage`. After the cut it must not. TAT and
//! WKT other than Timestamp / Duration / Empty / wrappers / FieldMask
//! stay on `DynamicMessage`. Real
//! oneof cover is `generated_oneof.rs`. WKT cover is `generated_wkt.rs`.

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
    Cardinality, DynamicMessage, EnumDescriptor, FieldDescriptor, FieldType, MapKeyValue,
    MessageDescriptor, Presence, Value,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn kind_enum() -> Arc<EnumDescriptor> {
    let mut en = EnumDescriptor {
        name: "Kind".into(),
        full_name: "example.Kind".into(),
        ..EnumDescriptor::default()
    };
    en.values.insert(0, "KIND_UNSPECIFIED".into());
    en.values.insert(1, "KIND_A".into());
    en.values.insert(2, "KIND_B".into());
    en.names.insert("KIND_UNSPECIFIED".into(), 0);
    en.names.insert("KIND_A".into(), 1);
    en.names.insert("KIND_B".into(), 2);
    en.listed = vec![
        (0, "KIND_UNSPECIFIED".into()),
        (1, "KIND_A".into()),
        (2, "KIND_B".into()),
    ];
    Arc::new(en)
}

fn named(name: &str, n: u32, ty: FieldType) -> FieldDescriptor {
    let mut f = FieldDescriptor::new(name, n, ty, Cardinality::Optional, Presence::Implicit);
    f.json_name = name.to_string();
    f
}

fn enum_field(name: &str, n: u32, repeated: bool) -> FieldDescriptor {
    let mut f = FieldDescriptor::new(
        name,
        n,
        FieldType::Enum,
        if repeated {
            Cardinality::Repeated
        } else {
            Cardinality::Optional
        },
        Presence::Implicit,
    );
    f.json_name = name.to_string();
    f.enum_ty = Some(kind_enum());
    f
}

fn map_field(
    name: &str,
    n: u32,
    key: FieldType,
    val: FieldType,
    val_enum: bool,
) -> FieldDescriptor {
    let mut kf = named("key", 1, key);
    kf.presence = Presence::Implicit;
    let mut vf = if val_enum {
        enum_field("value", 2, false)
    } else {
        named("value", 2, val)
    };
    vf.presence = Presence::Implicit;
    let entry = Arc::new(
        MessageDescriptor::builder(format!("example.ExtraScalars.{name}Entry"))
            .map_entry(true)
            .field(kf)
            .field(vf)
            .build(),
    );
    let mut f = FieldDescriptor::new(
        name,
        n,
        FieldType::Message,
        Cardinality::Repeated,
        Presence::Implicit,
    );
    f.json_name = name.to_string();
    f.is_map = true;
    f.message = Some(entry);
    f
}

fn extra_desc() -> Arc<MessageDescriptor> {
    let mut maybe = named("maybe", 21, FieldType::Bool);
    maybe.presence = Presence::Explicit;
    let mut note = named("note", 22, FieldType::Bytes);
    note.presence = Presence::Explicit;
    let mut flags = named("flags", 15, FieldType::Bool);
    flags.cardinality = Cardinality::Repeated;
    let mut ids = named("ids", 16, FieldType::Int64);
    ids.cardinality = Cardinality::Repeated;
    Arc::new(
        MessageDescriptor::builder("example.ExtraScalars")
            .field(named("ok", 1, FieldType::Bool))
            .field(named("big", 2, FieldType::Int64))
            .field(named("seq", 3, FieldType::Uint32))
            .field(named("wide", 4, FieldType::Uint64))
            .field(named("zig", 5, FieldType::Sint32))
            .field(named("zag", 6, FieldType::Sint64))
            .field(named("fix32", 7, FieldType::Fixed32))
            .field(named("fix64", 8, FieldType::Fixed64))
            .field(named("sfix32", 9, FieldType::Sfixed32))
            .field(named("sfix64", 10, FieldType::Sfixed64))
            .field(named("flt", 11, FieldType::Float))
            .field(named("dbl", 12, FieldType::Double))
            .field(named("blob", 13, FieldType::Bytes))
            .field(enum_field("kind", 14, false))
            .field(flags)
            .field(ids)
            .field(enum_field("kinds", 17, true))
            .field(map_field(
                "counts",
                18,
                FieldType::String,
                FieldType::Int64,
                false,
            ))
            .field(map_field(
                "bits",
                19,
                FieldType::Int32,
                FieldType::Bool,
                false,
            ))
            .field(maybe)
            .field(note)
            .build(),
    )
}

fn populated() -> DynamicMessage {
    let mut msg = DynamicMessage::new(extra_desc());
    msg.set(1, Value::Bool(true));
    msg.set(2, Value::Int64(99));
    msg.set(3, Value::Uint32(7));
    msg.set(4, Value::Uint64(8));
    msg.set(5, Value::Int32(-3));
    msg.set(6, Value::Int64(-4));
    msg.set(7, Value::Uint32(11));
    msg.set(8, Value::Uint64(12));
    msg.set(9, Value::Int32(-13));
    msg.set(10, Value::Int64(-14));
    msg.set(11, Value::Float(1.5));
    msg.set(12, Value::Double(2.5));
    msg.set(13, Value::Bytes(b"hi".as_slice().into()));
    msg.set(14, Value::Enum(1));
    msg.push(15, Value::Bool(true));
    msg.push(15, Value::Bool(false));
    msg.push(16, Value::Int64(1));
    msg.push(16, Value::Int64(2));
    msg.push(17, Value::Enum(2));
    msg.insert_map(18, MapKeyValue::String("n".into()), Value::Int64(9));
    msg.insert_map(19, MapKeyValue::I32(3), Value::Bool(true));
    msg.set(21, Value::Bool(false));
    msg.set(22, Value::Bytes(b"".as_slice().into()));
    msg
}

/// Official DynamicMessage JSON goldens the generated ExtraScalars path must match.
#[test]
fn official_dynamic_json_goldens_for_extra_scalars() {
    let json = populated().to_json().expect("dm to_json");
    assert!(json.contains("\"ok\":true"), "{json}");
    assert!(json.contains("\"big\":\"99\""), "{json}");
    assert!(json.contains("\"seq\":7"), "{json}");
    assert!(json.contains("\"wide\":\"8\""), "{json}");
    assert!(json.contains("\"kind\":\"KIND_A\""), "{json}");
    assert!(json.contains("\"blob\":\"aGk=\""), "{json}");
    assert!(json.contains("\"flags\":[true,false]"), "{json}");
    assert!(json.contains("\"ids\":[\"1\",\"2\"]"), "{json}");
    assert!(json.contains("\"kinds\":[\"KIND_B\"]"), "{json}");
    assert!(json.contains("\"counts\":{\"n\":\"9\"}"), "{json}");
    assert!(json.contains("\"bits\":{\"3\":true}"), "{json}");
    assert!(json.contains("\"maybe\":false"), "{json}");
    assert!(json.contains("\"note\":\"\""), "{json}");

    let empty = DynamicMessage::new(extra_desc());
    assert_eq!(empty.to_json().unwrap(), "{}");

    let parsed =
        DynamicMessage::from_json(extra_desc(), "{\"big\":\"42\",\"kind\":\"KIND_B\"}").unwrap();
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(42)));
    assert_eq!(parsed.get_singular(14), Some(&Value::Enum(2)));
}

/// Official DynamicMessage text goldens the generated ExtraScalars path must match.
#[test]
fn official_dynamic_text_goldens_for_extra_scalars() {
    let text = populated().to_text().expect("dm to_text");
    assert!(text.contains("ok: true"), "{text}");
    assert!(text.contains("big: 99"), "{text}");
    assert!(text.contains("kind: KIND_A"), "{text}");
    assert!(text.contains("blob: \"hi\""), "{text}");
    assert!(text.contains("flags: true"), "{text}");
    assert!(text.contains("flags: false"), "{text}");
    assert!(text.contains("ids: 1"), "{text}");
    assert!(text.contains("kinds: KIND_B"), "{text}");
    assert!(
        text.contains("counts {\n  key: \"n\"\n  value: 9\n}"),
        "{text}"
    );
    assert!(
        text.contains("bits {\n  key: 3\n  value: true\n}"),
        "{text}"
    );
    assert!(text.contains("maybe: false"), "{text}");
    assert!(text.contains("note: \"\""), "{text}");

    let empty = DynamicMessage::new(extra_desc());
    assert_eq!(empty.to_text().unwrap(), "");

    let parsed = DynamicMessage::from_text(extra_desc(), "big: 0x2a kind: KIND_B").unwrap();
    assert_eq!(parsed.get_singular(2), Some(&Value::Int64(42)));
    assert_eq!(parsed.get_singular(14), Some(&Value::Enum(2)));
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
            "[package]\nname = \"generated-scalars-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
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
fn generated_extra_scalars_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-scalars");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate(&tmp);

    let extra_json = json_method_block(&generated, "ExtraScalars");
    assert!(
        extra_json.contains("to_json_value"),
        "generated ExtraScalars JSON must be field-wise:\n{extra_json}"
    );
    assert!(
        !extra_json.contains("DynamicMessage"),
        "generated ExtraScalars JSON must not allocate DynamicMessage:\n{extra_json}"
    );
    let official = populated().to_json().expect("dm to_json");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official = {official:?};
    assert_eq!(ExtraScalars::new().to_json().unwrap(), "{{}}");

    let mut p = ExtraScalars::new();
    p.set_ok(true);
    p.set_big(99);
    p.set_seq(7);
    p.set_wide(8);
    p.set_zig(-3);
    p.set_zag(-4);
    p.set_fix32(11);
    p.set_fix64(12);
    p.set_sfix32(-13);
    p.set_sfix64(-14);
    p.set_flt(1.5);
    p.set_dbl(2.5);
    p.set_blob(b"hi".as_slice());
    p.set_kind(Kind::A);
    p.flags_mut().push(true);
    p.flags_mut().push(false);
    p.ids_mut().push(1i64);
    p.ids_mut().push(2i64);
    p.kinds_mut().push(i32::from(Kind::B));
    p.counts_mut().insert("n", 9i64);
    p.bits_mut().insert(3, true);
    p.set_maybe(false);
    p.set_note(b"".as_slice());

    let json = p.to_json().expect("to_json");
    assert_eq!(json, official, "generated to_json must match DynamicMessage");
    assert!(!json.contains("DynamicMessage"), "{{json}}");

    let q = ExtraScalars::from_json(&json).expect("from_json");
    assert!(q.ok());
    assert_eq!(q.big(), 99);
    assert_eq!(q.seq(), 7);
    assert_eq!(q.wide(), 8);
    assert_eq!(q.zig(), -3);
    assert_eq!(q.zag(), -4);
    assert_eq!(q.fix32(), 11);
    assert_eq!(q.fix64(), 12);
    assert_eq!(q.sfix32(), -13);
    assert_eq!(q.sfix64(), -14);
    assert_eq!(q.flt(), 1.5);
    assert_eq!(q.dbl(), 2.5);
    assert_eq!(q.blob(), b"hi");
    assert_eq!(q.kind(), Kind::A);
    assert_eq!(q.flags().len(), 2);
    assert_eq!(q.ids().get(0).unwrap(), 1);
    assert_eq!(q.kinds().get(0).unwrap(), i32::from(Kind::B));
    assert_eq!(q.counts().get("n").unwrap(), 9);
    assert_eq!(q.bits().get(3).unwrap(), true);
    assert!(q.has_maybe());
    assert!(!q.maybe());
    assert!(q.has_note());
    assert_eq!(q.note(), b"");

    // Implicit defaults omitted; proto3 optional false / empty bytes present.
    let mut z = ExtraScalars::new();
    z.set_big(0);
    z.set_ok(false);
    z.set_kind(Kind::Unspecified);
    assert_eq!(z.to_json().unwrap(), "{{}}");
    let mut e = ExtraScalars::new();
    e.set_maybe(false);
    assert_eq!(e.to_json().unwrap(), "{{\"maybe\":false}}");

    // Official proto3: int64 accepts a JSON string or number; enum name or number.
    let n = ExtraScalars::from_json("{{\"big\":\"42\",\"kind\":\"KIND_B\"}}").unwrap();
    assert_eq!(n.big(), 42);
    assert_eq!(n.kind(), Kind::B);
    let n = ExtraScalars::from_json("{{\"kind\":1}}").unwrap();
    assert_eq!(n.kind(), Kind::A);
    assert!(ExtraScalars::from_json("{{\"kind\":\"NOPE\"}}").is_err());
    let ign = ExtraScalars::from_json_ignore("{{\"kind\":\"NOPE\",\"big\":3}}", true).unwrap();
    assert_eq!(ign.big(), 3);
    assert_eq!(ign.kind(), Kind::Unspecified);

    // OneofHole in this file stays a sibling fixture; field-wise cover is generated_oneof.rs.
    let hole = OneofHole::from_json("{{\"a\":\"x\"}}").unwrap();
    assert_eq!(hole.a(), "x");
    assert_eq!(hole.to_json().unwrap(), "{{\"a\":\"x\"}}");

    println!("ok scalars json");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok scalars json");
}

#[test]
fn generated_extra_scalars_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-scalars");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate(&tmp);

    let extra_text = text_method_block(&generated, "ExtraScalars");
    assert!(
        extra_text.contains("write_text"),
        "generated ExtraScalars text must be field-wise:\n{extra_text}"
    );
    assert!(
        !extra_text.contains("DynamicMessage"),
        "generated ExtraScalars text must not allocate DynamicMessage:\n{extra_text}"
    );
    let official = populated().to_text().expect("dm to_text");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official = {official:?};
    assert_eq!(ExtraScalars::new().to_text().unwrap(), "");

    let mut p = ExtraScalars::new();
    p.set_ok(true);
    p.set_big(99);
    p.set_seq(7);
    p.set_wide(8);
    p.set_zig(-3);
    p.set_zag(-4);
    p.set_fix32(11);
    p.set_fix64(12);
    p.set_sfix32(-13);
    p.set_sfix64(-14);
    p.set_flt(1.5);
    p.set_dbl(2.5);
    p.set_blob(b"hi".as_slice());
    p.set_kind(Kind::A);
    p.flags_mut().push(true);
    p.flags_mut().push(false);
    p.ids_mut().push(1i64);
    p.ids_mut().push(2i64);
    p.kinds_mut().push(i32::from(Kind::B));
    p.counts_mut().insert("n", 9i64);
    p.bits_mut().insert(3, true);
    p.set_maybe(false);
    p.set_note(b"".as_slice());

    let text = p.to_text().expect("to_text");
    assert_eq!(text, official, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");

    let q = ExtraScalars::from_text(&text).expect("from_text");
    assert!(q.ok());
    assert_eq!(q.big(), 99);
    assert_eq!(q.seq(), 7);
    assert_eq!(q.wide(), 8);
    assert_eq!(q.blob(), b"hi");
    assert_eq!(q.kind(), Kind::A);
    assert_eq!(q.ids().get(0).unwrap(), 1);
    assert_eq!(q.kinds().get(0).unwrap(), i32::from(Kind::B));
    assert_eq!(q.counts().get("n").unwrap(), 9);
    assert_eq!(q.bits().get(3).unwrap(), true);
    assert!(q.has_maybe());
    assert!(!q.maybe());
    assert!(q.has_note());

    let mut z = ExtraScalars::new();
    z.set_big(0);
    z.set_ok(false);
    assert_eq!(z.to_text().unwrap(), "");
    let mut e = ExtraScalars::new();
    e.set_maybe(false);
    assert_eq!(e.to_text().unwrap(), "maybe: false\n");

    let n = ExtraScalars::from_text("big: 0x2a kind: KIND_B").unwrap();
    assert_eq!(n.big(), 42);
    assert_eq!(n.kind(), Kind::B);
    assert!(ExtraScalars::from_text("kind: NOPE").is_err());
    assert!(ExtraScalars::from_text("big: \"42\"").is_err());

    let hole = OneofHole::from_text("a: \"x\"").unwrap();
    assert_eq!(hole.a(), "x");
    assert_eq!(hole.to_text().unwrap(), "a: \"x\"\n");

    println!("ok scalars text");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok scalars text");
}
