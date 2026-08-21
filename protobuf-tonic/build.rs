use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("hello.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fds = out.join("hello.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", fds.display()))
        .arg("-I")
        .arg(&proto_dir)
        .arg(&proto)
        .status()
        .expect("protoc");
    assert!(status.success(), "protoc failed: {status}");
    let bytes = std::fs::read(&fds).expect("fds");
    let files = pbrs::codegen::generate_from_file_descriptor_set(&bytes, &["hello.proto".into()])
        .expect("codegen");
    for (name, src) in files {
        std::fs::write(out.join(name), src).expect("write generated");
    }
}
