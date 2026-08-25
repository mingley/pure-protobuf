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
use std::path::PathBuf;
use std::process::Command;

#[test]
fn compile_protos_consumer_parses_ada() {
    let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("pbrs-build-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("name.proto"),
        "syntax = \"proto3\";\npackage pbrsbuild;\nmessage Name { string name = 1; }\n",
    )
    .unwrap();
    std::fs::write(
        tmp.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::compile_protos(&["name.proto"], &["."]).expect("compile_protos");
}
"#,
    )
    .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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
    let cargo_home = std::env::var("CARGO_HOME").ok();
    let mut build = Command::new("cargo");
    build
        .arg("run")
        .arg("--offline")
        .arg("--quiet")
        .current_dir(&tmp);
    if let Some(h) = cargo_home {
        build.env("CARGO_HOME", h);
    }
    let run = build.output().expect("cargo run pbrs-build consumer");
    assert!(
        run.status.success(),
        "pbrs-build consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert_eq!(out.trim(), "ada");
    println!("{}", out.trim());
}
