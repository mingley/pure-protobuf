use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let native = out.join("native");
    let native_external = out.join("native-external");
    let control = out.join("prost");
    let control_external = out.join("prost-external");
    for path in [&native, &native_external, &control, &control_external] {
        fs::create_dir_all(path).unwrap();
    }
    let configure = |config: &mut pbrs::codegen::Config| {
        config
            .owned_message_fields(env::var_os("CARGO_FEATURE_OWNED").is_some())
            .safe_message_defaults(env::var_os("CARGO_FEATURE_SAFE").is_some())
            .emit_kernel_stubs(false)
            .emit_reflection(false);
    };
    let mut external = pbrs::codegen::Config::new();
    configure(&mut external);
    external
        .out_dir(&native_external)
        .compile_protos(&["external.proto"], &["proto"])
        .unwrap();
    let mut config = pbrs::codegen::Config::new();
    configure(&mut config);
    config
        .out_dir(&native)
        .extern_path(
            ".proof.external.External",
            "crate::external_native::External",
        )
        .compile_protos(
            &[
                "valid.proto",
                "collisions.proto",
                "groups.proto",
                "child.proto",
            ],
            &["proto"],
        )
        .unwrap();
    prost_build::Config::new()
        .out_dir(&control_external)
        .compile_protos(&["external.proto"], &["proto"])
        .unwrap();
    prost_build::Config::new()
        .out_dir(&control)
        .include_file("mod.rs")
        .extern_path(
            ".proof.external.External",
            "crate::external_control::External",
        )
        .compile_protos(
            &[
                "valid.proto",
                "collisions.proto",
                "groups.proto",
                "child.proto",
            ],
            &["proto"],
        )
        .unwrap();
    for file in [
        "child.proto",
        "external.proto",
        "valid.proto",
        "collisions.proto",
        "groups.proto",
    ] {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}
