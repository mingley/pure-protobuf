use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mode = env::var("PBRS_NS_RUNTIME").unwrap_or_else(|_| "default".into());
    let runtime = match mode.as_str() {
        "default" => None,
        "bare" => Some("pb_runtime"),
        "absolute" => Some("::pb_runtime"),
        "local" => Some("crate::runtime"),
        other => panic!("unknown runtime mode {other}"),
    };
    for directory in ["native", "native-external", "prost", "prost-external"] {
        fs::create_dir_all(out.join(directory)).unwrap();
    }
    let mut external = pbrs::codegen::Config::new();
    if let Some(runtime) = runtime {
        external.runtime_crate(runtime);
    }
    external
        .out_dir(out.join("native-external"))
        .safe_message_defaults(env::var_os("CARGO_FEATURE_SAFE").is_some())
        .emit_kernel_stubs(false)
        .compile_protos(&["proto/external.proto"], &["proto"])
        .unwrap();
    let mut native = pbrs::codegen::Config::new();
    if let Some(runtime) = runtime {
        native.runtime_crate(runtime);
    }
    native
        .out_dir(out.join("native"))
        .safe_message_defaults(env::var_os("CARGO_FEATURE_SAFE").is_some())
        .owned_message_fields(env::var_os("CARGO_FEATURE_OWNED").is_some())
        .enable_type_names()
        .include_source_info(true)
        .emit_kernel_stubs(false)
        .message_attribute(
            ".proof.symbols.Record",
            "#[doc = \"user attribute pbrs::Child ::pbrs::Child ::std::Path\"]",
        )
        .extern_path(".proof.external.External", "crate::foreign::pbrs::Child")
        .compile_protos(&["proto/symbols.proto", "proto/closed.proto"], &["proto"])
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-external"))
        .compile_protos(&["proto/external.proto"], &["proto"])
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost"))
        .extern_path(".proof.external.External", "crate::prost_external::Result")
        .compile_protos(&["proto/symbols.proto", "proto/closed.proto"], &["proto"])
        .unwrap();
    let source = fs::read_to_string(out.join("native/symbols.rs")).unwrap();
    for name in [
        "Clone",
        "Eq",
        "PartialEq",
        "ParseError",
        "SerializeError",
        "ProtoBytes",
        "ProtoString",
        "Repeated",
        "RepeatedView_",
        "RepeatedMut_",
        "UnknownEnumValue",
        "UnknownFields",
        "Some",
        "None",
        "Ok",
        "Err",
        "IntoIterator",
        "pbrs",
        "std",
        "core",
    ] {
        assert!(
            source.contains(&format!("pub struct {name} {{"))
                || source.contains(&format!("pub struct {name}(")),
            "{name}"
        );
    }
    for established in ["PbOption", "PbResult", "PbVec", "PbBox", "PbString"] {
        assert!(
            source.contains(&format!("pub struct {established} {{")),
            "{established}"
        );
    }
    assert!(source.contains("crate::foreign::pbrs::Child"));
    assert!(source.contains("user attribute pbrs::Child ::pbrs::Child ::std::Path"));
    assert!(source.contains("user docs pbrs::Child ::pbrs::Child ::std::Path"));
    let expected = runtime.unwrap_or("::pbrs");
    let expected = if expected == "pb_runtime" {
        "::pb_runtime"
    } else {
        expected
    };
    assert!(source.contains(&format!("use {expected}::prelude::*;")));
    for name in ["external", "symbols", "closed"] {
        println!("cargo:rerun-if-changed=proto/{name}.proto");
    }
    println!("cargo:rerun-if-env-changed=PBRS_NS_RUNTIME");
}
