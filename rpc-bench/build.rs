//! Generate tonic TestService stubs for the tonic 0.14 side of the bench.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "build.rs"
)]

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let testing = proto_dir.join("grpc/testing/test.proto");
    pbrs::codegen::Config::new()
        .emit_deps(true)
        .compile_protos(&[&testing], &[&proto_dir])
        .expect("tonic TestService codegen");
}
