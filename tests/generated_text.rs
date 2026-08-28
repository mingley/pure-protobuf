//! Field-wise text on generated proto3 Person (and hello, same mechanism).
//!
//! These checks fail on current main: generated `to_text` / `from_text`
//! still serialize then `DynamicMessage`. After the cut they must not.

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
    Cardinality, DynamicMessage, FieldDescriptor, FieldType, MapKeyValue, MessageDescriptor,
    Presence, Value,
};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn string_field(name: &str, n: u32, presence: Presence) -> FieldDescriptor {
    let mut f = FieldDescriptor::new(name, n, FieldType::String, Cardinality::Optional, presence);
    f.json_name = name.to_string();
    f
}

fn map_string_i32(name: &str, n: u32) -> FieldDescriptor {
    let entry = Arc::new(
        MessageDescriptor::builder(format!("example.Person.{name}Entry"))
            .map_entry(true)
            .field(string_field("key", 1, Presence::Implicit))
            .field(FieldDescriptor::new(
                "value",
                2,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Implicit,
            ))
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

fn person_desc() -> Arc<MessageDescriptor> {
    let address = Arc::new(
        MessageDescriptor::builder("example.Address")
            .field(string_field("city", 1, Presence::Implicit))
            .build(),
    );
    let mut address_f = FieldDescriptor::new(
        "address",
        6,
        FieldType::Message,
        Cardinality::Optional,
        Presence::Explicit,
    );
    address_f.json_name = "address".into();
    address_f.message = Some(address);
    let mut tags = FieldDescriptor::new(
        "tags",
        4,
        FieldType::String,
        Cardinality::Repeated,
        Presence::Implicit,
    );
    tags.json_name = "tags".into();
    Arc::new(
        MessageDescriptor::builder("example.Person")
            .field(FieldDescriptor::new(
                "id",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                Presence::Implicit,
            ))
            .field(string_field("name", 2, Presence::Implicit))
            .field(string_field("email", 3, Presence::Explicit))
            .field(tags)
            .field(map_string_i32("scores", 5))
            .field(address_f)
            .field(map_string_i32("extras", 16))
            .build(),
    )
}

fn populated_person() -> DynamicMessage {
    let mut addr = DynamicMessage::new(Arc::new(
        MessageDescriptor::builder("example.Address")
            .field(string_field("city", 1, Presence::Implicit))
            .build(),
    ));
    addr.set(1, Value::String("nyc".into()));

    let mut msg = DynamicMessage::new(person_desc());
    msg.set(1, Value::Int32(1));
    msg.set(2, Value::String("ada".into()));
    msg.set(3, Value::String("ada@ex".into()));
    msg.push(4, Value::String("a".into()));
    msg.push(4, Value::String("b".into()));
    msg.insert_map(5, MapKeyValue::String("math".into()), Value::Int32(9));
    msg.set(6, Value::Message(addr));
    msg.insert_map(16, MapKeyValue::String("k".into()), Value::Int32(7));
    msg
}

/// Official DynamicMessage text goldens the generated Person path must match.
#[test]
fn official_dynamic_text_goldens_for_person_shape() {
    let text = populated_person().to_text().expect("dm to_text");
    assert!(text.contains("id: 1"), "{text}");
    assert!(text.contains("name: \"ada\""), "{text}");
    assert!(text.contains("email: \"ada@ex\""), "{text}");
    assert!(text.contains("tags: \"a\""), "{text}");
    assert!(text.contains("tags: \"b\""), "{text}");
    assert!(
        text.contains("scores {\n  key: \"math\"\n  value: 9\n}"),
        "{text}"
    );
    assert!(text.contains("address {\n  city: \"nyc\"\n}"), "{text}");
    assert!(
        text.contains("extras {\n  key: \"k\"\n  value: 7\n}"),
        "{text}"
    );

    let empty = DynamicMessage::new(person_desc());
    assert_eq!(empty.to_text().unwrap(), "");

    let mut email = DynamicMessage::new(person_desc());
    email.set(3, Value::String("".into()));
    assert_eq!(email.to_text().unwrap(), "email: \"\"\n");

    let empty_email = DynamicMessage::from_text(person_desc(), "email: \"\"").unwrap();
    assert_eq!(empty_email.get_singular(3), Some(&Value::String("".into())));

    let parsed = DynamicMessage::from_text(person_desc(), "id: \"nope\"");
    assert!(parsed.is_err(), "quoted int32 must fail");
    let parsed = DynamicMessage::from_text(person_desc(), "id: 0x2a").unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(42)));
    let parsed = DynamicMessage::from_text(person_desc(), "tags: [\"a\", \"b\"]").unwrap();
    assert_eq!(parsed.get_repeated(4).map(<[Value]>::len), Some(2));
}

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pbrs") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/protoc-gen-pbrs")
}

fn text_method_blocks(src: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("pub fn to_text(") {
        let chunk = &rest[i..];
        let end = chunk
            .find("pbrs::impl_typed_message")
            .expect("to_text without impl_typed_message in generated source");
        blocks.push(&chunk[..end]);
        rest = &chunk[end..];
    }
    blocks
}

fn assert_field_wise_text(src: &str) {
    let blocks = text_method_blocks(src);
    assert!(
        !blocks.is_empty(),
        "generated source must emit to_text:\n{src}"
    );
    for block in blocks {
        assert!(
            block.contains("write_text"),
            "generated text must be field-wise:\n{block}"
        );
        assert!(
            !block.contains("DynamicMessage"),
            "generated text must not allocate DynamicMessage:\n{block}"
        );
    }
}

fn write_consumer(dir: &std::path::Path, generated: &str, main_rs: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated-text-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
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

fn generate(proto: &str, out: &std::path::Path) -> String {
    generate_with_stubs(proto, out, true)
}

fn generate_with_stubs(proto: &str, out: &std::path::Path, stubs: bool) -> String {
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(proto);
    if stubs {
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
        assert!(
            status.success(),
            "protoc plugin failed for {}",
            proto.display()
        );
    } else {
        pbrs::codegen::Config::new()
            .emit_tonic_stubs(false)
            .out_dir(out)
            .compile_protos(&[&proto], &[proto.parent().unwrap()])
            .expect("compile_protos");
    }
    let stem = proto.file_stem().unwrap().to_str().unwrap();
    std::fs::read_to_string(out.join(format!("{stem}.rs"))).expect("generated rs")
}

#[test]
fn generated_person_text_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-person");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/person.proto", &tmp);
    assert_field_wise_text(&generated);

    let official = populated_person().to_text().expect("dm to_text");
    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        &format!(
            r#"
fn main() {{
    let official = {official:?};

    // Empty omits implicit defaults (official proto3 text).
    let empty = Person::new();
    assert_eq!(empty.to_text().unwrap(), "");
    let parsed = Person::from_text("").unwrap();
    assert_eq!(parsed.id(), 0);
    assert!(parsed.name().as_bytes().is_empty());
    assert!(!parsed.has_email());

    let mut p = Person::new();
    p.set_id(1);
    p.set_name("ada");
    p.set_email("ada@ex");
    p.tags_mut().push("a");
    p.tags_mut().push("b");
    p.scores_mut().insert("math", 9);
    let mut addr = Address::new();
    addr.set_city("nyc");
    p.set_address(addr);
    p.extras_mut().insert("k", 7);

    let text = p.to_text().expect("to_text");
    assert_eq!(text, official, "generated to_text must match DynamicMessage");
    assert!(!text.contains("DynamicMessage"), "{{text}}");

    let q = Person::from_text(&text).expect("from_text");
    assert_eq!(q.id(), 1);
    assert_eq!(q.name(), "ada");
    assert_eq!(q.email(), "ada@ex");
    assert_eq!(q.tags().len(), 2);
    assert_eq!(q.tags().get(0).unwrap(), "a");
    assert_eq!(q.scores().get("math").unwrap(), 9);
    assert_eq!(q.address().city(), "nyc");
    assert_eq!(q.extras().get("k").unwrap(), 7);

    // Implicit zero / empty omitted.
    let mut z = Person::new();
    z.set_id(0);
    z.set_name("");
    assert_eq!(z.to_text().unwrap(), "");

    // proto3 optional empty string is present.
    let mut e = Person::new();
    e.set_email("");
    assert_eq!(e.to_text().unwrap(), "email: \"\"\n");
    let e2 = Person::from_text("email: \"\"").unwrap();
    assert!(e2.has_email());
    assert_eq!(e2.email(), "");

    // Official proto3 text: int32 accepts hex / octal.
    let n = Person::from_text("id: 0x2a").unwrap();
    assert_eq!(n.id(), 42);
    let n = Person::from_text("id: 052").unwrap();
    assert_eq!(n.id(), 42);

    // Quoted int32 is not a number (same as DynamicMessage).
    assert!(Person::from_text("id: \"42\"").is_err());

    // Repeated list syntax and adjacent concatenated strings.
    let t = Person::from_text("tags: [\"a\", \"b\"]").unwrap();
    assert_eq!(t.tags().len(), 2);
    assert_eq!(t.tags().get(0).unwrap(), "a");
    let t = Person::from_text("name: \"a\" \"da\"").unwrap();
    assert_eq!(t.name(), "ada");

    // Comments, separators, angle-bracket messages.
    let n = Person::from_text("id: 1; name: \"x\" # c\naddress < city: \"nyc\" >").unwrap();
    assert_eq!(n.id(), 1);
    assert_eq!(n.name(), "x");
    assert_eq!(n.address().city(), "nyc");

    // Nested merge (second empty must not wipe city).
    let m = Person::from_text("address {{ city: \"nyc\" }} address {{ }}").unwrap();
    assert!(m.has_address());
    assert_eq!(m.address().city(), "nyc");

    // Map last-wins and missing entry fields default.
    let m = Person::from_text("scores {{ key: \"math\" value: 1 }} scores {{ key: \"math\" value: 9 }}").unwrap();
    assert_eq!(m.scores().get("math").unwrap(), 9);
    let m = Person::from_text("scores {{ }}").unwrap();
    assert_eq!(m.scores().get("").unwrap(), 0);

    // unknown field rejected.
    assert!(Person::from_text("nope: 1").is_err());
    assert!(Person::from_text("id: 1 leftover").is_err());

    // Unknown fields print only on to_text_with_unknown.
    let mut raw = pbrs::Serialize::serialize(&p).unwrap();
    raw.extend_from_slice(&[0x98, 0x06, 0x07]);
    let u = <Person as pbrs::Parse>::parse(&raw).unwrap();
    assert!(!u.to_text().unwrap().contains("99:"), "{{}}", u.to_text().unwrap());
    assert!(u.to_text_with_unknown().unwrap().contains("99: 7"), "{{}}", u.to_text_with_unknown().unwrap());

    println!("ok person");
}}
"#,
        ),
    );
    assert_eq!(run_consumer(&consumer), "ok person");
}

#[test]
fn generated_hello_text_is_free_with_same_mechanism() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-text-hello");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate_with_stubs("proto/hello.proto", &tmp, false);
    assert_field_wise_text(&generated);

    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        r#"
fn main() {
    let mut req = HelloRequest::new();
    req.set_name("ada");
    assert_eq!(req.to_text().unwrap(), "name: \"ada\"\n");
    let parsed = HelloRequest::from_text("name: \"ada\"").unwrap();
    assert_eq!(parsed.name(), "ada");
    assert_eq!(HelloRequest::new().to_text().unwrap(), "");

    let mut rep = HelloReply::new();
    rep.set_message("Hello ada");
    assert_eq!(rep.to_text().unwrap(), "message: \"Hello ada\"\n");
    assert_eq!(
        HelloReply::from_text("message: \"Hello ada\"")
            .unwrap()
            .message(),
        "Hello ada"
    );
    println!("ok hello");
}
"#,
    );
    assert_eq!(run_consumer(&consumer), "ok hello");
}
