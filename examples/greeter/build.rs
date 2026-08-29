//! Generate kernel stubs from `proto/hello.proto`.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::panic,
    reason = "build.rs is a sync compile-time script; panic fails the build"
)]

fn main() {
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(true)
        .compile_protos(&["proto/hello.proto"], &["proto"])
        .expect("codegen helloworld");
}
