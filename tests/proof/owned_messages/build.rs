#![allow(
    clippy::disallowed_methods,
    reason = "synchronous build-time source identity checks"
)]
use std::{env, fs, path::PathBuf};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Default immediately follows owned=true to check per-request reset.
    for (name, profile) in [
        ("owned", Some(true)),
        ("default", None),
        ("lazy", Some(false)),
    ] {
        let path = out.join(name);
        fs::create_dir_all(&path).unwrap();
        let mut config = pbrs::codegen::Config::new();
        config.out_dir(path);
        if let Some(owned) = profile {
            config.owned_message_fields(owned);
        }
        config
            .compile_protos(&["proof.proto", "strict.proto"], &["."])
            .unwrap();
    }
    for name in ["proof.rs", "strict.rs"] {
        assert_eq!(
            fs::read(out.join("default").join(name)).unwrap(),
            fs::read(out.join("lazy").join(name)).unwrap(),
            "default output differs after owned profile"
        );
        assert_ne!(
            fs::read(out.join("owned").join(name)).unwrap(),
            fs::read(out.join("lazy").join(name)).unwrap(),
            "profile did not alter nested storage"
        );
    }
    let reference = out.join("reference");
    fs::create_dir_all(&reference).unwrap();
    prost_build::Config::new()
        .out_dir(reference)
        .btree_map(["."])
        .compile_protos(&["proof.proto", "strict.proto"], &["."])
        .unwrap();
    let codec = out.join("api-codec");
    fs::create_dir_all(&codec).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    pbrs::codegen::Config::new()
        .out_dir(codec)
        .owned_message_fields(true)
        .compile_protos(
            &[root.join("proto/codec_cases.proto")],
            &[root.join("proto")],
        )
        .unwrap();
    println!("cargo:rerun-if-changed=proof.proto");
    println!("cargo:rerun-if-changed=strict.proto");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("proto/codec_cases.proto").display()
    );
}
