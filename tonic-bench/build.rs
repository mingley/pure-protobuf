use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_dir = manifest.join("../proto");
    let hello = proto_dir.join("hello.proto");
    let cases = proto_dir.join("codec_cases.proto");
    let checked = manifest.join("checked_v4");
    println!("cargo:rerun-if-changed={}", hello.display());
    println!("cargo:rerun-if-changed={}", cases.display());
    for name in ["codec_cases.proto", "generated.rs", "codec_cases.u.pb.rs"] {
        println!("cargo:rerun-if-changed={}", checked.join(name).display());
    }
    assert!(
        fs::read(&cases).expect("read benchmark schema")
            == fs::read(checked.join("codec_cases.proto")).expect("read checked v4 schema"),
        "benchmark schema changed; regenerate and review checked v4 output"
    );

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let pbrs_dir = out.join("pbrs");
    let prost_dir = out.join("prost");
    let v4_dir = out.join("v4");
    fs::create_dir_all(&pbrs_dir).unwrap();
    fs::create_dir_all(&prost_dir).unwrap();
    fs::create_dir_all(&v4_dir).unwrap();

    prost_build::Config::new()
        .out_dir(&prost_dir)
        .compile_protos(&[&hello, &cases], &[&proto_dir])
        .expect("prost-build");

    gen_pbrs(&cases, &proto_dir, &pbrs_dir);
    println!("cargo:rerun-if-env-changed=PURE_PROTOBUF_CHECKED_V4");
    println!("cargo:rerun-if-env-changed=PROTOC");
    match env::var("PURE_PROTOBUF_CHECKED_V4") {
        Ok(mode) if mode == "1" => gen_checked_v4(&checked, &v4_dir),
        Err(env::VarError::NotPresent) => gen_v4(&cases, &proto_dir, &checked, &v4_dir),
        Ok(mode) => panic!("PURE_PROTOBUF_CHECKED_V4 must be 1 when set, got {mode:?}"),
        Err(error) => panic!("invalid PURE_PROTOBUF_CHECKED_V4: {error}"),
    }
}

fn gen_pbrs(proto: &Path, proto_dir: &Path, out: &Path) {
    pbrs::codegen::Config::new()
        .out_dir(out)
        .compile_protos(&[proto], &[proto_dir])
        .expect("pbrs codegen");
}

fn gen_v4(proto: &Path, proto_dir: &Path, checked: &Path, out: &Path) {
    let protoc = env::var_os("PROTOC").unwrap_or_else(|| "protoc".into());
    let version = Command::new(&protoc)
        .arg("--version")
        .output()
        .expect("run pinned v35.1 protoc --version");
    assert!(version.status.success(), "protoc --version failed");
    assert_eq!(
        String::from_utf8(version.stdout)
            .expect("protoc --version returned non-UTF-8")
            .trim(),
        "libprotoc 35.1",
        "tonic-bench v4 requires pinned protoc 35.1; set PROTOC or PURE_PROTOBUF_CHECKED_V4=1"
    );
    let status = Command::new(&protoc)
        .arg(format!("--rust_out={}", out.display()))
        .arg("--rust_opt=experimental-codegen=enabled,kernel=upb")
        .arg("-I")
        .arg(proto_dir)
        .arg(proto)
        .status()
        .expect("protoc --rust_out");
    assert!(status.success(), "protoc --rust_out failed: {status}");
    for name in ["generated.rs", "codec_cases.u.pb.rs"] {
        assert!(
            fs::read(out.join(name)).expect("read generated v4 binding")
                == fs::read(checked.join(name)).expect("read checked v4 binding"),
            "pinned protoc generated unexpected {name}; review before timing"
        );
    }
    rewrite_generated_paths(out);
}

fn gen_checked_v4(checked: &Path, out: &Path) {
    for name in ["generated.rs", "codec_cases.u.pb.rs"] {
        fs::copy(checked.join(name), out.join(name)).expect("copy checked v4 binding");
    }
    rewrite_generated_paths(out);
}

fn rewrite_generated_paths(out: &Path) {
    let r#gen = out.join("generated.rs");
    let src = fs::read_to_string(&r#gen)
        .unwrap_or_else(|_| panic!("missing v4 generated.rs in {}", out.display()));
    let relative = "#[path=\"codec_cases.u.pb.rs\"]";
    assert!(
        src.contains(relative),
        "unexpected v4 generated.rs module path"
    );
    let absolute = format!("#[path=\"{}\"]", out.join("codec_cases.u.pb.rs").display());
    fs::write(r#gen, src.replace(relative, &absolute)).expect("rewrite v4 generated.rs path");
}
