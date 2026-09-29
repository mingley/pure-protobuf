//! Consumer-crate prost-build + native pbrs-grpc prost stub fixtures.

#![cfg(feature = "prost")]
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests invoke fixture crates"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

static CARGO_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn prost_helloworld_native_stubs_compile_and_run() {
    run_fixture(
        "prost-helloworld-native",
        "helloworld/helloworld.proto",
        include_str!("fixtures/prost/helloworld_native.rs"),
        "prost helloworld ok",
    );
}

#[test]
fn prost_routeguide_native_stubs_compile() {
    run_fixture(
        "prost-routeguide-native",
        "routeguide/route_guide.proto",
        include_str!("fixtures/prost/routeguide_native.rs"),
        "prost routeguide ok",
    );
}

#[test]
fn tonic_prost_client_interops_with_native_prost_server() {
    run_manual_fixture(
        "prost-tonic-client-interop",
        include_str!("fixtures/prost/tonic_client_native.rs"),
        "tonic interop ok",
    );
}

fn run_fixture(package: &str, proto_rel: &str, main_rs: &str, want_stdout: &str) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().expect("repo root");
    let fixture_dir = manifest.join("tests/fixtures/compat");
    let dir = manifest
        .join("target")
        .join(format!("prost-fixture-{package}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("build.rs"),
        format!(
            r#"fn main() {{
    let fixture_dir = std::path::PathBuf::from(r"{fixture_dir}");
    let proto = fixture_dir.join(r"{proto_rel}");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let fds = out.join("prost-fixture.fds");
    let mut prost = prost_build::Config::new();
    prost.file_descriptor_set_path(&fds);
    prost.compile_protos(&[proto.as_path()], &[fixture_dir.as_path()])
        .expect("prost-build codegen");
    pbrs::codegen::prost_stubs::compile_descriptor_set(&fds, &["{proto_rel}"], &out)
        .expect("pbrs prost stubs");
}}
"#,
            fixture_dir = fixture_dir.display(),
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{package}"
version = "0.0.1"
edition = "2021"
[workspace]
[dependencies]
pbrs-grpc = {{ path = "{repo}/pbrs-grpc", features = ["prost"] }}
prost = "0.14"
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net"] }}
[build-dependencies]
pbrs = {{ path = "{repo}" }}
prost-build = "0.14"
"#,
            repo = repo.display(),
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();

    let output = cargo_run(&dir);
    assert!(
        output.status.success(),
        "fixture {package} failed:\n{}",
        dump(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(want_stdout), "{stdout}");
}

fn run_manual_fixture(package: &str, main_rs: &str, want_stdout: &str) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().expect("repo root");
    let dir = manifest
        .join("target")
        .join(format!("prost-fixture-{package}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("build.rs"), "fn main() {}\n").unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{package}"
version = "0.0.1"
edition = "2021"
[workspace]
[dependencies]
pbrs-grpc = {{ path = "{repo}/pbrs-grpc", features = ["prost"] }}
prost = "0.14"
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net"] }}
tonic = {{ version = "0.14", default-features = false, features = ["transport", "codegen"] }}
tonic-prost = "0.14"
"#,
            repo = repo.display(),
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), main_rs).unwrap();
    let output = cargo_run(&dir);
    assert!(
        output.status.success(),
        "fixture {package} failed:\n{}",
        dump(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(want_stdout), "{stdout}");
}

fn cargo_run(dir: &Path) -> Output {
    let _guard = CARGO_MUTEX.lock().unwrap();
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--quiet").current_dir(dir);
    cmd.output().expect("cargo run")
}

fn dump(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}
