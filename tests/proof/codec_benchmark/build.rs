use std::{env, fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let proto = root.join("proto/codec_cases.proto");
    let include = root.join("proto");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in ["pbrs", "prost"] {
        fs::create_dir_all(out.join(name)).unwrap();
    }
    pbrs::codegen::Config::new()
        .out_dir(out.join("pbrs"))
        .compile_protos(&[&proto], &[&include])
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost"))
        .compile_protos(&[&proto], &[&include])
        .unwrap();
    println!("cargo:rerun-if-changed={}", proto.display());
}
