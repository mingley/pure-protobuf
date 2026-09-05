//! Generate `hello.rs` from `proto/hello.proto` via `pbrs::codegen::compile_protos`.
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
    let proto_dir = if manifest.join("proto/hello.proto").exists() {
        manifest.join("proto")
    } else {
        manifest.join("../proto")
    };
    let proto = proto_dir.join("hello.proto");
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&[&proto], &[&proto_dir])
        .expect("codegen");
}
