//! Field-wise JSON on generated proto3 Person (and hello, same mechanism).
//!
//! These checks fail on current main: generated `to_json` / `from_json`
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

/// Official DynamicMessage JSON goldens the generated Person path must match.
#[test]
fn official_dynamic_json_goldens_for_person_shape() {
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
    let json = msg.to_json().expect("dm to_json");
    assert!(json.contains("\"id\":1"), "{json}");
    assert!(json.contains("\"name\":\"ada\""), "{json}");
    assert!(json.contains("\"email\":\"ada@ex\""), "{json}");
    assert!(json.contains("\"tags\":[\"a\",\"b\"]"), "{json}");
    assert!(json.contains("\"scores\":{\"math\":9}"), "{json}");
    assert!(json.contains("\"address\":{\"city\":\"nyc\"}"), "{json}");
    assert!(json.contains("\"extras\":{\"k\":7}"), "{json}");

    let empty = DynamicMessage::new(person_desc());
    assert_eq!(empty.to_json().unwrap(), "{}");

    let mut email = DynamicMessage::new(person_desc());
    email.set(3, Value::String("".into()));
    assert_eq!(email.to_json().unwrap(), "{\"email\":\"\"}");

    let parsed = DynamicMessage::from_json(person_desc(), "{\"id\":\"42\"}").unwrap();
    assert_eq!(parsed.get_singular(1), Some(&Value::Int32(42)));
}

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pbrs") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/protoc-gen-pbrs")
}

fn json_method_blocks(src: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("pub fn to_json(") {
        let chunk = &rest[i..];
        let end = chunk
            .find("pub fn to_text(")
            .expect("to_json without to_text in generated source");
        blocks.push(&chunk[..end]);
        rest = &chunk[end..];
    }
    blocks
}

fn assert_field_wise_json(src: &str) {
    let blocks = json_method_blocks(src);
    assert!(
        !blocks.is_empty(),
        "generated source must emit to_json:\n{src}"
    );
    for block in blocks {
        assert!(
            block.contains("to_json_value"),
            "generated JSON must be field-wise:\n{block}"
        );
        assert!(
            !block.contains("DynamicMessage"),
            "generated JSON must not allocate DynamicMessage:\n{block}"
        );
    }
}

fn write_consumer(dir: &std::path::Path, generated: &str, main_rs: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated-json-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
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
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(proto);
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
    let stem = proto.file_stem().unwrap().to_str().unwrap();
    std::fs::read_to_string(out.join(format!("{stem}.rs"))).expect("generated rs")
}

#[test]
fn generated_person_json_is_field_wise_and_matches_proto3() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-person");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/person.proto", &tmp);
    assert_field_wise_json(&generated);

    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        r#"
use pbrs::prelude::*;
fn main() {
    // Empty omits implicit defaults (official proto3 JSON).
    let empty = Person::new();
    assert_eq!(empty.to_json().unwrap(), "{}");
    let parsed = Person::from_json("{}").unwrap();
    assert_eq!(parsed.id(), 0);
    assert!(parsed.name().is_empty());
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

    let json = p.to_json().expect("to_json");
    assert!(json.contains("\"id\":1"), "{json}");
    assert!(json.contains("\"name\":\"ada\""), "{json}");
    assert!(json.contains("\"email\":\"ada@ex\""), "{json}");
    assert!(json.contains("\"tags\":[\"a\",\"b\"]"), "{json}");
    assert!(json.contains("\"scores\":{\"math\":9}"), "{json}");
    assert!(json.contains("\"address\":{\"city\":\"nyc\"}"), "{json}");
    assert!(json.contains("\"extras\":{\"k\":7}"), "{json}");
    assert!(!json.contains("DynamicMessage"), "{json}");

    let q = Person::from_json(&json).expect("from_json");
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
    assert_eq!(z.to_json().unwrap(), "{}");

    // proto3 optional empty string is present.
    let mut e = Person::new();
    e.set_email("");
    assert_eq!(e.to_json().unwrap(), "{\"email\":\"\"}");
    let e2 = Person::from_json("{\"email\":\"\"}").unwrap();
    assert!(e2.has_email());
    assert_eq!(e2.email(), "");

    // Official proto3: int32 accepts a JSON string.
    let n = Person::from_json("{\"id\":\"42\"}").unwrap();
    assert_eq!(n.id(), 42);

    // null field is absent.
    let n = Person::from_json("{\"id\":null,\"name\":\"x\"}").unwrap();
    assert_eq!(n.id(), 0);
    assert_eq!(n.name(), "x");

    // unknown field rejected unless ignore.
    assert!(Person::from_json("{\"nope\":1}").is_err());
    let ign = Person::from_json_ignore("{\"nope\":1,\"id\":3}", true).unwrap();
    assert_eq!(ign.id(), 3);

    // duplicate JSON object keys rejected (same as DynamicMessage path).
    assert!(Person::from_json("{\"id\":1,\"id\":2}").is_err());

    println!("ok person");
}
"#,
    );
    assert_eq!(run_consumer(&consumer), "ok person");
}

#[test]
fn generated_hello_json_is_free_with_same_mechanism() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("generated-json-hello");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let generated = generate("proto/hello.proto", &tmp);
    assert_field_wise_json(&generated);

    let consumer = tmp.join("consumer");
    write_consumer(
        &consumer,
        &generated,
        r#"
fn main() {
    let mut req = HelloRequest::new();
    req.set_name("ada");
    assert_eq!(req.to_json().unwrap(), "{\"name\":\"ada\"}");
    let parsed = HelloRequest::from_json("{\"name\":\"ada\"}").unwrap();
    assert_eq!(parsed.name(), "ada");
    assert_eq!(HelloRequest::new().to_json().unwrap(), "{}");

    let mut rep = HelloReply::new();
    rep.set_message("Hello ada");
    assert_eq!(rep.to_json().unwrap(), "{\"message\":\"Hello ada\"}");
    assert_eq!(
        HelloReply::from_json("{\"message\":\"Hello ada\"}")
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
