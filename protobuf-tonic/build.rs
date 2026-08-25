use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let proto = proto_dir.join("hello.proto");
    pbrs::codegen::compile_protos(&[&proto], &[&proto_dir]).expect("codegen");
}
