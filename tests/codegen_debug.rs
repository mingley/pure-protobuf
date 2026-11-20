//! Plugin and builder parity for per-type Debug generation.

#![cfg(feature = "codegen")]
#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "isolated generated consumer and compiler diagnostics"
)]

use pbrs::codegen::Config;
use std::{fs, path::Path, process::Command};

#[test]
fn selective_debug_compiles_with_custom_impls() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch = repo
        .join("target")
        .join(format!("selective-debug-{}", std::process::id()));
    let builder_dir = scratch.join("builder");
    let plugin_dir = scratch.join("plugin");
    let src = scratch.join("src");
    for dir in [&builder_dir, &plugin_dir, &src] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        scratch.join("selectors.proto"),
        r#"syntax = "proto3";
package debug_fixture;
message Envelope {
  message Secret { int32 id = 1; }
  message SecretSibling {}
  oneof content {
    Secret secret = 1;
    string title = 2;
  }
  State state = 3;
}
enum State { STATE_UNSPECIFIED = 0; STATE_READY = 1; }
enum Stateful { STATEFUL_UNSPECIFIED = 0; }
"#,
    )
    .unwrap();

    Config::new()
        .out_dir(&builder_dir)
        .emit_reflection(false)
        .emit_json(false)
        .emit_text(false)
        .skip_debug_for([".debug_fixture.Envelope.Secret", "State", "a,b"])
        .compile_protos(&[scratch.join("selectors.proto")], &[&scratch])
        .expect("builder invocation");
    let protoc = std::env::var_os("PROTOC").unwrap_or_else(|| "protoc".into());
    let result = Command::new(protoc)
        .current_dir(&scratch)
        .arg(concat!(
            "--plugin=protoc-gen-pbrs=",
            env!("CARGO_BIN_EXE_protoc-gen-pbrs")
        ))
        .arg("--pbrs_out=plugin")
        .arg(concat!(
            "--pbrs_opt=no_reflect=true,emit_json=false,emit_text=false,",
            "skip_debug_for=.debug_fixture.Envelope.Secret,skip_debug_for=State,",
            "skip_debug_for_hex=612c62"
        ))
        .arg("selectors.proto")
        .output()
        .expect("plugin invocation");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    fs::write(
        scratch.join("Cargo.toml"),
        format!(
            "[package]\nname = \"selective-debug-consumer\"\nversion = \"0.0.0\"\n\
             edition = \"2024\"\n[workspace]\n[dependencies]\npbrs = {{ path = {:?} }}\n",
            repo
        ),
    )
    .unwrap();
    fs::write(
        src.join("main.rs"),
        r#"
include!("generated.rs");

impl std::fmt::Debug for envelope::Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "secret/{}", self.id())
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "state/{}", self.0)
    }
}

fn main() {
    use pbrs::{Parse, Serialize};

    let mut message = Envelope::new();
    message.set_title("replaced");
    let mut secret = envelope::Secret::new();
    secret.set_id(7);
    message.set_secret(secret);
    assert!(!message.has_title());
    message.set_state(State::Ready);
    let parsed = Envelope::parse(&message.serialize().unwrap()).unwrap();
    assert!(parsed.has_secret());
    assert!(!parsed.has_title());
    assert_eq!(parsed.secret().id(), 7);
    assert_eq!(parsed.state(), State::Ready);
    assert_eq!(format!("{:?}", parsed.secret()), "secret/7");
    assert_eq!(format!("{:?}", parsed.state()), "state/1");
    assert!(format!("{parsed:?}").contains("Envelope"));
    assert!(format!("{:?}", envelope::SecretSibling::new()).contains("SecretSibling"));
    assert!(format!("{:?}", Stateful::default()).contains("Stateful"));
}
"#,
    )
    .unwrap();
    for output in [builder_dir, plugin_dir] {
        fs::copy(output.join("selectors.rs"), src.join("generated.rs")).unwrap();
        let result = Command::new("cargo")
            .args(["run", "--offline", "--quiet"])
            .current_dir(&scratch)
            .env(
                "CARGO_TARGET_DIR",
                repo.join("target/integration-consumers"),
            )
            .output()
            .expect("compile and run generated consumer");
        assert!(
            result.status.success(),
            "{}: {}",
            output.display(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::remove_dir_all(scratch).unwrap();
}
