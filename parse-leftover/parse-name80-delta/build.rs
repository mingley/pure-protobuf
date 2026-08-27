use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../../proto");
    let proto = proto_dir.join("codec_cases.proto");
    println!("cargo:rerun-if-changed={}", proto.display());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pbrs_dir = out.join("pbrs");
    let prost_dir = out.join("prost");
    fs::create_dir_all(&pbrs_dir).unwrap();
    fs::create_dir_all(&prost_dir).unwrap();

    prost_build::Config::new()
        .out_dir(&prost_dir)
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("prost-build codec_cases.proto");

    pbrs::codegen::Config::new()
        .out_dir(&pbrs_dir)
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("pbrs codegen codec_cases.proto");
}
