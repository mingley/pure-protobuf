//! Downstream `build.rs` uses `pbrs::codegen::compile_protos` (not `scripts/gen.sh`).

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
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]
use std::ffi::{OsStr, OsString};
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

/// Keep the real PATH so `cc`/`cargo`/`rustc` stay resolvable. Prepend a
/// directory with a `protoc` shim that exits 127 so `Command::new("protoc")`
/// finds the shim first. Does not rewrite HOME / CARGO_HOME / RUSTUP_HOME.
fn path_without_protoc() -> OsString {
    use std::os::unix::fs::PermissionsExt;
    let shim_dir = std::env::temp_dir().join(format!("pbrs-hide-protoc-{}", std::process::id()));
    std::fs::create_dir_all(&shim_dir).expect("hide-protoc dir");
    let shim = shim_dir.join("protoc");
    std::fs::write(&shim, "#!/bin/sh\nexit 127\n").expect("write protoc shim");
    let mut perms = std::fs::metadata(&shim)
        .expect("shim metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&shim, perms).expect("chmod protoc shim");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![shim_dir];
    dirs.extend(std::env::split_paths(&path));
    std::env::join_paths(dirs).expect("join PATH")
}

/// First PATH entry is the one `/usr/bin/env protoc` finds.
fn path_has_protoc(path: &OsStr) -> bool {
    std::env::split_paths(path)
        .next()
        .map(|dir| dir.join("protoc").exists() || dir.join("protoc.exe").exists())
        .unwrap_or(false)
}

fn env_protoc_runs(path: &OsStr) -> bool {
    Command::new("/usr/bin/env")
        .arg("protoc")
        .arg("--version")
        .env("PATH", path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn env_cc_runs(path: &OsStr) -> bool {
    Command::new("/usr/bin/env")
        .arg("cc")
        .arg("--version")
        .env("PATH", path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn assert_filtered_path(path: &OsStr) {
    assert!(
        std::env::split_paths(path).any(|d| d.join("cargo").exists() || d.join("rustc").exists()),
        "PATH filter dropped cargo/rustc"
    );
    assert!(
        path_has_protoc(path),
        "shim PATH must start with a protoc shim"
    );
    assert!(
        !env_protoc_runs(path),
        "protoc still runs on the filtered PATH"
    );
    assert!(
        env_cc_runs(path)
            || std::env::split_paths(path)
                .any(|d| d.join("cc").exists() || d.join("cc.exe").exists()),
        "PATH hid cc; keep the real PATH and prepend a failing protoc shim"
    );
}

fn dump(out: &Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn cargo_run(dir: &Path, path: Option<&OsStr>, quiet: bool) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--offline");
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.current_dir(dir).env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cmd.output().expect("cargo run")
}

fn assert_build_failed_without_protoc(out: &Output) {
    let text = dump(out);
    assert!(
        !out.status.success(),
        "consumer must fail without protoc:\n{text}"
    );
    assert!(
        !text.contains("linker `cc` not found"),
        "failure should be the build script / protoc path, not a skipped test:\n{text}"
    );
    assert!(
        text.contains("failed to run custom build command")
            || text.contains("compile_protos")
            || text.contains("codegen")
            || text.contains("parse error"),
        "failure should be the build script / protoc path, not a skipped test:\n{text}"
    );
}

#[test]
fn compile_protos_consumer_parses_ada() {
    let tmp = repo_root().join("target").join("pbrs-build-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("name.proto"),
        "syntax = \"proto3\";\npackage pbrsbuild;\nmessage Name { string name = 1; }\n",
    )
    .unwrap();
    // Messages-only: no service, and stubs explicitly off so a later service
    // addition cannot silently emit kernel/tonic RPC types.
    std::fs::write(
        tmp.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(false)
        .compile_protos(&["name.proto"], &["."])
        .expect("compile_protos");
}
"#,
    )
    .unwrap();
    let root = repo_root();
    std::fs::write(
        tmp.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-build-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n[build-dependencies]\npbrs = {{ path = \"{}\" }}\n",
            root.display(),
            root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/name.rs"));
use pbrs::Parse;
fn main() {
    let wire = [0x0a, 0x03, b'a', b'd', b'a'];
    let m = Name::parse(&wire).expect("parse");
    assert_eq!(m.name(), "ada");
    println!("ada");
}
"#,
    )
    .unwrap();

    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    assert_build_failed_without_protoc(&cargo_run(&tmp, Some(&no_protoc), false));

    let run = cargo_run(&tmp, None, true);
    assert!(
        run.status.success(),
        "pbrs-build consumer failed:\n{}",
        dump(&run)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert_eq!(out.trim(), "ada");
    println!("{}", out.trim());
}

#[test]
fn compile_protos_defaults_to_kernel_stubs() {
    assert_eq!(
        pbrs::codegen::Stubs::default(),
        pbrs::codegen::Stubs::Kernel
    );
}
