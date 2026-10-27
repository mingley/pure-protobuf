use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in [
        "native",
        "native-external",
        "prost",
        "prost-external",
        "prost-configured",
    ] {
        fs::create_dir_all(out.join(name)).unwrap();
    }
    let includes = [PathBuf::from("proto")];
    let mut external = pbrs::codegen::Config::new();
    external
        .out_dir(out.join("native-external"))
        .emit_kernel_stubs(false)
        .compile_protos(&["external.proto"], &includes)
        .unwrap();
    let mut native = pbrs::codegen::Config::new();
    native
        .out_dir(out.join("native"))
        .emit_kernel_stubs(false)
        .emit_deps(true)
        .extern_path(
            ".proof.external.External",
            "crate::native_external::External",
        )
        .compile_protos(&["case.proto", "imported.proto"], &includes)
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-external"))
        .compile_protos(&["external.proto"], &includes)
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost"))
        .extern_path(
            ".proof.external.External",
            "crate::prost_external::External",
        )
        .compile_protos(&["case.proto", "imported.proto"], &includes)
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-configured"))
        .btree_map(["."])
        .bytes(["."])
        .boxed(".proof.case.Payload.nested")
        .retain_enum_prefix()
        .enable_type_names()
        .type_name_domain(["."], "proof.example")
        .compile_protos(&["case.proto", "imported.proto"], &includes)
        .unwrap();
    let configured = fs::read_to_string(out.join("prost-configured/proof.case.rs")).unwrap();
    assert!(configured.contains("::prost::bytes::Bytes"));
    assert!(configured.contains("::prost::alloc::collections::BTreeMap"));
    assert!(configured.contains("::prost::alloc::boxed::Box"));
    assert!(configured.contains("impl ::prost::Name for Payload"));
    assert!(configured.contains("proof.example/proof.case.Payload"));
    let configured_enum = fs::read_to_string(out.join("prost-configured/proof.shared.rs")).unwrap();
    assert!(configured_enum.contains("StateReady = 1"));
    for file in ["case.proto", "external.proto", "imported.proto"] {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}
