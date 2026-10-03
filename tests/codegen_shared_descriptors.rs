//! Opt-in GN-03 source descriptor sharing; defaults and flat includes stay stable.
#![cfg(feature = "codegen")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "synchronous generated-consumer regression tests"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

const HELPER: &str = "__pbrs_shared_descriptors.rs";

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(u8::try_from(value & 127).unwrap() | 128);
        value >>= 7;
    }
    out.push(u8::try_from(value).unwrap());
}

fn field_bytes(out: &mut Vec<u8>, number: u32, value: &[u8]) {
    varint(out, (u64::from(number) << 3) | 2);
    varint(out, u64::try_from(value.len()).unwrap());
    out.extend_from_slice(value);
}

fn field_string(out: &mut Vec<u8>, number: u32, value: &str) {
    field_bytes(out, number, value.as_bytes());
}

fn field_number(out: &mut Vec<u8>, number: u32, value: u64) {
    varint(out, u64::from(number) << 3);
    varint(out, value);
}

fn schema(name: &str, package: &str, message: &str, field: &str, kind: u64) -> Vec<u8> {
    let mut f = Vec::new();
    field_string(&mut f, 1, field);
    field_number(&mut f, 3, 1);
    field_number(&mut f, 4, 1);
    field_number(&mut f, 5, kind);
    let mut m = Vec::new();
    field_string(&mut m, 1, message);
    field_bytes(&mut m, 2, &f);
    let mut file = Vec::new();
    field_string(&mut file, 1, name);
    field_string(&mut file, 2, package);
    field_bytes(&mut file, 4, &m);
    field_string(&mut file, 12, "proto3");
    file
}

fn fixture() -> Vec<Vec<u8>> {
    vec![
        schema("first.proto", "first", "First", "id", 5),
        schema("second.proto", "second", "Second", "label", 9),
    ]
}

fn any_schema() -> Vec<u8> {
    let mut message = Vec::new();
    field_string(&mut message, 1, "Any");
    for (name, number, kind) in [("type_url", 1, 9), ("value", 2, 12)] {
        let mut field = Vec::new();
        field_string(&mut field, 1, name);
        field_number(&mut field, 3, number);
        field_number(&mut field, 4, 1);
        field_number(&mut field, 5, kind);
        field_bytes(&mut message, 2, &field);
    }
    let mut file = Vec::new();
    field_string(&mut file, 1, "google/protobuf/any.proto");
    field_string(&mut file, 2, "google.protobuf");
    field_bytes(&mut file, 4, &message);
    field_string(&mut file, 12, "proto3");
    file
}

fn generate(
    files: &[Vec<u8>],
    targets: &[&str],
    options: &str,
) -> Result<BTreeMap<String, String>, pbrs::codegen::CodegenError> {
    let mut request = Vec::new();
    for target in targets {
        field_string(&mut request, 1, target);
    }
    if !options.is_empty() {
        field_string(&mut request, 2, options);
    }
    for file in files {
        field_bytes(&mut request, 15, file);
    }
    pbrs::codegen::generate_from_code_generator_request(&request)
        .map(|files| files.into_iter().collect())
}

fn compile(
    name: &str,
    generated: &BTreeMap<String, String>,
    source: &str,
    features: &[&str],
    succeeds: bool,
) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("target/gn03-tests").join(name);
    if let Err(error) = std::fs::remove_dir_all(&out) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
    std::fs::create_dir_all(out.join("src")).unwrap();
    for (name, value) in generated {
        let path = out.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value).unwrap();
    }
    std::fs::write(out.join("Cargo.toml"), format!(
        "[package]\nname = \"gn03-{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nrenamed_pbrs = {{ package = \"pbrs\", path = {root:?}, default-features = false, features = {features:?} }}\n"
    )).unwrap();
    std::fs::write(out.join("src/main.rs"), source).unwrap();
    let result = Command::new("cargo")
        .args(["run", "--offline", "--quiet"])
        .current_dir(&out)
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/integration-consumers"),
        )
        .env("CARGO_BUILD_JOBS", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    assert_eq!(result.status.success(), succeeds, "{stderr}");
    stderr
}

#[test]
fn opt_in_embeds_one_literal_and_default_single_output_remain_exact() {
    let files = fixture();
    let targets = ["first.proto", "second.proto"];
    let default = generate(&files, &targets, "stubs=none").unwrap();
    assert_eq!(
        default
            .values()
            .map(|s| s.matches("pub const FILE_DESCRIPTOR_SET: &[u8] = ").count())
            .sum::<usize>(),
        2
    );
    let shared = generate(&files, &targets, "stubs=none,shared_descriptor_set=true").unwrap();
    assert_eq!(
        shared
            .values()
            .map(|s| s.matches("pub const FILE_DESCRIPTOR_SET: &[u8] = ").count())
            .sum::<usize>(),
        1
    );
    assert!(shared.contains_key(HELPER));
    assert!(shared[HELPER].contains("pub const FILE_DESCRIPTOR_SET: &[u8] = b\""));
    let mut reversed = files.clone();
    reversed.reverse();
    assert_eq!(
        shared,
        generate(
            &reversed,
            &["second.proto", "first.proto"],
            "stubs=none,shared_descriptor_set=true"
        )
        .unwrap()
    );
    assert!(shared["mod.rs"].contains("mod __pbrs_shared_descriptors"));
    let custom = generate(
        &files,
        &targets,
        "stubs=none,shared_descriptor_set=true,include_file=registry.rs",
    )
    .unwrap();
    assert_eq!(custom["registry.rs"], shared["mod.rs"]);
    assert_eq!(custom[HELPER], shared[HELPER]);
    for target in ["first.rs", "second.rs"] {
        assert!(shared[target].contains("crate::__pbrs_shared_descriptors::FILE_DESCRIPTOR_SET"));
        assert!(shared[target].contains("crate::__pbrs_shared_descriptors::generated_pool()"));
    }
    assert_eq!(
        default,
        generate(&files, &targets, "stubs=none,shared_descriptor_set=false").unwrap()
    );
    assert_eq!(
        generate(&files, &["first.proto"], "stubs=none").unwrap(),
        generate(
            &files,
            &["first.proto"],
            "stubs=none,shared_descriptor_set=true"
        )
        .unwrap()
    );
}

#[test]
fn shared_pool_is_lazy_and_reflection_json_text_and_binary_are_unchanged() {
    let mut files = fixture();
    files.push(any_schema());
    let generated = generate(
        &files,
        &["first.proto", "second.proto", "google/protobuf/any.proto"],
        "stubs=none,shared_descriptor_set=true,runtime_crate=renamed_pbrs",
    )
    .unwrap();
    compile(
        "runtime",
        &generated,
        r#"
include!("../mod.rs");
fn main() {
    use renamed_pbrs::{Parse, Serialize};
    assert!(__pbrs_shared_descriptors::POOL.get().is_none());
    let mut first = first::First::new();
    first.set_id(41);
    let wire = first.serialize().unwrap();
    assert_eq!(first::First::parse(&wire).unwrap().id(), 41);
    assert!(__pbrs_shared_descriptors::POOL.get().is_none());
    assert_eq!(first.to_json().unwrap(), "{\"id\":41}");
    assert_eq!(first::First::from_json("{\"id\":41}").unwrap().id(), 41);
    let text = first.to_text().unwrap();
    assert_eq!(first::First::from_text(&text).unwrap().id(), 41);
    assert!(__pbrs_shared_descriptors::POOL.get().is_none());
    let mut any = google::protobuf::Any::new();
    any.set_type_url("type.googleapis.com/first.First");
    any.set_value(wire.clone());
    let json = any.to_json().unwrap();
    assert_eq!(json, "{\"@type\":\"type.googleapis.com/first.First\",\"id\":41}");
    assert_eq!(google::protobuf::Any::from_json(&json).unwrap().value(), wire);
    let any_text = any.to_text().unwrap();
    assert_eq!(google::protobuf::Any::from_text(&any_text).unwrap().value(), wire);
    let pool = __pbrs_shared_descriptors::POOL.get().unwrap();
    assert!(pool.get_message("first.First").is_some());
    assert!(pool.get_message("second.Second").is_some());
    assert!(std::sync::Arc::ptr_eq(pool, &__pbrs_shared_descriptors::generated_pool()));
    assert!(std::ptr::eq(first::FILE_DESCRIPTOR_SET, second::FILE_DESCRIPTOR_SET));
    assert!(std::ptr::eq(first::FILE_DESCRIPTOR_SET, google::protobuf::FILE_DESCRIPTOR_SET));
    assert!(std::ptr::eq(first::FILE_DESCRIPTOR_SET, __pbrs_shared_descriptors::FILE_DESCRIPTOR_SET));
    let parsed = renamed_pbrs::DescriptorPool::from_file_descriptor_set(first::FILE_DESCRIPTOR_SET).unwrap();
    assert!(parsed.get_message("second.Second").is_some());
    let mut second = second::Second::new();
    second.set_label("payload");
    assert_eq!(second::Second::from_json(&second.to_json().unwrap()).unwrap().label(), "payload");
    assert_eq!(second::Second::from_text(&second.to_text().unwrap()).unwrap().label(), "payload");
}
"#,
        &["reflect", "json", "text"],
        true,
    );
    let stderr = compile(
        "flat-missing-registry",
        &generated,
        "mod first { include!(\"../first.rs\"); } fn main() {}",
        &["reflect", "json", "text"],
        false,
    );
    assert!(stderr.contains("__pbrs_shared_descriptors"), "{stderr}");
    compile(
        "flat-with-helper",
        &generated,
        r#"
mod __pbrs_shared_descriptors { include!("../__pbrs_shared_descriptors.rs"); }
mod first { include!("../first.rs"); }
mod second { include!("../second.rs"); }
fn main() {
    let mut m = first::First::new(); m.set_id(53);
    assert_eq!(first::First::from_text(&m.to_text().unwrap()).unwrap().id(), 53);
    assert!(std::ptr::eq(first::FILE_DESCRIPTOR_SET, second::FILE_DESCRIPTOR_SET));
}
"#,
        &["reflect", "json", "text"],
        true,
    );
}

#[test]
fn reflection_free_and_single_flat_consumers_do_not_require_shared_metadata() {
    let generated = generate(
        &fixture(),
        &["first.proto", "second.proto"],
        "stubs=none,no_reflect=true,shared_descriptor_set=true,runtime_crate=renamed_pbrs",
    )
    .unwrap();
    assert!(!generated.contains_key(HELPER));
    for source in generated.values() {
        assert!(!source.contains("FILE_DESCRIPTOR_SET"));
        assert!(!source.contains("generated_pool"));
        assert!(!source.contains("DescriptorPool"));
    }
    compile(
        "no-reflect",
        &generated,
        r#"
mod first { include!("../first.rs"); }
fn main() {
    use renamed_pbrs::{Parse, Serialize};
    let mut m = first::First::new(); m.set_id(43);
    assert_eq!(first::First::parse(&m.serialize().unwrap()).unwrap().id(), 43);
}
"#,
        &[],
        true,
    );
    let single = generate(
        &fixture(),
        &["first.proto"],
        "stubs=none,shared_descriptor_set=true,runtime_crate=renamed_pbrs",
    )
    .unwrap();
    compile(
        "single-flat",
        &single,
        r#"
mod first { include!("../first.rs"); }
fn main() { let mut m = first::First::new(); m.set_id(47); assert_eq!(m.id(), 47); }
"#,
        &["reflect", "json", "text"],
        true,
    );
}

#[test]
fn reserved_outputs_packages_and_shared_pool_conflicts_fail_explicitly() {
    let files = fixture();
    assert!(
        generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,shared_descriptor_set=invalid"
        )
        .unwrap_err()
        .to_string()
        .contains("shared_descriptor_set")
    );
    assert_eq!(
        generate(&files, &["first.proto"], "stubs=none,shared_pool=true").unwrap(),
        generate(
            &files,
            &["first.proto"],
            "stubs=none,shared_pool=true,shared_descriptor_set=true"
        )
        .unwrap()
    );
    assert_eq!(
        generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,no_reflect=true,shared_pool=true"
        )
        .unwrap(),
        generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,no_reflect=true,shared_pool=true,shared_descriptor_set=true"
        )
        .unwrap()
    );
    for option in [
        "shared_pool=true",
        "include_file=__pbrs_shared_descriptors.rs",
        "include_file=./__pbrs_shared_descriptors.rs",
        "include_file=first.rs",
        "include_file=./first.rs",
        "include_file=nested/mod.rs",
        "include_file=/mod.rs",
        "include_file=../mod.rs",
        "include_file=mod.rs/",
    ] {
        let error = generate(
            &files,
            &["first.proto", "second.proto"],
            &format!("stubs=none,shared_descriptor_set=true,{option}"),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("shared_descriptor_set"),
            "{error}"
        );
    }
    for (name, package) in [
        ("__pbrs_shared_descriptors.proto", "collision"),
        ("nested/__pbrs_shared_descriptors.proto", "collision"),
        ("first.proto", "__pbrs_shared_descriptors.child"),
    ] {
        let colliding = vec![schema(name, package, "First", "id", 5), files[1].clone()];
        let error = generate(
            &colliding,
            &[name, "second.proto"],
            "stubs=none,shared_descriptor_set=true",
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("shared_descriptor_set"),
            "{error}"
        );
    }
}

#[test]
fn config_builder_and_plugin_select_identical_shared_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("target/gn03-tests/builder");
    std::fs::create_dir_all(&out).unwrap();
    let mut fds = Vec::new();
    for file in fixture() {
        field_bytes(&mut fds, 1, &file);
    }
    let path = out.join("fixture.fds");
    std::fs::write(&path, &fds).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(&out)
        .emit_kernel_stubs(false)
        .shared_descriptor_set(true)
        .compile_descriptor_set(
            &path,
            &["first.proto", "second.proto"],
            &[std::path::Path::new(".")],
        )
        .unwrap();
    for (name, expected) in generate(
        &fixture(),
        &["first.proto", "second.proto"],
        "stubs=none,shared_descriptor_set=true",
    )
    .unwrap()
    {
        assert_eq!(std::fs::read_to_string(out.join(name)).unwrap(), expected);
    }
}

fn root_enum_schema() -> Vec<u8> {
    let mut value = Vec::new();
    field_string(&mut value, 1, "ZERO");
    field_number(&mut value, 2, 0);
    let mut en = Vec::new();
    field_string(&mut en, 1, "__pbrs_shared_descriptors");
    field_bytes(&mut en, 2, &value);
    let mut file = Vec::new();
    field_string(&mut file, 1, "first.proto");
    field_bytes(&mut file, 5, &en);
    field_string(&mut file, 12, "proto3");
    file
}

fn root_nested_schema() -> Vec<u8> {
    let mut child = Vec::new();
    field_string(&mut child, 1, "Child");
    let mut parent = Vec::new();
    field_string(&mut parent, 1, "_PbrsSharedDescriptors");
    field_bytes(&mut parent, 3, &child);
    let mut file = Vec::new();
    field_string(&mut file, 1, "first.proto");
    field_bytes(&mut file, 4, &parent);
    field_string(&mut file, 12, "proto3");
    file
}

#[test]
fn reserved_root_types_and_nested_modules_cannot_be_shadowed() {
    for (name, schema, expression) in [
        (
            "message",
            schema("first.proto", "", "__pbrs_shared_descriptors", "id", 5),
            "__pbrs_shared_descriptors::new()",
        ),
        (
            "enum",
            root_enum_schema(),
            "__pbrs_shared_descriptors::default()",
        ),
        (
            "nested",
            root_nested_schema(),
            "__pbrs_shared_descriptors::Child::new()",
        ),
    ] {
        let files = vec![schema, fixture()[1].clone()];
        let default = generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,runtime_crate=renamed_pbrs",
        )
        .unwrap();
        compile(
            &format!("root-{name}-default"),
            &default,
            &format!("include!(\"../mod.rs\"); fn main() {{ let _value = {expression}; }}"),
            &["reflect", "json", "text"],
            true,
        );
        match generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,shared_descriptor_set=true",
        ) {
            Ok(_) => panic!("reserved root {name} would be shadowed by the metadata helper"),
            Err(error) => assert!(
                error.to_string().contains("shared_descriptor_set"),
                "{error}"
            ),
        }
    }
}

#[test]
fn reserved_root_service_trait_is_rejected_only_when_emitted() {
    let mut first = schema("first.proto", "", "First", "id", 5);
    let mut service = Vec::new();
    field_string(&mut service, 1, "__pbrs_shared_descriptors");
    field_bytes(&mut first, 6, &service);
    let files = vec![first, fixture()[1].clone()];
    for stubs in ["kernel", "tonic", "compat"] {
        let default = generate(
            &files,
            &["first.proto", "second.proto"],
            &format!("stubs={stubs}"),
        )
        .unwrap();
        assert!(default["first.rs"].contains("pub trait __pbrs_shared_descriptors"));
        let error = generate(
            &files,
            &["first.proto", "second.proto"],
            &format!("stubs={stubs},shared_descriptor_set=true"),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("shared_descriptor_set"),
            "{error}"
        );
    }
    assert!(
        generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=none,shared_descriptor_set=true"
        )
        .is_ok()
    );
    assert!(
        generate(
            &files,
            &["first.proto", "second.proto"],
            "stubs=kernel,build_server=false,shared_descriptor_set=true"
        )
        .is_ok()
    );
}
