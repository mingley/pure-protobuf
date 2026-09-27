//! Generate tonic echo stubs. Needs protoc in PATH (same requirement
//! as bench/ and tonic-bench/); without it the crate fails to build,
//! exactly like those harnesses.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto = manifest.join("proto/echo.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[&proto], &[&manifest.join("proto")])
        .expect("tonic-prost-build echo.proto");
}
