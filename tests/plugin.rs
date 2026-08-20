use std::path::PathBuf;
use std::process::Command;

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pure-protobuf") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/protoc-gen-pure-protobuf")
}

#[test]
fn protoc_plugin_generates_and_roundtrips() {
    let tmp = tempfile_dir();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/person.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pure-protobuf={}",
            plugin_bin().display()
        ))
        .arg(format!("--pure-protobuf_out={}", tmp.display()))
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed");
    let generated = std::fs::read_to_string(tmp.join("person.rs")).expect("generated person.rs");
    assert!(
        generated.contains("impl_generated_message!(Person")
            || generated.contains("pub struct Person"),
        "missing Person:\n{generated}"
    );
    assert!(
        generated.contains("Serialize") || generated.contains("serialize"),
        "{generated}"
    );
    assert!(generated.contains("set_id"), "{generated}");
    assert!(
        !generated.contains("OwnedMessageInner"),
        "must not emit Google upb gencode"
    );

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"plugin-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nprotobuf = {{ package = \"pure-protobuf\", path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nfn main() {{\n  let mut p = Person::new();\n  p.set_id(1);\n  p.set_name(\"ada\");\n  let b = protobuf::Serialize::serialize(&p).unwrap();\n  let q = <Person as protobuf::Parse>::parse(&b).unwrap();\n  assert_eq!(q.id(), 1);\n  assert_eq!(q.name(), \"ada\");\n  let p2 = proto!(Person {{ id: 2, name: \"bob\" }});\n  assert_eq!(p2.id(), 2);\n  // nested merge: second empty Address must not wipe city\n  let mut split = vec![0x32, 0x05, 0x0a, 0x03, b'n', b'y', b'c'];\n  split.extend_from_slice(&[0x32, 0x00]);\n  let merged = <Person as protobuf::Parse>::parse(&split).unwrap();\n  assert_eq!(merged.address().expect(\"address\").city(), \"nyc\");\n  // map field 16: serialized_len must match serialize().len()\n  let mut m = Person::new();\n  m.extras_mut().insert(\"k\".into(), 7);\n  let mb = protobuf::Serialize::serialize(&m).unwrap();\n  assert_eq!(protobuf::Serialize::serialized_len(&m), mb.len());\n  println!(\"ok {{}}\", q.id());\n}}\n"
        ),
    )
    .unwrap();
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = Command::new("cargo");
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run1 = build.output().expect("cargo run consumer");
    assert!(
        run1.status.success(),
        "consumer 1 failed:\n{}\n{}",
        String::from_utf8_lossy(&run1.stdout),
        String::from_utf8_lossy(&run1.stderr)
    );
    let run2 = Command::new("cargo")
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer)
        .output()
        .unwrap();
    assert!(run2.status.success(), "consumer 2 failed");
    assert_eq!(run1.stdout, run2.stdout);
    assert_eq!(String::from_utf8_lossy(&run1.stdout).trim(), "ok 1");
}

#[test]
fn plugin_generates_test_all_types_proto3() {
    let tmp = tempfile_dir_tat();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("third_party/protobuf/src/google/protobuf/test_messages_proto3.proto");
    if !proto.exists() {
        return;
    }
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("third_party/protobuf/src");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pure-protobuf={}",
            plugin_bin().display()
        ))
        .arg(format!("--pure-protobuf_out={}", tmp.display()))
        .arg("-I")
        .arg(&src)
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed for TestAllTypes");
    let generated = std::fs::read_to_string(tmp.join("test_messages_proto3.rs"))
        .expect("generated test_messages_proto3.rs");
    assert!(
        generated.contains("impl_generated_message!(TestAllTypesProto3")
            || generated.contains("pub struct TestAllTypesProto3"),
        "missing TestAllTypesProto3:\n{}",
        &generated[..generated.len().min(2000)]
    );
    assert!(
        !generated.contains("OwnedMessageInner"),
        "must not emit Google upb gencode"
    );
    assert!(generated.contains("set_optional_int32"), "{generated}");
    assert!(
        generated.contains("repeated_from(self.inner.get_repeated"),
        "repeated getters must project DynamicMessage values"
    );
    assert!(
        generated.contains("map_from(self.inner.get_map"),
        "map getters must project DynamicMessage values"
    );
    assert!(
        !generated.contains("-> Repeated<i32> { Repeated::new() }"),
        "repeated_int32 getter must not be a stub"
    );
    assert!(
        !generated.contains("-> Map<i32, i32> { Map::new() }"),
        "map_int32_int32 getter must not be a stub"
    );

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"tat-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nprotobuf = {{ package = \"pure-protobuf\", path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nfn main() {{\n  let mut nested = NestedMessage::new();\n  nested.set_a(9);\n  let mut m = TestAllTypesProto3::new();\n  m.set_optional_int32(7);\n  m.set_optional_string(\"ada\");\n  m.set_optional_nested_message(nested);\n  m.repeated_int32_mut().push(1);\n  m.repeated_int32_mut().push(2);\n  m.map_int32_int32_mut().insert(3, 4);\n  let b = protobuf::Serialize::serialize(&m).unwrap();\n  let q = <TestAllTypesProto3 as protobuf::Parse>::parse(&b).unwrap();\n  assert_eq!(q.optional_int32(), 7);\n  assert_eq!(q.optional_string(), \"ada\");\n  assert_eq!(q.optional_nested_message().expect(\"nested\").a(), 9);\n  assert_eq!(q.repeated_int32().len(), 2);\n  assert_eq!(*q.repeated_int32().get(0).unwrap(), 1);\n  assert_eq!(*q.repeated_int32().get(1).unwrap(), 2);\n  assert_eq!(*q.map_int32_int32().get(&3).unwrap(), 4);\n  println!(\"ok {{}}\", q.optional_int32());\n}}\n"
        ),
    )
    .unwrap();
    let run = Command::new("cargo")
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer)
        .output()
        .expect("cargo run tat consumer");
    assert!(
        run.status.success(),
        "tat consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok 7");
    println!(
        "tat consumer {}",
        String::from_utf8_lossy(&run.stdout).trim()
    );
}

fn tempfile_dir_tat() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-tat");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn tempfile_dir() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
