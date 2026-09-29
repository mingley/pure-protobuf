//! Runtime checks for tonic-shaped compat shims.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "integration tests assert fixture behavior; sync cargo lock never held across await; fixture files written synchronously"
)]

use pbrs_grpc::HelloReply;
use pbrs_grpc::compat::{self, IntoRequest, Request, Response, Status};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;

static CARGO_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn compat_unary_request_conversion_matches_tonic_shape() {
    let request: Request<&'static str> = "hello".into_request();
    assert_eq!(*request.get_ref(), "hello");

    let request = Request::new("wrapped");
    let request: Request<&'static str> = request.into_request();
    assert_eq!(*request.get_ref(), "wrapped");
}

#[tokio::test]
async fn compat_response_stream_forwards_items_and_status() {
    fn reply(message: &str) -> HelloReply {
        let mut reply = HelloReply::new();
        reply.set_message(message);
        reply
    }

    let response = Response::new(compat::iter(vec![
        Ok::<_, Status>(reply("a")),
        Ok(reply("b")),
        Err(Status::unavailable("closed")),
    ]));
    let mut stream = compat::response_stream(response).into_inner();
    assert_eq!(stream.message().await.unwrap().unwrap().message(), "a");
    assert_eq!(stream.message().await.unwrap().unwrap().message(), "b");
    let err = stream.message().await.expect_err("status");
    assert_eq!(err.code(), pbrs_grpc::Code::Unavailable);
}

#[test]
fn tonic_compat_helloworld_port_compiles_and_runs() {
    run_fixture(
        "helloworld-compat-port",
        "helloworld/helloworld.proto",
        include_str!("fixtures/compat/helloworld_compat.rs"),
        "helloworld compat ok",
    );
}

#[test]
fn tonic_compat_routeguide_port_compiles_and_runs() {
    run_fixture(
        "routeguide-compat-port",
        "routeguide/route_guide.proto",
        include_str!("fixtures/compat/routeguide_compat.rs"),
        "routeguide compat ok",
    );
}

fn run_fixture(package: &str, proto_rel: &str, main_rs: &str, want_stdout: &str) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().expect("repo root");
    let fixture_dir = manifest.join("tests/fixtures/compat");
    let dir = manifest
        .join("target")
        .join(format!("compat-fixture-{package}-{}", std::process::id()));
    let _removed = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("build.rs"),
        format!(
            r#"fn main() {{
    let fixture_dir = std::path::PathBuf::from(r"{fixture_dir}");
    let proto = fixture_dir.join(r"{proto_rel}");
    pbrs::codegen::Config::new()
        .tonic_compat(true)
        .compile_protos(&[&proto], &[&fixture_dir])
        .expect("tonic_compat codegen");
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
pbrs = {{ path = "{repo}" }}
pbrs-grpc = {{ path = "{repo}/pbrs-grpc" }}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net"] }}
[build-dependencies]
pbrs = {{ path = "{repo}" }}
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
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), want_stdout);
}

fn cargo_run(dir: &Path) -> Output {
    let _guard = CARGO_MUTEX.lock().expect("consumer Cargo lock");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().expect("repo root");
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(dir)
        .env(
            "CARGO_TARGET_DIR",
            repo.join("target/integration-consumers"),
        )
        .env("CARGO_TERM_COLOR", "never");
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        cmd.env("CARGO_HOME", home);
    }
    cmd.output().expect("cargo run fixture")
}

fn dump(out: &Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}
