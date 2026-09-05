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
    let proto_dir = if manifest.join("proto/hello.proto").exists() {
        manifest.join("proto")
    } else {
        manifest.join("../proto")
    };

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&[&proto_dir.join("hello.proto")], &[&proto_dir])
        .expect("codegen helloworld");

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .emit_deps(true)
        .compile_protos(&[&proto_dir.join("grpc/testing/test.proto")], &[&proto_dir])
        .expect("codegen grpc.testing");

    // `kv.Store` is only used by `tests/codegen.rs`. An integration test is a
    // separate crate, so compiling generated stubs there proves they resolve
    // `::pbrs_grpc` as an ordinary dependency rather than through the
    // `extern crate self` alias this crate uses internally.
    let kv_dir = manifest.join("tests/proto");
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&[&kv_dir.join("kv.proto")], &[&kv_dir])
        .expect("codegen kv.Store");

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&[&kv_dir.join("extend.proto")], &[&kv_dir])
        .expect("codegen demo.ext");

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(
            &[&proto_dir.join("grpc/health/v1/health.proto")],
            &[&proto_dir],
        )
        .expect("codegen grpc.health.v1");

    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(
            &[&proto_dir.join("grpc/reflection/v1/reflection.proto")],
            &[&proto_dir],
        )
        .expect("codegen grpc.reflection.v1");

    // google.rpc.Status and the standard error-detail messages. Compiled
    // separately so each FileDescriptorSet only pulls the WKT it imports
    // (Any vs Duration) and the two generated files can live in sibling
    // modules without duplicate `mod __gen`.
    pbrs::codegen::Config::new()
        .compile_protos(&[&proto_dir.join("google/rpc/status.proto")], &[&proto_dir])
        .expect("codegen google.rpc.Status");

    pbrs::codegen::Config::new()
        .compile_protos(
            &[&proto_dir.join("google/rpc/error_details.proto")],
            &[&proto_dir],
        )
        .expect("codegen google.rpc error details");
}
