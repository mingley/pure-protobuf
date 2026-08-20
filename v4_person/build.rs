use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../proto/person.proto");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let proto_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../proto");
    let proto = proto_dir.join("person.proto");
    let status = Command::new("protoc")
        .arg(format!("--rust_out={}", out.display()))
        .arg("--rust_opt=experimental-codegen=enabled,kernel=upb")
        .arg("-I")
        .arg(&proto_dir)
        .arg(&proto)
        .status()
        .expect("run protoc --rust_out");
    if !status.success() {
        panic!("protoc --rust_out failed: {status}");
    }
    let gen = out.join("generated.rs");
    let pb = out.join("person.u.pb.rs");
    let src = std::fs::read_to_string(&gen).expect("read generated.rs");
    let src = src.replace(
        "#[path=\"person.u.pb.rs\"]",
        &format!("#[path=\"{}\"]", pb.display()),
    );
    std::fs::write(&gen, src).expect("rewrite generated.rs path");
}
