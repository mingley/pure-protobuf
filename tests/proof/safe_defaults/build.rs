use std::{env, fs, path::PathBuf};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let ordinary = env::var_os("CARGO_FEATURE_ORDINARY_CONTROL").is_some();
    let negative = env::var_os("CARGO_FEATURE_FORBID_LEGACY").is_some();
    let safe = !ordinary && !negative;
    let mut config = pbrs::codegen::Config::new();
    config
        .safe_message_defaults(safe)
        .owned_message_fields(env::var_os("CARGO_FEATURE_OWNED").is_some())
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .out_dir(&out);
    if env::var_os("CARGO_FEATURE_BOXED").is_some() {
        config.boxed(".safe.defaults.Record.child");
        config.boxed(".safe.defaults.Record.selection.chosen");
        config.boxed(".safe.defaults.Presence.child");
    }
    config
        .compile_protos(&["proto/legacy.proto", "proto/record.proto"], &["proto"])
        .unwrap();
    for file in ["legacy.rs", "record.rs"] {
        let source = fs::read_to_string(out.join(file)).unwrap();
        assert_eq!(source.contains("zeroed_message()"), !safe, "{file}");
        assert_eq!(source.contains("allow(unsafe_code"), !safe, "{file}");
        if safe {
            assert!(
                source.contains("derive(::core::default::Default)"),
                "{file}"
            );
        }
    }
    assert!(
        fs::read_to_string(out.join("legacy.rs"))
            .unwrap()
            .contains("struct PresenceCold")
    );
    println!("cargo:rerun-if-changed=proto/legacy.proto");
    println!("cargo:rerun-if-changed=proto/record.proto");
}
