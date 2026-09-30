//! Generate `hello.rs` from the bundled `proto/hello.fds`.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "build.rs is a sync compile-time script; panic fails the build"
)]

use std::path::PathBuf;

use prost::Message as _;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("proto");
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .include_source_info(true)
        .compile_descriptor_set(proto_dir.join("hello.fds"), &["hello.proto"], &[&proto_dir])
        .expect("codegen");

    // Prove the migration path for projects that already own a
    // tonic-prost-build pipeline. Prost generates only the service surface;
    // every protobuf type is redirected to the pbrs module above.
    let descriptors = std::fs::read(proto_dir.join("hello.fds")).expect("read hello.fds");
    let descriptors = tonic_prost_build::FileDescriptorSet::decode(descriptors.as_slice())
        .expect("decode hello.fds");
    let tonic_out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("external-tonic");
    std::fs::create_dir_all(&tonic_out).expect("create external tonic output directory");
    tonic_prost_build::configure()
        .out_dir(tonic_out)
        .codec_path("::protobuf_tonic::ProtobufCodec")
        .extern_path(".helloworld", "crate::messages")
        .compile_fds(descriptors)
        .expect("generate tonic stubs over external pbrs messages");
}
