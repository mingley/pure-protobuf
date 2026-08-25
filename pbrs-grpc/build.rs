//! Generate helloworld messages only (no tonic stubs).
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "build.rs is a sync compile-time script; panic fails the build"
)]

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("hello.proto");
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(false)
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("codegen");
}
