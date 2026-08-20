use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("person.proto");
    println!("cargo:rerun-if-changed={}", proto.display());
    buffa_build::Config::new()
        .files(&[&proto])
        .includes(&[&proto_dir])
        .generate_json(false)
        .generate_text(false)
        .include_file("_include.rs")
        .compile()
        .expect("buffa-build person.proto");
}
