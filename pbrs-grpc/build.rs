//! Generate `helloworld` and `grpc.testing` messages plus native kernel stubs.
//!
//! The kernel's own services go through the same code generator users do, so a
//! regression in `emit_kernel_stubs` breaks this crate's build rather than
//! shipping quietly.
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

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&[&proto_dir.join("hello.proto")], &[&proto_dir])
        .expect("codegen helloworld");

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .emit_deps(true)
        .compile_protos(&[&proto_dir.join("grpc/testing/test.proto")], &[&proto_dir])
        .expect("codegen grpc.testing");
}
