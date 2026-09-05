//! Isolated consumers of `cargo package` tarballs, not the git tree.
//!
//! Unpacks each `.crate` outside the workspace and `cargo run`s a tiny bin
//! against the path. Adapters' packaged manifests depend on crates.io `pbrs`;
//! `[patch.crates-io]` points that at the unpacked core so this test does not
//! need the version to be live.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    reason = "integration tests are sync; they spawn cargo and write fixtures"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn apply_cargo_home(cmd: &mut Command) {
    if let Some(h) = std::env::var_os("CARGO_HOME") {
        cmd.env("CARGO_HOME", h);
    }
}

fn dump(out: &Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// First `[package]` `name` / `version` in a manifest. Not a TOML parser.
fn package_ident(manifest: &Path) -> (String, String) {
    let text = std::fs::read_to_string(manifest).unwrap();
    let mut in_package = false;
    let mut name = None;
    let mut version = None;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = t.strip_prefix("name = \"") {
            name = Some(rest.trim_end_matches('"').to_string());
        }
        if let Some(rest) = t.strip_prefix("version = \"") {
            version = Some(rest.trim_end_matches('"').to_string());
        }
    }
    (
        name.expect("package name"),
        version.expect("package version"),
    )
}

fn outside_dir(kind: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pbrs-{kind}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cargo_package_list(pkg: &str, target_dir: &Path) -> Vec<String> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "package",
        "-p",
        pkg,
        "--list",
        "--no-verify",
        "--offline",
        "--allow-dirty",
    ])
    .current_dir(repo_root())
    .env("CARGO_TARGET_DIR", target_dir)
    .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    let out = cmd.output().expect("cargo package --list");
    assert!(
        out.status.success(),
        "cargo package --list -p {pkg} failed:\n{}",
        dump(&out)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn cargo_package(pkg: &str, target_dir: &Path) {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "package",
        "-p",
        pkg,
        "--no-verify",
        "--offline",
        "--allow-dirty",
    ])
    .current_dir(repo_root())
    .env("CARGO_TARGET_DIR", target_dir)
    .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    let out = cmd.output().expect("cargo package");
    assert!(
        out.status.success(),
        "cargo package -p {pkg} failed:\n{}",
        dump(&out)
    );
}

fn unpack_crate(crate_file: &Path, dest: &Path) -> PathBuf {
    std::fs::create_dir_all(dest).unwrap();
    let out = Command::new("tar")
        .args(["-xzf"])
        .arg(crate_file)
        .current_dir(dest)
        .output()
        .expect("tar");
    assert!(
        out.status.success(),
        "tar -xzf {} failed:\n{}",
        crate_file.display(),
        dump(&out)
    );
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dest).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            dirs.push(entry.path());
        }
    }
    assert_eq!(
        dirs.len(),
        1,
        "expected one top-level directory in {}",
        dest.display()
    );
    dirs.pop().unwrap()
}

fn write_consumer(dir: &Path, toml: &str, main_rs: &str) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), toml).unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();
}

fn cargo_run_consumer(dir: &Path, expected_stdout: &str) {
    let target = dir.join("target");
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--offline", "--quiet"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    let out = cmd.output().expect("cargo run consumer");
    assert!(
        out.status.success(),
        "isolated consumer in {} failed:\n{}",
        dir.display(),
        dump(&out)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        expected_stdout,
        "consumer stdout:\n{}",
        dump(&out)
    );
    println!("{}", expected_stdout);
}

fn assert_licenses(pkg: &str, list: &[String]) {
    for want in ["LICENSE-MIT", "LICENSE-APACHE"] {
        assert!(
            list.iter()
                .any(|line| { Path::new(line).file_name().is_some_and(|n| n == want) }),
            "{pkg} package list missing {want}:\n{}",
            list.join("\n")
        );
    }
}

#[test]
fn unpacked_crates_build_isolated_consumers() {
    let (pbrs_name, pbrs_ver) = package_ident(&repo_root().join("Cargo.toml"));
    let (grpc_name, grpc_ver) = package_ident(&repo_root().join("pbrs-grpc/Cargo.toml"));
    let (tonic_name, tonic_ver) = package_ident(&repo_root().join("protobuf-tonic/Cargo.toml"));
    assert_eq!(pbrs_name, "pbrs");
    assert_eq!(grpc_name, "pbrs-grpc");
    assert_eq!(tonic_name, "protobuf-tonic");

    let pack_target = repo_root().join("target").join("package-consumer-pack");
    let _ = std::fs::remove_dir_all(&pack_target);
    std::fs::create_dir_all(&pack_target).unwrap();

    for (pkg, ver) in [
        (pbrs_name.as_str(), pbrs_ver.as_str()),
        (grpc_name.as_str(), grpc_ver.as_str()),
        (tonic_name.as_str(), tonic_ver.as_str()),
    ] {
        let list = cargo_package_list(pkg, &pack_target);
        println!("--- cargo package -p {pkg} --list ({pkg}-{ver}.crate) ---");
        for line in &list {
            println!("{line}");
        }
        assert_licenses(pkg, &list);
        cargo_package(pkg, &pack_target);
    }

    let crate_dir = pack_target.join("package");
    let pbrs_crate = crate_dir.join(format!("{pbrs_name}-{pbrs_ver}.crate"));
    let grpc_crate = crate_dir.join(format!("{grpc_name}-{grpc_ver}.crate"));
    let tonic_crate = crate_dir.join(format!("{tonic_name}-{tonic_ver}.crate"));
    for p in [&pbrs_crate, &grpc_crate, &tonic_crate] {
        assert!(p.is_file(), "missing {}", p.display());
        println!("packed {}", p.file_name().unwrap().to_string_lossy());
    }

    let unpack_root = outside_dir("unpacked", "crates");
    assert!(
        !unpack_root.starts_with(repo_root()),
        "unpack dir must be outside the workspace: {}",
        unpack_root.display()
    );
    let pbrs_src = unpack_crate(&pbrs_crate, &unpack_root.join("pbrs"));
    let grpc_src = unpack_crate(&grpc_crate, &unpack_root.join("pbrs-grpc"));
    let tonic_src = unpack_crate(&tonic_crate, &unpack_root.join("protobuf-tonic"));

    let pbrs_consumer = outside_dir("consumer", "pbrs");
    write_consumer(
        &pbrs_consumer,
        &format!(
            "[package]\nname = \"pbrs-pkg-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            pbrs_src.display()
        ),
        r#"use pbrs::Parse;
fn main() {
    let mut p = pbrs::testdata::Person::new();
    p.set_name("ada");
    p.set_id(42);
    let bytes = pbrs::Serialize::serialize(&p).expect("serialize");
    let q = pbrs::testdata::Person::parse(&bytes).expect("parse");
    assert_eq!(q.name(), "ada");
    println!("ada");
}
"#,
    );
    cargo_run_consumer(&pbrs_consumer, "ada");

    let grpc_consumer = outside_dir("consumer", "pbrs-grpc");
    write_consumer(
        &grpc_consumer,
        &format!(
            "[package]\nname = \"pbrs-grpc-pkg-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs-grpc = {{ path = \"{}\" }}\n[patch.crates-io]\npbrs = {{ path = \"{}\" }}\n",
            grpc_src.display(),
            pbrs_src.display()
        ),
        r#"fn main() {
    let s = pbrs_grpc::Status::from_code(pbrs_grpc::Code::Ok);
    assert!(s.is_ok());
    println!("{}", s.code().name());
}
"#,
    );
    cargo_run_consumer(&grpc_consumer, "OK");

    let tonic_consumer = outside_dir("consumer", "protobuf-tonic");
    write_consumer(
        &tonic_consumer,
        &format!(
            "[package]\nname = \"protobuf-tonic-pkg-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\nprotobuf-tonic = {{ path = \"{}\" }}\n[patch.crates-io]\npbrs = {{ path = \"{}\" }}\n",
            tonic_src.display(),
            pbrs_src.display()
        ),
        r#"fn main() {
    let _ = protobuf_tonic::ProtobufCodec::<(), ()>::default();
    println!("ProtobufCodec");
}
"#,
    );
    cargo_run_consumer(&tonic_consumer, "ProtobufCodec");
}
