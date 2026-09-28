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
        .emit_tonic_stubs(true)
        .compile_protos(&[&testing], &[&proto_dir])
        .expect("tonic TestService codegen");

    let bench_proto_dir = manifest.join("proto");
    let benchmark = bench_proto_dir.join("grpc/testing/benchmark_service.proto");
    let control = bench_proto_dir.join("grpc/testing/control.proto");
    let stats = bench_proto_dir.join("grpc/testing/stats.proto");
    let payloads = bench_proto_dir.join("grpc/testing/payloads.proto");
    let worker = bench_proto_dir.join("grpc/testing/worker_service.proto");

    pbrs::codegen::Config::new()
        .emit_deps(true)
        .emit_kernel_stubs(true)
        .compile_protos(
            &[&benchmark, &control, &stats, &payloads, &worker],
            &[&bench_proto_dir, &proto_dir],
        )
        .expect("kernel BenchmarkService codegen");
}
