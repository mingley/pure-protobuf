#![allow(
    clippy::disallowed_methods,
    reason = "synchronous Cargo build script reads descriptors and writes generated modules"
)]
use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let descriptor = out.join("proof.fds");
    let protoc = env::var_os("PROTOC").unwrap_or_else(|| "protoc".into());
    let status = Command::new(protoc)
        .arg(format!("--descriptor_set_out={}", descriptor.display()))
        .args(["--include_imports", "-I.", "proof.proto", "required.proto"])
        .status()
        .expect("run protoc (set PROTOC if not on PATH)");
    assert!(status.success(), "protoc descriptor generation failed");
    let bytes = fs::read(&descriptor).unwrap();
    for (name, source) in pbrs::codegen::generate_from_file_descriptor_set(
        &bytes,
        &["proof.proto".into(), "required.proto".into()],
    )
    .expect("pbrs code generation")
    {
        fs::write(out.join(format!("pbrs_{name}")), source).unwrap();
    }
    prost_build::Config::new()
        .btree_map(["."])
        .compile_protos(&["proof.proto", "required.proto"], &["."])
        .expect("prost code generation");
    println!("cargo:rerun-if-changed=proof.proto");
    println!("cargo:rerun-if-changed=required.proto");
}
