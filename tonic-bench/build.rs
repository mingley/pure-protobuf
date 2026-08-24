use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let hello = proto_dir.join("hello.proto");
    let cases = proto_dir.join("codec_cases.proto");
    println!("cargo:rerun-if-changed={}", hello.display());
    println!("cargo:rerun-if-changed={}", cases.display());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pbrs_dir = out.join("pbrs");
    let prost_dir = out.join("prost");
    let v4_dir = out.join("v4");
    fs::create_dir_all(&pbrs_dir).unwrap();
    fs::create_dir_all(&prost_dir).unwrap();
    fs::create_dir_all(&v4_dir).unwrap();

    prost_build::Config::new()
        .out_dir(&prost_dir)
        .compile_protos(&[&hello, &cases], &[&proto_dir])
        .expect("prost-build");

    gen_pbrs(&cases, &proto_dir, &pbrs_dir);
    gen_v4(&cases, &proto_dir, &v4_dir);
}

fn gen_pbrs(proto: &Path, proto_dir: &Path, out: &Path) {
    let fds = out.join("codec_cases.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", fds.display()))
        .arg("-I")
        .arg(proto_dir)
        .arg(proto)
        .status()
        .expect("protoc fds");
    assert!(status.success(), "protoc fds failed: {status}");
    let bytes = fs::read(&fds).expect("fds");
    let files =
        pbrs::codegen::generate_from_file_descriptor_set(&bytes, &["codec_cases.proto".into()])
            .expect("pbrs codegen");
    assert!(!files.is_empty(), "pbrs codegen emitted no files");
    for (name, src) in files {
        fs::write(out.join(name), src).expect("write pbrs gencode");
    }
}

fn gen_v4(proto: &Path, proto_dir: &Path, out: &Path) {
    let status = Command::new("protoc")
        .arg(format!("--rust_out={}", out.display()))
        .arg("--rust_opt=experimental-codegen=enabled,kernel=upb")
        .arg("-I")
        .arg(proto_dir)
        .arg(proto)
        .status()
        .expect("protoc --rust_out");
    assert!(status.success(), "protoc --rust_out failed: {status}");
    let gen = out.join("generated.rs");
    let mut src = fs::read_to_string(&gen)
        .unwrap_or_else(|_| panic!("missing v4 generated.rs in {}", out.display()));
    let dir = out.display().to_string();
    src = src.replace("#[path=\"", &format!("#[path=\"{dir}/"));
    fs::write(gen, src).expect("rewrite v4 generated.rs path");
}
