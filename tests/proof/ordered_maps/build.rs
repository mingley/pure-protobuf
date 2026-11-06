use std::{
    env, fs,
    path::{Path, PathBuf},
};

const FILES: [&str; 2] = ["ordered.proto", "required.proto"];
const PATHS: [&str; 4] = [
    ".proof.ordered.Child",
    ".proof.ordered.Parent.Local",
    ".proof.ordered.Recursive",
    ".proof.ordered_required",
];

fn config(out: &Path) -> pbrs::codegen::Config {
    let mut config = pbrs::codegen::Config::new();
    config
        .out_dir(out)
        .emit_kernel_stubs(false)
        .include_source_info(true);
    config
}

fn selected(source: &str, message: &str, field: &str, native: bool) -> bool {
    let declaration = format!("struct {message} {{");
    let body = source
        .split_once(&declaration)
        .expect("message declaration")
        .1
        .split_once("\n}")
        .expect("message end")
        .0;
    let field_name = format!("{field}:");
    let field = body
        .lines()
        .find(|line| line.contains(&field_name))
        .or_else(|| {
            let declaration = format!("struct {message}Cold {{");
            let body = source.split_once(&declaration)?.1.split_once("\n}")?.0;
            body.lines().find(|line| line.contains(&field_name))
        })
        .expect("field declaration in hot or cold struct");
    field.contains(if native {
        "::pbrs::TreeMap<"
    } else {
        "::prost::alloc::collections::BTreeMap<"
    })
}

fn identity(left: &Path, right: &Path) {
    for filename in ["ordered.rs", "required.rs", "mod.rs"] {
        assert_eq!(
            fs::read(left.join(filename)).unwrap(),
            fs::read(right.join(filename)).unwrap(),
            "{filename}"
        );
    }
}

fn request(fds: &[u8], parameter: &str) -> Vec<u8> {
    let mut request = Vec::new();
    for filename in FILES {
        pbrs::rt::encode_len_field(&mut request, 1, filename.as_bytes());
    }
    pbrs::rt::encode_len_field(&mut request, 2, parameter.as_bytes());
    let mut pos = 0;
    while pos < fds.len() {
        let (number, wire) = pbrs::rt::decode_tag(fds, &mut pos).unwrap();
        assert_eq!((number, wire), (1, pbrs::rt::WIRE_LEN));
        let (start, end) = pbrs::rt::read_len_span(fds, &mut pos).unwrap();
        pbrs::rt::encode_len_field(&mut request, 15, &fds[start..end]);
    }
    request
}

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let includes = [PathBuf::from("proto")];
    let descriptor = out.join("ordered.fds");
    for (profile, owned, safe) in [
        ("selected", false, false),
        ("owned", true, false),
        ("safe", false, true),
    ] {
        let mut profile_config = config(&out.join(profile));
        if profile == "selected" {
            profile_config.file_descriptor_set_path(&descriptor);
        }
        profile_config
            .btree_map(PATHS)
            .bytes(["."])
            .owned_message_fields(owned)
            .safe_message_defaults(safe)
            .compile_protos(&FILES, &includes)
            .unwrap();
    }
    config(&out.join("descriptor"))
        .btree_map(PATHS)
        .bytes(["."])
        .compile_descriptor_set(&descriptor, &FILES, &includes)
        .unwrap();
    identity(&out.join("selected"), &out.join("descriptor"));
    let reference = out.join("reference");
    fs::create_dir_all(&reference).unwrap();
    prost_build::Config::new()
        .out_dir(&reference)
        .btree_map(PATHS)
        .bytes(["."])
        .compile_protos(&FILES, &includes)
        .unwrap();
    config(&out.join("current-parent"))
        .btree_map(PATHS)
        .extern_path(
            ".proof.ordered.Child",
            "crate::selected::proof::ordered::Child",
        )
        .compile_protos(&["old_parent.proto"], &includes)
        .unwrap();

    let default = out.join("default");
    config(&default).compile_protos(&FILES, &includes).unwrap();
    config(&out.join("empty"))
        .btree_map(std::iter::empty::<&str>())
        .compile_protos(&FILES, &includes)
        .unwrap();
    config(&out.join("nonmatching"))
        .btree_map(["*", ".proof.ordered.Childish"])
        .compile_protos(&FILES, &includes)
        .unwrap();
    identity(&default, &out.join("empty"));
    identity(&default, &out.join("nonmatching"));
    let fds = fs::read(descriptor).unwrap();
    let parameter = format!(
        "stubs=none,bytes=.,{}",
        PATHS.map(|path| format!("btree_map={path}")).join(",")
    );
    let selected_request =
        pbrs::codegen::generate_from_code_generator_request(&request(&fds, &parameter)).unwrap();
    for (name, source) in selected_request {
        assert_eq!(
            source.as_bytes(),
            fs::read(out.join("selected").join(name)).unwrap()
        );
    }
    // The request guard must reset both ordered-field and forced-eager indices.
    for (name, source) in
        pbrs::codegen::generate_from_code_generator_request(&request(&fds, "stubs=none")).unwrap()
    {
        assert_eq!(source.as_bytes(), fs::read(default.join(name)).unwrap());
    }
    println!("cargo:rustc-env=DEFAULT_ROOT={}", default.display());

    let cases = [
        ("global", "."),
        ("package", ".proof.ordered"),
        ("message", ".proof.ordered.Child"),
        ("field", ".proof.ordered.Child.numbers"),
        ("suffix", "Child.numbers"),
        ("field-suffix", "numbers"),
        ("bare-message", "Child"),
        ("boundary", ".proof.ordered.Childish"),
        ("suffix-boundary", "xnumbers"),
        ("star", "*"),
        ("empty", ""),
        ("synthetic-value", ".proof.ordered.Child.value"),
        ("synthetic-entry", ".proof.ordered.Child.NumbersEntry"),
        ("entry-value", ".proof.ordered.Child.NumbersEntry.value"),
        ("nested", ".proof.ordered.Parent.Local"),
        ("ordinary", ".proof.ordered.Control.ordinary"),
    ];
    let mut report = String::new();
    for (index, (label, selector)) in cases.into_iter().enumerate() {
        let native = out.join(format!("selector-native-{index}"));
        let peer = out.join(format!("selector-prost-{index}"));
        fs::create_dir_all(&peer).unwrap();
        config(&native)
            .btree_map([selector])
            .compile_protos(&FILES, &includes)
            .unwrap();
        prost_build::Config::new()
            .out_dir(&peer)
            .btree_map([selector])
            .compile_protos(&FILES, &includes)
            .unwrap();
        let native = fs::read_to_string(native.join("ordered.rs")).unwrap();
        let peer = fs::read_to_string(peer.join("proof.ordered.rs")).unwrap();
        for (message, field) in [
            ("Child", "numbers"),
            ("Child", "labels"),
            ("Child", "flags"),
            ("Child", "signed"),
            ("Child", "payloads"),
            ("Child", "values"),
            ("Child", "states"),
            ("Parent", "by_name"),
            ("Local", "entries"),
            ("Recursive", "numbers"),
            ("Control", "ordinary"),
        ] {
            assert_eq!(
                selected(&native, message, field, true),
                selected(&peer, message, field, false),
                "{label} {message}.{field}"
            );
        }
        report.push_str(&format!("PASS selector {label}\n"));
    }
    for (label, clear) in [("replace", false), ("clear", true)] {
        let native = out.join(label);
        let peer = out.join(format!("reference-{label}"));
        fs::create_dir_all(&peer).unwrap();
        let mut native_config = config(&native);
        let mut peer_config = prost_build::Config::new();
        peer_config.out_dir(&peer);
        native_config.btree_map(["."]);
        peer_config.btree_map(["."]);
        if clear {
            native_config.btree_map(std::iter::empty::<&str>());
            peer_config.btree_map(std::iter::empty::<&str>());
        } else {
            native_config.btree_map([".proof.ordered.Child.numbers"]);
            peer_config.btree_map([".proof.ordered.Child.numbers"]);
        }
        native_config.compile_protos(&FILES, &includes).unwrap();
        peer_config.compile_protos(&FILES, &includes).unwrap();
        let native_source = fs::read_to_string(native.join("ordered.rs")).unwrap();
        let peer_source = fs::read_to_string(peer.join("proof.ordered.rs")).unwrap();
        for field in ["numbers", "labels", "payloads"] {
            assert_eq!(
                selected(&native_source, "Child", field, true),
                selected(&peer_source, "Child", field, false)
            );
        }
        if clear {
            identity(&native, &default);
        }
        report.push_str(&format!("PASS selector {label}\n"));
    }
    fs::write(out.join("selectors.txt"), report).unwrap();
    println!(
        "cargo:rustc-env=SELECTOR_REPORT={}",
        out.join("selectors.txt").display()
    );

    for file in FILES.into_iter().chain(["old_parent.proto"]) {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
    println!("cargo:rerun-if-changed=old_parent/old_parent.rs");
}
