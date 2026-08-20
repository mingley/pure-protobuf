use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("../third_party/protobuf/src");
    let proto = src.join("google/protobuf/test_messages_proto3.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut cmd = Command::new("protoc");
    cmd.arg(format!("--rust_out={}", out.display()))
        .arg("--rust_opt=experimental-codegen=enabled,kernel=upb")
        .arg("-I")
        .arg(&src)
        .arg(&proto);
    for wkt in [
        "google/protobuf/any.proto",
        "google/protobuf/duration.proto",
        "google/protobuf/timestamp.proto",
        "google/protobuf/wrappers.proto",
        "google/protobuf/struct.proto",
        "google/protobuf/field_mask.proto",
        "google/protobuf/empty.proto",
    ] {
        cmd.arg(src.join(wkt));
    }
    let status = cmd.status().expect("protoc --rust_out");
    if !status.success() {
        panic!("protoc --rust_out TAT failed: {status}");
    }
    let gen_dir = out.join("google/protobuf");
    let gen = gen_dir.join("generated.rs");
    let mut src = std::fs::read_to_string(&gen).unwrap_or_else(|_| {
        panic!("missing {}", gen.display())
    });
    let dir = gen_dir.display().to_string();
    src = src.replace("#[path=\"", &format!("#[path=\"{dir}/"));
    std::fs::write(out.join("generated.rs"), src).expect("write generated.rs");
}
