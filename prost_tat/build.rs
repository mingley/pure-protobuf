use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("../third_party/protobuf/src");
    let proto = src.join("google/protobuf/test_messages_proto3.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    prost_build::Config::new()
        .compile_protos(&[&proto], &[&src])
        .expect("prost-build TestAllTypesProto3");
}
