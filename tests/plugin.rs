//! Live protoc --plugin=protoc-gen-pbrs round-trips.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
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
    reason = "synchronous test subprocesses share a Cargo cache and generated fixtures"
)]
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

static CONSUMER_CARGO_LOCK: Mutex<()> = Mutex::new(());

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pbrs") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/protoc-gen-pbrs")
}

fn shared_consumer_cargo() -> Command {
    let mut cargo = Command::new("cargo");
    cargo
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/integration-consumers"),
        )
        .env("CARGO_BUILD_JOBS", "2");
    cargo
}

fn run_shared_consumer_cargo(command: &mut Command) -> std::io::Result<std::process::Output> {
    let _guard = CONSUMER_CARGO_LOCK.lock().expect("consumer Cargo lock");
    command.output()
}

#[test]
fn protoc_plugin_generates_and_roundtrips() {
    let tmp = tempfile_dir();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/person.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed");
    let generated = std::fs::read_to_string(tmp.join("person.rs")).expect("generated person.rs");
    assert!(
        generated.contains("pub struct Person"),
        "missing Person:\n{generated}"
    );
    assert!(
        generated.contains("id: i32"),
        "Person must store id as a field, not DynamicMessage:\n{generated}"
    );
    assert!(
        !generated.contains("impl_generated_message!"),
        "must not wrap DynamicMessage"
    );
    assert!(
        generated.contains("impl_typed_message!(Person"),
        "missing typed impl"
    );
    assert!(generated.contains("set_id"), "{generated}");
    assert!(
        !generated.contains("OwnedMessageInner"),
        "must not emit Google upb gencode"
    );
    assert_generated_json_is_field_wise(&generated);
    assert_generated_text_is_field_wise(&generated);

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"plugin-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nuse pbrs::prelude::*;\nfn main() {{\n  let mut p = Person::new();\n  p.set_id(1);\n  p.set_name(\"ada\");\n  let b = pbrs::Serialize::serialize(&p).unwrap();\n  let q = <Person as pbrs::Parse>::parse(&b).unwrap();\n  assert_eq!(q.id(), 1);\n  assert_eq!(q.name(), \"ada\");\n  let p2 = proto!(Person {{ id: 2, name: \"bob\" }});\n  assert_eq!(p2.id(), 2);\n  // nested merge: second empty Address must not wipe city\n  let mut split = vec![0x32, 0x05, 0x0a, 0x03, b'n', b'y', b'c'];\n  split.extend_from_slice(&[0x32, 0x00]);\n  let merged = <Person as pbrs::Parse>::parse(&split).unwrap();\n  assert_eq!(merged.address().city(), \"nyc\");\n  // map field 16: serialized_len must match serialize().len()\n  let mut m = Person::new();\n  m.extras_mut().insert(\"k\", 7);\n  let mb = pbrs::Serialize::serialize(&m).unwrap();\n  assert_eq!(pbrs::Serialize::serialized_len(&m), mb.len());\n  println!(\"ok {{}}\", q.id());\n}}\n"
        ),
    )
    .unwrap();
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run1 = run_shared_consumer_cargo(&mut build).expect("cargo run consumer");
    assert!(
        run1.status.success(),
        "consumer 1 failed:\n{}\n{}",
        String::from_utf8_lossy(&run1.stdout),
        String::from_utf8_lossy(&run1.stderr)
    );
    let run2 = run_shared_consumer_cargo(
        shared_consumer_cargo()
            .arg("run")
            .arg("--offline")
            .arg("--quiet")
            .current_dir(&consumer),
    )
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
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&src)
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed for TestAllTypes");
    let generated = std::fs::read_to_string(tmp.join("test_messages_proto3.rs"))
        .expect("generated test_messages_proto3.rs");
    assert!(
        generated.contains("pub struct TestAllTypesProto3"),
        "missing TestAllTypesProto3:\n{}",
        &generated[..generated.len().min(2000)]
    );
    assert!(
        generated.contains("optional_int32: i32")
            || generated.contains("optional_int32: Option<i32>"),
        "TestAllTypes must use per-field storage"
    );
    assert!(
        !generated.contains("impl_generated_message!"),
        "must not wrap DynamicMessage"
    );
    assert!(
        !generated.contains("OwnedMessageInner"),
        "must not emit Google upb gencode"
    );
    assert!(generated.contains("set_optional_int32"), "{generated}");
    assert!(
        generated.contains("repeated_int32: pbrs::rt::PackedI32"),
        "packed repeated_int32 storage"
    );
    assert!(
        generated.contains("optional_nested_message: pbrs::rt::LazyMsg<NestedMessage>"),
        "nested LEN stored as LazyMsg"
    );
    assert!(
        generated.contains("map_int32_int32: Map<i32, i32>"),
        "map field storage"
    );

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"tat-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nfn main() {{\n  let mut nested = NestedMessage::new();\n  nested.set_a(9);\n  let mut m = TestAllTypesProto3::new();\n  m.set_optional_int32(7);\n  m.set_optional_string(\"ada\");\n  m.set_optional_nested_message(nested);\n  m.repeated_int32_mut().push(1);\n  m.repeated_int32_mut().push(2);\n  m.map_int32_int32_mut().insert(3, 4);\n  let b = pbrs::Serialize::serialize(&m).unwrap();\n  let q = <TestAllTypesProto3 as pbrs::Parse>::parse(&b).unwrap();\n  assert_eq!(q.optional_int32(), 7);\n  assert_eq!(q.optional_string(), \"ada\");\n  assert_eq!(q.optional_nested_message().a(), 9);\n  assert_eq!(q.repeated_int32().len(), 2);\n  assert_eq!(q.repeated_int32().get(0).unwrap(), 1);\n  assert_eq!(q.repeated_int32().get(1).unwrap(), 2);\n  assert_eq!(q.map_int32_int32().get(3).unwrap(), 4);\n  println!(\"ok {{}}\", q.optional_int32());\n}}\n"
        ),
    )
    .unwrap();
    let run = run_shared_consumer_cargo(
        shared_consumer_cargo()
            .arg("run")
            .arg("--offline")
            .arg("--quiet")
            .current_dir(&consumer),
    )
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

#[test]
fn gen_script_emits_person() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-gen-sh");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/person.proto");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/gen.sh");
    let status = Command::new(&script)
        .env("PBRS_PLUGIN", plugin_bin())
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg("-o")
        .arg(&tmp)
        .arg(&proto)
        .status()
        .expect("run gen.sh");
    assert!(status.success(), "gen.sh failed");
    let generated = std::fs::read_to_string(tmp.join("person.rs")).expect("person.rs");
    assert!(generated.contains("pub struct Person"), "{generated}");
    assert!(generated.contains("use pbrs::prelude::*"), "{generated}");
}

fn generate_hello_with_options(
    out_name: &str,
    opt: Option<&str>,
    env_stubs: Option<&str>,
) -> Result<String, (std::process::ExitStatus, String)> {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(out_name);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/hello.proto");
    let mut cmd = Command::new("protoc");
    match env_stubs {
        Some(value) => {
            cmd.env("PURE_PROTOBUF_STUBS", value);
        }
        None => {
            cmd.env_remove("PURE_PROTOBUF_STUBS");
        }
    }
    cmd.arg(format!(
        "--plugin=protoc-gen-pbrs={}",
        plugin_bin().display()
    ));
    cmd.arg(format!("--pbrs_out={}", tmp.display()));
    if let Some(opt_str) = opt {
        cmd.arg(format!("--pbrs_opt={opt_str}"));
    }
    cmd.arg("-I").arg(proto.parent().unwrap()).arg(&proto);
    let output = cmd.output().expect("run protoc");
    if !output.status.success() {
        return Err((
            output.status,
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    Ok(std::fs::read_to_string(tmp.join("hello.rs")).expect("hello.rs"))
}

fn generate_hello_stubs(out_name: &str, stubs_env: Option<&str>) -> String {
    generate_hello_with_options(out_name, None, stubs_env).expect("generate_hello_stubs")
}

#[test]
fn plugin_generates_grpc_stubs() {
    let generated = generate_hello_stubs("plugin-test-grpc", None);
    assert!(
        generated.contains("pub struct HelloRequest"),
        "missing HelloRequest:\n{}",
        &generated[..generated.len().min(1500)]
    );
    assert!(
        generated.contains("name: pbrs::rt::LazyStr"),
        "HelloRequest must store name as a field"
    );
    assert!(
        generated.contains("LazyStr::from_parse_span(wire, data, s, e)"),
        "hello string path must skip Wire::ensure on the inline path:\n{}",
        &generated[..generated.len().min(2500)]
    );
    assert!(
        !generated.contains("LazyStr::from_span(pbrs::rt::Wire::ensure"),
        "hello must not Arc the parent frame before from_span:\n{}",
        &generated[..generated.len().min(2500)]
    );
    assert!(
        !generated.contains("impl_generated_message!"),
        "must not wrap DynamicMessage"
    );
    assert!(
        generated.contains("pub struct GreeterClient"),
        "missing GreeterClient:\n{}",
        &generated[generated.len().saturating_sub(2000)..]
    );
    assert!(
        generated.contains("pub struct GreeterServer"),
        "missing GreeterServer"
    );
    assert!(
        generated.contains("pub fn intercept"),
        "kernel stubs must expose intercept:\n{}",
        &generated[generated.len().saturating_sub(2500)..]
    );
    assert!(
        generated.contains("::pbrs_grpc::Channel"),
        "kernel stubs must name Channel:\n{}",
        &generated[generated.len().saturating_sub(2500)..]
    );
    assert!(
        !generated.contains("fn with_interceptor"),
        "kernel default must not emit tonic with_interceptor"
    );
    assert!(
        !generated.contains("ProtobufCodec"),
        "kernel default must not emit protobuf-tonic codec"
    );
    assert!(
        generated.contains("fn max_decoding_message_size"),
        "missing max_decoding_message_size"
    );
    assert!(
        generated.contains("fn max_encoding_message_size"),
        "missing max_encoding_message_size"
    );
    assert_generated_json_is_field_wise(&generated);
    assert_generated_text_is_field_wise(&generated);
    assert!(generated.contains("fn say_hello"), "missing say_hello");
    assert!(
        generated.contains("fn stream_hello"),
        "missing stream_hello"
    );
    assert!(
        !generated.contains("tonic_prost") && !generated.contains("prost::Message"),
        "must not use prost"
    );
}

#[test]
fn plugin_generates_tonic_stubs_when_env_is_tonic() {
    let generated = generate_hello_stubs("plugin-test-grpc-tonic", Some("tonic"));
    assert!(
        generated.contains("pub struct GreeterClient"),
        "missing GreeterClient:\n{}",
        &generated[generated.len().saturating_sub(2000)..]
    );
    assert!(
        generated.contains("pub struct GreeterServer"),
        "missing GreeterServer"
    );
    assert!(
        generated.contains("fn with_interceptor"),
        "missing with_interceptor:\n{}",
        &generated[generated.len().saturating_sub(2500)..]
    );
    assert!(
        generated.contains("ProtobufCodec"),
        "tonic stubs must use protobuf-tonic codec, not tonic-prost"
    );
    for method in ["SayHello", "ClientHello", "ServerHello", "StreamHello"] {
        let path =
            format!("http::uri::PathAndQuery::from_static(\"/helloworld.Greeter/{method}\")");
        assert!(
            generated.contains(&path),
            "missing static tonic path: {path}"
        );
    }
    assert!(
        !generated.contains(".parse().unwrap()"),
        "tonic stubs must not unwrap a static gRPC path"
    );
    assert!(
        !generated.contains("::pbrs_grpc::Channel"),
        "tonic stubs must not name the kernel Channel"
    );
    assert!(
        !generated.contains("tonic_prost") && !generated.contains("prost::Message"),
        "must not use prost"
    );
}

#[test]
fn plugin_repeated_string_same_tag_parses_32() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-tags");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("tags.proto"),
        "syntax = \"proto3\";\npackage tags;\nmessage Tags { repeated string tags = 1; }\n",
    )
    .unwrap();
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&tmp)
        .arg(tmp.join("tags.proto"))
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed for tags.proto");
    let generated = std::fs::read_to_string(tmp.join("tags.rs")).expect("tags.rs");
    assert!(
        generated.contains("Ok((n2, w2)) if n2 == 1 && w2 == pbrs::rt::WIRE_LEN"),
        "repeated string must same-tag run:\n{}",
        &generated[generated.len().saturating_sub(2500)..]
    );
    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"tags-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nuse pbrs::Parse;\nfn main() {{\n  let mut wire = Vec::new();\n  for i in 0..32 {{\n    let s = format!(\"t{{i:02}}\");\n    wire.push(0x0a);\n    wire.push(s.len() as u8);\n    wire.extend(s.bytes());\n  }}\n  let m = <Tags as Parse>::parse(&wire).expect(\"parse 32\");\n  assert_eq!(m.tags().len(), 32);\n  assert_eq!(m.tags().get(0).unwrap(), \"t00\");\n  assert_eq!(m.tags().get(31).unwrap(), \"t31\");\n  println!(\"ok {{}}\", m.tags().len());\n}}\n"
        ),
    )
    .unwrap();
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = run_shared_consumer_cargo(&mut build).expect("cargo run tags consumer");
    assert!(
        run.status.success(),
        "tags consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok 32");
}

#[test]
fn plugin_proto2_none_parses_non_utf8_proto3_rejects() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-utf8");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let utf8_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/google/rust-tests/shared/utf8");
    for name in ["no_features_proto2.proto", "no_features_proto3.proto"] {
        let proto = utf8_dir.join(name);
        let status = Command::new("protoc")
            .arg(format!(
                "--plugin=protoc-gen-pbrs={}",
                plugin_bin().display()
            ))
            .arg(format!("--pbrs_out={}", tmp.display()))
            .arg("-I")
            .arg(&utf8_dir)
            .arg(&proto)
            .status()
            .expect("run protoc");
        assert!(status.success(), "protoc plugin failed for {name}");
    }
    let p2_src = std::fs::read_to_string(tmp.join("no_features_proto2.rs")).expect("p2");
    let p3_src = std::fs::read_to_string(tmp.join("no_features_proto3.rs")).expect("p3");
    assert!(
        p2_src.contains("from_parse_span_unchecked(wire, data, s, e)"),
        "proto2 NONE must skip UTF-8 on merge:\n{}",
        &p2_src[p2_src.len().saturating_sub(2500)..]
    );
    assert!(
        !p2_src.contains("from_parse_span(wire, data, s, e)"),
        "proto2 NONE must not call the validating parse"
    );
    assert!(
        p3_src.contains("from_parse_span(wire, data, s, e)?"),
        "proto3 must still UTF-8-check:\n{}",
        &p3_src[p3_src.len().saturating_sub(2500)..]
    );
    assert!(
        !p3_src.contains("from_parse_span_unchecked"),
        "proto3 must not skip UTF-8"
    );

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"utf8-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::copy(
        tmp.join("no_features_proto2.rs"),
        consumer.join("src/no_features_proto2.rs"),
    )
    .unwrap();
    std::fs::copy(
        tmp.join("no_features_proto3.rs"),
        consumer.join("src/no_features_proto3.rs"),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        r#"mod no_features_proto2;
mod no_features_proto3;
use no_features_proto2::NoFeaturesProto2;
use no_features_proto3::NoFeaturesProto3;
use pbrs::Parse;
fn main() {
    let wire = [0x0a, 0x01, 0x80];
    let p2 = NoFeaturesProto2::parse(&wire).expect("proto2 NONE Parse of \\x80");
    assert_eq!(p2.my_field().as_bytes(), &[0x80]);
    assert!(NoFeaturesProto3::parse(&wire).is_err(), "proto3 must reject \\x80");
    let mut long = vec![0x0a, 0x18];
    long.extend(std::iter::repeat(0x80).take(24));
    let p2l = NoFeaturesProto2::parse(&long).expect("proto2 NONE long");
    assert_eq!(p2l.my_field().as_bytes(), &[0x80; 24]);
    assert!(NoFeaturesProto3::parse(&long).is_err());
    println!("ok");
}
"#,
    )
    .unwrap();
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = run_shared_consumer_cargo(&mut build).expect("cargo run utf8 consumer");
    assert!(
        run.status.success(),
        "utf8 consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}

fn tempfile_dir_tat() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-tat");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// JSON methods from `pub fn to_json` up to `pub fn to_text`.
/// Fails on current main, where that slice still serializes then
/// `DynamicMessage::from_json_with_pool`.
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

fn assert_generated_json_is_field_wise(src: &str) {
    let blocks = json_method_blocks(src);
    assert!(
        !blocks.is_empty(),
        "generated source must emit to_json:\n{src}"
    );
    for block in blocks {
        assert!(
            block.contains("to_json_value"),
            "generated JSON must be field-wise (no DynamicMessage round-trip):\n{block}"
        );
        assert!(
            !block.contains("DynamicMessage"),
            "generated JSON must not allocate DynamicMessage:\n{block}"
        );
        assert!(
            block.contains("pbrs::json::parse"),
            "from_json must use pbrs::json::parse:\n{block}"
        );
    }
}

/// Text methods from `pub fn to_text` up to `impl_typed_message`.
/// Fails on current main, where that slice still serializes then
/// `DynamicMessage::from_text_with_pool`.
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

fn assert_generated_text_is_field_wise(src: &str) {
    let blocks = text_method_blocks(src);
    assert!(
        !blocks.is_empty(),
        "generated source must emit to_text:\n{src}"
    );
    for block in blocks {
        assert!(
            block.contains("write_text"),
            "generated text must be field-wise (no DynamicMessage round-trip):\n{block}"
        );
        assert!(
            !block.contains("DynamicMessage"),
            "generated text must not allocate DynamicMessage:\n{block}"
        );
        assert!(
            block.contains("TextReader"),
            "generated from_text must stream via TextReader:\n{block}"
        );
        assert!(
            !block.contains("pbrs::text::parse"),
            "generated from_text must not build an intermediate tree:\n{block}"
        );
    }
}

fn tempfile_dir() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn plugin_parameter_stubs_tonic_selects_tonic_stubs() {
    let generated = generate_hello_with_options("plugin-opt-tonic", Some("stubs=tonic"), None)
        .expect("protoc with --pbrs_opt=stubs=tonic");
    assert!(
        generated.contains("ProtobufCodec"),
        "stubs=tonic must emit ProtobufCodec stubs"
    );
    assert!(
        generated.contains("pub struct GreeterClient"),
        "missing GreeterClient"
    );
    assert!(
        !generated.contains("::pbrs_grpc::Channel"),
        "tonic stubs must not name pbrs_grpc Channel"
    );
}

#[test]
fn plugin_parameter_stubs_none_omits_stubs() {
    let generated = generate_hello_with_options("plugin-opt-none", Some("stubs=none"), None)
        .expect("protoc with --pbrs_opt=stubs=none");
    assert!(
        generated.contains("pub struct HelloRequest"),
        "missing HelloRequest"
    );
    assert!(
        !generated.contains("pub struct GreeterClient"),
        "stubs=none must not emit GreeterClient"
    );
    assert!(
        !generated.contains("pub struct GreeterServer"),
        "stubs=none must not emit GreeterServer"
    );
}

#[test]
fn plugin_parameter_stubs_kernel_selects_kernel_stubs() {
    let generated = generate_hello_with_options("plugin-opt-kernel", Some("stubs=kernel"), None)
        .expect("protoc with --pbrs_opt=stubs=kernel");
    assert!(
        generated.contains("::pbrs_grpc::Channel"),
        "stubs=kernel must emit pbrs_grpc kernel stubs"
    );
    assert!(
        generated.contains("pub struct GreeterClient"),
        "missing GreeterClient"
    );
    assert!(
        !generated.contains("ProtobufCodec"),
        "kernel stubs must not emit tonic ProtobufCodec"
    );
}

#[test]
fn plugin_parameter_takes_precedence_over_ambient_env() {
    // Env says kernel, but --pbrs_opt says tonic -> tonic MUST win.
    let generated = generate_hello_with_options(
        "plugin-prec-tonic-over-kernel",
        Some("stubs=tonic"),
        Some("kernel"),
    )
    .expect("precedence: opt tonic over env kernel");
    assert!(
        generated.contains("ProtobufCodec"),
        "explicit --pbrs_opt=stubs=tonic must override ambient PURE_PROTOBUF_STUBS=kernel"
    );
    assert!(
        !generated.contains("::pbrs_grpc::Channel"),
        "tonic must override kernel"
    );

    // Env says tonic, but --pbrs_opt says none -> none MUST win.
    let generated_none = generate_hello_with_options(
        "plugin-prec-none-over-tonic",
        Some("stubs=none"),
        Some("tonic"),
    )
    .expect("precedence: opt none over env tonic");
    assert!(
        !generated_none.contains("pub struct GreeterClient"),
        "explicit --pbrs_opt=stubs=none must override ambient PURE_PROTOBUF_STUBS=tonic"
    );

    // Env says tonic, but --pbrs_opt says kernel -> kernel MUST win.
    let generated_kernel = generate_hello_with_options(
        "plugin-prec-kernel-over-tonic",
        Some("stubs=kernel"),
        Some("tonic"),
    )
    .expect("precedence: opt kernel over env tonic");
    assert!(
        generated_kernel.contains("::pbrs_grpc::Channel"),
        "explicit --pbrs_opt=stubs=kernel must override ambient PURE_PROTOBUF_STUBS=tonic"
    );
    assert!(
        !generated_kernel.contains("ProtobufCodec"),
        "kernel must override tonic"
    );
}

#[test]
fn plugin_rejects_unknown_parameter() {
    let err = generate_hello_with_options("plugin-unknown-opt", Some("unknown_key=foo"), None)
        .expect_err("unknown parameter must fail protoc");
    assert!(!err.0.success());
    assert!(
        err.1.contains("unknown_key"),
        "stderr must identify unknown parameter key: {}",
        err.1
    );

    // Multiple options with an unknown key
    let err2 = generate_hello_with_options(
        "plugin-unknown-opt2",
        Some("stubs=tonic,invalid_flag=true"),
        None,
    )
    .expect_err("unknown parameter in list must fail");
    assert!(!err2.0.success());
    assert!(
        err2.1.contains("invalid_flag"),
        "stderr must identify invalid_flag: {}",
        err2.1
    );
}

#[test]
fn plugin_rejects_invalid_parameter_value() {
    let err =
        generate_hello_with_options("plugin-invalid-stubs", Some("stubs=invalid_flavour"), None)
            .expect_err("invalid stubs value must fail protoc");
    assert!(!err.0.success());
    assert!(
        err.1.contains("stubs"),
        "stderr must identify parameter key 'stubs': {}",
        err.1
    );

    let err2 = generate_hello_with_options("plugin-invalid-bool", Some("emit_deps=notabool"), None)
        .expect_err("invalid bool value must fail protoc");
    assert!(!err2.0.success());
    assert!(
        err2.1.contains("emit_deps"),
        "stderr must identify parameter key 'emit_deps': {}",
        err2.1
    );
}

#[test]
fn direct_parameter_parsing_and_error_variants() {
    use pbrs::codegen::{CodegenError, generate_from_code_generator_request};

    fn make_req(parameter: Option<&str>) -> Vec<u8> {
        let mut req = Vec::new();
        let f = b"hello.proto";
        req.push(0x0a);
        req.push(f.len() as u8);
        req.extend_from_slice(f);
        if let Some(p) = parameter {
            req.push(0x12);
            req.push(p.len() as u8);
            req.extend_from_slice(p.as_bytes());
        }
        req
    }

    // Unknown parameter
    let req = make_req(Some("bogus_option=123"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    match &err {
        CodegenError::UnknownParameter { key, detail } => {
            assert_eq!(key, "bogus_option");
            assert!(detail.contains("bogus_option"));
        }
        other => panic!("expected UnknownParameter, got: {other:?}"),
    }
    assert_eq!(err.parameter_key(), Some("bogus_option"));
    assert!(err.to_string().contains("bogus_option"));

    // Invalid parameter value
    let req = make_req(Some("stubs=unsupported"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    match &err {
        CodegenError::InvalidParameter { key, detail } => {
            assert_eq!(key, "stubs");
            assert!(detail.contains("unsupported"));
        }
        other => panic!("expected InvalidParameter, got: {other:?}"),
    }
    assert_eq!(err.parameter_key(), Some("stubs"));
}

#[test]
fn sequential_calls_with_alternating_configs_are_isolated() {
    // Sequential calls in the same test runner process:
    // Call 1: tonic stubs
    let out1 = generate_hello_with_options("plugin-seq-1", Some("stubs=tonic"), None)
        .expect("call 1 tonic");
    assert!(out1.contains("ProtobufCodec"));
    assert!(!out1.contains("::pbrs_grpc::Channel"));

    // Call 2: none (messages only)
    let out2 =
        generate_hello_with_options("plugin-seq-2", Some("stubs=none"), None).expect("call 2 none");
    assert!(!out2.contains("ProtobufCodec"));
    assert!(!out2.contains("GreeterClient"));

    // Call 3: kernel stubs explicitly
    let out3 = generate_hello_with_options("plugin-seq-3", Some("stubs=kernel"), None)
        .expect("call 3 kernel");
    assert!(out3.contains("::pbrs_grpc::Channel"));
    assert!(!out3.contains("ProtobufCodec"));

    // Call 4: default (no options) - must default to kernel, not leak tonic or none!
    let out4 = generate_hello_with_options("plugin-seq-4", None, None).expect("call 4 default");
    assert!(
        out4.contains("::pbrs_grpc::Channel"),
        "default call 4 must emit kernel stubs, not leak prior stubs"
    );
    assert!(
        !out4.contains("ProtobufCodec"),
        "default call 4 must not leak tonic from call 1"
    );

    // Call 5: tonic again
    let out5 = generate_hello_with_options("plugin-seq-5", Some("stubs=tonic"), None)
        .expect("call 5 tonic");
    assert!(out5.contains("ProtobufCodec"));
    assert!(!out5.contains("::pbrs_grpc::Channel"));

    // Call 6: default again
    let out6 = generate_hello_with_options("plugin-seq-6", None, None).expect("call 6 default");
    assert!(out6.contains("::pbrs_grpc::Channel"));
    assert!(!out6.contains("ProtobufCodec"));
}

#[test]
fn plugin_parameter_shared_pool_and_no_reflect() {
    let out_pool = generate_hello_with_options(
        "plugin-shared-pool",
        Some("stubs=none,shared_pool=true"),
        None,
    )
    .expect("shared pool");
    assert!(out_pool.contains("conformance_pool()"));

    let out_no_reflect = generate_hello_with_options(
        "plugin-no-reflect",
        Some("stubs=none,no_reflect=true"),
        None,
    )
    .expect("no reflect");
    assert!(!out_no_reflect.contains("FILE_DESCRIPTOR_SET"));
}

#[test]
fn config_options_take_precedence_over_ambient_env() {
    if std::env::var_os("PBRS_PLUGIN_CONFIG_ENV_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "config_options_take_precedence_over_ambient_env"])
            .env("PBRS_PLUGIN_CONFIG_ENV_CHILD", "1")
            .env("PURE_PROTOBUF_STUBS", "kernel")
            .output()
            .expect("run config/env precedence in a child");
        assert!(
            output.status.success(),
            "child generation failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    assert_eq!(
        std::env::var("PURE_PROTOBUF_STUBS").as_deref(),
        Ok("kernel")
    );

    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-config-prec");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let proto_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");

    let res = pbrs::codegen::Config::new()
        .out_dir(&tmp)
        .emit_tonic_stubs(true)
        .compile_protos(&[proto_dir.join("hello.proto")], &[&proto_dir]);

    res.expect("compile_protos with explicit tonic stubs");
    let generated = std::fs::read_to_string(tmp.join("hello.rs")).expect("hello.rs");
    assert!(
        generated.contains("ProtobufCodec"),
        "Config::emit_tonic_stubs(true) must override ambient PURE_PROTOBUF_STUBS=kernel"
    );
    assert!(
        !generated.contains("::pbrs_grpc::Channel"),
        "tonic stubs must override kernel"
    );
}

#[test]
fn direct_sequential_calls_isolate_thread_locals() {
    use pbrs::codegen::generate_from_code_generator_request;

    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/hello.proto");
    let tmp_fds = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-direct-fds.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", tmp_fds.display()))
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("protoc fds");
    assert!(status.success());
    let fds_bytes = std::fs::read(&tmp_fds).expect("read fds");

    fn build_req(fds: &[u8], opt: Option<&str>) -> Vec<u8> {
        let mut req = Vec::new();
        // 1: file_to_generate = "hello.proto"
        req.push(0x0a);
        let f = b"hello.proto";
        req.push(f.len() as u8);
        req.extend_from_slice(f);
        if let Some(p) = opt {
            req.push(0x12);
            req.push(p.len() as u8);
            req.extend_from_slice(p.as_bytes());
        }
        // 15: proto_file
        let mut pos = 0;
        while pos < fds.len() {
            let (n, w) = pbrs::rt::decode_tag(fds, &mut pos).unwrap();
            if n == 1 && w == pbrs::rt::WIRE_LEN {
                let blob = pbrs::rt::read_len_bytes(fds, &mut pos).unwrap();
                pbrs::rt::encode_len_field(&mut req, 15, blob);
            } else {
                pbrs::rt::skip_field(fds, &mut pos, w).unwrap();
            }
        }
        req
    }

    // Call 1: tonic stubs
    let req1 = build_req(&fds_bytes, Some("stubs=tonic"));
    let res1 = generate_from_code_generator_request(&req1).expect("call 1");
    let code1 = &res1[0].1;
    assert!(code1.contains("ProtobufCodec"));
    assert!(!code1.contains("::pbrs_grpc::Channel"));

    // Call 2: none
    let req2 = build_req(&fds_bytes, Some("stubs=none"));
    let res2 = generate_from_code_generator_request(&req2).expect("call 2");
    let code2 = &res2[0].1;
    assert!(!code2.contains("ProtobufCodec"));
    assert!(!code2.contains("GreeterClient"));

    // Call 3: kernel explicitly
    let req3 = build_req(&fds_bytes, Some("stubs=kernel"));
    let res3 = generate_from_code_generator_request(&req3).expect("call 3");
    let code3 = &res3[0].1;
    assert!(code3.contains("::pbrs_grpc::Channel"));
    assert!(!code3.contains("ProtobufCodec"));

    // Call 4: default (None) - must NOT leak tonic from call 1 or none from call 2!
    let req4 = build_req(&fds_bytes, None);
    let res4 = generate_from_code_generator_request(&req4).expect("call 4");
    let code4 = &res4[0].1;
    assert!(code4.contains("::pbrs_grpc::Channel"));
    assert!(!code4.contains("ProtobufCodec"));
}

fn tempfile_dir_multi() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-multi");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn protoc_plugin_multi_file_stem_collision_and_cross_package_references() {
    let tmp = tempfile_dir_multi();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_proto = root.join("tests/fixtures/codegen-layout/proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=none")
        .arg("-I")
        .arg(&fixture_proto)
        .arg(fixture_proto.join("pkg_a/common.proto"))
        .arg(fixture_proto.join("pkg_b/common.proto"))
        .arg(fixture_proto.join("pkg_b/service.proto"))
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed");

    // Assert collision-safe output layout
    assert!(
        tmp.join("pkg_a/common.rs").exists(),
        "missing pkg_a/common.rs"
    );
    assert!(
        tmp.join("pkg_b/common.rs").exists(),
        "missing pkg_b/common.rs"
    );
    assert!(
        tmp.join("pkg_b/service.rs").exists(),
        "missing pkg_b/service.rs"
    );
    assert!(tmp.join("mod.rs").exists(), "missing mod.rs");
    assert!(
        !tmp.join("common.rs").exists(),
        "ambiguous common.rs must not be emitted at root"
    );

    let a_content = std::fs::read_to_string(tmp.join("pkg_a/common.rs")).unwrap();
    assert!(
        a_content.contains("pub struct CommonMsg"),
        "pkg_a must define CommonMsg"
    );
    assert!(
        a_content.contains("a_name"),
        "pkg_a CommonMsg must have a_name"
    );

    let b_content = std::fs::read_to_string(tmp.join("pkg_b/common.rs")).unwrap();
    assert!(
        b_content.contains("pub struct CommonMsg"),
        "pkg_b must define CommonMsg"
    );
    assert!(b_content.contains("b_id"), "pkg_b CommonMsg must have b_id");

    let svc_content = std::fs::read_to_string(tmp.join("pkg_b/service.rs")).unwrap();
    assert!(
        svc_content.contains("crate::pkg::a::CommonMsg"),
        "service must reference external pkg.a.CommonMsg via crate path: {svc_content}"
    );
    assert!(
        svc_content.contains("crate::pkg::b::CommonMsg"),
        "service must reference pkg.b.CommonMsg via crate path: {svc_content}"
    );

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"plugin-multi-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();

    let gen_dir = consumer.join("src/gen");
    std::fs::create_dir_all(gen_dir.join("pkg_a")).unwrap();
    std::fs::create_dir_all(gen_dir.join("pkg_b")).unwrap();
    std::fs::copy(tmp.join("pkg_a/common.rs"), gen_dir.join("pkg_a/common.rs")).unwrap();
    std::fs::copy(tmp.join("pkg_b/common.rs"), gen_dir.join("pkg_b/common.rs")).unwrap();
    std::fs::copy(
        tmp.join("pkg_b/service.rs"),
        gen_dir.join("pkg_b/service.rs"),
    )
    .unwrap();
    std::fs::copy(tmp.join("mod.rs"), gen_dir.join("mod.rs")).unwrap();

    std::fs::write(
        consumer.join("src/main.rs"),
        r#"include!("gen/mod.rs");
use pkg::a::CommonMsg as ACommonMsg;
use pkg::b::CommonMsg as BCommonMsg;
use pkg::b::ServiceRequest;

fn main() {
    let mut a = ACommonMsg::new();
    a.set_a_name("alice");
    a.set_a_code(42);

    let mut b = BCommonMsg::new();
    b.set_b_id(999);

    let mut req = ServiceRequest::new();
    req.set_a_msg(a);
    req.set_b_msg(b);

    assert_eq!(req.a_msg().a_name(), "alice");
    assert_eq!(req.a_msg().a_code(), 42);
    assert_eq!(req.b_msg().b_id(), 999);
    println!("plugin multi-file ok");
}
"#,
    )
    .unwrap();

    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = run_shared_consumer_cargo(&mut build).expect("cargo run consumer");
    assert!(
        run.status.success(),
        "consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "plugin multi-file ok"
    );
}

fn tempfile_dir_extern() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-extern");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn protoc_plugin_extern_path_across_two_modules_without_duplicates() {
    let tmp = tempfile_dir_extern();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_proto = root.join("tests/fixtures/codegen-layout/proto");
    let core_proto = root.join("proto");

    // First generate pkg_a/common.rs so the consumer can compile it as the "external" shared crate/module
    let status_a = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&fixture_proto)
        .arg(fixture_proto.join("pkg_a/common.proto"))
        .status()
        .expect("run protoc for pkg_a");
    assert!(status_a.success(), "protoc plugin failed for pkg_a");
    let common_rs = std::fs::read_to_string(tmp.join("pkg_a/common.rs")).unwrap();
    assert!(
        common_rs.contains("pub struct CommonMsg"),
        "pkg_a must define CommonMsg"
    );

    // Also generate wkt/timestamp.rs so the consumer has the external WKT crate/module
    let status_wkt = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&core_proto)
        .arg(core_proto.join("google/protobuf/timestamp.proto"))
        .status()
        .expect("run protoc for wkt");
    assert!(status_wkt.success(), "protoc plugin failed for wkt");
    let ts_rs = std::fs::read_to_string(tmp.join("google/protobuf/timestamp.rs")).unwrap();
    assert!(
        ts_rs.contains("pub struct Timestamp"),
        "wkt must define Timestamp"
    );

    // Now compile external/client.proto and external/service.proto with extern_path mappings
    let status_ext = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=extern_path=.google.protobuf=crate::pbrs_wkt,extern_path=.pkg.a=crate::shared_types::pkg::a")
        .arg("-I")
        .arg(&fixture_proto)
        .arg("-I")
        .arg(&core_proto)
        .arg(fixture_proto.join("external/client.proto"))
        .arg(fixture_proto.join("external/service.proto"))
        .status()
        .expect("run protoc for external");
    assert!(
        status_ext.success(),
        "protoc plugin failed for external protos"
    );

    assert!(
        tmp.join("external/client.rs").exists(),
        "missing external/client.rs"
    );
    assert!(
        tmp.join("external/service.rs").exists(),
        "missing external/service.rs"
    );

    let client_rs = std::fs::read_to_string(tmp.join("external/client.rs")).unwrap();
    assert!(
        client_rs.contains("pub struct ExternalPayload"),
        "client.rs must define ExternalPayload"
    );
    assert!(
        client_rs.contains("pbrs::rt::LazyMsg<crate::pbrs_wkt::Timestamp>"),
        "client.rs must use mapped extern_path for Timestamp: {client_rs}"
    );
    assert!(
        client_rs.contains("pbrs::rt::LazyMsg<crate::shared_types::pkg::a::CommonMsg>"),
        "client.rs must use mapped extern_path for CommonMsg: {client_rs}"
    );
    assert!(
        !client_rs.contains("pub struct Timestamp"),
        "client.rs must NOT emit duplicate Timestamp struct: {client_rs}"
    );
    assert!(
        !client_rs.contains("pub struct CommonMsg"),
        "client.rs must NOT emit duplicate CommonMsg struct: {client_rs}"
    );

    let service_rs = std::fs::read_to_string(tmp.join("external/service.rs")).unwrap();
    assert!(
        service_rs.contains("pub struct ExternalBatch"),
        "service.rs must define ExternalBatch"
    );
    assert!(
        service_rs.contains("pbrs::rt::LazyMsg<crate::pbrs_wkt::Timestamp>"),
        "service.rs must use mapped extern_path for Timestamp: {service_rs}"
    );
    assert!(
        service_rs.contains("pbrs::rt::LazyMsg<crate::shared_types::pkg::a::CommonMsg>"),
        "service.rs must use mapped extern_path for CommonMsg: {service_rs}"
    );
    assert!(
        !service_rs.contains("pub struct Timestamp"),
        "service.rs must NOT emit duplicate Timestamp struct: {service_rs}"
    );
    assert!(
        !service_rs.contains("pub struct CommonMsg"),
        "service.rs must NOT emit duplicate CommonMsg struct: {service_rs}"
    );

    // Build downstream consumer compiling both modules together
    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src/gen/external")).unwrap();
    std::fs::create_dir_all(consumer.join("src/gen/pkg_a")).unwrap();
    std::fs::create_dir_all(consumer.join("src/gen/wkt/google/protobuf")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"extern-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::copy(
        tmp.join("pkg_a/common.rs"),
        consumer.join("src/gen/pkg_a/common.rs"),
    )
    .unwrap();
    std::fs::copy(
        tmp.join("google/protobuf/timestamp.rs"),
        consumer.join("src/gen/wkt/google/protobuf/timestamp.rs"),
    )
    .unwrap();
    std::fs::copy(
        tmp.join("external/client.rs"),
        consumer.join("src/gen/external/client.rs"),
    )
    .unwrap();
    std::fs::copy(
        tmp.join("external/service.rs"),
        consumer.join("src/gen/external/service.rs"),
    )
    .unwrap();
    std::fs::copy(tmp.join("mod.rs"), consumer.join("src/gen/mod.rs")).unwrap();

    std::fs::write(
        consumer.join("src/main.rs"),
        r#"pub mod shared_types {
    pub mod pkg {
        pub mod a {
            include!("gen/pkg_a/common.rs");
        }
    }
}
pub mod pbrs_wkt {
    include!("gen/wkt/google/protobuf/timestamp.rs");
}

include!("gen/mod.rs");

use consumer::external::ExternalPayload;
use consumer::external::ExternalBatch;
use shared_types::pkg::a::CommonMsg;
use pbrs_wkt::Timestamp;

fn main() {
    let mut msg = CommonMsg::new();
    msg.set_a_name("shared-item");
    msg.set_a_code(1234);

    let mut ts = Timestamp::new();
    ts.set_seconds(1700000000);

    let mut payload = ExternalPayload::new();
    payload.set_event_id("evt-1");
    payload.set_payload(msg.clone());
    payload.set_event_time(ts.clone());

    let mut batch = ExternalBatch::new();
    batch.set_batch_id("batch-100");
    batch.set_summary(msg);
    batch.set_batch_time(ts);
    batch.items_mut().push(payload);

    let serialized = pbrs::Serialize::serialize(&batch).expect("serialize batch");
    let parsed = <ExternalBatch as pbrs::Parse>::parse(&serialized).expect("parse batch");

    assert_eq!(parsed.batch_id(), "batch-100");
    assert_eq!(parsed.summary().a_name(), "shared-item");
    assert_eq!(parsed.summary().a_code(), 1234);
    assert_eq!(parsed.batch_time().seconds(), 1700000000);
    assert_eq!(parsed.items().len(), 1);
    let item = parsed.items().get(0).unwrap();
    assert_eq!(item.event_id(), "evt-1");
    assert_eq!(item.payload().a_name(), "shared-item");
    assert_eq!(item.event_time().seconds(), 1700000000);
    println!("extern path two modules ok");
}
"#,
    )
    .unwrap();

    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = run_shared_consumer_cargo(&mut build).expect("cargo run extern consumer");
    assert!(
        run.status.success(),
        "extern consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "extern path two modules ok"
    );
}

fn tempfile_dir_alias() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-alias");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn protoc_plugin_custom_runtime_and_adapter_crate_aliases() {
    let tmp = tempfile_dir_alias();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto = root.join("proto/person.proto");

    // 1. Test runtime_crate=my_pbrs
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=runtime_crate=my_pbrs")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with runtime_crate");
    assert!(status.success(), "protoc plugin failed with runtime_crate");
    let generated = std::fs::read_to_string(tmp.join("person.rs")).expect("generated person.rs");
    assert!(
        generated.contains("use my_pbrs::prelude::*;"),
        "must import from my_pbrs prelude:\n{generated}"
    );
    assert!(
        generated.contains("my_pbrs::rt::LazyStr"),
        "must use my_pbrs::rt::LazyStr:\n{generated}"
    );
    assert!(
        generated.contains("my_pbrs::impl_typed_message!(Person"),
        "must call my_pbrs::impl_typed_message!:\n{generated}"
    );
    assert!(
        generated.contains("my_pbrs::DescriptorPool"),
        "must reference my_pbrs::DescriptorPool:\n{generated}"
    );
    assert!(
        !generated.contains("use pbrs::prelude::*;"),
        "must not contain hardcoded use pbrs::prelude::*:\n{generated}"
    );
    assert!(
        !generated.contains(" pbrs::rt::"),
        "must not contain hardcoded pbrs::rt:::\n{generated}"
    );

    // Build consumer with my_pbrs renamed in Cargo.toml
    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"alias-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nmy_pbrs = {{ package = \"pbrs\", path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/main.rs"),
        format!(
            "{generated}\nuse my_pbrs::prelude::*;\nfn main() {{\n  let mut p = Person::new();\n  p.set_id(42);\n  p.set_name(\"renamed\");\n  let b = my_pbrs::Serialize::serialize(&p).unwrap();\n  let q = <Person as my_pbrs::Parse>::parse(&b).unwrap();\n  assert_eq!(q.id(), 42);\n  assert_eq!(q.name(), \"renamed\");\n  println!(\"runtime crate alias ok\");\n}}\n"
        ),
    )
    .unwrap();
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = shared_consumer_cargo();
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = run_shared_consumer_cargo(&mut build).expect("cargo run alias consumer");
    assert!(
        run.status.success(),
        "alias consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "runtime crate alias ok"
    );

    // 2. Test runtime_crate=::custom_pbrs (leading colons)
    let tmp_colon = tmp.join("colon");
    std::fs::create_dir_all(&tmp_colon).unwrap();
    let status_colon = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp_colon.display()))
        .arg("--pbrs_opt=runtime_crate=::custom_pbrs")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with ::custom_pbrs");
    assert!(status_colon.success());
    let gen_colon = std::fs::read_to_string(tmp_colon.join("person.rs")).unwrap();
    assert!(gen_colon.contains("use ::custom_pbrs::prelude::*;"));
    assert!(gen_colon.contains("::custom_pbrs::rt::LazyStr"));

    // 3. Test grpc_crate=::custom_grpc
    let hello_proto = root.join("proto/hello.proto");
    let tmp_grpc = tmp.join("grpc");
    std::fs::create_dir_all(&tmp_grpc).unwrap();
    let status_grpc = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp_grpc.display()))
        .arg("--pbrs_opt=stubs=kernel,grpc_crate=::custom_grpc")
        .arg("-I")
        .arg(hello_proto.parent().unwrap())
        .arg(&hello_proto)
        .status()
        .expect("run protoc with grpc_crate");
    assert!(status_grpc.success());
    let gen_grpc = std::fs::read_to_string(tmp_grpc.join("hello.rs")).unwrap();
    assert!(
        gen_grpc.contains("::custom_grpc::Channel"),
        "stubs must reference custom grpc crate: {gen_grpc}"
    );
    assert!(
        gen_grpc.contains("::custom_grpc::Request"),
        "stubs must reference custom grpc Request: {gen_grpc}"
    );
    assert!(
        !gen_grpc.contains("::pbrs_grpc::"),
        "stubs must not reference ::pbrs_grpc::"
    );

    // 4. Test tonic_crate=::custom_tonic
    let tmp_tonic = tmp.join("tonic");
    std::fs::create_dir_all(&tmp_tonic).unwrap();
    let status_tonic = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp_tonic.display()))
        .arg("--pbrs_opt=stubs=tonic,tonic_crate=::custom_tonic")
        .arg("-I")
        .arg(hello_proto.parent().unwrap())
        .arg(&hello_proto)
        .status()
        .expect("run protoc with tonic_crate");
    assert!(status_tonic.success());
    let gen_tonic = std::fs::read_to_string(tmp_tonic.join("hello.rs")).unwrap();
    assert!(
        gen_tonic.contains("use ::custom_tonic::ProtobufCodec;"),
        "tonic stubs must reference custom tonic crate: {gen_tonic}"
    );
    assert!(
        !gen_tonic.contains("protobuf_tonic::ProtobufCodec"),
        "tonic stubs must not reference protobuf_tonic"
    );
}

#[test]
fn protoc_plugin_conflicting_and_malformed_mappings_diagnostics() {
    use pbrs::codegen::{CodegenError, Config, generate_from_code_generator_request};

    fn make_req(parameter: Option<&str>) -> Vec<u8> {
        let mut req = Vec::new();
        let f = b"hello.proto";
        req.push(0x0a);
        req.push(f.len() as u8);
        req.extend_from_slice(f);
        if let Some(p) = parameter {
            req.push(0x12);
            req.push(p.len() as u8);
            req.extend_from_slice(p.as_bytes());
        }
        req
    }

    // 1. Missing value for extern_path
    let req = make_req(Some("extern_path"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    match &err {
        CodegenError::InvalidParameter { key, detail } => {
            assert_eq!(key, "extern_path");
            assert!(detail.contains("expected 'proto_path=rust_path'"));
        }
        other => panic!("expected InvalidParameter, got: {other:?}"),
    }

    // 2. Missing '=' in extern_path
    let req = make_req(Some("extern_path=.foo.bar"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    assert_eq!(err.parameter_key(), Some("extern_path"));

    // 3. Empty proto path in extern_path
    let req = make_req(Some("extern_path==crate::foo"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    assert_eq!(err.parameter_key(), Some("extern_path"));

    // 4. Empty rust path in extern_path
    let req = make_req(Some("extern_path=.foo.bar="));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    assert_eq!(err.parameter_key(), Some("extern_path"));

    // 5. Conflicting extern_path
    let req = make_req(Some(
        "extern_path=.foo.bar=crate::foo,extern_path=.foo.bar=crate::other",
    ));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    match &err {
        CodegenError::InvalidParameter { key, detail } => {
            assert_eq!(key, "extern_path");
            assert!(
                detail.contains("conflicting mapping for '.foo.bar'"),
                "got detail: {detail}"
            );
        }
        other => panic!("expected InvalidParameter, got: {other:?}"),
    }

    // 6. Conflicting extern_path with/without leading dot
    let req = make_req(Some(
        "extern_path=.foo.bar=crate::foo,extern_path=foo.bar=crate::other",
    ));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    assert_eq!(err.parameter_key(), Some("extern_path"));

    // 7. Empty runtime_crate
    let req = make_req(Some("runtime_crate="));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    assert_eq!(err.parameter_key(), Some("runtime_crate"));

    // 8. Conflicting runtime_crate
    let req = make_req(Some("runtime_crate=crate1,runtime_crate=crate2"));
    let err = generate_from_code_generator_request(&req).unwrap_err();
    match &err {
        CodegenError::InvalidParameter { key, detail } => {
            assert_eq!(key, "runtime_crate");
            assert!(
                detail.contains("conflicting runtime_crate"),
                "got detail: {detail}"
            );
        }
        other => panic!("expected InvalidParameter, got: {other:?}"),
    }

    // 9. Config::compile_protos builder with conflicting extern_path fails
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-config-conflict");
    let err = Config::new()
        .out_dir(&tmp)
        .extern_path(".foo.bar", "crate::a")
        .extern_path(".foo.bar", "crate::b")
        .compile_protos(&["proto/hello.proto"], &["proto"])
        .unwrap_err();
    assert_eq!(err.parameter_key(), Some("extern_path"));
    assert!(err.to_string().contains("conflicting mapping"));
}

fn tempfile_dir_docs() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-test-docs");
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn protoc_plugin_emits_useful_rustdoc_and_passes_denied_warnings() {
    let tmp = tempfile_dir_docs();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto = root.join("tests/fixtures/codegen-docs/hostile_docs.proto");

    // 1. Compile with stubs=none (messages, enums, fields)
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=none")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with hostile_docs.proto");
    assert!(
        status.success(),
        "protoc plugin failed on hostile_docs.proto"
    );

    let generated =
        std::fs::read_to_string(tmp.join("hostile_docs.rs")).expect("read hostile_docs.rs");

    // Assert schema comments and escape sanitizations
    assert!(
        generated.contains(
            r"Broken intra-doc links: \[NonExistentType\] and \[BrokenReference\]\[ref\]"
        ),
        "must escape broken intra-doc links:\n{generated}"
    );
    assert!(
        generated
            .contains(r"Bare URLs: <https://example.com/api?v=1&x=2> and <http://foo.bar.baz>"),
        "must wrap bare URLs in autolinks:\n{generated}"
    );
    assert!(
        generated.contains(r"Valid markdown link: [Example Site](https://example.com)"),
        "must preserve valid markdown links:\n{generated}"
    );
    assert!(
        generated
            .contains(r"Hostile HTML tags: \<custom-element\> and \<T\> and Map\<string, int32\>"),
        "must escape hostile HTML angle brackets:\n{generated}"
    );
    assert!(
        generated.contains("/// ```text\n/// fn invalid_rust_syntax()"),
        "untagged code fence must be tagged with text:\n{generated}"
    );
    assert!(
        generated.contains("/// ```text\n/// panic!(\"untrusted doctest executed!\");"),
        "hostile rust doctest code fence must be tagged with text:\n{generated}"
    );
    assert!(
        generated.contains("/// ```text\n/// unclosed block\n/// ```"),
        "unclosed code fence must be terminated:\n{generated}"
    );

    // Assert presence and default semantics in API docs
    assert!(
        generated.contains("/// Implicit presence string (default: \"\")."),
        "string getter must document implicit presence:\n{generated}"
    );
    assert!(
        generated.contains("/// Explicit optional field. Returns the value of `count` or the default (`0`) if unset."),
        "count getter must document explicit optional presence and default value:\n{generated}"
    );
    assert!(
        generated.contains("/// Returns `true` if field `count` is set."),
        "count has_count must be documented:\n{generated}"
    );
    assert!(
        generated.contains("/// Repeated field of `pbrs::rt::LazyStr`. Empty by default."),
        "repeated tags must document repeated presence and default:\n{generated}"
    );
    assert!(
        generated.contains(
            "/// Map field with key `pbrs::rt::LazyStr` and value `i32`. Empty by default."
        ),
        "map scores must document map presence and default:\n{generated}"
    );
    assert!(
        generated
            .contains("/// Part of a oneof: setting this field clears other fields in the oneof."),
        "oneof member getters/setters must document oneof clearing semantics:\n{generated}"
    );

    // Assert deprecation annotations and rustdoc
    assert!(
        generated.contains("/// # Deprecated\n    #[deprecated]\n    pub fn old_id"),
        "old_id getter must document and annotate deprecation:\n{generated}"
    );
    assert!(
        generated
            .contains("/// Sets the value of `old_id`.\n    #[deprecated]\n    pub fn set_old_id"),
        "old_id setter must annotate deprecation:\n{generated}"
    );
    assert!(
        generated.contains("/// # Deprecated\n#[deprecated]\n#[derive(Clone, Debug)]\npub struct DeprecatedHostileMessage"),
        "deprecated message must document and annotate deprecation:\n{generated}"
    );
    assert!(
        generated.contains("/// # Deprecated\n    #[deprecated]\n    pub const HostileOne"),
        "deprecated enum value must document and annotate deprecation:\n{generated}"
    );

    // Assert constructor and metadata docs
    assert!(
        generated.contains("/// Creates a new, default instance of [`HostileMessage`]."),
        "new() must be documented:\n{generated}"
    );
    assert!(
        generated.contains("/// Whether an empty byte slice is a valid encoding of this message."),
        "EMPTY_PARSE_OK must be documented:\n{generated}"
    );
    assert!(
        generated.contains("/// The fully-qualified protobuf name of this message."),
        "FULL_NAME must be documented:\n{generated}"
    );

    // Build consumer crate and verify rustdoc with -D warnings
    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"doc-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        format!("//! Documentation verification library.\n\n{generated}\n"),
    )
    .unwrap();

    let mut doc_cmd = shared_consumer_cargo();
    doc_cmd
        .arg("doc")
        .arg("--offline")
        .arg("--no-deps")
        .env("RUSTDOCFLAGS", "-D warnings")
        .current_dir(&consumer);
    if let Ok(h) = std::env::var("CARGO_HOME") {
        doc_cmd.env("CARGO_HOME", h);
    }
    let doc_res = run_shared_consumer_cargo(&mut doc_cmd).expect("run cargo doc on consumer");
    assert!(
        doc_res.status.success(),
        "cargo doc with -D warnings failed on generated code:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&doc_res.stdout),
        String::from_utf8_lossy(&doc_res.stderr)
    );

    // Verify cargo test --doc executes 0 hostile doctests
    let mut test_doc_cmd = shared_consumer_cargo();
    test_doc_cmd
        .arg("test")
        .arg("--doc")
        .arg("--offline")
        .current_dir(&consumer);
    if let Ok(h) = std::env::var("CARGO_HOME") {
        test_doc_cmd.env("CARGO_HOME", h);
    }
    let test_doc_res =
        run_shared_consumer_cargo(&mut test_doc_cmd).expect("run cargo test --doc on consumer");
    assert!(
        test_doc_res.status.success(),
        "cargo test --doc failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&test_doc_res.stdout),
        String::from_utf8_lossy(&test_doc_res.stderr)
    );
    let stdout = String::from_utf8_lossy(&test_doc_res.stdout);
    assert!(
        stdout.contains("0 passed; 0 failed") || stdout.contains("running 0 tests"),
        "must not execute any untrusted doctests: {stdout}"
    );

    // 2. Also compile with stubs=kernel and verify streaming signatures and docs
    let tmp_kernel = tmp.join("kernel");
    std::fs::create_dir_all(&tmp_kernel).unwrap();
    let status_kernel = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp_kernel.display()))
        .arg("--pbrs_opt=stubs=kernel")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with stubs=kernel");
    assert!(status_kernel.success());
    let gen_kernel = std::fs::read_to_string(tmp_kernel.join("hostile_docs.rs")).unwrap();

    // Verify streaming signatures in kernel trait docs
    assert!(
        gen_kernel.contains("/// Streaming signature: Unary `HostileMessage` -> `HostileMessage`."),
        "kernel service must document unary streaming signature:\n{gen_kernel}"
    );
    assert!(
        gen_kernel.contains("/// Streaming signature: Client-streaming stream of `HostileMessage` -> `HostileMessage`."),
        "kernel service must document client streaming signature:\n{gen_kernel}"
    );
    assert!(
        gen_kernel.contains("/// Streaming signature: Server-streaming `HostileMessage` -> stream of `HostileMessage`."),
        "kernel service must document server streaming signature:\n{gen_kernel}"
    );
    assert!(
        gen_kernel.contains("/// Streaming signature: Bidirectional-streaming stream of `HostileMessage` -> stream of `HostileMessage`."),
        "kernel service must document bidi streaming signature:\n{gen_kernel}"
    );
    assert!(
        gen_kernel.contains("/// # Deprecated\n    #[deprecated]\n    fn deprecated_method"),
        "kernel trait deprecated method must document and annotate deprecation:\n{gen_kernel}"
    );
    assert!(
        gen_kernel.contains("/// See \\[ServiceLink\\] and \\<ServiceTag\\>"),
        "kernel service doc comments must be escaped:\n{gen_kernel}"
    );

    // 3. Also compile with stubs=tonic and verify streaming signatures and deprecation
    let tmp_tonic = tmp.join("tonic");
    std::fs::create_dir_all(&tmp_tonic).unwrap();
    let status_tonic = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp_tonic.display()))
        .arg("--pbrs_opt=stubs=tonic")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with stubs=tonic");
    assert!(status_tonic.success());
    let gen_tonic = std::fs::read_to_string(tmp_tonic.join("hostile_docs.rs")).unwrap();

    assert!(
        gen_tonic.contains("/// Streaming signature: Unary `HostileMessage` -> `HostileMessage`."),
        "tonic service must document unary streaming signature:\n{gen_tonic}"
    );
    assert!(
        gen_tonic.contains("/// Streaming signature: Client-streaming stream of `HostileMessage` -> `HostileMessage`."),
        "tonic service must document client streaming signature:\n{gen_tonic}"
    );
    assert!(
        gen_tonic.contains("/// # Deprecated\n    #[deprecated]\n    fn deprecated_method"),
        "tonic trait deprecated method must document and annotate deprecation:\n{gen_tonic}"
    );
    assert!(
        gen_tonic
            .contains("/// # Deprecated\n    #[deprecated]\n    pub async fn deprecated_method"),
        "tonic client deprecated method must document and annotate deprecation:\n{gen_tonic}"
    );
}

fn tempfile_dir_perm() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("plugin-test-perm-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn byte_stability_across_code_generator_request_input_permutations() {
    let tmp = tempfile_dir_perm();
    let p_alpha = tmp.join("alpha.proto");
    let p_beta = tmp.join("beta.proto");

    std::fs::write(
        &p_alpha,
        r#"syntax = "proto3";
package test.plugin_perm;
message Alpha {
    string id = 1;
    oneof payload {
        string text = 2;
        int32 code = 3;
    }
}
enum AlphaStatus {
    ALPHA_UNSPECIFIED = 0;
    ALPHA_OK = 1;
}
"#,
    )
    .unwrap();

    std::fs::write(
        &p_beta,
        r#"syntax = "proto3";
package test.plugin_perm;
message Beta {
    string desc = 1;
}
"#,
    )
    .unwrap();

    let fds_path = tmp.join("test.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", fds_path.display()))
        .arg("-I")
        .arg(&tmp)
        .arg(&p_alpha)
        .arg(&p_beta)
        .status()
        .expect("run protoc");
    assert!(status.success());
    let fds_bytes = std::fs::read(&fds_path).expect("read fds");

    let build_req = |files: &[&str], blobs: &[&[u8]]| -> Vec<u8> {
        let mut req = Vec::new();
        for f in files {
            pbrs::rt::encode_len_field(&mut req, 1, f.as_bytes());
        }
        for b in blobs {
            pbrs::rt::encode_len_field(&mut req, 15, b);
        }
        req
    };

    let mut blobs = Vec::new();
    let mut pos = 0;
    while pos < fds_bytes.len() {
        if let Ok((n, w)) = pbrs::rt::decode_tag(&fds_bytes, &mut pos) {
            if n == 1 && w == pbrs::rt::WIRE_LEN {
                let blob = pbrs::rt::read_len_bytes(&fds_bytes, &mut pos).unwrap();
                blobs.push(blob);
            } else {
                let _ = pbrs::rt::skip_field(&fds_bytes, &mut pos, w);
            }
        } else {
            break;
        }
    }
    assert_eq!(blobs.len(), 2, "expected 2 proto files in descriptor set");

    // Permutation 1: [alpha, beta]
    let req1 = build_req(&["alpha.proto", "beta.proto"], &[blobs[0], blobs[1]]);
    // Permutation 2: [beta, alpha] with permuted blobs
    let req2 = build_req(&["beta.proto", "alpha.proto"], &[blobs[1], blobs[0]]);

    let files1 =
        pbrs::codegen::generate_from_code_generator_request(&req1).expect("generate permutation 1");
    let files2 =
        pbrs::codegen::generate_from_code_generator_request(&req2).expect("generate permutation 2");

    assert_eq!(files1.len(), files2.len(), "file counts must match");
    for (f1, f2) in files1.iter().zip(files2.iter()) {
        assert_eq!(
            f1.0, f2.0,
            "file names must match in deterministic sorted order"
        );
        assert_eq!(
            f1.1, f2.1,
            "file content must be byte-identical for {}",
            f1.0
        );
    }

    let resp1 = pbrs::codegen::encode_code_generator_response(&files1);
    let resp2 = pbrs::codegen::encode_code_generator_response(&files2);
    assert_eq!(
        resp1, resp2,
        "encoded CodeGeneratorResponse must be 100% byte-identical across input permutations"
    );
}

#[test]
fn protoc_plugin_separate_processes_yield_identical_bytes() {
    let tmp = tempfile_dir_perm();
    let p_a = tmp.join("svc_a.proto");
    let p_b = tmp.join("svc_b.proto");

    std::fs::write(
        &p_a,
        r#"syntax = "proto3";
package test.proc;
message SvcAMsg {
    string name = 1;
    int32 count = 2;
}
service SvcAService {
    rpc ZRpc (SvcAMsg) returns (SvcAMsg);
    rpc ARpc (SvcAMsg) returns (SvcAMsg);
}
"#,
    )
    .unwrap();

    std::fs::write(
        &p_b,
        r#"syntax = "proto3";
package test.proc;
message SvcBMsg {
    string desc = 1;
}
"#,
    )
    .unwrap();

    let out1 = tmp.join("proc_out1");
    let out2 = tmp.join("proc_out2");
    std::fs::create_dir_all(&out1).unwrap();
    std::fs::create_dir_all(&out2).unwrap();

    // Process 1: protoc invocation with input order: p_a, p_b
    let status1 = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", out1.display()))
        .arg("--pbrs_opt=stubs=kernel")
        .arg("-I")
        .arg(&tmp)
        .arg(&p_a)
        .arg(&p_b)
        .status()
        .expect("run protoc process 1");
    assert!(status1.success());

    // Process 2: separate protoc process invocation with permuted input order: p_b, p_a
    let status2 = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", out2.display()))
        .arg("--pbrs_opt=stubs=kernel")
        .arg("-I")
        .arg(&tmp)
        .arg(&p_b)
        .arg(&p_a)
        .status()
        .expect("run protoc process 2");
    assert!(status2.success());

    // Compare bytes between the two separate processes
    let bytes_a1 = std::fs::read(out1.join("svc_a.rs")).unwrap();
    let bytes_a2 = std::fs::read(out2.join("svc_a.rs")).unwrap();
    assert_eq!(
        bytes_a1, bytes_a2,
        "svc_a.rs must be byte-identical between separate processes"
    );

    let bytes_b1 = std::fs::read(out1.join("svc_b.rs")).unwrap();
    let bytes_b2 = std::fs::read(out2.join("svc_b.rs")).unwrap();
    assert_eq!(
        bytes_b1, bytes_b2,
        "svc_b.rs must be byte-identical between separate processes"
    );

    let bytes_mod1 = std::fs::read(out1.join("mod.rs")).unwrap();
    let bytes_mod2 = std::fs::read(out2.join("mod.rs")).unwrap();
    assert_eq!(
        bytes_mod1, bytes_mod2,
        "mod.rs must be byte-identical between separate processes"
    );

    // Also assert that identical re-run preserves mtime
    let out3 = tmp.join("proc_out3");
    std::fs::create_dir_all(&out3).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(&out3)
        .emit_kernel_stubs(true)
        .compile_protos(&[&p_a, &p_b], &[&tmp])
        .expect("initial compile out3");

    let mtime_b1 = std::fs::metadata(out3.join("svc_b.rs"))
        .unwrap()
        .modified()
        .unwrap();
    let mtime_a1 = std::fs::metadata(out3.join("svc_a.rs"))
        .unwrap()
        .modified()
        .unwrap();
    let mtime_mod1 = std::fs::metadata(out3.join("mod.rs"))
        .unwrap()
        .modified()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Re-run compilation into out3 with identical inputs using Config
    pbrs::codegen::Config::new()
        .out_dir(&out3)
        .emit_kernel_stubs(true)
        .compile_protos(&[&p_a, &p_b], &[&tmp])
        .expect("recompile out3 with identical inputs");

    let mtime_b2 = std::fs::metadata(out3.join("svc_b.rs"))
        .unwrap()
        .modified()
        .unwrap();
    let mtime_a2 = std::fs::metadata(out3.join("svc_a.rs"))
        .unwrap()
        .modified()
        .unwrap();
    let mtime_mod2 = std::fs::metadata(out3.join("mod.rs"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        mtime_b1, mtime_b2,
        "svc_b.rs mtime must be preserved on identical inputs"
    );
    assert_eq!(
        mtime_a1, mtime_a2,
        "svc_a.rs mtime must be preserved on identical inputs"
    );
    assert_eq!(
        mtime_mod1, mtime_mod2,
        "mod.rs mtime must be preserved on identical inputs"
    );
}

fn tempfile_dir_lints() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("plugin-test-lints-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn protoc_plugin_enum_deprecation_is_independent_of_allow_alias() {
    let tmp = tempfile_dir_lints();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codegen-lints/enum_option_cases.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=none")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("generate enum option fixture");
    assert!(
        status.success(),
        "protoc plugin failed on enum_option_cases.proto"
    );
    let generated = std::fs::read_to_string(tmp.join("enum_option_cases.rs"))
        .expect("read enum option fixture");
    let deprecated_type = "#[deprecated]\n#[repr(transparent)]\n#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub struct UnaliasedDeprecatedEnum";
    assert!(
        generated.contains(deprecated_type),
        "actual enum deprecation was lost:\n{generated}"
    );
    assert_eq!(
        generated.matches("#[deprecated]").count(),
        1,
        "allow_alias must not deprecate AliasedActiveEnum"
    );
    assert!(generated.contains("pub const OneAlias: AliasedActiveEnum = AliasedActiveEnum(1);"));
}

#[test]
fn protoc_plugin_generated_lint_allowances_have_explicit_reasons_and_no_broad_restriction() {
    let tmp = tempfile_dir_lints();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codegen-lints/lint_cases.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=kernel")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc");
    assert!(status.success(), "protoc plugin failed on lint_cases.proto");

    let generated = std::fs::read_to_string(tmp.join("lint_cases.rs")).expect("read lint_cases.rs");

    // Assert that clippy::restriction is NOT present
    assert!(
        !generated.contains("clippy::restriction"),
        "must not contain broad clippy::restriction allow:\n{generated}"
    );

    // Assert that clippy::all and clippy::pedantic are present with explicit justifications
    assert!(
        generated.contains(r#"#[allow(clippy::all, reason = "#),
        "clippy::all must have an explicit reason attribute:\n{generated}"
    );
    assert!(
        generated.contains(r#"#[allow(clippy::pedantic, reason = "#),
        "clippy::pedantic must have an explicit reason attribute:\n{generated}"
    );

    assert_eq!(
        generated.matches("clippy::expect_used").count(),
        1,
        "expect_used must only be allowed on the validated descriptor pool helper"
    );
    assert!(
        generated
            .lines()
            .zip(generated.lines().skip(1))
            .any(|(attribute, next)| attribute
                .starts_with("#[allow(clippy::expect_used, reason = \"")
                && next.starts_with("fn generated_pool()")),
        "expect_used allowance must be scoped to generated_pool"
    );

    // Verify all #[allow(...)] and #![allow(...)] occurrences have an explicit reason = "..."
    for line in generated.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[allow(") || trimmed.starts_with("#![allow(") {
            assert!(
                trimmed.contains("reason = "),
                "allow attribute is missing reason in line: {trimmed}"
            );
        }
    }
}

#[test]
fn protoc_plugin_generated_messages_compile_under_strict_consumer_lint_policy() {
    let tmp = tempfile_dir_lints();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codegen-lints/lint_cases.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=none")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with stubs=none");
    assert!(status.success(), "protoc plugin failed");

    let generated = std::fs::read_to_string(tmp.join("lint_cases.rs")).expect("read lint_cases.rs");

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "message-strict-consumer"
version = "0.0.1"
edition = "2021"

[workspace]

[dependencies]
pbrs = {{ path = "{root}" }}
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        consumer.join("src/lib.rs"),
        format!(
            r#"//! Strict consumer test for message generation.
#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
            #![deny(clippy::expect_used)]

            {generated}

#[test]
fn test_message_keywords_and_non_standard_casings() {{
    let mut msg = r#type::new();
    msg.set_type("test_type");
    msg.set_match(42);
    msg.set_fn(true);
    msg.set_struct(12345);
    msg.set_for("for_val");
    msg.set_let("let_val");
    msg.set_mut("mut_val");
    msg.set_ref("ref_val");
    msg.set_pub("pub_val");
    msg.set_self("self_val");
    msg.set_crate("crate_val");
    msg.set_super("super_val");
    msg.set_loop("loop_val");
    msg.set_while("while_val");
    msg.set_if("if_val");
    msg.set_else("else_val");
    msg.set_return("return_val");
    msg.set_trait("trait_val");
    msg.set_impl("impl_val");
    msg.set_const("const_val");

    // Optional fields
    msg.set_optional_type("opt_t");
    msg.set_optional_match(77);
    msg.set_optional_fn(true);
    msg.set_optional_struct(999);

    assert_eq!(msg.r#type(), "test_type");
    assert_eq!(msg.r#match(), 42);
    assert!(msg.r#fn());
    assert_eq!(msg.r#struct(), 12345);
    assert_eq!(msg.self_(), "self_val");
    assert_eq!(msg.crate_(), "crate_val");
    assert_eq!(msg.super_(), "super_val");

    assert!(msg.has_optional_type());
    assert_eq!(msg.optional_type(), "opt_t");
    assert_eq!(msg.optional_type_opt().map(|s| s.as_bytes()), Some(b"opt_t".as_slice()));
    assert!(msg.has_optional_match());
    assert_eq!(msg.optional_match(), 77);
    assert_eq!(msg.optional_match_opt(), Some(77));
    assert!(msg.has_optional_fn());
    assert_eq!(msg.optional_fn(), true);
    assert_eq!(msg.optional_fn_opt(), Some(true));
    assert!(msg.has_optional_struct());
    assert_eq!(msg.optional_struct(), 999);
    assert_eq!(msg.optional_struct_opt(), Some(999));

    // Non-standard casing fields
    msg.set_PascalCaseField("pascal");
    msg.set_UPPER_CASE_FIELD(100);
    msg.set_camelCaseField(true);
    msg.set_mixed_Case_Field("mixed");
    assert_eq!(msg.PascalCaseField(), "pascal");
    assert_eq!(msg.UPPER_CASE_FIELD(), 100);
    assert!(msg.camelCaseField());
    assert_eq!(msg.mixed_Case_Field(), "mixed");

    // Enums
    msg.set_enum_field(non_standard_enum::Pascalval.0);
    assert_eq!(msg.enum_field(), non_standard_enum::Pascalval);

    // Repeated and Map
    msg.repeated_fn_mut().push("rep1");
    msg.map_match_mut().insert("k1", 10);
    assert_eq!(msg.repeated_fn().len(), 1);
    assert_eq!(msg.map_match().get("k1"), Some(10));

    // Roundtrip serialization
    let bytes = pbrs::Serialize::serialize(&msg).expect("serialize");
    let parsed = <r#type as pbrs::Parse>::parse(&bytes).expect("parse");
    assert_eq!(parsed.r#type(), "test_type");
    assert_eq!(parsed.r#match(), 42);
    assert!(parsed.r#fn());
    assert_eq!(parsed.r#struct(), 12345);
    assert_eq!(parsed.optional_type(), "opt_t");
    assert_eq!(parsed.PascalCaseField(), "pascal");

    // Bad casing messages
    let mut snake = snake_case_message::new();
    snake.set_field_one("one");
    assert_eq!(snake.field_one(), "one");

    let mut upper = UPPER_CASE_MESSAGE::new();
    upper.set_ID("id1");
    assert_eq!(upper.ID(), "id1");

    let mut camel = camelCaseMessage::new();
    camel.set_myField("my_val");
    assert_eq!(camel.myField(), "my_val");
}}
"#
        ),
    )
    .unwrap();

    let cargo_home = std::env::var("CARGO_HOME").ok();

    let mut clippy_cmd = shared_consumer_cargo();
    clippy_cmd
        .arg("clippy")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        clippy_cmd.env("CARGO_HOME", h);
    }
    let clippy_out = run_shared_consumer_cargo(&mut clippy_cmd).expect("run cargo clippy");
    assert!(
        clippy_out.status.success(),
        "cargo clippy on message consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&clippy_out.stdout),
        String::from_utf8_lossy(&clippy_out.stderr)
    );

    let mut test_cmd = shared_consumer_cargo();
    test_cmd
        .arg("test")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        test_cmd.env("CARGO_HOME", h);
    }
    let test_out = run_shared_consumer_cargo(&mut test_cmd).expect("run cargo test");
    assert!(
        test_out.status.success(),
        "cargo test on message consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

#[test]
fn protoc_plugin_deprecated_enums_keep_implementation_lints_scoped() {
    let tmp = tempfile_dir_lints();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("tests/fixtures/codegen-lints");
    let mut modules = String::new();
    let mut generated_enums = Vec::new();
    for (module, stem, name) in [
        ("open", "deprecated_open_enum", "DeprecatedOpenEnum"),
        ("closed", "deprecated_closed_enum", "DeprecatedClosedEnum"),
    ] {
        let status = Command::new("protoc")
            .arg(format!(
                "--plugin=protoc-gen-pbrs={}",
                plugin_bin().display()
            ))
            .arg(format!("--pbrs_out={}", tmp.display()))
            .arg("--pbrs_opt=stubs=none")
            .arg("-I")
            .arg(&fixtures)
            .arg(fixtures.join(format!("{stem}.proto")))
            .status()
            .expect("generate deprecated enum fixture");
        assert!(status.success(), "protoc plugin failed on {stem}.proto");
        let generated = std::fs::read_to_string(tmp.join(format!("{stem}.rs")))
            .expect("read generated deprecated enum");
        assert!(
            generated.contains("#[deprecated]\n#[repr(transparent)]"),
            "public enum type must retain its deprecated annotation: {name}"
        );
        assert!(
            generated.contains("#[deprecated]\n    pub const Old:"),
            "public enum value must retain its deprecated annotation: {name}"
        );
        modules.push_str(&format!("pub mod {module} {{\n{generated}\n}}\n"));
        generated_enums.push(generated);
    }

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"deprecated-enum-strict-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        format!(
            r#"//! Strict consumer for deprecated enum implementations.
#![deny(warnings, clippy::all, clippy::pedantic, clippy::nursery, clippy::expect_used)]
{modules}

#[test]
#[allow(deprecated, clippy::expect_used, reason = "verify legacy enum behavior explicitly")]
fn deprecated_enum_behavior() {{
    use closed::DeprecatedClosedEnum as Closed;
    use open::DeprecatedOpenEnum as Open;

    assert_eq!(Open::default(), Open::Zero);
    assert_eq!(Open::One, Open::OneAlias);
    assert_eq!(Open::from(2), Open::Old);
    assert_eq!(i32::from(Open::OneAlias), 1);
    assert!(<Open as pbrs::Enum>::is_known(2));
    assert!(!<Open as pbrs::Enum>::is_known(-9));
    let unknown = Open::from(-9);
    assert_eq!(i32::from(unknown), -9);
    assert_eq!(format!("{{:?}}", Open::OneAlias), "DeprecatedOpenEnum::One");
    assert_eq!(format!("{{unknown:?}}"), "DeprecatedOpenEnum::from(-9)");
    assert_eq!(pbrs::AsView::as_view(&unknown), unknown);
    assert_eq!(pbrs::IntoView::into_view(unknown), unknown);

    assert_eq!(Closed::default(), Closed::Zero);
    assert_eq!(Closed::One, Closed::OneAlias);
    assert_eq!(Closed::try_from(2).expect("known legacy value"), Closed::Old);
    assert_eq!(i32::from(Closed::OneAlias), 1);
    assert!(<Closed as pbrs::Enum>::is_known(2));
    assert!(!<Closed as pbrs::Enum>::is_known(-9));
    let unknown = Closed::try_from(-9).expect_err("closed enum rejects unknown values");
    assert_eq!(unknown.value(), -9);
    assert_eq!(format!("{{:?}}", Closed::OneAlias), "DeprecatedClosedEnum::One");
    assert_eq!(pbrs::AsView::as_view(&Closed::One), Closed::One);
    assert_eq!(pbrs::IntoView::into_view(Closed::One), Closed::One);
}}
"#
        ),
    )
    .unwrap();

    for subcommand in ["clippy", "test", "doc"] {
        let mut command = shared_consumer_cargo();
        command
            .args([subcommand, "--offline", "--quiet"])
            .current_dir(&consumer);
        match subcommand {
            "clippy" => {
                command.args(["--all-targets", "--", "-D", "warnings"]);
            }
            "doc" => {
                command.arg("--no-deps").env("RUSTDOCFLAGS", "-D warnings");
            }
            _ => {}
        }
        let output = run_shared_consumer_cargo(&mut command).expect("check strict enum consumer");
        assert!(
            output.status.success(),
            "strict deprecated enum consumer {subcommand} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for generated in &generated_enums {
        let allowances = generated
            .lines()
            .zip(generated.lines().skip(1))
            .filter(|(attribute, _)| attribute.trim_start().starts_with("#[allow(deprecated,"))
            .collect::<Vec<_>>();
        assert_eq!(
            allowances.len(),
            10,
            "each generated enum impl needs its own allowance"
        );
        for (attribute, next) in allowances {
            assert!(attribute.contains("reason = "), "{attribute}");
            assert!(
                next.starts_with("impl"),
                "deprecated allowance escaped an impl: {next}"
            );
        }
    }

    // Check the public API from a separate crate so impl allowances cannot hide caller warnings.
    let caller = tmp.join("caller");
    std::fs::create_dir_all(caller.join("src")).unwrap();
    std::fs::write(
        caller.join("Cargo.toml"),
        "[package]\nname = \"deprecated-enum-external-caller\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nlegacy = { package = \"deprecated-enum-strict-consumer\", path = \"../consumer\" }\n",
    )
    .unwrap();
    for (module, name) in [
        ("open", "DeprecatedOpenEnum"),
        ("closed", "DeprecatedClosedEnum"),
    ] {
        std::fs::write(
            caller.join("src/lib.rs"),
            format!(
                "#![deny(deprecated)]\npub fn value() -> i32 {{ i32::from(legacy::{module}::{name}::OneAlias) }}\n"
            ),
        )
        .unwrap();
        let output = run_shared_consumer_cargo(
            shared_consumer_cargo()
                .args(["check", "--offline", "--quiet"])
                .current_dir(&caller),
        )
        .expect("check external caller deprecation diagnostics");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "external use of {name} must be deprecated"
        );
        assert!(stderr.contains("use of deprecated struct"), "{stderr}");
        assert!(stderr.contains(name), "{stderr}");
    }
}

#[test]
fn protoc_plugin_generated_native_kernel_compiles_under_strict_consumer_lint_policy() {
    let tmp = tempfile_dir_lints();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codegen-lints/lint_cases.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=kernel")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with stubs=kernel");
    assert!(status.success(), "protoc plugin failed");

    let generated = std::fs::read_to_string(tmp.join("lint_cases.rs")).expect("read lint_cases.rs");

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "native-strict-consumer"
version = "0.0.1"
edition = "2024"

[workspace]

[dependencies]
pbrs = {{ path = "{root}" }}
pbrs-grpc = {{ path = "{root}/pbrs-grpc" }}
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        consumer.join("src/lib.rs"),
        format!(
            r#"//! Strict consumer test for native kernel gRPC generation.
#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]

{generated}

/// Concrete implementation of [`KeywordService`].
/// Demonstrates implementing all 4 call shapes with keyword names,
/// while leaving optional methods (`non_standard__casing`) omitted.
pub struct MyNativeKeywordService;

impl KeywordService for MyNativeKeywordService {{
    // 1. Unary call shape with keyword name: r#type
    async fn r#type(
        &self,
        request: ::pbrs_grpc::Request<r#type>,
    ) -> ::core::result::Result<::pbrs_grpc::Response<r#match>, ::pbrs_grpc::Status> {{
        std::future::ready(()).await;
        let req = request.into_inner();
        let mut resp = r#match::new();
        resp.set_text(req.r#type());
        Ok(::pbrs_grpc::Response::new(resp))
    }}

    // 2. Client-streaming call shape with keyword name: r#match
    async fn r#match(
        &self,
        request: ::pbrs_grpc::Request<::pbrs_grpc::Streaming<r#type>>,
    ) -> ::core::result::Result<::pbrs_grpc::Response<r#match>, ::pbrs_grpc::Status> {{
        std::future::ready(()).await;
        drop(request);
        let mut resp = r#match::new();
        resp.set_text("client_stream");
        Ok(::pbrs_grpc::Response::new(resp))
    }}

    // 3. Server-streaming call shape with keyword name: r#fn
    async fn r#fn(
        &self,
        request: ::pbrs_grpc::Request<r#type>,
    ) -> ::core::result::Result<::pbrs_grpc::Response<::pbrs_grpc::Streaming<r#match>>, ::pbrs_grpc::Status> {{
        drop(request);
        let (tx, rx) = ::pbrs_grpc::Streaming::channel(4);
        let mut m = r#match::new();
        m.set_text("server_stream_item");
        let _ = tx.send(m).await;
        drop(tx);
        Ok(::pbrs_grpc::Response::new(rx))
    }}

    // 4. Bidirectional-streaming call shape with keyword name: r#struct
    async fn r#struct(
        &self,
        request: ::pbrs_grpc::Request<::pbrs_grpc::Streaming<r#type>>,
    ) -> ::core::result::Result<::pbrs_grpc::Response<::pbrs_grpc::Streaming<r#match>>, ::pbrs_grpc::Status> {{
        drop(request);
        let (tx, rx) = ::pbrs_grpc::Streaming::channel(4);
        let mut m = r#match::new();
        m.set_text("bidi_stream_item");
        let _ = tx.send(m).await;
        drop(tx);
        Ok(::pbrs_grpc::Response::new(rx))
    }}

    // Optional method `non_standard__casing` is omitted to verify default unimplemented answer!
}}

#[test]
fn test_native_server_and_client_instantiation() {{
    let svc = std::sync::Arc::new(MyNativeKeywordService);
    let server = KeywordServiceServer::from_arc(svc);
    assert_eq!(KeywordServiceServer::<MyNativeKeywordService>::NAME, "test.codegen_lints.KeywordService");
    let _ = server.clone();
}}
"#
        ),
    )
    .unwrap();

    let cargo_home = std::env::var("CARGO_HOME").ok();

    let mut clippy_cmd = shared_consumer_cargo();
    clippy_cmd
        .arg("clippy")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        clippy_cmd.env("CARGO_HOME", h);
    }
    let clippy_out = run_shared_consumer_cargo(&mut clippy_cmd).expect("run cargo clippy");
    assert!(
        clippy_out.status.success(),
        "cargo clippy on native kernel consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&clippy_out.stdout),
        String::from_utf8_lossy(&clippy_out.stderr)
    );

    let mut test_cmd = shared_consumer_cargo();
    test_cmd
        .arg("test")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        test_cmd.env("CARGO_HOME", h);
    }
    let test_out = run_shared_consumer_cargo(&mut test_cmd).expect("run cargo test");
    assert!(
        test_out.status.success(),
        "cargo test on native kernel consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

#[test]
fn protoc_plugin_generated_tonic_compiles_under_strict_consumer_lint_policy() {
    let tmp = tempfile_dir_lints();
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codegen-lints/lint_cases.proto");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("--pbrs_opt=stubs=tonic")
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("run protoc with stubs=tonic");
    assert!(status.success(), "protoc plugin failed");

    let generated = std::fs::read_to_string(tmp.join("lint_cases.rs")).expect("read lint_cases.rs");

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "tonic-strict-consumer"
version = "0.0.1"
edition = "2024"

[workspace]

[dependencies]
pbrs = {{ path = "{root}" }}
protobuf-tonic = {{ path = "{root}/protobuf-tonic" }}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net", "time", "sync"] }}
tokio-stream = {{ version = "0.1", features = ["net"] }}
tonic = {{ version = "0.14", default-features = false, features = ["transport", "codegen", "router", "gzip"] }}
http = "1"
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        consumer.join("src/lib.rs"),
        format!(
            r#"//! Strict consumer test for tonic gRPC generation.
#![deny(warnings)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]

{generated}

/// Concrete implementation of tonic [`KeywordService`].
pub struct MyTonicKeywordService;

impl KeywordService for MyTonicKeywordService {{
    type fnStream = tokio_stream::Iter<std::vec::IntoIter<Result<r#match, tonic::Status>>>;
    type structStream = tokio_stream::Iter<std::vec::IntoIter<Result<r#match, tonic::Status>>>;

    // 1. Unary call shape
    async fn r#type(&self, _request: tonic::Request<r#type>) -> Result<tonic::Response<r#match>, tonic::Status> {{
        tokio::task::yield_now().await;
        let mut m = r#match::new();
        m.set_text("unary");
        Ok(tonic::Response::new(m))
    }}

    // 2. Client-streaming call shape
    async fn r#match(&self, _request: tonic::Request<tonic::Streaming<r#type>>) -> Result<tonic::Response<r#match>, tonic::Status> {{
        tokio::task::yield_now().await;
        let mut m = r#match::new();
        m.set_text("client_streaming");
        Ok(tonic::Response::new(m))
    }}

    // 3. Server-streaming call shape
    async fn r#fn(&self, _request: tonic::Request<r#type>) -> Result<tonic::Response<Self::fnStream>, tonic::Status> {{
        tokio::task::yield_now().await;
        let mut m = r#match::new();
        m.set_text("server_streaming");
        Ok(tonic::Response::new(tokio_stream::iter(vec![Ok(m)])))
    }}

    // 4. Bidirectional-streaming call shape
    async fn r#struct(&self, _request: tonic::Request<tonic::Streaming<r#type>>) -> Result<tonic::Response<Self::structStream>, tonic::Status> {{
        tokio::task::yield_now().await;
        let mut m = r#match::new();
        m.set_text("bidi_streaming");
        Ok(tonic::Response::new(tokio_stream::iter(vec![Ok(m)])))
    }}

    // NonStandard_Casing
    async fn non_standard__casing(&self, _request: tonic::Request<snake_case_message>) -> Result<tonic::Response<UPPER_CASE_MESSAGE>, tonic::Status> {{
        tokio::task::yield_now().await;
        let mut resp = UPPER_CASE_MESSAGE::new();
        resp.set_ID("ok");
        Ok(tonic::Response::new(resp))
    }}
}}

#[tokio::test]
async fn test_tonic_service_instantiation() {{
    let svc = MyTonicKeywordService;
    let mut req_msg = r#type::new();
    req_msg.set_type("test");
    let resp = svc.r#type(tonic::Request::new(req_msg)).await.expect("unary");
    assert_eq!(resp.into_inner().text(), "unary");
}}
"#
        ),
    )
    .unwrap();

    let cargo_home = std::env::var("CARGO_HOME").ok();

    let mut clippy_cmd = shared_consumer_cargo();
    clippy_cmd
        .arg("clippy")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        clippy_cmd.env("CARGO_HOME", h);
    }
    let clippy_out = run_shared_consumer_cargo(&mut clippy_cmd).expect("run cargo clippy");
    assert!(
        clippy_out.status.success(),
        "cargo clippy on tonic consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&clippy_out.stdout),
        String::from_utf8_lossy(&clippy_out.stderr)
    );

    let mut test_cmd = shared_consumer_cargo();
    test_cmd
        .arg("test")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&consumer);
    if let Some(ref h) = cargo_home {
        test_cmd.env("CARGO_HOME", h);
    }
    let test_out = run_shared_consumer_cargo(&mut test_cmd).expect("run cargo test");
    assert!(
        test_out.status.success(),
        "cargo test on tonic consumer failed:\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&test_out.stdout),
        String::from_utf8_lossy(&test_out.stderr)
    );
}

fn hello_via_config(out_name: &str, configure: &dyn Fn(&mut pbrs::codegen::Config)) -> String {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(out_name);
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let proto_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    let mut cfg = pbrs::codegen::Config::new();
    configure(&mut cfg);
    cfg.out_dir(&tmp)
        .compile_protos(&[proto_dir.join("hello.proto")], &[&proto_dir])
        .expect("Config::compile_protos");
    std::fs::read_to_string(tmp.join("hello.rs")).expect("hello.rs from Config")
}

/// Replace the embedded `FILE_DESCRIPTOR_SET` byte literal with a placeholder.
///
/// protoc always attaches `SourceCodeInfo` to the `CodeGeneratorRequest`
/// descriptors it sends to plugins, while the `Config` path requests it only
/// with `include_source_info(true)`; the embedded reflection bytes therefore
/// legitimately differ between entry points even for identical options.
/// Everything outside that literal must still match exactly.
fn mask_embedded_fds(src: &str) -> String {
    let start_marker = "pub const FILE_DESCRIPTOR_SET: &[u8] = &[";
    let start = src
        .find(start_marker)
        .expect("generated code embeds FILE_DESCRIPTOR_SET");
    let end_rel = src[start..]
        .find("];")
        .expect("FILE_DESCRIPTOR_SET literal terminator");
    let end = start + end_rel + 2;
    format!("{}<masked-fds>{}", &src[..start], &src[end..])
}

fn configure_stubs_for_mode(cfg: &mut pbrs::codegen::Config, mode: &str) {
    match mode {
        "none" => {
            cfg.stubs(pbrs::codegen::Stubs::None);
        }
        "kernel" => {
            cfg.stubs(pbrs::codegen::Stubs::Kernel);
        }
        "tonic" => {
            cfg.emit_tonic_stubs(true);
        }
        other => panic!("unknown stub mode: {other}"),
    }
}

fn assert_stub_mode_markers(mode: &str, generated: &str) {
    match mode {
        "none" => {
            assert!(
                generated.contains("pub struct HelloRequest"),
                "messages-only must emit messages"
            );
            assert!(
                !generated.contains("GreeterClient"),
                "messages-only must not emit stubs"
            );
            assert!(
                !generated.contains("GreeterServer"),
                "messages-only must not emit stubs"
            );
        }
        "kernel" => {
            assert!(
                generated.contains("::pbrs_grpc::Channel"),
                "kernel mode must emit pbrs_grpc stubs"
            );
            assert!(
                generated.contains("pub struct GreeterClient"),
                "kernel mode must emit GreeterClient"
            );
            assert!(
                !generated.contains("ProtobufCodec"),
                "kernel mode must not emit tonic stubs"
            );
        }
        "tonic" => {
            assert!(
                generated.contains("ProtobufCodec"),
                "tonic mode must emit ProtobufCodec stubs"
            );
            assert!(
                generated.contains("pub struct GreeterClient"),
                "tonic mode must emit GreeterClient"
            );
            assert!(
                !generated.contains("::pbrs_grpc::Channel"),
                "tonic mode must not emit kernel stubs"
            );
        }
        other => panic!("unknown stub mode: {other}"),
    }
}

#[test]
fn config_and_plugin_inputs_produce_identical_output_per_mode() {
    // CG-02: messages / native-kernel / tonic modes must produce the same
    // output through equivalent Config builder and --pbrs_opt inputs.
    // Both sides use explicit selections, so ambient environment cannot skew
    // either side of the comparison.
    for mode in ["none", "kernel", "tonic"] {
        let opt = format!("stubs={mode}");
        let from_config = hello_via_config(&format!("plugin-equiv-config-{mode}"), &|c| {
            configure_stubs_for_mode(c, mode)
        });
        let from_plugin =
            generate_hello_with_options(&format!("plugin-equiv-plugin-{mode}"), Some(&opt), None)
                .expect("protoc --pbrs_opt");
        assert!(
            !from_config.is_empty() && !from_plugin.is_empty(),
            "{mode}: neither side may be empty"
        );
        assert_eq!(
            mask_embedded_fds(&from_config),
            mask_embedded_fds(&from_plugin),
            "{mode}: Config and --pbrs_opt={opt} must agree outside the entry-point reflection bytes"
        );
        assert_stub_mode_markers(mode, &from_config);
        assert_stub_mode_markers(mode, &from_plugin);
    }
}

#[test]
fn config_with_source_info_matches_plugin_bytes_exactly() {
    // CG-02: with equivalent descriptor inputs on both sides (protoc always
    // sends SourceCodeInfo to plugins; Config opts in via
    // include_source_info(true)), equivalent config inputs must produce
    // byte-identical output.
    let proto_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto");
    for (mode, opt, param) in [
        ("none", "stubs=none", "stubs=none,include_source_info=true"),
        (
            "kernel",
            "stubs=kernel",
            "stubs=kernel,include_source_info=true",
        ),
        (
            "tonic",
            "stubs=tonic",
            "stubs=tonic,include_source_info=true",
        ),
    ] {
        let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("plugin-equiv-si-config-{mode}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut cfg = pbrs::codegen::Config::new();
        configure_stubs_for_mode(&mut cfg, mode);
        cfg.include_source_info(true);
        cfg.out_dir(&tmp)
            .compile_protos(&[proto_dir.join("hello.proto")], &[&proto_dir])
            .expect("Config::compile_protos with source info");
        let from_config = std::fs::read_to_string(tmp.join("hello.rs")).expect("hello.rs");
        let from_plugin =
            generate_hello_with_options(&format!("plugin-equiv-si-plugin-{mode}"), Some(opt), None)
                .expect("protoc --pbrs_opt");
        assert_eq!(
            from_config, from_plugin,
            "{mode}: Config({param}) must match --pbrs_opt={opt} byte-for-byte"
        );
        assert_stub_mode_markers(mode, &from_config);
    }
}

#[test]
fn parallel_direct_calls_with_mixed_configs_are_isolated() {
    // CG-02: parallel mixed-config generation calls on multiple threads must
    // each observe exactly their own explicit configuration.
    let proto = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("proto/hello.proto");
    let tmp_fds = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("plugin-parallel-fds.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", tmp_fds.display()))
        .arg("-I")
        .arg(proto.parent().unwrap())
        .arg(&proto)
        .status()
        .expect("protoc fds");
    assert!(status.success());
    let fds_bytes = std::fs::read(&tmp_fds).expect("read fds");

    fn build_req(fds: &[u8], opt: Option<&str>) -> Vec<u8> {
        let mut req = Vec::new();
        // 1: file_to_generate = "hello.proto"
        req.push(0x0a);
        let f = b"hello.proto";
        req.push(f.len() as u8);
        req.extend_from_slice(f);
        if let Some(p) = opt {
            req.push(0x12);
            req.push(p.len() as u8);
            req.extend_from_slice(p.as_bytes());
        }
        // 15: proto_file
        let mut pos = 0;
        while pos < fds.len() {
            let (n, w) = pbrs::rt::decode_tag(fds, &mut pos).unwrap();
            if n == 1 && w == pbrs::rt::WIRE_LEN {
                let blob = pbrs::rt::read_len_bytes(fds, &mut pos).unwrap();
                pbrs::rt::encode_len_field(&mut req, 15, blob);
            } else {
                pbrs::rt::skip_field(fds, &mut pos, w).unwrap();
            }
        }
        req
    }

    let modes: &[(&str, Option<&str>)] = &[
        ("none", Some("stubs=none")),
        ("kernel", Some("stubs=kernel")),
        ("tonic", Some("stubs=tonic")),
        ("default", None),
    ];
    let requests: Vec<(&str, Vec<u8>)> = modes
        .iter()
        .map(|(m, o)| (*m, build_req(&fds_bytes, *o)))
        .collect();

    std::thread::scope(|s| {
        for (mode, req) in &requests {
            for _ in 0..4 {
                s.spawn(move || {
                    for _ in 0..5 {
                        let res = pbrs::codegen::generate_from_code_generator_request(req)
                            .expect("parallel generate");
                        let code = res
                            .iter()
                            .find(|(name, _)| name == "hello.rs")
                            .map(|(_, src)| src)
                            .expect("hello.rs in response");
                        match *mode {
                            "none" => {
                                assert!(
                                    !code.contains("GreeterClient"),
                                    "none leaked stubs under parallelism"
                                );
                                assert!(!code.contains("ProtobufCodec"));
                            }
                            "kernel" | "default" => {
                                assert!(
                                    code.contains("::pbrs_grpc::Channel"),
                                    "{mode} lost kernel stubs under parallelism"
                                );
                                assert!(
                                    !code.contains("ProtobufCodec"),
                                    "{mode} leaked tonic stubs under parallelism"
                                );
                            }
                            "tonic" => {
                                assert!(
                                    code.contains("ProtobufCodec"),
                                    "tonic lost tonic stubs under parallelism"
                                );
                                assert!(
                                    !code.contains("::pbrs_grpc::Channel"),
                                    "tonic leaked kernel stubs under parallelism"
                                );
                            }
                            other => panic!("unknown mode: {other}"),
                        }
                    }
                });
            }
        }
    });
}

fn edition2024_generated_fixture(name: &str) -> String {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edition2024/fds")
        .join(format!("{name}.fds"));
    let fds = std::fs::read(fixture).expect("checked Edition 2024 descriptor");
    let output = format!("{name}.rs");
    pbrs::codegen::generate_from_file_descriptor_set(&fds, &[format!("{name}.proto")])
        .expect("generate Edition 2024 descriptor directly")
        .into_iter()
        .find(|(path, _)| path == &output)
        .expect("generated fixture source")
        .1
}

#[test]
fn edition2024_map_decoder_uses_entry_utf8_feature() {
    let generated = edition2024_generated_fixture("defaults");
    let decoder = generated
        .split_once("fn decode_map_entry_DefaultMessage_map_field_13(")
        .expect("generated map decoder")
        .1;
    let decoder = decoder
        .split_once("\nfn ")
        .map_or(decoder, |(body, _)| body);
    assert!(
        decoder.contains("pbrs::rt::require_utf8(&data[s..e])?; key ="),
        "Edition 2024 map key must validate UTF-8 even though its outer field does not"
    );
}

#[test]
fn edition2024_extension_fields_remain_unknown_until_typed_api() {
    let generated = edition2024_generated_fixture("extensions");
    assert!(generated.contains("pub struct ExtendableMessage"));
    assert!(generated.contains("unknown: UnknownFields"));
    for extension in [
        "nested_scoped_extension",
        "ext_int32",
        "ext_string",
        "ext_repeated_int32",
        "ext_submessage",
        "ext_closed_enum",
        "ext_int32_with_default",
    ] {
        assert!(
            !generated.contains(&format!("pub fn {extension}(")),
            "unqualified Edition 2024 extension accessor {extension} was emitted"
        );
    }
}

fn selected_int32_extension_sources(
    targets: &[&str],
    fds: &[u8],
    names: &[&str],
) -> Vec<(String, String)> {
    let mut request = edition2024_plugin_request(targets, fds);
    let parameter = names
        .iter()
        .map(|name| format!("typed_extension={name}"))
        .collect::<Vec<_>>()
        .join(",");
    pbrs::rt::encode_len_field(&mut request, 2, parameter.as_bytes());
    pbrs::codegen::generate_from_code_generator_request(&request)
        .expect("selected singular Edition 2024 int32 extensions must generate")
}

#[test]
fn edition2024_selected_int32_extensions_generate_checked_identifiers() {
    let sources = selected_int32_extension_sources(
        &["extensions.proto"],
        include_bytes!("fixtures/edition2024/fds/extensions.fds"),
        &[
            "edition2024.extensions.ext_int32",
            "edition2024.extensions.ext_int32_with_default",
            "edition2024.extensions.ExtendableMessage.nested_scoped_extension",
        ],
    );
    let source = &sources
        .iter()
        .find(|(path, _)| path == "extensions.rs")
        .expect("checked extension output")
        .1;
    for identifier in [
        "EXT_INT32",
        "EXT_INT32_WITH_DEFAULT",
        "EXTENDABLE_MESSAGE_NESTED_SCOPED_EXTENSION",
    ] {
        assert!(source.contains(&format!("pub const {identifier}:")));
    }
    assert!(source.contains("pub fn get_extension<"));
    assert!(source.contains("unknown: UnknownFields"));
    assert!(!source.contains("pub fn ext_string("));
}

#[test]
fn edition2024_selected_int32_extensions_generate_original_identifiers() {
    let sources = selected_int32_extension_sources(
        &["rust/test/extensions.proto"],
        include_bytes!("fixtures/edition2024/fds/cg14_preview.fds"),
        &[
            "third_party_protobuf_rust_test.i32_extension",
            "third_party_protobuf_rust_test.i32_extension_with_default",
            "third_party_protobuf_rust_test.TestExtensions.nested_extension",
        ],
    );
    let source = &sources
        .iter()
        .find(|(path, _)| path == "rust/test/extensions.rs")
        .expect("original extension output")
        .1;
    for identifier in [
        "I32_EXTENSION",
        "I32_EXTENSION_WITH_DEFAULT",
        "TEST_EXTENSIONS_NESTED_EXTENSION",
    ] {
        assert!(source.contains(&format!("pub const {identifier}:")));
    }
    assert!(!source.contains("pub fn closed_enum_extension("));
}

fn typed_extension_result(
    targets: &[&str],
    fds: &[u8],
    parameter: &str,
) -> Result<Vec<(String, String)>, pbrs::codegen::CodegenError> {
    let mut request = edition2024_plugin_request(targets, fds);
    pbrs::rt::encode_len_field(&mut request, 2, parameter.as_bytes());
    pbrs::codegen::generate_from_code_generator_request(&request)
}

#[test]
fn edition2024_selected_extensions_reject_unsupported_kinds_and_scope() {
    let checked = include_bytes!("fixtures/edition2024/fds/extensions.fds");
    for name in [
        "ext_string",
        "ext_repeated_int32",
        "ext_submessage",
        "ext_closed_enum",
    ] {
        let parameter = format!("typed_extension=edition2024.extensions.{name}");
        let error = typed_extension_result(&["extensions.proto"], checked, &parameter).unwrap_err();
        assert!(
            matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
            if key == "typed_extension" && detail.contains("only singular int32")),
            "{error}"
        );
    }
    for parameter in [
        "typed_extension=missing.extension",
        "typed_extension=edition2024.extensions.ext_int32,extern_path=edition2024.extensions.ExtendableMessage=crate::Foreign",
    ] {
        let error = typed_extension_result(&["extensions.proto"], checked, parameter).unwrap_err();
        assert!(
            matches!(error, pbrs::codegen::CodegenError::InvalidParameter { key, .. } if key == "typed_extension")
        );
    }
    for parameter in [
        "typed_extension",
        "typed_extension=",
        "typed_extension=bad..name",
        "typed_extension=.bad-name",
    ] {
        let error = typed_extension_result(&["extensions.proto"], checked, parameter).unwrap_err();
        assert!(
            matches!(error, pbrs::codegen::CodegenError::InvalidParameter { key, .. } if key == "typed_extension")
        );
    }
}

fn scalar_extension_test_fds(
    edition: u64,
    number: u32,
    ordinary_field: Option<(&str, u32)>,
    nested_module: bool,
) -> Vec<u8> {
    let mut message = Vec::new();
    pbrs::rt::encode_len_field(&mut message, 1, b"Host");
    let mut range = Vec::new();
    for (tag, value) in [(1, 100), (2, 536_870_912)] {
        pbrs::rt::encode_tag(&mut range, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut range, value);
    }
    pbrs::rt::encode_len_field(&mut message, 5, &range);
    if let Some((name, number)) = ordinary_field {
        let mut field = Vec::new();
        pbrs::rt::encode_len_field(&mut field, 1, name.as_bytes());
        for (tag, value) in [(3, u64::from(number)), (4, 1), (5, 5)] {
            pbrs::rt::encode_tag(&mut field, tag, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(&mut field, value);
        }
        pbrs::rt::encode_len_field(&mut message, 2, &field);
    }
    let mut extension = Vec::new();
    pbrs::rt::encode_len_field(&mut extension, 1, b"value");
    pbrs::rt::encode_len_field(&mut extension, 2, b".test.Host");
    for (tag, value) in [(3, u64::from(number)), (4, 1), (5, 5)] {
        pbrs::rt::encode_tag(&mut extension, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut extension, value);
    }
    let mut file = Vec::new();
    pbrs::rt::encode_len_field(&mut file, 1, b"scalar.proto");
    pbrs::rt::encode_len_field(&mut file, 2, b"test");
    pbrs::rt::encode_len_field(&mut file, 4, &message);
    pbrs::rt::encode_len_field(&mut file, 7, &extension);
    if nested_module {
        let mut outer = Vec::new();
        let mut inner = Vec::new();
        pbrs::rt::encode_len_field(&mut outer, 1, b"Extensions");
        pbrs::rt::encode_len_field(&mut inner, 1, b"Nested");
        pbrs::rt::encode_len_field(&mut outer, 3, &inner);
        pbrs::rt::encode_len_field(&mut file, 4, &outer);
    }
    if edition == 0 {
        pbrs::rt::encode_len_field(&mut file, 12, b"proto2");
    } else {
        pbrs::rt::encode_len_field(&mut file, 12, b"editions");
        pbrs::rt::encode_tag(&mut file, 14, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut file, edition);
    }
    let mut fds = Vec::new();
    pbrs::rt::encode_len_field(&mut fds, 1, &file);
    fds
}

#[test]
fn edition2024_selected_extensions_reject_accessor_and_namespace_collisions() {
    for field in [
        "extension",
        "get_extension",
        "has_extension",
        "set_extension",
        "clear_extension",
    ] {
        let fds = scalar_extension_test_fds(1001, 101, Some((field, 1)), false);
        let error = typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value")
            .unwrap_err();
        assert!(
            matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
            if key == "typed_extension" && detail.contains("accessor collides")),
            "{field}: {error}"
        );
    }
    let fds = scalar_extension_test_fds(1001, 101, None, true);
    let error =
        typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").unwrap_err();
    assert!(
        matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
        if key == "typed_extension" && detail.contains("nested module collides")),
        "{error}"
    );
}

#[test]
fn edition2024_selected_extensions_check_only_emitted_service_namespace() {
    let fds = scalar_extension_test_fds(1001, 101, None, false);
    let mut pos = 0;
    assert_eq!(
        pbrs::rt::decode_tag(&fds, &mut pos).unwrap(),
        (1, pbrs::rt::WIRE_LEN)
    );
    let mut file = pbrs::rt::read_len_bytes(&fds, &mut pos).unwrap().to_vec();
    let mut naming = Vec::new();
    pbrs::rt::encode_tag(&mut naming, 7, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut naming, 2); // STYLE_LEGACY allows lowercase service names.
    let mut options = Vec::new();
    pbrs::rt::encode_len_field(&mut options, 50, &naming);
    pbrs::rt::encode_len_field(&mut file, 8, &options);
    let mut service = Vec::new();
    pbrs::rt::encode_len_field(&mut service, 1, b"extensions");
    pbrs::rt::encode_len_field(&mut file, 6, &service);
    let mut fds = Vec::new();
    pbrs::rt::encode_len_field(&mut fds, 1, &file);
    for stubs in ["kernel", "tonic", "compat"] {
        let parameter = format!("typed_extension=test.value,stubs={stubs}");
        let error = typed_extension_result(&["scalar.proto"], &fds, &parameter).unwrap_err();
        assert!(
            matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
            if key == "typed_extension" && detail.contains("service trait collides")),
            "{error}"
        );
        for suffix in [
            ",build_server=false",
            ",extern_path=test.extensions=crate::Foreign",
        ] {
            typed_extension_result(&["scalar.proto"], &fds, &(parameter.clone() + suffix)).unwrap();
        }
        // Without a selection the former service output remains permitted.
        typed_extension_result(&["scalar.proto"], &fds, &format!("stubs={stubs}")).unwrap();
    }
    typed_extension_result(
        &["scalar.proto"],
        &fds,
        "typed_extension=test.value,stubs=none",
    )
    .unwrap();
}

fn scalar_fds_with_second_extension(name: &str) -> Vec<u8> {
    let fds = scalar_extension_test_fds(1001, 101, None, false);
    let mut pos = 0;
    assert_eq!(
        pbrs::rt::decode_tag(&fds, &mut pos).unwrap(),
        (1, pbrs::rt::WIRE_LEN)
    );
    let mut file = pbrs::rt::read_len_bytes(&fds, &mut pos).unwrap().to_vec();
    let mut extension = Vec::new();
    pbrs::rt::encode_len_field(&mut extension, 1, name.as_bytes());
    pbrs::rt::encode_len_field(&mut extension, 2, b".test.Host");
    for (tag, value) in [(3, 101), (4, 1), (5, 5)] {
        pbrs::rt::encode_tag(&mut extension, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut extension, value);
    }
    pbrs::rt::encode_len_field(&mut file, 7, &extension);
    let mut result = Vec::new();
    pbrs::rt::encode_len_field(&mut result, 1, &file);
    result
}

fn scalar_test_file(fds: &[u8]) -> Vec<u8> {
    let mut pos = 0;
    assert_eq!(
        pbrs::rt::decode_tag(fds, &mut pos).unwrap(),
        (1, pbrs::rt::WIRE_LEN)
    );
    let file = pbrs::rt::read_len_bytes(fds, &mut pos).unwrap().to_vec();
    assert_eq!(pos, fds.len());
    file
}

fn scalar_test_descriptor_set(files: &[Vec<u8>]) -> Vec<u8> {
    let mut fds = Vec::new();
    for file in files {
        pbrs::rt::encode_len_field(&mut fds, 1, file);
    }
    fds
}

fn scalar_test_without_tag(bytes: &[u8], removed: u32) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let start = pos;
        let (number, wire) = pbrs::rt::decode_tag(bytes, &mut pos).unwrap();
        pbrs::rt::skip_field(bytes, &mut pos, wire).unwrap();
        if number != removed {
            output.extend_from_slice(&bytes[start..pos]);
        }
    }
    output
}

fn scalar_test_set_len(bytes: &[u8], number: u32, payload: &[u8]) -> Vec<u8> {
    let mut output = scalar_test_without_tag(bytes, number);
    pbrs::rt::encode_len_field(&mut output, number, payload);
    output
}

fn scalar_test_set_varint(bytes: &[u8], number: u32, value: u64) -> Vec<u8> {
    let mut output = scalar_test_without_tag(bytes, number);
    pbrs::rt::encode_tag(&mut output, number, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut output, value);
    output
}

fn scalar_test_rewrite_len(
    bytes: &[u8],
    rewritten: u32,
    mut rewrite: impl FnMut(&[u8]) -> Vec<u8>,
) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;
    let mut count = 0;
    while pos < bytes.len() {
        let start = pos;
        let (number, wire) = pbrs::rt::decode_tag(bytes, &mut pos).unwrap();
        if number == rewritten {
            assert_eq!(wire, pbrs::rt::WIRE_LEN);
            let payload = pbrs::rt::read_len_bytes(bytes, &mut pos).unwrap();
            pbrs::rt::encode_len_field(&mut output, number, &rewrite(payload));
            count += 1;
        } else {
            pbrs::rt::skip_field(bytes, &mut pos, wire).unwrap();
            output.extend_from_slice(&bytes[start..pos]);
        }
    }
    assert_ne!(count, 0, "synthetic mutation must reach its descriptor");
    output
}

fn scalar_test_field(name: &str, extendee: &str, number: u32, ty: u64, label: u64) -> Vec<u8> {
    let mut field = Vec::new();
    pbrs::rt::encode_len_field(&mut field, 1, name.as_bytes());
    if !extendee.is_empty() {
        pbrs::rt::encode_len_field(&mut field, 2, extendee.as_bytes());
    }
    for (tag, value) in [(3, u64::from(number)), (4, label), (5, ty)] {
        pbrs::rt::encode_tag(&mut field, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut field, value);
    }
    field
}

fn scalar_test_invalid_selection(targets: &[&str], fds: &[u8], parameter: &str, reason: &str) {
    let error = typed_extension_result(targets, fds, parameter).unwrap_err();
    assert!(
        matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
        if key == "typed_extension" && detail.contains(reason)),
        "{reason}: {error}"
    );
}

struct TypedExtensionConsumerLock {
    bytes: Vec<u8>,
    accepted: Option<std::path::PathBuf>,
}

fn typed_extension_registry_tuples(
    lock: &[u8],
) -> std::collections::BTreeSet<(String, String, String, String)> {
    std::str::from_utf8(lock)
        .unwrap()
        .split("[[package]]")
        .filter_map(|block| {
            let value = |key: &str| {
                block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(key))
                    .map(str::to_owned)
            };
            let source = value("source = ")?;
            Some((
                value("name = ").expect("locked provider package name"),
                value("version = ").expect("locked provider package version"),
                source,
                value("checksum = ").unwrap_or_default(),
            ))
        })
        .collect()
}

#[test]
fn typed_extension_lock_tuple_guard_preserves_registry_identity() {
    let provider = "version = 4\n[[package]]\nname = \"bytes\"\nversion = \"1.11.1\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"recorded-checksum\"\n";
    let tuples = typed_extension_registry_tuples(provider.as_bytes());
    assert_eq!(tuples.len(), 1);
    for (old, new) in [
        ("bytes", "another-package"),
        ("1.11.1", "1.11.2"),
        ("registry+", "git+"),
        ("recorded-checksum", "different-checksum"),
    ] {
        assert!(
            !typed_extension_registry_tuples(provider.replace(old, new).as_bytes())
                .is_subset(&tuples),
            "provider drift was accepted: {old} -> {new}"
        );
    }
    let local = "[[package]]\nname = \"typed-int32-consumer\"\nversion = \"0.0.0\"\n";
    assert!(typed_extension_registry_tuples(local.as_bytes()).is_empty());
}

fn typed_extension_consumer_seed_lock(
    root: &std::path::Path,
    consumer: &std::path::Path,
    name: &str,
) -> TypedExtensionConsumerLock {
    let root_lock = std::fs::read(root.join("Cargo.lock")).unwrap();
    let mut seed = root_lock.clone();
    seed.extend_from_slice(
        format!(
            "\n[[package]]\nname = {name:?}\nversion = \"0.0.0\"\ndependencies = [\n \"pbrs\",\n]\n"
        )
        .as_bytes(),
    );
    let accepted_key = match name {
        "typed-int32-consumer" => "PBRS_TYPED_EXTENSION_ACCEPTED_CONSUMER_LOCK",
        "typed-int32-alias-consumer" => "PBRS_TYPED_EXTENSION_ACCEPTED_ALIAS_LOCK",
        _ => panic!("unexpected typed-extension consumer name: {name}"),
    };
    let accepted = std::env::var_os(accepted_key).map(PathBuf::from);
    let mut expected = accepted
        .as_ref()
        .map_or_else(|| seed.clone(), |path| std::fs::read(path).unwrap());
    let path = consumer.join("Cargo.lock");
    if path.exists() {
        // Never replace an existing lock, including a retained 1.85 replay.
        if accepted.is_some() {
            assert_eq!(
                std::fs::read(&path).unwrap(),
                expected,
                "requested consumer lock differs from the exact accepted handoff"
            );
        }
    } else {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(&expected).unwrap();
    }
    std::fs::write(consumer.join("Cargo.lock.root-source"), &root_lock).unwrap();
    std::fs::write(consumer.join("Cargo.lock.seed"), &seed).unwrap();
    if accepted.is_none() {
        // Standard invocation explicitly prepares the graph before the locked
        // test. The qualified path never runs this setup operation.
        let records = consumer.join("lock-setup-records");
        std::fs::create_dir_all(&records).unwrap();
        let mut index = 0u32;
        let record = loop {
            let directory = records.join(format!("run-{index:04}"));
            match std::fs::create_dir(&directory) {
                Ok(()) => break directory,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    index = index.checked_add(1).expect("lock setup index overflow");
                }
                result => result.unwrap(),
            }
        };
        std::fs::write(record.join("root-source.Cargo.lock"), &root_lock).unwrap();
        std::fs::write(record.join("raw-seed.Cargo.lock"), seed).unwrap();
        std::fs::write(
            record.join("before.Cargo.lock"),
            std::fs::read(&path).unwrap(),
        )
        .unwrap();
        let mut metadata = shared_consumer_cargo();
        metadata
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_INCREMENTAL", "0")
            .args(["metadata", "--offline", "--format-version=1"])
            .current_dir(consumer);
        std::fs::write(
            record.join("metadata.command.txt"),
            format!("{metadata:?}\npolicy: explicit offline setup; actual test stays --locked\n"),
        )
        .unwrap();
        let result = run_shared_consumer_cargo(&mut metadata).unwrap();
        std::fs::write(record.join("metadata.stdout"), &result.stdout).unwrap();
        std::fs::write(record.join("metadata.stderr"), &result.stderr).unwrap();
        std::fs::write(
            record.join("metadata.exit"),
            format!("{:?}\n", result.status.code()),
        )
        .unwrap();
        expected = std::fs::read(&path).unwrap();
        std::fs::write(record.join("after.Cargo.lock"), &expected).unwrap();
        assert_eq!(
            std::fs::read(root.join("Cargo.lock")).unwrap(),
            root_lock,
            "root provider changed during explicit metadata setup"
        );
        assert!(
            result.status.success(),
            "offline child graph preparation failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::write(record.join("accepted.Cargo.lock"), &expected).unwrap();
    }
    assert!(
        typed_extension_registry_tuples(&expected)
            .is_subset(&typed_extension_registry_tuples(&root_lock)),
        "child selected a version/source/checksum outside the frozen root provider"
    );
    std::fs::write(consumer.join("Cargo.lock.before"), &expected).unwrap();
    if accepted.is_some() {
        std::fs::write(consumer.join("Cargo.lock.accepted"), &expected).unwrap();
    }
    TypedExtensionConsumerLock {
        bytes: expected,
        accepted,
    }
}

fn typed_extension_consumer_assert_lock(
    consumer: &std::path::Path,
    expected: &TypedExtensionConsumerLock,
    label: &str,
) {
    let actual = std::fs::read(consumer.join("Cargo.lock")).unwrap();
    std::fs::write(consumer.join(format!("Cargo.lock.after-{label}")), &actual).unwrap();
    assert_eq!(actual, expected.bytes, "consumer lock changed during {label}");
    if let Some(path) = &expected.accepted {
        assert_eq!(
            std::fs::read(path).unwrap(),
            expected.bytes,
            "immutable accepted lock changed during {label}"
        );
    }
}

#[test]
fn edition2024_selected_extensions_validate_int32_default_syntax_and_bounds() {
    let file = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    for (text, expected) in [
        ("0", 0),
        ("-0", 0),
        ("+0", 0),
        ("010", 8),
        ("-010", -8),
        ("+0x2a", 42),
        ("-0X2A", -42),
        ("2147483647", i32::MAX),
        ("+2147483647", i32::MAX),
        ("-2147483648", i32::MIN),
        ("0x7fffffff", i32::MAX),
        ("+0X7FFFFFFF", i32::MAX),
        ("-0x80000000", i32::MIN),
        ("017777777777", i32::MAX),
        ("-020000000000", i32::MIN),
    ] {
        let changed = scalar_test_rewrite_len(&file, 7, |field| {
            scalar_test_set_len(field, 7, text.as_bytes())
        });
        let fds = scalar_test_descriptor_set(&[changed]);
        let output =
            typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").unwrap();
        let source = &output
            .iter()
            .find(|(name, _)| name == "scalar.rs")
            .unwrap()
            .1;
        let compact: String = source.split_whitespace().collect();
        assert!(
            compact.contains(&format!("Extension::__new(101,\"test.value\",{expected})")),
            "{text}: {source}"
        );
    }
    for text in [
        "",
        "+",
        "-",
        "++1",
        "+-1",
        "-+1",
        "--1",
        "0x",
        "0X",
        "0x+1",
        "0x-1",
        "+0x-1",
        "08",
        "018",
        "0b10",
        "1.0",
        " 1",
        "1 ",
        "2147483648",
        "-2147483649",
        "0x80000000",
        "-0x80000001",
        "020000000000",
        "-020000000001",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        let changed = scalar_test_rewrite_len(&file, 7, |field| {
            scalar_test_set_len(field, 7, text.as_bytes())
        });
        let fds = scalar_test_descriptor_set(&[changed]);
        // The existing pool retains the literal. The selected-only guard owns
        // the numeric rejection; an unrelated descriptor parse error is wrong.
        let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
        assert_eq!(
            pool.get_extension("test.value")
                .unwrap()
                .1
                .default
                .as_deref(),
            Some(text)
        );
        scalar_test_invalid_selection(
            &["scalar.proto"],
            &fds,
            "typed_extension=test.value",
            "int32 default is invalid",
        );
    }
}

#[test]
fn edition2024_selected_extensions_reject_cross_file_and_nonrequested_hosts() {
    let original = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    let host = scalar_test_set_len(&scalar_test_without_tag(&original, 7), 1, b"host.proto");
    let mut declaration =
        scalar_test_set_len(&scalar_test_without_tag(&original, 4), 1, b"cross.proto");
    pbrs::rt::encode_len_field(&mut declaration, 3, b"host.proto");
    let fds = scalar_test_descriptor_set(&[host, declaration]);
    let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
    let (host, field) = pool.get_extension("test.value").unwrap();
    assert_eq!(host.file_name, "host.proto");
    assert_eq!(
        pool.file_for_extension(&host.full_name, field.number),
        Some("cross.proto")
    );
    scalar_test_invalid_selection(
        &["host.proto", "cross.proto"],
        &fds,
        "typed_extension=test.value",
        "cross-file extendees are not supported",
    );
    typed_extension_result(&["host.proto", "cross.proto"], &fds, "").unwrap();

    let ignored = scalar_test_set_len(
        &scalar_test_without_tag(&scalar_test_without_tag(&original, 4), 7),
        1,
        b"ignored.proto",
    );
    let fds = scalar_test_descriptor_set(&[original, ignored]);
    typed_extension_result(&["ignored.proto"], &fds, "").unwrap();
    scalar_test_invalid_selection(
        &["ignored.proto"],
        &fds,
        "typed_extension=test.value",
        "host must be an owned generated target type",
    );
    typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").unwrap();
}

#[test]
fn edition2024_selected_extensions_reject_required_delimited_messageset_and_maps() {
    let original = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    let required =
        scalar_test_rewrite_len(&original, 7, |field| scalar_test_set_varint(field, 4, 2));
    let fds = scalar_test_descriptor_set(&[required]);
    // LABEL_REQUIRED is already invalid Edition 2024, before opt-in selection.
    assert!(pbrs::DescriptorPool::from_file_descriptor_set(&fds).is_err());
    assert!(matches!(
        typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").unwrap_err(),
        pbrs::codegen::CodegenError::MalformedDescriptor { .. }
    ));
    let legacy_required = scalar_test_rewrite_len(&original, 7, |field| {
        let features = scalar_test_set_varint(&[], 1, 3); // LEGACY_REQUIRED presence.
        scalar_test_set_len(field, 8, &scalar_test_set_len(&[], 21, &features))
    });
    let fds = scalar_test_descriptor_set(&[legacy_required]);
    let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
    assert_eq!(
        pool.get_extension("test.value").unwrap().1.cardinality,
        pbrs::Cardinality::Required
    );
    scalar_test_invalid_selection(
        &["scalar.proto"],
        &fds,
        "typed_extension=test.value",
        "only singular int32",
    );
    for (option, reason) in [
        (1, "MessageSet is not supported"),
        (7, "host must be an owned generated target type"),
    ] {
        let options = scalar_test_set_varint(&[], option, 1);
        let changed = scalar_test_rewrite_len(&original, 4, |message| {
            scalar_test_set_len(message, 7, &options)
        });
        let fds = scalar_test_descriptor_set(&[changed]);
        let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
        let host = pool.get_message("test.Host").unwrap();
        assert_eq!(host.message_set_wire_format, option == 1);
        assert_eq!(host.is_map_entry, option == 7);
        scalar_test_invalid_selection(
            &["scalar.proto"],
            &fds,
            "typed_extension=test.value",
            reason,
        );
    }
    let mut payload = Vec::new();
    pbrs::rt::encode_len_field(&mut payload, 1, b"Payload");
    let mut delimited = original.clone();
    pbrs::rt::encode_len_field(&mut delimited, 4, &payload);
    let features = scalar_test_set_varint(&[], 5, 2); // DELIMITED message encoding.
    let options = scalar_test_set_len(&[], 21, &features);
    let delimited = scalar_test_rewrite_len(&delimited, 7, |field| {
        let field = scalar_test_set_varint(field, 5, 11);
        let field = scalar_test_set_len(&field, 6, b".test.Payload");
        scalar_test_set_len(&field, 8, &options)
    });
    let fds = scalar_test_descriptor_set(&[delimited]);
    let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
    assert!(pool.get_extension("test.value").unwrap().1.delimited);
    scalar_test_invalid_selection(
        &["scalar.proto"],
        &fds,
        "typed_extension=test.value",
        "only singular int32",
    );

    let mut entry = Vec::new();
    pbrs::rt::encode_len_field(&mut entry, 1, b"Entry");
    pbrs::rt::encode_len_field(&mut entry, 7, &scalar_test_set_varint(&[], 7, 1));
    for field in [
        scalar_test_field("key", "", 1, 9, 1),
        scalar_test_field("value", "", 2, 5, 1),
    ] {
        pbrs::rt::encode_len_field(&mut entry, 2, &field);
    }
    let mut map = original;
    pbrs::rt::encode_len_field(&mut map, 4, &entry);
    let map = scalar_test_rewrite_len(&map, 7, |field| {
        let field = scalar_test_set_varint(field, 5, 11);
        let field = scalar_test_set_varint(&field, 4, 3);
        scalar_test_set_len(&field, 6, b".test.Entry")
    });
    let fds = scalar_test_descriptor_set(&[map]);
    let pool = pbrs::DescriptorPool::from_file_descriptor_set(&fds).unwrap();
    assert!(pool.get_extension("test.value").unwrap().1.is_map);
    scalar_test_invalid_selection(
        &["scalar.proto"],
        &fds,
        "typed_extension=test.value",
        "only singular int32",
    );
}

#[test]
fn edition2024_selected_extensions_reject_multiple_owners_and_constant_collisions() {
    let original = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    let other = scalar_test_set_len(&original, 1, b"other.proto");
    let other =
        scalar_test_rewrite_len(&other, 4, |host| scalar_test_set_len(host, 1, b"OtherHost"));
    let other = scalar_test_rewrite_len(&other, 7, |field| {
        let field = scalar_test_set_len(field, 1, b"other_value");
        scalar_test_set_len(&field, 2, b".test.OtherHost")
    });
    let fds = scalar_test_descriptor_set(&[original.clone(), other]);
    for selection in ["test.value", "test.other_value"] {
        typed_extension_result(
            &["scalar.proto", "other.proto"],
            &fds,
            &format!("typed_extension={selection}"),
        )
        .unwrap();
    }
    scalar_test_invalid_selection(
        &["scalar.proto", "other.proto"],
        &fds,
        "typed_extension=test.value,typed_extension=test.other_value",
        "multiple selected owner files",
    );
    let collision = scalar_test_rewrite_len(&original, 7, |field| {
        scalar_test_set_len(field, 1, b"host_nested_value")
    });
    let collision = scalar_test_rewrite_len(&collision, 4, |host| {
        let mut host = host.to_vec();
        pbrs::rt::encode_len_field(
            &mut host,
            6,
            &scalar_test_field("nested_value", ".test.Host", 102, 5, 1),
        );
        host
    });
    let fds = scalar_test_descriptor_set(&[collision]);
    for selection in ["test.host_nested_value", "test.Host.nested_value"] {
        let output = typed_extension_result(
            &["scalar.proto"],
            &fds,
            &format!("typed_extension={selection}"),
        )
        .unwrap();
        assert!(
            output
                .iter()
                .any(|(_, source)| source.contains("pub const HOST_NESTED_VALUE:"))
        );
    }
    scalar_test_invalid_selection(
        &["scalar.proto"],
        &fds,
        "typed_extension=test.host_nested_value,typed_extension=test.Host.nested_value",
        "identifier HOST_NESTED_VALUE collides",
    );
}

fn scalar_test_imported_service(public: bool) -> Vec<u8> {
    let mut owner = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    pbrs::rt::encode_len_field(&mut owner, 3, b"imported.proto");
    if public {
        owner = scalar_test_set_varint(&owner, 10, 0);
    }
    let mut imported = scalar_test_file(&scalar_extension_test_fds(1001, 101, None, false));
    imported = scalar_test_without_tag(&scalar_test_without_tag(&imported, 4), 7);
    imported = scalar_test_set_len(&imported, 1, b"imported.proto");
    imported = scalar_test_set_len(&imported, 2, b"imported");
    let legacy = scalar_test_set_varint(&[], 7, 2);
    imported = scalar_test_set_len(&imported, 8, &scalar_test_set_len(&[], 50, &legacy));
    let service = scalar_test_set_len(&[], 1, b"extensions");
    pbrs::rt::encode_len_field(&mut imported, 6, &service);
    scalar_test_descriptor_set(&[owner, imported])
}

#[test]
fn edition2024_selected_extensions_reject_only_emitted_imported_service_collisions() {
    let public = scalar_test_imported_service(true);
    let private = scalar_test_imported_service(false);
    for stubs in ["kernel", "tonic", "compat"] {
        let parameter = format!("typed_extension=test.value,stubs={stubs}");
        scalar_test_invalid_selection(
            &["scalar.proto"],
            &public,
            &parameter,
            "service trait collides",
        );
        typed_extension_result(&["scalar.proto"], &private, &parameter).unwrap();
        typed_extension_result(&["scalar.proto"], &public, &format!("stubs={stubs}")).unwrap();
        for suffix in [
            ",build_server=false",
            ",extern_path=imported.extensions=crate::Foreign",
        ] {
            typed_extension_result(&["scalar.proto"], &public, &(parameter.clone() + suffix))
                .unwrap();
        }
    }
    typed_extension_result(
        &["scalar.proto"],
        &public,
        "typed_extension=test.value,stubs=none",
    )
    .unwrap();
}

#[test]
fn edition2024_selected_extensions_custom_runtime_alias_generated_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let retained = std::env::var_os("PBRS_TYPED_EXTENSION_CONSUMER_DIR");
    let consumer = retained.as_ref().map_or_else(
        || {
            root.join("target")
                .join(format!("typed-int32-alias-consumer-{}", std::process::id()))
        },
        |directory| PathBuf::from(directory).join("alias"),
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let fds = scalar_extension_test_fds(1001, 101, None, false);
    let default =
        typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").unwrap();
    let default = &default
        .iter()
        .find(|(name, _)| name == "scalar.rs")
        .unwrap()
        .1;
    for (name, alias) in [("relative.rs", "my_pbrs"), ("absolute.rs", "::my_pbrs")] {
        let parameter = format!("typed_extension=test.value,runtime_crate={alias}");
        let output = typed_extension_result(&["scalar.proto"], &fds, &parameter).unwrap();
        let source = &output
            .iter()
            .find(|(name, _)| name == "scalar.rs")
            .unwrap()
            .1;
        assert_eq!(source, &default.replace("pbrs::", &format!("{alias}::")));
        assert!(source.contains(&format!("{alias}::Extension<")));
        assert!(source.contains(&format!("{alias}::ExtensionHost")));
        std::fs::write(consumer.join("src").join(name), source).unwrap();
    }
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname=\"typed-int32-alias-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nmy_pbrs={{package=\"pbrs\",path={:?}}}\n", root.to_str().unwrap()
    )).unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        r#"
pub mod relative { include!("relative.rs"); }
pub mod absolute { include!("absolute.rs"); }

#[test]
fn renamed_runtime_typed_extensions_round_trip() {
    use my_pbrs::{Parse, Serialize};
    let mut relative = relative::Host::new();
    assert_eq!(relative.get_extension(&relative::extensions::VALUE), 0);
    assert!(!relative.has_extension(&relative::extensions::VALUE));
    relative.set_extension(&relative::extensions::VALUE, -7);
    let wire = relative.serialize().unwrap();
    assert_eq!(relative.serialized_len(), wire.len());
    let mut absolute = absolute::Host::parse(&wire).unwrap();
    assert!(absolute.has_extension(&absolute::extensions::VALUE));
    assert_eq!(absolute.get_extension(&absolute::extensions::VALUE), -7);
    absolute.clear_extension(&absolute::extensions::VALUE);
    assert!(!absolute.has_extension(&absolute::extensions::VALUE));
    assert_eq!(absolute.serialize().unwrap(), [0_u8; 0]);
    absolute.set_extension(&absolute::extensions::VALUE, 0);
    assert!(absolute.has_extension(&absolute::extensions::VALUE));
    let parsed = relative::Host::parse(&absolute.serialize().unwrap()).unwrap();
    assert!(parsed.has_extension(&relative::extensions::VALUE));
    assert_eq!(parsed.get_extension(&relative::extensions::VALUE), 0);
}
"#,
    )
    .unwrap();
    let expected_lock =
        typed_extension_consumer_seed_lock(&root, &consumer, "typed-int32-alias-consumer");
    let mut command = shared_consumer_cargo();
    command
        .env("CARGO_BUILD_JOBS", "1")
        .args([
            "test",
            "--offline",
            "--locked",
            "--quiet",
            "--lib",
            "--",
            "--test-threads=1",
        ])
        .current_dir(&consumer);
    let result = run_shared_consumer_cargo(&mut command).unwrap();
    if retained.is_some() {
        std::fs::write(consumer.join("test.command.txt"), format!("{command:?}\n")).unwrap();
        std::fs::write(consumer.join("test.stdout"), &result.stdout).unwrap();
        std::fs::write(consumer.join("test.stderr"), &result.stderr).unwrap();
        std::fs::write(
            consumer.join("test.exit"),
            format!("{:?}\n", result.status.code()),
        )
        .unwrap();
    }
    typed_extension_consumer_assert_lock(&consumer, &expected_lock, "test");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed; 0 failed"));
    if retained.is_none() {
        std::fs::remove_dir_all(consumer).unwrap();
    }
}

#[test]
fn edition2024_selected_extensions_reject_hidden_descriptor_collisions() {
    for fds in [
        scalar_extension_test_fds(1001, 99, None, false),
        scalar_extension_test_fds(1001, 101, Some(("ordinary", 101)), false),
        scalar_fds_with_second_extension("other"),
        scalar_fds_with_second_extension("value"),
    ] {
        // Characterize the existing pool's overwrite behavior, without changing
        // no-option parsing/generation for these malformed synthetic schemas.
        pbrs::codegen::generate_from_file_descriptor_set(&fds, &["scalar.proto".into()]).unwrap();
        let error = typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value")
            .unwrap_err();
        assert!(
            matches!(error, pbrs::codegen::CodegenError::InvalidParameter { key, .. } if key == "typed_extension")
        );
    }
    let fds = scalar_fds_with_second_extension("other");
    let error =
        typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.other").unwrap_err();
    assert!(
        matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
        if key == "typed_extension" && detail.contains("collides")),
        "{error}"
    );
}

#[test]
fn edition2024_selected_extensions_reject_illegal_tags_and_other_editions() {
    for tag in [0, 19_000, 19_999, 536_870_912] {
        let fds = scalar_extension_test_fds(1001, tag, None, false);
        assert!(
            typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value").is_err(),
            "illegal {tag}"
        );
    }
    for edition in [0, 1000] {
        let fds = scalar_extension_test_fds(edition, 101, None, false);
        let error = typed_extension_result(&["scalar.proto"], &fds, "typed_extension=test.value")
            .unwrap_err();
        assert!(
            matches!(&error, pbrs::codegen::CodegenError::InvalidParameter { key, detail }
            if key == "typed_extension" && detail.contains("only Edition 2024")),
            "{error}"
        );
    }
}

#[test]
fn edition2024_selected_extensions_deduplicate_and_preserve_default_output() {
    let fds = include_bytes!("fixtures/edition2024/fds/extensions.fds");
    let direct =
        pbrs::codegen::generate_from_file_descriptor_set(fds, &["extensions.proto".into()])
            .unwrap();
    assert_eq!(
        direct,
        typed_extension_result(&["extensions.proto"], fds, "").unwrap()
    );
    let once = typed_extension_result(
        &["extensions.proto"],
        fds,
        "typed_extension=edition2024.extensions.ext_int32",
    )
    .unwrap();
    let twice = typed_extension_result(&["extensions.proto"], fds, "typed_extension=.edition2024.extensions.ext_int32,typed_extension=edition2024.extensions.ext_int32").unwrap();
    assert_eq!(once, twice);
}

#[test]
fn edition2024_selected_extensions_builder_matches_parameter_and_rejects_injection() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let descriptor = root.join("tests/fixtures/edition2024/fds/extensions.fds");
    let output = tempfile_dir();
    pbrs::codegen::Config::new()
        .out_dir(&output)
        .emit_rerun_if_changed(false)
        .typed_extension(".edition2024.extensions.ext_int32")
        .compile_descriptor_set(&descriptor, &["extensions.proto"], &[] as &[PathBuf])
        .unwrap();
    let expected = typed_extension_result(
        &["extensions.proto"],
        include_bytes!("fixtures/edition2024/fds/extensions.fds"),
        "typed_extension=edition2024.extensions.ext_int32",
    )
    .unwrap();
    for (path, source) in expected {
        assert_eq!(std::fs::read_to_string(output.join(path)).unwrap(), source);
    }
    let error = pbrs::codegen::Config::new()
        .out_dir(output.join("invalid"))
        .emit_rerun_if_changed(false)
        .typed_extension("edition2024.extensions.ext_int32,stubs=none")
        .compile_descriptor_set(&descriptor, &["extensions.proto"], &[] as &[PathBuf])
        .unwrap_err();
    assert!(
        matches!(error, pbrs::codegen::CodegenError::InvalidParameter { key, .. } if key == "typed_extension")
    );
    assert!(!output.join("invalid").exists());
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn edition2024_selected_int32_extensions_generated_consumer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let retained = std::env::var_os("PBRS_TYPED_EXTENSION_CONSUMER_DIR");
    let consumer = retained.as_ref().map_or_else(
        || {
            root.join("target")
                .join(format!("typed-int32-consumer-{}", std::process::id()))
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let default = edition2024_generated_fixture("extensions");
    let checked = selected_int32_extension_sources(
        &["extensions.proto"],
        include_bytes!("fixtures/edition2024/fds/extensions.fds"),
        &[
            "edition2024.extensions.ext_int32",
            "edition2024.extensions.ext_int32_with_default",
            "edition2024.extensions.ExtendableMessage.nested_scoped_extension",
        ],
    );
    let original = selected_int32_extension_sources(
        &["rust/test/extensions.proto"],
        include_bytes!("fixtures/edition2024/fds/cg14_preview.fds"),
        &[
            "third_party_protobuf_rust_test.i32_extension",
            "third_party_protobuf_rust_test.i32_extension_with_default",
            "third_party_protobuf_rust_test.TestExtensions.nested_extension",
        ],
    );
    for (file, source) in [
        ("default_checked.rs", default),
        (
            "checked.rs",
            checked
                .into_iter()
                .find(|(path, _)| path == "extensions.rs")
                .unwrap()
                .1,
        ),
        (
            "original.rs",
            original
                .into_iter()
                .find(|(path, _)| path == "rust/test/extensions.rs")
                .unwrap()
                .1,
        ),
    ] {
        std::fs::write(consumer.join("src").join(file), source).unwrap();
    }
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname=\"typed-int32-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\npbrs={{path={:?}}}\n", root.to_str().unwrap()
    )).unwrap();
    let source = include_str!("fixtures/edition2024/typed_int32_consumer.rs")
        .replace("@ROOT@", root.to_str().unwrap());
    std::fs::write(consumer.join("src/lib.rs"), source).unwrap();
    let expected_lock =
        typed_extension_consumer_seed_lock(&root, &consumer, "typed-int32-consumer");
    let mut command = shared_consumer_cargo();
    command
        .env("CARGO_BUILD_JOBS", "1")
        .args([
            "test",
            "--offline",
            "--locked",
            "--quiet",
            "--lib",
            "--",
            "--test-threads=1",
        ])
        .current_dir(&consumer);
    let result = run_shared_consumer_cargo(&mut command).unwrap();
    if retained.is_some() {
        std::fs::write(consumer.join("test.command.txt"), format!("{command:?}\n")).unwrap();
        std::fs::write(consumer.join("test.stdout"), &result.stdout).unwrap();
        std::fs::write(consumer.join("test.stderr"), &result.stderr).unwrap();
        std::fs::write(
            consumer.join("test.exit"),
            format!("{:?}\n", result.status.code()),
        )
        .unwrap();
    }
    typed_extension_consumer_assert_lock(&consumer, &expected_lock, "test");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("7 passed; 0 failed"));
    std::fs::create_dir_all(consumer.join("src/bin")).unwrap();
    std::fs::write(consumer.join("src/bin/wrong_host.rs"),
        "fn main() { let value = typed_int32_consumer::original::TestExtensions::new(); let _ = value.get_extension(&typed_int32_consumer::checked::extensions::EXT_INT32); }\n"
    ).unwrap();
    let mut command = shared_consumer_cargo();
    command
        .env("CARGO_BUILD_JOBS", "1")
        .args([
            "check",
            "--offline",
            "--locked",
            "--quiet",
            "--bin",
            "wrong_host",
        ])
        .current_dir(&consumer);
    let wrong = run_shared_consumer_cargo(&mut command).unwrap();
    if retained.is_some() {
        std::fs::write(
            consumer.join("wrong-host.command.txt"),
            format!("{command:?}\n"),
        )
        .unwrap();
        std::fs::write(consumer.join("wrong-host.stdout"), &wrong.stdout).unwrap();
        std::fs::write(consumer.join("wrong-host.stderr"), &wrong.stderr).unwrap();
        std::fs::write(
            consumer.join("wrong-host.exit"),
            format!("{:?}\n", wrong.status.code()),
        )
        .unwrap();
    }
    typed_extension_consumer_assert_lock(&consumer, &expected_lock, "wrong-host");
    assert!(!wrong.status.success(), "wrong-host identifier compiled");
    let error = String::from_utf8_lossy(&wrong.stderr);
    assert!(
        error.contains("E0308")
            && error.contains("ExtendableMessage")
            && error.contains("TestExtensions"),
        "{error}"
    );
    std::fs::remove_file(consumer.join("src/bin/wrong_host.rs")).unwrap();
    if retained.is_none() {
        std::fs::remove_dir_all(consumer).unwrap();
    }
}

#[test]
fn edition2024_original_shared_extension_suite_is_empty_at_pin() {
    // CG-14 original shared-test evidence. The pinned upstream shared suite
    // has no runnable Edition 2024 extension cases: at v35.1
    // rust/test/shared/extensions_test.rs is a license-only stub (the vendored
    // copy below is byte-identical to third_party/protobuf at the pinned SHA).
    // The applicable original evidence is therefore the Edition 2024
    // rust/test/extensions.proto schema itself: every declared extension must
    // resolve in the checked preview descriptor set.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stub =
        std::fs::read_to_string(root.join("vendor/google/rust-tests/shared/extensions_test.rs"))
            .expect("vendored original extensions_test.rs");
    assert!(
        !stub.contains("#[test]")
            && !stub.contains("fn ")
            && !stub.contains("gtest")
            && !stub.contains("test_"),
        "pinned original extensions_test.rs gained runnable cases: {stub}"
    );
    let preview = std::fs::read(root.join("tests/fixtures/edition2024/fds/cg14_preview.fds"))
        .expect("checked Edition 2024 supplemental descriptor set");
    let pool = pbrs::DescriptorPool::from_file_descriptor_set(&preview)
        .expect("checked preview descriptor resolves");
    assert_eq!(
        pool.file_edition("rust/test/extensions.proto"),
        Some(1001),
        "original extension schema must be Edition 2024"
    );
    let host = "third_party_protobuf_rust_test.TestExtensions";
    assert!(
        pool.get_message(host).is_some(),
        "missing original extension host {host}"
    );
    assert_eq!(
        pool.extension_numbers_of(host),
        vec![
            1, 2, 11, 12, 13, 14, 15, 16, 20, 30, 31, 100, 500, 801, 802, 803, 811, 812, 813, 814,
            815, 816, 830
        ],
        "every original extension number must resolve"
    );
    for (full, number) in [
        ("third_party_protobuf_rust_test.i32_extension", 1),
        ("third_party_protobuf_rust_test.str_extension", 2),
        ("third_party_protobuf_rust_test.submessage_extension", 20),
        (
            "third_party_protobuf_rust_test.repeated_enum_extension",
            830,
        ),
        ("third_party_protobuf_rust_test.closed_enum_extension", 31),
        (
            "third_party_protobuf_rust_test.TestExtensions.nested_extension",
            500,
        ),
    ] {
        let (desc, field) = pool
            .get_extension(full)
            .unwrap_or_else(|| panic!("missing original extension {full}"));
        assert_eq!(desc.full_name, host);
        assert_eq!(field.number, number, "wrong number for {full}");
        assert_eq!(
            pool.file_for_extension(host, number),
            Some("rust/test/extensions.proto"),
            "wrong declaring file for {full}"
        );
    }
}

#[test]
#[ignore = "requires protoc 36.1 for Edition 2024 source syntax; run explicitly with the pinned source compiler"]
fn edition2024_rejected_source_fixtures_fail_in_protoc() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rejected = root.join("tests/fixtures/edition2024/rejected");
    let output = root.join("target/plugin-edition2024-rejected.fds");
    for (name, diagnostic) in [
        ("import_weak", "weak import is not supported"),
        ("ctype_option", "ctype option is not allowed"),
        (
            "java_multiple_files",
            "java_multiple_files has been removed",
        ),
        ("group_syntax", "Group syntax is no longer supported"),
        ("optional_keyword", "Label \"optional\" is not supported"),
        ("required_keyword", "Label \"required\" is not supported"),
        ("naming_style", "Message name badName should"),
        ("visibility_import_local", "is not visible"),
    ] {
        let result = Command::new("protoc")
            .arg("-I")
            .arg(&root)
            .arg("-I")
            .arg(&rejected)
            .arg(format!("--descriptor_set_out={}", output.display()))
            .arg(rejected.join(format!("{name}.proto")))
            .output()
            .expect("run installed protoc on rejected fixture");
        assert!(!result.status.success(), "{name} unexpectedly compiled");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(diagnostic),
            "{name} did not fail for its approved reason: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
#[ignore = "requires pinned libprotoc 36.1 to regenerate the checked Edition 2024 preview FDS"]
fn edition2024_preview_descriptor_matches_pinned_source_compiler() {
    let version = Command::new("protoc")
        .arg("--version")
        .output()
        .expect("run pinned protoc");
    assert!(version.status.success(), "pinned protoc version failed");
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "libprotoc 36.1"
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = root.join("target").join(format!(
        "plugin-edition2024-preview-{}.fds",
        std::process::id()
    ));
    let result = Command::new("protoc")
        .arg("-I")
        .arg(root.join("tests/fixtures/edition2024/proto"))
        .arg("-I")
        .arg(root.join("vendor/google"))
        .arg("--retain_options")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", output.display()))
        .args([
            "legacy_style.proto",
            "maps.proto",
            "maps_none.proto",
            "rust/test/extensions.proto",
        ])
        .output()
        .expect("regenerate pinned preview FDS");
    assert!(
        result.status.success(),
        "preview descriptor regeneration failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let fresh = std::fs::read(&output).expect("regenerated descriptor");
    assert!(
        fresh == include_bytes!("fixtures/edition2024/fds/cg14_preview.fds"),
        "checked CG-14 preview FDS differs from pinned source compilation"
    );
    std::fs::remove_file(output).expect("clean preview descriptor scratch");
}

#[test]
fn edition2024_direct_generation_rejects_unknown_feature_and_bad_name() {
    let mut file = Vec::new();
    pbrs::rt::encode_len_field(&mut file, 1, b"invalid.proto");
    pbrs::rt::encode_len_field(&mut file, 12, b"editions");
    pbrs::rt::encode_tag(&mut file, 14, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut file, 1001);
    let mut feature = Vec::new();
    pbrs::rt::encode_tag(&mut feature, 9, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut feature, 1);
    let mut options = Vec::new();
    pbrs::rt::encode_len_field(&mut options, 50, &feature);
    pbrs::rt::encode_len_field(&mut file, 8, &options);
    let mut fds = Vec::new();
    pbrs::rt::encode_len_field(&mut fds, 1, &file);
    assert!(
        matches!(
            pbrs::codegen::generate_from_file_descriptor_set(&fds, &["invalid.proto".into()]),
            Err(pbrs::codegen::CodegenError::MalformedDescriptor { .. })
        ),
        "unknown 2024 feature must not produce generated code"
    );

    let mut name_file = Vec::new();
    pbrs::rt::encode_len_field(&mut name_file, 1, b"invalid.proto");
    pbrs::rt::encode_len_field(&mut name_file, 12, b"editions");
    pbrs::rt::encode_tag(&mut name_file, 14, pbrs::rt::WIRE_VARINT);
    pbrs::rt::encode_varint(&mut name_file, 1001);
    let mut message = Vec::new();
    pbrs::rt::encode_len_field(&mut message, 1, b"badName");
    pbrs::rt::encode_len_field(&mut name_file, 4, &message);
    let mut fds = Vec::new();
    pbrs::rt::encode_len_field(&mut fds, 1, &name_file);
    assert!(
        matches!(
            pbrs::codegen::generate_from_file_descriptor_set(&fds, &["invalid.proto".into()]),
            Err(pbrs::codegen::CodegenError::MalformedDescriptor { .. })
        ),
        "invalid inherited 2024 naming must fail before generation"
    );
}

fn response_maximum_edition(response: &[u8]) -> Option<u64> {
    let mut pos = 0;
    let mut maximum = None;
    while pos < response.len() {
        let (number, wire) = pbrs::rt::decode_tag(response, &mut pos).unwrap();
        if number == 4 {
            assert_eq!(wire, pbrs::rt::WIRE_VARINT);
            maximum = Some(pbrs::rt::decode_varint(response, &mut pos).unwrap());
        } else {
            pbrs::rt::skip_field(response, &mut pos, wire).unwrap();
        }
    }
    maximum
}

#[test]
fn edition2024_plugin_cap_is_2024() {
    assert_eq!(
        response_maximum_edition(&pbrs::codegen::encode_code_generator_response(&[])),
        Some(1001)
    );
    assert_eq!(
        response_maximum_edition(&pbrs::codegen::encode_code_generator_response_error(
            "unsupported"
        )),
        Some(1001)
    );
}

#[test]
fn edition2024_checked_generated_consumer_compiles_under_rust_2024_lints() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let consumer = root.join("target").join(format!(
        "plugin-edition2024-consumer-{}",
        std::process::id()
    ));
    if consumer.exists() {
        std::fs::remove_dir_all(&consumer).expect("clean prior consumer scratch");
    }
    std::fs::create_dir_all(&consumer).expect("create consumer scratch");
    let names = [
        "defaults",
        "overrides",
        "inheritance",
        "visibility",
        "extensions",
    ];
    let mut fds = Vec::new();
    for name in names {
        fds.extend(
            std::fs::read(
                root.join("tests/fixtures/edition2024/fds")
                    .join(format!("{name}.fds")),
            )
            .expect("checked Edition 2024 descriptor"),
        );
    }
    let preview = std::fs::read(root.join("tests/fixtures/edition2024/fds/cg14_preview.fds"))
        .expect("checked Edition 2024 supplemental descriptor set");
    let preview_pool = pbrs::DescriptorPool::from_file_descriptor_set(&preview)
        .expect("checked preview descriptor resolves without a source compiler");
    assert_eq!(
        preview_pool.file_naming_style("legacy_style.proto"),
        Some(2)
    );
    assert_eq!(
        preview_pool.symbol_naming_style("edition2024.legacy.badName.nestedName"),
        Some(2)
    );
    for (message, validate) in [
        ("edition2024.map_cases.Maps", true),
        ("edition2024.map_none.Maps", false),
    ] {
        let map = preview_pool
            .get_message(message)
            .expect("checked map message");
        let entry = map
            .field(1)
            .and_then(|field| field.message.as_ref())
            .expect("checked map entry");
        assert_eq!(entry.field(1).expect("string key").utf8_validate, validate);
        assert_eq!(
            entry.field(2).expect("string value").utf8_validate,
            validate
        );
    }
    assert!(
        preview_pool
            .get_message("third_party_protobuf_rust_test.TestExtensions")
            .is_some()
    );
    fds.extend(preview);
    let mut targets: Vec<_> = names.iter().map(|name| format!("{name}.proto")).collect();
    targets.push("rust/test/extensions.proto".into());
    targets.push("legacy_style.proto".into());
    targets.push("maps.proto".into());
    targets.push("maps_none.proto".into());
    let generated = pbrs::codegen::generate_from_file_descriptor_set(&fds, &targets)
        .expect("generate checked Edition 2024 fixtures and original extensions");

    let src_dir = consumer.join("src");
    std::fs::create_dir_all(&src_dir).expect("create consumer scratch");
    for (name, source) in generated {
        let path = src_dir.join(name);
        std::fs::create_dir_all(path.parent().expect("generated parent")).expect("output parent");
        std::fs::write(path, source).expect("write generated fixture");
    }
    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"edition2024-consumer\"\nversion = \"0.0.1\"\nedition = \"2024\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display()
        ),
    )
    .expect("consumer manifest");
    let consumer_source = r###"#![deny(warnings)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery)]
include!("mod.rs");

#[cfg(test)]
mod checks {
    use super::edition2024;
    use super::third_party_protobuf_rust_test;

    #[test]
    fn checked_defaults_and_utf8_map() -> Result<(), Box<dyn std::error::Error>> {
        let zero = include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/defaults_zero_set.bin");
        let parsed = <edition2024::defaults::DefaultMessage as pbrs::Parse>::parse(zero)?;
        assert!(parsed.has_int32_field());
        assert!(parsed.has_bool_field());
        assert!(parsed.has_string_field());
        assert_eq!(pbrs::Serialize::serialize(&parsed)?.as_slice(), zero);

        let full = include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/defaults_populated.bin");
        let parsed = <edition2024::defaults::DefaultMessage as pbrs::Parse>::parse(full)?;
        assert_eq!(pbrs::Serialize::serialize(&parsed)?.as_slice(), full);
        let open = edition2024::defaults::DefaultEnum::from(99);
        assert_eq!(i32::from(open), 99);
        let invalid_key = [0x6a, 0x05, 0x0a, 0x01, 0xff, 0x10, 0x01];
        assert!(
            <edition2024::defaults::DefaultMessage as pbrs::Parse>::parse(&invalid_key).is_err(),
            "Edition 2024 map keys must validate UTF-8 during parse"
        );
        Ok(())
    }

    #[test]
    fn checked_overrides_and_inheritance() -> Result<(), Box<dyn std::error::Error>> {
        let mut value = edition2024::overrides::OverridesMessage::new();
        value.set_required_int32(1);
        value.set_implicit_int32(0);
        assert_eq!(pbrs::Serialize::serialize(&value)?, [0x10, 0x01]);
        value.set_implicit_int32(42);
        let mut expected = include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_implicit_set.bin").to_vec();
        expected.extend([0x10, 0x01]);
        assert_eq!(pbrs::Serialize::serialize(&value)?, expected);

        let mut expanded = edition2024::overrides::OverridesMessage::new();
        expanded.set_required_int32(1);
        expanded.expanded_int32_mut().push(10);
        expanded.expanded_int32_mut().push(20);
        expanded.expanded_int32_mut().push(30);
        let mut expected = vec![0x10, 0x01];
        expected.extend(include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_expanded_repeated.bin"));
        assert_eq!(pbrs::Serialize::serialize(&expanded)?, expected);

        let mut child = edition2024::overrides::SubDelimited::new();
        child.set_text("hello");
        let mut delimited = edition2024::overrides::OverridesMessage::new();
        delimited.set_required_int32(1);
        delimited.set_delimited_message(child);
        let mut expected = vec![0x10, 0x01];
        expected.extend(include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_delimited_message.bin"));
        assert_eq!(pbrs::Serialize::serialize(&delimited)?, expected);
        assert!(<edition2024::overrides::OverridesMessage as pbrs::Parse>::parse(&[]).is_err());
        let unknown_closed = [0x10, 0x01, 0x38, 0x63];
        let parsed = <edition2024::overrides::OverridesMessage as pbrs::Parse>::parse(&unknown_closed)?;
        assert!(!parsed.has_closed_enum());
        assert_eq!(pbrs::Serialize::serialize(&parsed)?, unknown_closed);
        let known_closed = [0x10, 0x01, 0x38, 0x01];
        let parsed = <edition2024::overrides::OverridesMessage as pbrs::Parse>::parse(&known_closed)?;
        assert!(parsed.has_closed_enum());
        assert_eq!(i32::from(parsed.closed_enum()), 1);
        assert_eq!(pbrs::Serialize::serialize(&parsed)?, known_closed);

        let mut inherited = edition2024::inheritance::FileDefaultsConsumer::new();
        inherited.set_file_implicit_int(0);
        assert!(pbrs::Serialize::serialize(&inherited)?.is_empty());
        let mut explicit = edition2024::inheritance::FieldLevelOverrides::new();
        explicit.set_explicit_int(0);
        assert_eq!(pbrs::Serialize::serialize(&explicit)?, [0x08, 0x00]);
        assert!(<edition2024::inheritance::FieldLevelOverrides as pbrs::Parse>::parse(&[0x1a, 0x01, 0xff]).is_err());
        Ok(())
    }

    #[test]
    fn checked_visibility_and_extensions() -> Result<(), Box<dyn std::error::Error>> {
        let pool = pbrs::DescriptorPool::from_file_descriptor_set(
            edition2024::visibility::FILE_DESCRIPTOR_SET
        )?;
        assert_eq!(pool.effective_symbol_visibility("edition2024.visibility.DefaultTopLevelMessage"), Some(2));
        assert_eq!(pool.effective_symbol_visibility("edition2024.visibility.DefaultTopLevelMessage.DefaultNestedLocalMessage"), Some(1));
        assert_eq!(pool.effective_symbol_visibility("edition2024.visibility.DefaultTopLevelMessage.ExportedNestedMessage"), Some(2));
        let outer = edition2024::visibility::DefaultTopLevelMessage::new();
        assert_eq!(outer.id(), 0);

        let wire = include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/extensions_populated.bin");
        let typed = <edition2024::extensions::ExtendableMessage as pbrs::Parse>::parse(wire)?;
        assert_eq!(pbrs::Serialize::serialize(&typed)?.as_slice(), wire);
        let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(
            edition2024::extensions::FILE_DESCRIPTOR_SET
        )?);
        let desc = pool.get_message("edition2024.extensions.ExtendableMessage").ok_or("missing extension host")?;
        let dynamic = pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), wire)?;
        assert_eq!(dynamic.get_extension(101), Some(&pbrs::Value::Int32(101)));
        assert_eq!(dynamic.get_extension(105), Some(&pbrs::Value::Enum(1)));
        Ok(())
    }

    #[test]
    fn inherited_legacy_naming_compiles() -> Result<(), Box<dyn std::error::Error>> {
        let mut value = edition2024::legacy::badName::new();
        value.set_BadField("Ada");
        let bytes = pbrs::Serialize::serialize(&value)?;
        let parsed = <edition2024::legacy::badName as pbrs::Parse>::parse(&bytes)?;
        assert_eq!(parsed.BadField(), "Ada");
        let pool = pbrs::DescriptorPool::from_file_descriptor_set(
            edition2024::legacy::FILE_DESCRIPTOR_SET
        )?;
        assert_eq!(pool.symbol_naming_style("edition2024.legacy.badName"), Some(2));
        assert_eq!(pool.symbol_naming_style("edition2024.legacy.badName.nestedName"), Some(2));
        Ok(())
    }

    #[test]
    fn map_key_and_value_validate_entry_utf8() -> Result<(), Box<dyn std::error::Error>> {
        type Maps = edition2024::map_cases::Maps;
        let bad_key = [0x0a, 0x06, 0x0a, 0x01, 0xff, 0x12, 0x01, b'v'];
        let bad_value = [0x0a, 0x06, 0x0a, 0x01, b'k', 0x12, 0x01, 0xff];
        for invalid in [bad_key, bad_value] {
            assert!(<Maps as pbrs::Parse>::parse(&invalid).is_err());
        }
        let valid = [0x0a, 0x06, 0x0a, 0x01, b'k', 0x12, 0x01, b'v'];
        let parsed = <Maps as pbrs::Parse>::parse(&valid)?;
        assert_eq!(parsed.strings().len(), 1);
        assert_eq!(pbrs::Serialize::serialize(&parsed)?, valid);
        Ok(())
    }

    #[test]
    fn inherited_map_utf8_none_accepts_unverified_entries() -> Result<(), Box<dyn std::error::Error>> {
        type Maps = edition2024::map_none::Maps;
        let bad_key = [0x0a, 0x06, 0x0a, 0x01, 0xff, 0x12, 0x01, b'v'];
        let bad_value = [0x0a, 0x06, 0x0a, 0x01, b'k', 0x12, 0x01, 0xff];
        for wire in [bad_key, bad_value] {
            let parsed = <Maps as pbrs::Parse>::parse(&wire)?;
            assert_eq!(pbrs::Serialize::serialize(&parsed)?, wire);
        }
        Ok(())
    }

    #[test]
    fn original_shared_extension_schema_preserves_wire() -> Result<(), Box<dyn std::error::Error>> {
        let wire = [0x08, 0x2a, 0xa0, 0x1f, 0x07];
        let host = <third_party_protobuf_rust_test::TestExtensions as pbrs::Parse>::parse(&wire)?;
        assert_eq!(pbrs::Serialize::serialize(&host)?, wire);
        let sub = third_party_protobuf_rust_test::SimpleSubmessage::new();
        assert_eq!(sub.i32_field(), 123);
        Ok(())
    }

    fn typed_serialize<T>(wire: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>>
    where
        T: pbrs::Parse + pbrs::Serialize,
    {
        let parsed = <T as pbrs::Parse>::parse(wire)?;
        Ok(pbrs::Serialize::serialize(&parsed)?)
    }

    fn dynamic_serialize(
        fds: &[u8],
        message: &str,
        wire: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(fds)?);
        let desc = pool.get_message(message).ok_or("missing differential message")?;
        let parsed = pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), wire)?;
        Ok(pbrs::Serialize::serialize(&parsed)?)
    }

    #[test]
    fn pinned_vectors_agree_between_typed_and_dynamic() -> Result<(), Box<dyn std::error::Error>> {
        // Overrides vectors need the required-field prefix in canonical
        // field-number order before they parse.
        let implicit = [
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_implicit_set.bin")
                .as_slice(),
            &[0x10, 0x01][..],
        ]
        .concat();
        let expanded = [
            &[0x10, 0x01][..],
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_expanded_repeated.bin")
                .as_slice(),
        ]
        .concat();
        let delimited = [
            &[0x10, 0x01][..],
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/overrides_delimited_message.bin")
                .as_slice(),
        ]
        .concat();
        let ext_wire: &[u8] =
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/extensions_populated.bin");

        // Every pinned vector must survive a typed parse/serialize and a
        // dynamic parse/serialize byte-identically.
        let zero: &[u8] =
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/defaults_zero_set.bin");
        assert_eq!(typed_serialize::<edition2024::defaults::DefaultMessage>(zero)?.as_slice(), zero);
        assert_eq!(
            dynamic_serialize(
                edition2024::defaults::FILE_DESCRIPTOR_SET,
                "edition2024.defaults.DefaultMessage",
                zero
            )?.as_slice(),
            zero
        );
        let full: &[u8] =
            include_bytes!("@ROOT@/tests/fixtures/edition2024/bin/defaults_populated.bin");
        assert_eq!(typed_serialize::<edition2024::defaults::DefaultMessage>(full)?.as_slice(), full);
        assert_eq!(
            dynamic_serialize(
                edition2024::defaults::FILE_DESCRIPTOR_SET,
                "edition2024.defaults.DefaultMessage",
                full
            )?.as_slice(),
            full
        );
        assert_eq!(
            typed_serialize::<edition2024::overrides::OverridesMessage>(&implicit)?.as_slice(),
            implicit.as_slice()
        );
        assert_eq!(
            dynamic_serialize(
                edition2024::overrides::FILE_DESCRIPTOR_SET,
                "edition2024.overrides.OverridesMessage",
                &implicit
            )?.as_slice(),
            implicit.as_slice()
        );
        assert_eq!(
            typed_serialize::<edition2024::overrides::OverridesMessage>(&expanded)?.as_slice(),
            expanded.as_slice()
        );
        assert_eq!(
            dynamic_serialize(
                edition2024::overrides::FILE_DESCRIPTOR_SET,
                "edition2024.overrides.OverridesMessage",
                &expanded
            )?.as_slice(),
            expanded.as_slice()
        );
        assert_eq!(
            typed_serialize::<edition2024::overrides::OverridesMessage>(&delimited)?.as_slice(),
            delimited.as_slice()
        );
        assert_eq!(
            dynamic_serialize(
                edition2024::overrides::FILE_DESCRIPTOR_SET,
                "edition2024.overrides.OverridesMessage",
                &delimited
            )?.as_slice(),
            delimited.as_slice()
        );
        assert_eq!(
            typed_serialize::<edition2024::extensions::ExtendableMessage>(ext_wire)?.as_slice(),
            ext_wire
        );
        assert_eq!(
            dynamic_serialize(
                edition2024::extensions::FILE_DESCRIPTOR_SET,
                "edition2024.extensions.ExtendableMessage",
                ext_wire
            )?.as_slice(),
            ext_wire
        );

        // Typed and dynamic views of the populated extensions agree field by field.
        let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(
            edition2024::extensions::FILE_DESCRIPTOR_SET,
        )?);
        let desc = pool
            .get_message("edition2024.extensions.ExtendableMessage")
            .ok_or("missing extension host")?;
        let dynamic = pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), ext_wire)?;
        assert_eq!(dynamic.get_singular(1), Some(&pbrs::Value::Int32(1)));
        assert_eq!(dynamic.get_extension(101), Some(&pbrs::Value::Int32(101)));
        assert_eq!(
            dynamic.get_extension(102),
            Some(&pbrs::Value::String("ext".into()))
        );
        assert_eq!(
            dynamic.get_repeated(103),
            Some([pbrs::Value::Int32(7), pbrs::Value::Int32(8)].as_slice())
        );
        assert!(
            matches!(dynamic.get_extension(104), Some(pbrs::Value::Message(_))),
            "missing ext_submessage"
        );
        assert_eq!(dynamic.get_extension(105), Some(&pbrs::Value::Enum(1)));
        assert_eq!(dynamic.get_extension(106), Some(&pbrs::Value::Int32(99)));
        Ok(())
    }

    #[test]
    fn original_extensions_preserve_all_wire_types() -> Result<(), Box<dyn std::error::Error>> {
        // One field of every original extension kind in canonical order:
        // singular scalars, strings, submessage, defaults, packed repeated
        // scalars, open and closed enums, and the nested-scoped extension.
        let wire: &[u8] = &[
            0x08, 0xAC, 0x02, // 1: i32_extension = 300
            0x12, 0x03, b'f', b'o', b'o', // 2: str_extension = "foo"
            0x58, 0xAC, 0x02, // 11: i64_extension = 300
            0x60, 0xAC, 0x02, // 12: u32_extension = 300
            0x68, 0xAC, 0x02, // 13: u64_extension = 300
            0x75, 0x00, 0x00, 0xC0, 0x3F, // 14: f32_extension = 1.5
            0x79, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F, // 15: f64 = 1.5
            0x80, 0x01, 0x01, // 16: bool_extension = true
            0xA2, 0x01, 0x02, 0x08, 0x7B, // 20: submessage_extension { i32_field: 123 }
            0xF0, 0x01, 0x01, // 30: enum_extension = BAR
            0xF8, 0x01, 0x01, // 31: closed_enum_extension = CLOSED_ONE
            0xA0, 0x06, 0x2A, // 100: i32_extension_with_default = 42
            0xA0, 0x1F, 0x07, // 500: nested_extension = 7
            0x8A, 0x32, 0x03, 0x07, 0xAC, 0x02, // 801: packed [7, 300]
            0xDA, 0x32, 0x03, 0x01, 0xAC, 0x02, // 811: packed [1, 300]
            0xF2, 0x33, 0x02, 0x00, 0x01, // 830: packed [FOO, BAR]
        ];
        let typed = <third_party_protobuf_rust_test::TestExtensions as pbrs::Parse>::parse(wire)?;
        assert_eq!(pbrs::Serialize::serialize(&typed)?.as_slice(), wire);
        let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(
            third_party_protobuf_rust_test::FILE_DESCRIPTOR_SET,
        )?);
        let desc = pool
            .get_message("third_party_protobuf_rust_test.TestExtensions")
            .ok_or("missing original extension host")?;
        let dynamic = pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), wire)?;
        assert_eq!(dynamic.get_extension(1), Some(&pbrs::Value::Int32(300)));
        assert_eq!(
            dynamic.get_extension(2),
            Some(&pbrs::Value::String("foo".into()))
        );
        assert_eq!(dynamic.get_extension(11), Some(&pbrs::Value::Int64(300)));
        assert_eq!(dynamic.get_extension(12), Some(&pbrs::Value::Uint32(300)));
        assert_eq!(dynamic.get_extension(13), Some(&pbrs::Value::Uint64(300)));
        assert_eq!(dynamic.get_extension(14), Some(&pbrs::Value::Float(1.5)));
        assert_eq!(dynamic.get_extension(15), Some(&pbrs::Value::Double(1.5)));
        assert_eq!(dynamic.get_extension(16), Some(&pbrs::Value::Bool(true)));
        assert!(
            matches!(dynamic.get_extension(20), Some(pbrs::Value::Message(_))),
            "missing submessage_extension"
        );
        assert_eq!(dynamic.get_extension(30), Some(&pbrs::Value::Enum(1)));
        assert_eq!(dynamic.get_extension(31), Some(&pbrs::Value::Enum(1)));
        assert_eq!(dynamic.get_extension(100), Some(&pbrs::Value::Int32(42)));
        assert_eq!(dynamic.get_extension(500), Some(&pbrs::Value::Int32(7)));
        assert_eq!(
            dynamic.get_repeated(801),
            Some([pbrs::Value::Int32(7), pbrs::Value::Int32(300)].as_slice())
        );
        assert_eq!(
            dynamic.get_repeated(811),
            Some([pbrs::Value::Int64(1), pbrs::Value::Int64(300)].as_slice())
        );
        assert_eq!(
            dynamic.get_repeated(830),
            Some([pbrs::Value::Enum(0), pbrs::Value::Enum(1)].as_slice())
        );
        assert_eq!(pbrs::Serialize::serialize(&dynamic)?.as_slice(), wire);

        // Unpacked repeated extensions still parse; dynamic re-emits packed.
        let unpacked: &[u8] = &[0xD8, 0x32, 0x01, 0xD8, 0x32, 0xAC, 0x02];
        let typed =
            <third_party_protobuf_rust_test::TestExtensions as pbrs::Parse>::parse(unpacked)?;
        assert_eq!(pbrs::Serialize::serialize(&typed)?.as_slice(), unpacked);
        let pool = std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(
            third_party_protobuf_rust_test::FILE_DESCRIPTOR_SET,
        )?);
        let desc = pool
            .get_message("third_party_protobuf_rust_test.TestExtensions")
            .ok_or("missing original extension host")?;
        let dynamic = pbrs::DynamicMessage::parse_with_pool(desc, Some(pool), unpacked)?;
        assert_eq!(
            dynamic.get_repeated(811),
            Some([pbrs::Value::Int64(1), pbrs::Value::Int64(300)].as_slice())
        );
        assert_eq!(
            pbrs::Serialize::serialize(&dynamic)?.as_slice(),
            &[0xDA, 0x32, 0x03, 0x01, 0xAC, 0x02]
        );
        Ok(())
    }
}
"###
    .replace("@ROOT@", root.to_str().expect("UTF-8 repository path"));
    std::fs::write(src_dir.join("lib.rs"), consumer_source).expect("write Rust 2024 consumer");
    for (subcommand, args) in [
        ("test", vec!["--offline", "--quiet"]),
        (
            "clippy",
            vec!["--offline", "--quiet", "--lib", "--", "-D", "warnings"],
        ),
    ] {
        let result = run_shared_consumer_cargo(
            shared_consumer_cargo()
                .arg(subcommand)
                .args(args)
                .current_dir(&consumer),
        )
        .expect("run shared-target Rust 2024 consumer");
        assert!(
            result.status.success(),
            "edition2024 consumer {subcommand} failed:\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if subcommand == "test" {
            assert!(
                String::from_utf8_lossy(&result.stdout).contains("9 passed; 0 failed"),
                "Edition 2024 consumer cases did not all run: {}",
                String::from_utf8_lossy(&result.stdout)
            );
        }
    }
    std::fs::remove_dir_all(&consumer).expect("clean consumer scratch");
}

#[test]
fn edition2024_direct_generation_enforces_imported_visibility() {
    let definitions = include_bytes!("fixtures/edition2024/fds/visibility.fds");
    for (target, allowed) in [
        ("ExportedTopLevelMessage", true),
        ("DefaultTopLevelMessage.ExportedNestedMessage", true),
        ("LocalTopLevelMessage", false),
        ("DefaultTopLevelMessage.DefaultNestedLocalMessage", false),
    ] {
        let mut file = Vec::new();
        pbrs::rt::encode_len_field(&mut file, 1, b"consumer.proto");
        pbrs::rt::encode_len_field(&mut file, 2, b"edition2024.consumer");
        pbrs::rt::encode_len_field(&mut file, 3, b"visibility.proto");
        pbrs::rt::encode_len_field(&mut file, 12, b"editions");
        pbrs::rt::encode_tag(&mut file, 14, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut file, 1001);
        let mut message = Vec::new();
        pbrs::rt::encode_len_field(&mut message, 1, b"Consumer");
        let mut field = Vec::new();
        pbrs::rt::encode_len_field(&mut field, 1, b"value");
        pbrs::rt::encode_tag(&mut field, 3, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut field, 1);
        pbrs::rt::encode_tag(&mut field, 4, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut field, 1);
        pbrs::rt::encode_tag(&mut field, 5, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut field, 11);
        pbrs::rt::encode_len_field(
            &mut field,
            6,
            format!(".edition2024.visibility.{target}").as_bytes(),
        );
        pbrs::rt::encode_len_field(&mut message, 2, &field);
        pbrs::rt::encode_len_field(&mut file, 4, &message);
        let mut fds = definitions.to_vec();
        pbrs::rt::encode_len_field(&mut fds, 1, &file);
        let result =
            pbrs::codegen::generate_from_file_descriptor_set(&fds, &["consumer.proto".into()]);
        assert_eq!(
            result.is_ok(),
            allowed,
            "2024 generated cross-file reference to {target}: {result:?}"
        );
    }
}

fn edition2024_plugin_request(targets: &[&str], fds: &[u8]) -> Vec<u8> {
    let mut request = Vec::new();
    for target in targets {
        pbrs::rt::encode_len_field(&mut request, 1, target.as_bytes());
    }
    let mut pos = 0;
    while pos < fds.len() {
        let (number, wire) = pbrs::rt::decode_tag(fds, &mut pos).unwrap();
        assert_eq!((number, wire), (1, pbrs::rt::WIRE_LEN));
        pbrs::rt::encode_len_field(
            &mut request,
            15,
            pbrs::rt::read_len_bytes(fds, &mut pos).unwrap(),
        );
    }
    request
}

fn run_plugin_bin(request: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(plugin_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch compiled protoc-gen-pbrs");
    child
        .stdin
        .take()
        .expect("plugin stdin")
        .write_all(request)
        .expect("send checked CodeGeneratorRequest");
    child
        .wait_with_output()
        .expect("read CodeGeneratorResponse")
}

fn plugin_response_files_and_error(response: &[u8]) -> (usize, Option<Vec<u8>>) {
    let mut pos = 0;
    let mut files = 0;
    let mut error = None;
    while pos < response.len() {
        let (number, wire) = pbrs::rt::decode_tag(response, &mut pos).unwrap();
        if number == 1 && wire == pbrs::rt::WIRE_LEN {
            error = Some(
                pbrs::rt::read_len_bytes(response, &mut pos)
                    .unwrap()
                    .to_vec(),
            );
        } else if number == 15 && wire == pbrs::rt::WIRE_LEN {
            files += 1;
            let _ = pbrs::rt::read_len_bytes(response, &mut pos).unwrap();
        } else {
            pbrs::rt::skip_field(response, &mut pos, wire).unwrap();
        }
    }
    (files, error)
}

#[test]
fn edition2024_plugin_binary_advertises_2024_for_qualified_consumers() {
    let preview: &[u8] = include_bytes!("fixtures/edition2024/fds/cg14_preview.fds");
    for (targets, fds) in [
        (
            &["defaults.proto"] as &[&str],
            include_bytes!("fixtures/edition2024/fds/defaults.fds").as_slice(),
        ),
        (
            &["overrides.proto"],
            include_bytes!("fixtures/edition2024/fds/overrides.fds").as_slice(),
        ),
        (
            &["inheritance.proto"],
            include_bytes!("fixtures/edition2024/fds/inheritance.fds").as_slice(),
        ),
        (
            &["visibility.proto"],
            include_bytes!("fixtures/edition2024/fds/visibility.fds").as_slice(),
        ),
        (
            &["extensions.proto"],
            include_bytes!("fixtures/edition2024/fds/extensions.fds").as_slice(),
        ),
        (
            &[
                "legacy_style.proto",
                "maps.proto",
                "maps_none.proto",
                "rust/test/extensions.proto",
            ] as &[&str],
            preview,
        ),
    ] {
        let request = edition2024_plugin_request(targets, fds);
        let result = run_plugin_bin(&request);
        assert!(
            result.status.success(),
            "plugin process failed for {targets:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            response_maximum_edition(&result.stdout),
            Some(1001),
            "advertised cap for {targets:?}"
        );
        let (files, error) = plugin_response_files_and_error(&result.stdout);
        assert!(
            error.is_none(),
            "plugin returned an error for {targets:?}: {error:?}"
        );
        assert!(
            files > 0,
            "qualified Edition 2024 request generated no files for {targets:?}"
        );
    }
}

#[test]
fn edition2024_plugin_binary_rejects_closed_collections_cleanly() {
    let fds: &[u8] = include_bytes!("fixtures/edition2024/fds/closed_enum.fds");
    let request = edition2024_plugin_request(&["closed_enum.proto"], fds);
    let result = run_plugin_bin(&request);
    assert!(
        result.status.success(),
        "plugin process failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(response_maximum_edition(&result.stdout), Some(1001));
    let (files, error) = plugin_response_files_and_error(&result.stdout);
    assert_eq!(files, 0, "closed collections must not generate files");
    let raw = error.expect("closed collections must fail with an error");
    let error = String::from_utf8_lossy(&raw);
    assert!(
        error.contains("closed enum in repeated/map field"),
        "unexpected closed-collection error: {error}"
    );
}

#[test]
fn edition2024_protoc_negotiates_2024_plugin_support() {
    // End-to-end proof that a 2024-capable protoc sends Edition 2024 files to
    // protoc-gen-pbrs now that the plugin advertises maximum_edition = 1001.
    // Capability-gated: environments whose protoc cannot compile Edition 2024
    // source skip; the checked-FDS tests cover the semantics hermetically.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_dir = root.join("tests/fixtures/edition2024/proto");
    let probe = root.join(format!(
        "target/plugin-edition2024-probe-{}.fds",
        std::process::id()
    ));
    let probe_result = Command::new("protoc")
        .arg("-I")
        .arg(&proto_dir)
        .arg(format!("--descriptor_set_out={}", probe.display()))
        .arg("defaults.proto")
        .output()
        .expect("probe installed protoc for Edition 2024 source support");
    if !probe_result.status.success() {
        eprintln!(
            "skipping Edition 2024 plugin negotiation: installed protoc cannot compile Edition 2024 source: {}",
            String::from_utf8_lossy(&probe_result.stderr)
        );
        let _ = std::fs::remove_file(&probe);
        return;
    }
    let _ = std::fs::remove_file(&probe);

    let tmp = root.join(format!(
        "target/plugin-edition2024-negotiate-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("negotiation scratch");
    let status = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&proto_dir)
        .arg("defaults.proto")
        .status()
        .expect("run protoc with pbrs plugin on Edition 2024 source");
    assert!(
        status.success(),
        "protoc refused the Edition 2024 plugin request"
    );
    let generated =
        std::fs::read_to_string(tmp.join("defaults.rs")).expect("generated defaults.rs");
    assert!(
        generated.contains("pub struct DefaultMessage"),
        "missing DefaultMessage:\n{generated}"
    );
    assert!(
        generated.contains("pub struct DefaultEnum(pub i32);"),
        "missing DefaultEnum:\n{generated}"
    );

    // The fail-closed subset still fails loudly through real negotiation.
    let output = Command::new("protoc")
        .arg(format!(
            "--plugin=protoc-gen-pbrs={}",
            plugin_bin().display()
        ))
        .arg(format!("--pbrs_out={}", tmp.display()))
        .arg("-I")
        .arg(&proto_dir)
        .arg("closed_enum.proto")
        .output()
        .expect("run protoc with pbrs plugin on closed_enum source");
    assert!(
        !output.status.success(),
        "closed_enum unexpectedly generated"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("closed enum in repeated/map field"),
        "closed_enum did not fail for its approved reason: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn edition2024_closed_enum_collections_fail_closed() {
    fn field(name: &str, number: u64, label: u64, ty: u64, type_name: Option<&str>) -> Vec<u8> {
        let mut bytes = Vec::new();
        pbrs::rt::encode_len_field(&mut bytes, 1, name.as_bytes());
        for (tag, value) in [(3, number), (4, label), (5, ty)] {
            pbrs::rt::encode_tag(&mut bytes, tag, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(&mut bytes, value);
        }
        if let Some(type_name) = type_name {
            pbrs::rt::encode_len_field(&mut bytes, 6, type_name.as_bytes());
        }
        bytes
    }

    let reference = include_bytes!("fixtures/edition2024/fds/overrides.fds");
    for kind in ["repeated", "map"] {
        let name = format!("{kind}.proto");
        let mut file = Vec::new();
        pbrs::rt::encode_len_field(&mut file, 1, name.as_bytes());
        pbrs::rt::encode_len_field(&mut file, 2, b"edition2024.closed");
        pbrs::rt::encode_len_field(&mut file, 3, b"overrides.proto");
        pbrs::rt::encode_len_field(&mut file, 12, b"editions");
        pbrs::rt::encode_tag(&mut file, 14, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut file, 1001);
        let mut message = Vec::new();
        pbrs::rt::encode_len_field(&mut message, 1, b"Host");
        let value_type = ".edition2024.overrides.ClosedEnum";
        if kind == "map" {
            let mut entry = Vec::new();
            pbrs::rt::encode_len_field(&mut entry, 1, b"ValuesEntry");
            pbrs::rt::encode_len_field(&mut entry, 2, &field("key", 1, 1, 9, None));
            pbrs::rt::encode_len_field(&mut entry, 2, &field("value", 2, 1, 14, Some(value_type)));
            let mut options = Vec::new();
            pbrs::rt::encode_tag(&mut options, 7, pbrs::rt::WIRE_VARINT);
            pbrs::rt::encode_varint(&mut options, 1);
            pbrs::rt::encode_len_field(&mut entry, 7, &options);
            pbrs::rt::encode_len_field(&mut message, 3, &entry);
            pbrs::rt::encode_len_field(
                &mut message,
                2,
                &field(
                    "values",
                    1,
                    3,
                    11,
                    Some(".edition2024.closed.Host.ValuesEntry"),
                ),
            );
        } else {
            pbrs::rt::encode_len_field(
                &mut message,
                2,
                &field("values", 1, 3, 14, Some(value_type)),
            );
        }
        pbrs::rt::encode_len_field(&mut file, 4, &message);
        let mut fds = reference.to_vec();
        pbrs::rt::encode_len_field(&mut fds, 1, &file);
        let error = pbrs::codegen::generate_from_file_descriptor_set(&fds, &[name])
            .expect_err("unimplemented closed enum collection must not generate");
        assert!(
            matches!(
                error,
                pbrs::codegen::CodegenError::MalformedDescriptor { .. }
            ) && error
                .to_string()
                .contains("closed enum in repeated/map field"),
            "{kind}: {error}"
        );
    }
}

#[test]
fn edition2024_checked_closed_enum_reference_remains_fail_closed() {
    let fds = include_bytes!("fixtures/edition2024/fds/closed_enum.fds");
    let error =
        pbrs::codegen::generate_from_file_descriptor_set(fds, &["closed_enum.proto".to_string()])
            .expect_err("checked repeated and map CLOSED enums are not yet generated");
    assert!(
        matches!(
            error,
            pbrs::codegen::CodegenError::MalformedDescriptor { .. }
        ) && error
            .to_string()
            .contains("closed enum in repeated/map field"),
        "{error}"
    );
}
