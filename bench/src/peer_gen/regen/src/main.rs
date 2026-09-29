//! SB-08 peer gencode driver. Generates into `/tmp/sb08out`; `stage.py`
//! (next to this file) checks the output into `bench/src/peer_gen/` with
//! SB-08 headers and renames. See `../SB08_PROVENANCE.md`.
//!
//! Environment: `SB08_ROOT` = worktree root (schemas come from there).
//! `protoc` on PATH must be 36.x: prost-build shells out to it for parsing,
//! and the `--rust_out` step needs the 36.x Rust generator. The pinned
//! 35.1 protoc (`scripts/build-pinned-protoc.sh`) is for building the old
//! path crates, not for this driver.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(std::env::var("SB08_ROOT").expect("set SB08_ROOT to the worktree root"))
}

fn protoc_rust_out(out_dir: &Path, includes: &[&Path], protos: &[&Path]) {
    let status = Command::new("protoc")
        .arg(format!("--rust_out={}", out_dir.display()))
        .arg("--rust_opt=experimental-codegen=enabled,kernel=upb")
        .args(
            includes
                .iter()
                .flat_map(|i| ["-I".to_owned(), i.display().to_string()]),
        )
        .args(protos.iter().map(|p| p.display().to_string()))
        .status()
        .expect("run protoc --rust_out (need protoc 36.x on PATH)");
    assert!(status.success(), "protoc --rust_out failed: {status}");
}

fn main() {
    let root = root();
    let out = PathBuf::from("/tmp/sb08out");
    std::fs::create_dir_all(&out).unwrap();
    let tp_src = root.join("third_party/protobuf/src");
    let tat = tp_src.join("google/protobuf/test_messages_proto3.proto");
    let person = root.join("proto/person.proto");
    let proto_inc = root.join("proto");

    // prost 0.14: TAT + person (native prost_path: bench `prost` is 0.13,
    // so 0.14 gencode must point at the renamed deps or the derive binds 0.13)
    for (name, protos, inc) in [
        ("prost14_tat", vec![tat.as_path()], vec![tp_src.as_path()]),
        (
            "prost14_person",
            vec![person.as_path()],
            vec![proto_inc.as_path()],
        ),
    ] {
        let dir = out.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        prost_build::Config::new()
            .out_dir(&dir)
            .prost_path("::prost14")
            .prost_types_path("::prost_types14")
            .compile_protos(&protos, &inc)
            .unwrap();
    }
    // prost 0.13: person only (TAT comes from the prost_tat path crate)
    {
        let dir = out.join("prost13_person");
        std::fs::create_dir_all(&dir).unwrap();
        prost_build_013::Config::new()
            .out_dir(&dir)
            .compile_protos(&[&person], &[&proto_inc])
            .unwrap();
    }
    // buffa 0.9.2: TAT + person, lazy views on
    for (name, proto, inc) in [
        ("buffa092_tat", tat.as_path(), tp_src.as_path()),
        ("buffa092_person", person.as_path(), proto_inc.as_path()),
    ] {
        let dir = out.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: single-threaded driver; buffa-build reads OUT_DIR.
        unsafe { std::env::set_var("OUT_DIR", &dir) };
        buffa_build::Config::new()
            .files(&[proto])
            .includes(&[inc])
            .lazy_views(true)
            .generate_json(false)
            .generate_text(false)
            .include_file("_include.rs")
            .compile()
            .unwrap();
    }
    // quick-protobuf (pb-rs): person only (pb-rs rejects the TAT schema)
    {
        let dir = out.join("qp_person");
        std::fs::create_dir_all(&dir).unwrap();
        let person = person.clone();
        let dir = dir.clone();
        let proto_inc = proto_inc.clone();
        let cfgs = pb_rs::ConfigBuilder::new(&[person], None, Some(&dir), &[proto_inc])
            .unwrap()
            .single_module(true)
            .build();
        pb_rs::types::FileDescriptor::run(&cfgs).unwrap();
    }
    // google-protobuf 0.36: TAT (+ WKTs) + person via protoc 36.x --rust_out
    {
        let dir = out.join("gpb36_person");
        std::fs::create_dir_all(&dir).unwrap();
        protoc_rust_out(&dir, &[proto_inc.as_path()], &[person.as_path()]);
        let dir = out.join("gpb36_tat");
        std::fs::create_dir_all(&dir).unwrap();
        let wkt: Vec<PathBuf> = [
            "google/protobuf/test_messages_proto3.proto",
            "google/protobuf/any.proto",
            "google/protobuf/duration.proto",
            "google/protobuf/timestamp.proto",
            "google/protobuf/wrappers.proto",
            "google/protobuf/struct.proto",
            "google/protobuf/field_mask.proto",
            "google/protobuf/empty.proto",
        ]
        .iter()
        .map(|p| tp_src.join(p))
        .collect();
        let refs: Vec<&Path> = wkt.iter().map(|p| p.as_path()).collect();
        protoc_rust_out(&dir, &[tp_src.as_path()], &refs);
    }
    println!("OK: generated into {}", out.display());
}
