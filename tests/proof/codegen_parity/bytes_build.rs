use std::{
    fs,
    path::{Path, PathBuf},
};

const FILES: [&str; 3] = ["bytes.proto", "bytes_external.proto", "bytes_legacy.proto"];
const SELECTION: [&str; 11] = [
    ".proof.bytes.Example.chosen",
    ".proof.bytes.Example.maybe",
    ".proof.bytes.Example.chunks",
    ".proof.bytes.Example.value",
    ".proof.bytes.Example.raw",
    ".proof.bytes.Example.type",
    ".proof.bytes.Child",
    ".proof.bytes.Cold",
    ".proof.bytes.Flat",
    ".proof.bytes_external",
    ".proof.bytes_legacy",
];

fn config() -> pbrs::codegen::Config {
    let mut config = pbrs::codegen::Config::new();
    config
        .emit_kernel_stubs(false)
        .emit_reflection(true)
        .emit_json(true)
        .emit_text(true)
        .bytes(SELECTION);
    config
}

fn selected(source: &str, field: &str, native: bool) -> bool {
    let rust_field = if field == "type" { "r#type" } else { field };
    let prefix = if native {
        format!("{rust_field}:")
    } else if field == "raw" {
        "Raw(".to_owned()
    } else {
        format!("pub {rust_field}:")
    };
    let mut lines = source
        .lines()
        .map(str::trim)
        .skip_while(|line| !line.starts_with(&prefix))
        .peekable();
    assert!(lines.peek().is_some(), "missing {prefix:?}");
    let mut declaration = String::new();
    let mut depth = 0_i32;
    'declaration: for line in lines {
        for character in line.chars() {
            declaration.push(character);
            match character {
                '<' | '(' => depth += 1,
                '>' | ')' => depth -= 1,
                ',' if depth == 0 => break 'declaration,
                _ => {}
            }
        }
    }
    declaration.contains(if native {
        "pbrs::ProtoBytes"
    } else {
        "::prost::bytes::Bytes"
    })
}

fn derives_default(source: &str, name: &str) -> bool {
    let position = source
        .find(&format!("struct {name} {{"))
        .expect("struct declaration");
    let prefix = &source[..position];
    prefix
        .strip_suffix("pub ")
        .unwrap_or(prefix)
        .lines()
        .rev()
        .take_while(|line| line.trim().starts_with("#[derive("))
        .any(|line| line.contains("Default"))
}

fn assert_same_output(left: &Path, right: &Path) {
    for filename in ["bytes.rs", "bytes_external.rs", "bytes_legacy.rs", "mod.rs"] {
        assert_eq!(
            fs::read(left.join(filename)).unwrap(),
            fs::read(right.join(filename)).unwrap(),
            "unmatched bytes configuration changed {filename}"
        );
    }
}

pub fn build(out: &Path, includes: &[PathBuf]) {
    build_prefix_controls(out, includes);
    let descriptor = out.join("bytes.fds");
    config()
        .out_dir(out.join("native-bytes"))
        .file_descriptor_set_path(&descriptor)
        .compile_protos(&FILES, includes)
        .unwrap();
    config()
        .out_dir(out.join("native-bytes-descriptor"))
        .compile_descriptor_set(&descriptor, &FILES, includes)
        .unwrap();
    for file in ["bytes.rs", "bytes_external.rs", "bytes_legacy.rs", "mod.rs"] {
        assert_eq!(
            fs::read(out.join("native-bytes").join(file)).unwrap(),
            fs::read(out.join("native-bytes-descriptor").join(file)).unwrap(),
            "bytes builder/descriptor output differs: {file}"
        );
    }
    for (directory, owned, safe) in [
        ("native-bytes-owned", true, false),
        ("native-bytes-safe", false, true),
    ] {
        config()
            .out_dir(out.join(directory))
            .owned_message_fields(owned)
            .safe_message_defaults(safe)
            .compile_protos(&FILES, includes)
            .unwrap();
    }
    config()
        .out_dir(out.join("native-bytes-lightweight-safe"))
        .emit_reflection(false)
        .safe_message_defaults(true)
        .compile_protos(&FILES, includes)
        .unwrap();
    let native = fs::read_to_string(out.join("native-bytes/bytes.rs")).unwrap();
    assert!(native.contains("struct ExampleCold"));
    assert!(derives_default(&native, "ExampleCold"));
    assert!(derives_default(&native, "Child"));
    assert!(native.contains("pub fn chosen_shared_3("));
    assert!(native.contains("pub fn maybe_shared_opt_2("));
    assert!(native.contains("pbrs::json::bytes(v)"));
    assert!(native.contains("pbrs::text::write_bytes_lit(v, out)"));
    let reference = out.join("prost-bytes");
    fs::create_dir_all(&reference).unwrap();
    prost_build::Config::new()
        .out_dir(reference)
        .bytes(SELECTION)
        .compile_protos(&FILES, includes)
        .unwrap();

    pbrs::codegen::Config::new()
        .out_dir(out.join("native-bytes-default"))
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .compile_protos(&FILES, includes)
        .unwrap();
    let mut report = String::new();
    for (index, (label, selector)) in [
        ("global", "."),
        ("exact", ".proof.bytes.Example.chosen"),
        ("message", ".proof.bytes.Example"),
        ("package", ".proof.bytes"),
        ("suffix", "Example.chosen"),
        ("field", "chosen"),
        ("repeated", ".proof.bytes.Example.chunks"),
        ("oneof-direct", ".proof.bytes.Example.raw"),
        ("oneof-named-nonmatch", ".proof.bytes.Example.choice.raw"),
        ("oneof-prefix-nonmatch", ".proof.bytes.Example.choice"),
        ("map-value", ".proof.bytes.Example.value"),
        ("map-field-nonmatch", ".proof.bytes.Example.lookup"),
        (
            "map-entry-nonmatch",
            ".proof.bytes.Example.LookupEntry.value",
        ),
        ("optional", ".proof.bytes.Example.maybe"),
        (
            "synthetic-name-nonmatch",
            ".proof.bytes.Example._maybe.maybe",
        ),
        ("boundary", ".proof.bytes.Exampleish"),
        ("suffix-boundary", "xchosen"),
        ("literal-star", "*"),
        ("empty", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let native_out = out.join(format!("bytes-selectors/native-{index}"));
        let reference_out = out.join(format!("bytes-selectors/prost-{index}"));
        fs::create_dir_all(&reference_out).unwrap();
        pbrs::codegen::Config::new()
            .out_dir(&native_out)
            .emit_kernel_stubs(false)
            .emit_reflection(false)
            .bytes([selector])
            .compile_protos(&FILES, includes)
            .unwrap();
        prost_build::Config::new()
            .out_dir(&reference_out)
            .bytes([selector])
            .compile_protos(&FILES, includes)
            .unwrap();
        let native = fs::read_to_string(native_out.join("bytes.rs")).unwrap();
        let reference = fs::read_to_string(reference_out.join("proof.bytes.rs")).unwrap();
        for field in [
            "chosen", "ordinary", "maybe", "chunks", "lookup", "raw", "type",
        ] {
            assert_eq!(
                selected(&native, field, true),
                selected(&reference, field, false),
                "bytes selector {selector:?} differs on {field}"
            );
        }
        if matches!(
            label,
            "boundary" | "suffix-boundary" | "literal-star" | "empty"
        ) {
            assert_same_output(&native_out, &out.join("native-bytes-default"));
        }
        report.push_str(&format!("PASS bytes-selector {label} {selector:?}\n"));
    }
    for (directory, clear) in [("bytes-replacement", false), ("bytes-cleared", true)] {
        let native_out = out.join(format!("native-{directory}"));
        let reference_out = out.join(format!("prost-{directory}"));
        fs::create_dir_all(&reference_out).unwrap();
        let mut native = pbrs::codegen::Config::new();
        native
            .out_dir(&native_out)
            .emit_kernel_stubs(false)
            .emit_reflection(false)
            .bytes(["."])
            .bytes(if clear { vec![] } else { vec!["ordinary"] });
        native.compile_protos(&FILES, includes).unwrap();
        let mut reference = prost_build::Config::new();
        reference
            .out_dir(&reference_out)
            .bytes(["."])
            .bytes(if clear { vec![] } else { vec!["ordinary"] });
        reference.compile_protos(&FILES, includes).unwrap();
        let native = fs::read_to_string(native_out.join("bytes.rs")).unwrap();
        let reference = fs::read_to_string(reference_out.join("proof.bytes.rs")).unwrap();
        for field in [
            "chosen", "ordinary", "maybe", "chunks", "lookup", "raw", "type",
        ] {
            assert_eq!(
                selected(&native, field, true),
                selected(&reference, field, false)
            );
        }
        if clear {
            assert_same_output(&native_out, &out.join("native-bytes-default"));
        }
        report.push_str(&format!("PASS bytes-selector {directory}\n"));
    }
    fs::write(out.join("bytes-selectors.txt"), report).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(out.join("native-bytes-old-parent"))
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .extern_path(".proof.bytes.Child", "crate::proof::bytes::Child")
        .compile_protos(&["bytes_old_parent.proto"], includes)
        .unwrap();
    // The consumer compiles both the frozen old parent and this current
    // unconfigured parent. Legitimate unrelated generator fixes may change
    // their text; ownership and wire-value behavior are checked at runtime.
    let reference_out = out.join("prost-bytes-old-parent");
    fs::create_dir_all(&reference_out).unwrap();
    prost_build::Config::new()
        .out_dir(reference_out)
        .extern_path(
            ".proof.bytes.Child",
            "crate::reference::proof::bytes::Child",
        )
        .compile_protos(&["bytes_old_parent.proto"], includes)
        .unwrap();
    for file in FILES.into_iter().chain(["bytes_old_parent.proto"]) {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
    println!("cargo:rerun-if-changed=bytes_build.rs");
    println!("cargo:rerun-if-changed=old_bytes_parent/bytes_old_parent.rs");
}

fn build_prefix_controls(out: &Path, includes: &[PathBuf]) {
    let files = ["bytes_prefix.proto", "bytes_prefix_defaults.proto"];
    for (label, prefix) in [
        ("default", None),
        ("relative", Some("runtime_alias")),
        ("absolute", Some("::runtime_alias")),
        ("local", Some("crate::runtime")),
    ] {
        let directory = out.join(format!("native-bytes-prefix-{label}"));
        let mut config = pbrs::codegen::Config::new();
        config
            .out_dir(&directory)
            .emit_kernel_stubs(false)
            .emit_reflection(false)
            .emit_json(true)
            .emit_text(true)
            .safe_message_defaults(true)
            .bytes(["."])
            .message_attribute(
                ".proof.bytes_prefix.pbrs",
                "#[doc = \"user pbrs::payload ::pbrs::payload runtime_alias::payload\"]",
            );
        if let Some(prefix) = prefix {
            config.runtime_crate(prefix);
        }
        config.compile_protos(&files, includes).unwrap();
        let source = fs::read_to_string(directory.join("bytes_prefix.rs")).unwrap();
        assert!(source.contains("user pbrs::payload ::pbrs::payload runtime_alias::payload"));
    }
    for filename in ["bytes_prefix.rs", "bytes_prefix_defaults.rs", "mod.rs"] {
        assert_eq!(
            fs::read(out.join("native-bytes-prefix-relative").join(filename)).unwrap(),
            fs::read(out.join("native-bytes-prefix-absolute").join(filename)).unwrap(),
            "relative/absolute runtime aliases differ: {filename}"
        );
    }
    for file in files {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}
