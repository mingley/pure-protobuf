use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("hello.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    // Message types only. No service stubs: the unary RPC table was dropped.
    prost_build::Config::new()
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("prost-build hello.proto");
}
