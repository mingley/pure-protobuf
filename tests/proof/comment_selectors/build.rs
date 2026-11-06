use std::{
    env, fs,
    path::{Path, PathBuf},
};
const FILES: [&str; 2] = ["main.proto", "imported.proto"];
const ATTR: &str = "#[doc = \"ATTR ::std::Vec ::pbrs::Message\"]";
const BASE: &str = "stubs=none,no_reflect=true,emit_json=false,emit_text=false,include_source_info=true,type_attribute=.proof.comments.Main=#[doc = \"ATTR ::std::Vec ::pbrs::Message\"]";
const SENTINELS: [&str; 12] = [
    "SRC_IMPORTED",
    "SRC_IMPORTED_FIELD",
    "SRC_MAIN",
    "SRC_FIELD",
    "SRC_NESTED_FIELD",
    "SRC_IMPORTED_SLOT",
    "SRC_ENUM_FIELD",
    "SRC_NESTED",
    "SRC_INNER_FIELD",
    "SRC_ENUM",
    "SRC_ENUM_ZERO",
    "SRC_ENUM_READY",
];
fn config(out: &Path) -> pbrs::codegen::Config {
    let mut config = pbrs::codegen::Config::new();
    config
        .out_dir(out)
        .include_source_info(true)
        .emit_kernel_stubs(false)
        .emit_reflection(false)
        .emit_json(false)
        .emit_text(false)
        .type_attribute(".proof.comments.Main", ATTR);
    config
}
fn source(path: &Path) -> String {
    fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .map(|p| fs::read_to_string(p).unwrap())
        .collect()
}
fn contains(source: &str, token: &str) -> bool {
    source.lines().any(|line| {
        line.trim()
            .strip_prefix("///")
            .is_some_and(|line| line.trim() == token)
    })
}
fn request(fds: &[u8], parameter: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for file in FILES {
        pbrs::rt::encode_len_field(&mut out, 1, file.as_bytes());
    }
    pbrs::rt::encode_len_field(&mut out, 2, parameter.as_bytes());
    let mut p = 0;
    while p < fds.len() {
        let (n, w) = pbrs::rt::decode_tag(fds, &mut p).unwrap();
        assert_eq!((n, w), (1, pbrs::rt::WIRE_LEN));
        let (s, e) = pbrs::rt::read_len_span(fds, &mut p).unwrap();
        pbrs::rt::encode_len_field(&mut out, 15, &fds[s..e]);
    }
    out
}
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let fds_path = out.join("fixture.fds");
    let fds = prost_build::Config::new()
        .file_descriptor_set_path(&fds_path)
        .load_fds(&FILES, &["proto"])
        .unwrap();
    let bytes = fs::read(&fds_path).unwrap();
    let cases: &[(&str, &[&str])] = &[
        ("none", &[]),
        ("global", &["."]),
        ("package", &[".proof.comments"]),
        ("message", &[".proof.comments.Main"]),
        ("field", &[".proof.comments.Main.value"]),
        ("suffix", &["value"]),
        ("nested", &["Main.Nested"]),
        ("enumeration", &[".proof.comments.State"]),
        ("enum-value", &["State.STATE_READY"]),
        ("imported", &[".proof.other"]),
        ("nonmatch", &[".λ", "*", " trailing ", "a,b"]),
        ("boundary", &[".proof.comments.Mai", "valu"]),
        ("reset", &[]),
        ("false-selected", &["Main.value"]),
        ("old-global", &["."]),
    ];
    for (name, selectors) in cases {
        let native = out.join(name).join("native");
        let reference = out.join(name).join("reference");
        fs::create_dir_all(&reference).unwrap();
        let mut cfg = config(&native);
        if *name == "reset" {
            cfg.disable_comments_for(["."]);
        }
        cfg.disable_comments_for(selectors.iter().copied());
        if *name == "false-selected" {
            cfg.disable_comments(false);
        }
        if *name == "old-global" {
            cfg.disable_comments(true)
                .disable_comments_for(["nonmatch"]);
        }
        cfg.compile_descriptor_set(&fds_path, &FILES, &["proto"])
            .unwrap();
        prost_build::Config::new()
            .out_dir(&reference)
            .type_attribute(".proof.comments.Main", ATTR)
            .disable_comments(selectors.iter().copied())
            .compile_fds(fds.clone())
            .unwrap();
        let actual = source(&native);
        let expected = source(&reference);
        for sentinel in SENTINELS {
            assert_eq!(
                contains(&actual, sentinel),
                contains(&expected, sentinel),
                "{name}: {sentinel}"
            );
        }
        assert_eq!(
            contains(&actual, "SRC_OTHER"),
            contains(&expected, "SRC_OTHER")
        );
        assert!(actual.contains(ATTR));
        assert!(actual.contains("The `proof.comments.Main` protobuf message."));
        // Public plugin/FDS entry points use the same descriptors and selection.
        let mut parameter = BASE.to_owned();
        if *name == "old-global" {
            parameter.push_str(",disable_comments=true");
        } else {
            for selector in *selectors {
                parameter.push_str(",disable_comments_for_hex=");
                for byte in selector.as_bytes() {
                    use std::fmt::Write as _;
                    write!(&mut parameter, "{byte:02x}").unwrap();
                }
            }
        }
        for (file, text) in
            pbrs::codegen::generate_from_code_generator_request(&request(&bytes, &parameter))
                .unwrap()
        {
            assert_eq!(
                text.as_bytes(),
                fs::read(native.join(file)).unwrap(),
                "plugin identity {name}"
            );
        }
    }
    // Explicit empty selection preserves default bytes, including after an earlier request.
    for file in ["main.rs", "imported.rs", "mod.rs"] {
        assert_eq!(
            fs::read(out.join("none/native").join(file)).unwrap(),
            fs::read(out.join("reset/native").join(file)).unwrap()
        );
    }
    println!("cargo:rerun-if-env-changed=COMMENT_BASELINE_PLUGIN");
    if let Some(plugin) = env::var_os("COMMENT_BASELINE_PLUGIN") {
        let baseline = out.join("baseline");
        fs::create_dir_all(&baseline).unwrap();
        let status =
            std::process::Command::new(env::var_os("PROTOC").unwrap_or_else(|| "protoc".into()))
                .arg("-Iproto")
                .arg(format!(
                    "--plugin=protoc-gen-pbrs={}",
                    Path::new(&plugin).display()
                ))
                .arg(format!("--pbrs_out={}", baseline.display()))
                .arg(format!("--pbrs_opt={BASE}"))
                .args(FILES)
                .status()
                .unwrap();
        assert!(status.success());
        for file in ["main.rs", "imported.rs", "mod.rs"] {
            assert_eq!(
                fs::read(baseline.join(file)).unwrap(),
                fs::read(out.join("none/native").join(file)).unwrap(),
                "baseline {file}"
            );
        }
    }
    std::thread::scope(|scope| {
        let bytes = &bytes;
        let selected = scope.spawn(move || {
            let p = format!("{BASE},disable_comments_for=.proof.comments.Main");
            let files =
                pbrs::codegen::generate_from_code_generator_request(&request(bytes, &p)).unwrap();
            assert!(!files.iter().any(|(_, s)| contains(s, "SRC_MAIN")));
        });
        let ordinary = scope.spawn(move || {
            let files =
                pbrs::codegen::generate_from_code_generator_request(&request(bytes, BASE)).unwrap();
            assert!(files.iter().any(|(_, s)| contains(s, "SRC_MAIN")));
        });
        selected.join().unwrap();
        ordinary.join().unwrap();
    });
    for mode in ["kernel", "tonic", "compat"] {
        let p = format!(
            "stubs={mode},no_reflect=true,emit_json=false,emit_text=false,disable_comments_for=.proof.comments.Echo.Call"
        );
        let files =
            pbrs::codegen::generate_from_code_generator_request(&request(&bytes, &p)).unwrap();
        let text = files.into_iter().map(|(_, s)| s).collect::<String>();
        assert!(!contains(&text, "SRC_METHOD"));
        assert!(contains(&text, "SRC_SERVICE"));
    }
    for file in FILES {
        println!("cargo:rerun-if-changed=proto/{file}");
    }
}
