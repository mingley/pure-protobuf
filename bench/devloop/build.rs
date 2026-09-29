//! Generate tonic echo stubs and pbrs blob stubs. Needs protoc in PATH
//! (same requirement as bench/ and tonic-bench/); without it the crate
//! fails to build, exactly like those harnesses.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let proto = manifest.join("proto/echo.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[&proto], &[&manifest.join("proto")])
        .expect("tonic-prost-build echo.proto");

    let blob = manifest.join("proto/blob.proto");
    println!("cargo:rerun-if-changed={}", blob.display());
    pbrs::codegen::Config::new()
        .emit_deps(true)
        .emit_kernel_stubs(true)
        .compile_protos(&[&blob], &[&manifest.join("proto")])
        .expect("pbrs blob.proto codegen");

    let cases = manifest.join("../../proto/codec_cases.proto");
    let cases_out = out.join("pbrs_cases");
    std::fs::create_dir_all(&cases_out).expect("create pbrs cases out dir");
    println!("cargo:rerun-if-changed={}", cases.display());
    pbrs::codegen::Config::new()
        .out_dir(&cases_out)
        .compile_protos(&[&cases], &[&manifest.join("../../proto")])
        .expect("pbrs codec_cases.proto codegen");
}
