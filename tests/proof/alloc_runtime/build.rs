use std::{env, fs, path::Path};

const FILES: [&str; 2] = ["fields.proto", "legacy.proto"];
const LITERAL: &str = "Keep user text ::std::boxed::Box, ::pbrs::Owned and ::std::sync::OnceLock";

fn config(out: &Path) -> pbrs::codegen::Config {
    let mut config = pbrs::codegen::Config::new();
    config
        .out_dir(out)
        .include_source_info(true)
        .enable_type_names();
    config
}

fn ordinary(out: &Path) -> pbrs::codegen::Config {
    let mut config = config(out);
    config
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .emit_json(false)
        .emit_text(false);
    config
}

fn equal(left: &Path, right: &Path) {
    for file in ["fields.rs", "legacy.rs", "mod.rs"] {
        assert_eq!(
            fs::read(left.join(file)).unwrap(),
            fs::read(right.join(file)).unwrap(),
            "{file}"
        );
    }
}

fn request(fds: &[u8], parameters: &str) -> Vec<u8> {
    let mut request = Vec::new();
    for file in FILES {
        pbrs::rt::encode_len_field(&mut request, 1, file.as_bytes());
    }
    pbrs::rt::encode_len_field(&mut request, 2, parameters.as_bytes());
    let mut position = 0;
    while position < fds.len() {
        let (number, wire) = pbrs::rt::decode_tag(fds, &mut position).unwrap();
        assert_eq!((number, wire), (1, pbrs::rt::WIRE_LEN));
        let (start, end) = pbrs::rt::read_len_span(fds, &mut position).unwrap();
        pbrs::rt::encode_len_field(&mut request, 15, &fds[start..end]);
    }
    request
}

fn main() {
    let out = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let includes = ["proto"];
    let fds = out.join("fields.fds");
    ordinary(&out.join("ordinary-before"))
        .compile_protos(&FILES, &includes)
        .unwrap();
    for (profile, owned, safe, selected) in [
        ("lazy", false, false, false),
        ("owned", true, false, false),
        ("selected", false, false, true),
        ("safe", false, true, false),
    ] {
        let mut config = config(&out.join(profile));
        config
            .alloc_only(true)
            .owned_message_fields(owned)
            .safe_message_defaults(safe)
            .type_attribute(
                ".proof.embedded.Fields",
                "#[doc = \"user attribute ::std::boxed::Box ::pbrs::Owned\"]",
            );
        if profile == "lazy" {
            config.file_descriptor_set_path(&fds);
        }
        if selected {
            config.bytes(["."]).boxed(".").btree_map(["."]);
        }
        if safe {
            config.runtime_crate("crate::wire_runtime");
        }
        config.compile_protos(&FILES, &includes).unwrap();
        let source = fs::read_to_string(out.join(profile).join("fields.rs")).unwrap();
        assert!(source.contains(LITERAL));
        assert!(source.contains("user attribute ::std::boxed::Box ::pbrs::Owned"));
    }
    config(&out.join("external"))
        .alloc_only(true)
        .extern_path(".proof.embedded.Child", "crate::foreign::std::Child")
        .compile_protos(&FILES, &includes)
        .unwrap();
    let external = fs::read_to_string(out.join("external/fields.rs")).unwrap();
    assert!(external.contains("crate::foreign::std::Child"));
    assert!(external.contains(LITERAL));
    let mut after = ordinary(&out.join("ordinary-after"));
    after
        .alloc_only(true)
        .alloc_only(false)
        .compile_protos(&FILES, &includes)
        .unwrap();
    equal(&out.join("ordinary-before"), &out.join("ordinary-after"));
    println!("cargo:rerun-if-env-changed=ALLOC_PROOF_BASELINE_PLUGIN");
    if let Some(plugin) = env::var_os("ALLOC_PROOF_BASELINE_PLUGIN") {
        let baseline = out.join("baseline");
        fs::create_dir_all(&baseline).unwrap();
        let status = std::process::Command::new(env::var_os("PROTOC").unwrap_or_else(|| "protoc".into()))
            .arg("-Iproto").arg(format!("--plugin=protoc-gen-pbrs={}", Path::new(&plugin).display()))
            .arg(format!("--pbrs_out={}", baseline.display()))
            .arg("--pbrs_opt=stubs=none,no_reflect=true,emit_json=false,emit_text=false,enable_type_names=true,include_source_info=true")
            .args(FILES).status().unwrap();
        assert!(status.success());
        equal(&out.join("ordinary-before"), &baseline);
    }

    let descriptor = fs::read(&fds).unwrap();
    // Compare entry points without adding attributes: both process the same FDS.
    let mut descriptor_config = config(&out.join("descriptor"));
    descriptor_config
        .alloc_only(true)
        .compile_descriptor_set(&fds, &FILES, &includes)
        .unwrap();
    for (name, source) in pbrs::codegen::generate_from_code_generator_request(&request(
        &descriptor,
        "alloc_only=true,enable_type_names=true,include_source_info=true",
    ))
    .unwrap()
    {
        assert_eq!(
            source.as_bytes(),
            fs::read(out.join("descriptor").join(name)).unwrap()
        );
    }
    for parameter in [
        "emit_json=true",
        "emit_text=true",
        "no_reflect=false",
        "stubs=kernel",
        "stubs=tonic",
        "stubs=compat",
        "shared_pool=true",
        "shared_descriptor_set=true",
    ] {
        assert!(
            matches!(pbrs::codegen::generate_from_code_generator_request(
            &request(&descriptor, &format!("alloc_only=true,{parameter}"))
        ), Err(pbrs::codegen::CodegenError::InvalidParameter { key, .. }) if key == "alloc_only")
        );
    }
    let reference = out.join("reference");
    fs::create_dir_all(&reference).unwrap();
    prost_build::Config::new()
        .out_dir(reference)
        .btree_map(["."])
        .enable_type_names()
        .compile_protos(&FILES, &includes)
        .unwrap();
    config(&out.join("extensions"))
        .alloc_only(true)
        .typed_extension(".proof.alloc_ext.value")
        .compile_protos(&["extensions.proto"], &includes)
        .unwrap();
    println!("cargo:rerun-if-changed=proto/extensions.proto");
    println!("cargo:rustc-env=ALLOC_PROOF_OUTPUT={}", out.display());
    for file in FILES {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}
