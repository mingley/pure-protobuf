use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let proto = root.join("proto");
    let mut includes = vec![proto.clone()];
    // A protoc built from source has no installed well-known-type directory.
    // The parent harness already requires this pinned checkout for v4_tat.
    // Standalone users can still rely on their installed protoc's includes.
    let pinned_wkt = root.join("../../../third_party/protobuf/src");
    if pinned_wkt.join("google/protobuf/any.proto").is_file() {
        includes.push(pinned_wkt);
    }
    let core = [proto.join("adoption.proto"), proto.join("sparse.proto")];
    let pbrs_out = out.join("pbrs");
    pbrs::codegen::Config::new()
        .out_dir(&pbrs_out)
        .emit_deps(true)
        .compile_protos(&core, &includes)
        .expect("generate pbrs adoption messages");
    let prost_out = out.join("prost");
    std::fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_out)
        .compile_well_known_types()
        .compile_protos(&core, &includes)
        .expect("generate prost adoption messages");

    let option_protos: Vec<_> = (0..20)
        .map(|i| proto.join(format!("options/part_{i:02}.proto")))
        .collect();
    let pbrs_options = out.join("pbrs_options");
    pbrs::codegen::Config::new()
        .out_dir(&pbrs_options)
        .compile_protos(&option_protos, &includes)
        .expect("generate pbrs option-heavy messages");
    let prost_options = out.join("prost_options");
    std::fs::create_dir_all(&prost_options).unwrap();
    prost_build::Config::new()
        .out_dir(&prost_options)
        .compile_protos(&option_protos, &includes)
        .expect("generate prost option-heavy messages");
    // Compile all generated option-heavy outputs, rather than merely proving
    // that the generators returned success.
    let mut modules = String::new();
    for i in 0..20 {
        modules.push_str(&format!(
            "pub mod part_{i:02} {{ pub mod pbrs {{ include!({:?}); }} pub mod prost {{ include!({:?}); }} }}\n",
            pbrs_options.join(format!("part_{i:02}.rs")),
            prost_options.join(format!("adoption.options.part{i:02}.rs")),
        ));
    }
    #[expect(clippy::disallowed_methods, reason = "synchronous Cargo build script")]
    std::fs::write(out.join("option_modules.rs"), modules).unwrap();
    for p in core.iter().chain(option_protos.iter()) {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        proto.join("options/definitions.proto").display()
    );
}
