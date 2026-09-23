//! Generate `hello.rs` from the bundled `proto/hello.fds`.
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
    let proto_dir = manifest.join("proto");
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .include_source_info(true)
        .compile_descriptor_set(proto_dir.join("hello.fds"), &["hello.proto"], &[&proto_dir])
        .expect("codegen");
}
