use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("hello.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    tonic_prost_build::configure()
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("tonic-prost-build hello.proto");
}
