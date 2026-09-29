//! Build tonic stubs for example cross-stack tests.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "build script fails the build with actionable diagnostics"
)]

fn main() {
    let proto = "../../../examples/tonic-ports/proto";
    let files = [
        format!("{proto}/helloworld.proto"),
        format!("{proto}/route_guide.proto"),
        format!("{proto}/echo.proto"),
    ];
    let includes = [proto.to_owned()];
    tonic_prost_build::configure()
        .compile_protos(&files, &includes)
    .expect("compile tonic example protos");
    println!("cargo:rerun-if-changed={proto}/helloworld.proto");
    println!("cargo:rerun-if-changed={proto}/route_guide.proto");
    println!("cargo:rerun-if-changed={proto}/echo.proto");
}
