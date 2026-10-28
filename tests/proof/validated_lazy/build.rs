//! Generate checked lazy bindings and add proof-only validation call counters.
#![allow(
    clippy::disallowed_methods,
    reason = "synchronous build-time generated instrumentation"
)]
use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let generated = out.join("pbrs");
    fs::create_dir_all(&generated).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(&generated)
        .compile_protos(&["proof.proto", "strict.proto"], &["."])
        .unwrap();
    for name in ["proof.rs", "strict.rs"] {
        let path = generated.join(name);
        let source = fs::read_to_string(&path).unwrap();
        let needle = "pub fn validate_inner(";
        let mut result = String::with_capacity(source.len());
        let mut rest = source.as_str();
        let mut inserted = 0;
        while let Some(start) = rest.find(needle) {
            let brace = start + rest[start..].find('{').unwrap() + 1;
            result.push_str(&rest[..brace]);
            result.push_str(" crate::GENERATED_VALIDATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed); ");
            rest = &rest[brace..];
            inserted += 1;
        }
        result.push_str(rest);
        assert!(inserted > 0, "generated validator signature changed");
        fs::write(&path, result).unwrap();
    }
    let reference = out.join("prost");
    fs::create_dir_all(&reference).unwrap();
    prost_build::Config::new()
        .out_dir(reference)
        .btree_map(["."])
        .compile_protos(&["proof.proto", "strict.proto"], &["."])
        .unwrap();
    println!("cargo:rerun-if-changed=proof.proto");
    println!("cargo:rerun-if-changed=strict.proto");
}
