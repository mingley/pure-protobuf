use std::{env, fs, path::PathBuf};
mod bytes_build;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for name in [
        "native",
        "native-external",
        "prost",
        "prost-external",
        "prost-configured",
        "native-options",
        "prost-options",
        "native-names-default",
        "prost-names-default",
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
    let mut native_options = pbrs::codegen::Config::new();
    native_options
        .out_dir(out.join("native-options"))
        .emit_kernel_stubs(false)
        .emit_deps(true)
        .enable_type_names()
        .retain_enum_prefix()
        .type_name_domain(["."], "root.example")
        .type_name_domain([".proof.shared"], "package.example")
        .type_name_domain([".proof.shared.Outer.Inner"], "exact.example")
        .type_name_domain(["External"], "suffix.example")
        .type_name_domain(["External"], "ignored.example")
        .type_name_domain([".Loose.Nested"], "trailing.example/")
        .compile_protos(
            &[
                "case.proto",
                "imported.proto",
                "external.proto",
                "unpackaged.proto",
                "options_proto2.proto",
            ],
            &includes,
        )
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-options"))
        .retain_enum_prefix()
        .enable_type_names()
        .type_name_domain(["."], "root.example")
        .type_name_domain([".proof.shared"], "package.example")
        .type_name_domain([".proof.shared.Outer.Inner"], "exact.example")
        .type_name_domain(["External"], "suffix.example")
        .type_name_domain(["External"], "ignored.example")
        .type_name_domain([".Loose.Nested"], "trailing.example/")
        .compile_protos(
            &[
                "case.proto",
                "imported.proto",
                "external.proto",
                "unpackaged.proto",
                "options_proto2.proto",
            ],
            &includes,
        )
        .unwrap();
    pbrs::codegen::Config::new()
        .out_dir(out.join("native-names-default"))
        .emit_kernel_stubs(false)
        .enable_type_names()
        .compile_protos(&["unpackaged.proto"], &includes)
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-names-default"))
        .enable_type_names()
        .compile_protos(&["unpackaged.proto"], &includes)
        .unwrap();
    let collision_files = ["type_collisions.proto", "type_collision_use.proto"];
    for (directory, owned) in [
        ("native-collisions", false),
        ("native-collisions-owned", true),
    ] {
        pbrs::codegen::Config::new()
            .out_dir(out.join(directory))
            .emit_kernel_stubs(false)
            .emit_reflection(false)
            .owned_message_fields(owned)
            .compile_protos(&collision_files, &includes)
            .unwrap();
    }
    // Binary-only generation has no embedded descriptor bytes whose closure
    // varies with the target list. Public names must agree across these modes.
    for (directory, batches) in [
        (
            "native-collisions-separate",
            vec![vec![collision_files[0]], vec![collision_files[1]]],
        ),
        (
            "native-collisions-reversed",
            vec![vec![collision_files[1], collision_files[0]]],
        ),
    ] {
        for batch in batches {
            pbrs::codegen::Config::new()
                .out_dir(out.join(directory))
                .emit_kernel_stubs(false)
                .emit_reflection(false)
                .compile_protos(&batch, &includes)
                .unwrap();
        }
        for filename in ["type_collisions.rs", "type_collision_use.rs"] {
            assert_eq!(
                fs::read(out.join(directory).join(filename)).unwrap(),
                fs::read(out.join("native-collisions").join(filename)).unwrap(),
                "target order/request boundary changed {filename} in {directory}"
            );
        }
    }
    fs::create_dir_all(out.join("prost-collisions")).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(out.join("native-collisions-extern"))
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .extern_path(".proof.collisions.Foo", "crate::proof::collisions::Foo")
        .extern_path(
            ".proof.collisions.FooMut",
            "crate::proof::collisions::FooMut_",
        )
        .compile_protos(&[collision_files[1]], &includes)
        .unwrap();
    prost_build::Config::new()
        .out_dir(out.join("prost-collisions"))
        .compile_protos(&collision_files, &includes)
        .unwrap();
    // Configuration must not change a later default request on this thread.
    let reset_out = out.join("native-reset");
    pbrs::codegen::Config::new()
        .out_dir(&reset_out)
        .emit_kernel_stubs(false)
        .type_name_domain(["."], "disabled.example")
        .compile_protos(&["imported.proto"], &includes)
        .unwrap();
    let plain_enum = fs::read_to_string(reset_out.join("imported.rs")).unwrap();
    assert!(!plain_enum.contains("impl pbrs::Name for"));
    assert!(plain_enum.contains("pub const Ready:"));
    assert!(!plain_enum.contains("pub const StateReady:"));
    build_boxed_controls(&out, &includes);
    build_runtime_symbol_controls(&out, &includes);
    bytes_build::build(&out, &includes);
    for file in [
        "case.proto",
        "external.proto",
        "imported.proto",
        "unpackaged.proto",
        "options_proto2.proto",
        "type_collisions.proto",
        "type_collision_use.proto",
        "boxed.proto",
        "boxed_imported.proto",
        "boxed_required.proto",
        "runtime_symbols.proto",
        "runtime_symbols_import.proto",
    ] {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}

fn build_runtime_symbol_controls(out: &std::path::Path, includes: &[PathBuf]) {
    let files = ["runtime_symbols.proto", "runtime_symbols_import.proto"];
    for (directory, owned) in [
        ("native-runtime-symbols", false),
        ("native-runtime-symbols-owned", true),
    ] {
        pbrs::codegen::Config::new()
            .out_dir(out.join(directory))
            .emit_kernel_stubs(false)
            .owned_message_fields(owned)
            .compile_protos(&files, includes)
            .unwrap();
    }
    let prost_out = out.join("prost-runtime-symbols");
    fs::create_dir_all(&prost_out).unwrap();
    prost_build::Config::new()
        .out_dir(prost_out)
        .compile_protos(&files, includes)
        .unwrap();
}

fn boxed_configuration() -> pbrs::codegen::Config {
    let mut config = pbrs::codegen::Config::new();
    for path in BOXED_SELECTION {
        config.boxed(path);
    }
    config.emit_kernel_stubs(false);
    config
}

const BOXED_SELECTION: [&str; 7] = [
    ".proof.boxed.Parent.chosen",
    ".proof.boxed.Parent.choice.one",
    ".proof.boxed.Parent.single.lone",
    ".proof.boxed.Parent.optional_child",
    ".proof.boxed.Parent.imported",
    ".proof.boxed.Parent.required_child",
    ".proof.boxed.Node.next",
];

fn field_boxed(source: &str, field: &str, native: bool) -> bool {
    let marker = match (native, field) {
        (false, "one") => "One(".to_owned(),
        (false, "two") => "Two(".to_owned(),
        (false, "lone") => "Lone(".to_owned(),
        (false, _) => format!("pub {field}:"),
        (true, _) => format!("{field}:"),
    };
    let mut declaration = String::new();
    let mut depth = 0_i32;
    let mut found = false;
    for line in source.lines().map(str::trim) {
        if !found && line.starts_with(&marker) {
            found = true;
        }
        if found {
            declaration.push_str(line);
            for character in line.chars() {
                if matches!(character, '<' | '(') {
                    depth += 1;
                } else if matches!(character, '>' | ')') {
                    depth -= 1;
                }
            }
            if depth == 0 && line.ends_with(',') {
                return declaration.contains("Box<");
            }
        }
    }
    panic!("field {field} declaration was not found in generated source");
}

fn build_boxed_controls(out: &std::path::Path, includes: &[PathBuf]) {
    let files = [
        "boxed.proto",
        "boxed_imported.proto",
        "boxed_required.proto",
    ];
    let descriptor = out.join("boxed.fds");
    boxed_configuration()
        .out_dir(out.join("native-boxed"))
        .file_descriptor_set_path(&descriptor)
        .compile_protos(&files, includes)
        .unwrap();
    boxed_configuration()
        .out_dir(out.join("native-boxed-descriptor"))
        .compile_descriptor_set(&descriptor, &files, includes)
        .unwrap();
    for filename in [
        "boxed.rs",
        "boxed_imported.rs",
        "boxed_required.rs",
        "mod.rs",
    ] {
        assert_eq!(
            fs::read(out.join("native-boxed").join(filename)).unwrap(),
            fs::read(out.join("native-boxed-descriptor").join(filename)).unwrap(),
            "boxed builder and descriptor entry points differ for {filename}"
        );
    }
    pbrs::codegen::Config::new()
        .out_dir(out.join("native-boxed-default"))
        .emit_kernel_stubs(false)
        .compile_protos(&files, includes)
        .unwrap();
    boxed_configuration()
        .out_dir(out.join("native-boxed-owned"))
        .owned_message_fields(true)
        .compile_protos(&files, includes)
        .unwrap();
    let reference_out = out.join("prost-boxed");
    fs::create_dir_all(&reference_out).unwrap();
    let mut reference = prost_build::Config::new();
    for selector in BOXED_SELECTION {
        reference.boxed(selector);
    }
    reference
        .out_dir(reference_out)
        .compile_protos(&files, includes)
        .unwrap();
    // Compare message field selection with Prost. Its unrestricted global
    // matcher also boxes scalar declarations, which its scalar derive rejects;
    // the compiled consumer uses eligible message selectors above.
    let mut report = String::new();
    for (index, (label, selector)) in [
        ("global", "."),
        ("qualified", ".proof.boxed.Parent.chosen"),
        ("message-prefix", ".proof.boxed.Parent"),
        ("package-prefix", ".proof.boxed"),
        ("relative", "Parent.chosen"),
        ("relative-field", "chosen"),
        ("oneof-qualified", ".proof.boxed.Parent.choice.one"),
        ("oneof-suffix", "choice.one"),
        ("oneof-prefix", ".proof.boxed.Parent.choice"),
        ("oneof-missing-name", ".proof.boxed.Parent.one"),
        ("one-member-oneof", ".proof.boxed.Parent.single.lone"),
        ("synthetic-optional", ".proof.boxed.Parent.optional_child"),
        (
            "synthetic-name-ignored",
            ".proof.boxed.Parent._optional_child.optional_child",
        ),
        ("component-boundary", ".proof.boxed.Parentish"),
        ("wildcard-is-literal", "*"),
        ("repeated-exempt", ".proof.boxed.Parent.many"),
        ("map-exempt", ".proof.boxed.Parent.lookup"),
        ("empty", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let native_out = out.join(format!("boxed-selectors/native-{index}"));
        let prost_out = out.join(format!("boxed-selectors/prost-{index}"));
        fs::create_dir_all(&prost_out).unwrap();
        pbrs::codegen::Config::new()
            .out_dir(&native_out)
            .emit_kernel_stubs(false)
            .boxed(selector)
            .compile_protos(&files, includes)
            .unwrap();
        prost_build::Config::new()
            .out_dir(&prost_out)
            .boxed(selector)
            .compile_protos(&files, includes)
            .unwrap();
        let native = fs::read_to_string(native_out.join("boxed.rs")).unwrap();
        let prost = fs::read_to_string(prost_out.join("proof.boxed.rs")).unwrap();
        for field in [
            "chosen",
            "deferred",
            "many",
            "lookup",
            "one",
            "two",
            "lone",
            "optional_child",
            "imported",
            "required_child",
        ] {
            assert_eq!(
                field_boxed(&native, field, true),
                field_boxed(&prost, field, false),
                "boxed selector {selector:?} differs for {field}"
            );
        }
        report.push_str(&format!("PASS boxed-selector {label} {selector:?}\n"));
    }
    fs::write(out.join("boxed-selectors.txt"), report).unwrap();
}
