use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=proto/person.proto");
    println!("cargo:rerun-if-changed=third_party/protobuf");
    let out = std::env::var("OUT_DIR").unwrap();
    let fds = Path::new(&out).join("conformance_fds.bin");
    let root = Path::new("third_party/protobuf");
    let src = root.join("src");
    if !src.exists() {
        let _ = std::fs::write(&fds, []);
        return;
    }
    let mut inputs: Vec<std::path::PathBuf> = Vec::new();
    for rel in [
        "google/protobuf/test_messages_proto3.proto",
        "google/protobuf/test_messages_proto2.proto",
        "google/protobuf/any.proto",
        "google/protobuf/duration.proto",
        "google/protobuf/timestamp.proto",
        "google/protobuf/struct.proto",
        "google/protobuf/wrappers.proto",
        "google/protobuf/field_mask.proto",
        "google/protobuf/empty.proto",
    ] {
        let p = src.join(rel);
        if p.exists() {
            inputs.push(p);
        }
    }
    for rel in [
        "conformance/test_protos/test_messages_edition2023.proto",
        "conformance/test_protos/test_messages_edition_unstable.proto",
        "editions/golden/test_messages_proto2_editions.proto",
        "editions/golden/test_messages_proto3_editions.proto",
    ] {
        let p = root.join(rel);
        if p.exists() {
            inputs.push(p);
        }
    }
    if inputs.is_empty() {
        let _ = std::fs::write(&fds, []);
        return;
    }
    let mut cmd = Command::new("protoc");
    cmd.arg("--include_imports")
        .arg("--descriptor_set_out")
        .arg(&fds)
        .arg("-I")
        .arg(&src)
        .arg("-I")
        .arg(root);
    for p in &inputs {
        cmd.arg(p);
    }
    if cmd.status().map(|s| s.success()).unwrap_or(false) {
        println!("cargo:warning=wrote conformance descriptor set");
    } else {
        let _ = std::fs::write(&fds, []);
    }
}
