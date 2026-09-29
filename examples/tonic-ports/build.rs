//! Build script for tonic example port generated modules.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "build script fails the build with actionable diagnostics"
)]

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("out dir"));
    let proto_dir = manifest.join("proto");
    let files = [
        "helloworld.proto",
        "route_guide.proto",
        "echo.proto",
        "unary_echo.proto",
    ];
    let protos: Vec<PathBuf> = files.iter().map(|file| proto_dir.join(file)).collect();

    let compat_out = out.join("compat");
    pbrs::codegen::Config::new()
        .out_dir(&compat_out)
        .tonic_compat(true)
        .include_source_info(true)
        .compile_protos(&protos, &[&proto_dir])
        .expect("compile tonic_compat example protos");

    let prost_out = out.join("prost");
    std::fs::create_dir_all(&prost_out).expect("create prost out dir");
    let fds = out.join("tonic_ports.fds");
    let mut prost = prost_build::Config::new();
    prost.out_dir(&prost_out).file_descriptor_set_path(&fds);
    prost
        .compile_protos(&protos, &[proto_dir.as_path()])
        .expect("compile prost example protos");
    pbrs::codegen::prost_stubs::compile_descriptor_set(&fds, &files, &prost_out)
        .expect("compile prost native stubs");

    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
}
