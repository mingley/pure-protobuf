//! Exercise the actual build script with read-only packaged descriptor inputs.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    reason = "standalone synchronous build-script fixture uses temporary filesystem and subprocesses"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const SCRIPT: &str = include_str!("../build.rs");
const DESCRIPTORS: &[u8] = include_bytes!("../vendor/google/conformance_fds.bin");
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = loop {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pbrs-build-script-fallback-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create fixture directory: {error}"),
            }
        };
        let script = root.join(format!("build-script{}", std::env::consts::EXE_SUFFIX));
        let fixture = Self { root, script };
        let source = fixture.root.join("build.rs");
        std::fs::write(&source, SCRIPT).unwrap();
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let compiled = Command::new(rustc)
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&fixture.script)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "compile actual build.rs: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        fixture
    }

    fn run(&self, case: &Path, out: &Path, conformance: bool) -> Output {
        let mut command = Command::new(&self.script);
        command.current_dir(case).env("OUT_DIR", out);
        if conformance {
            command.env("CARGO_FEATURE_CONFORMANCE", "1");
        } else {
            command.env_remove("CARGO_FEATURE_CONFORMANCE");
        }
        command.output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Windows cannot unlink read-only fixture inputs. Only fixture-owned
        // paths are made writable, without changing any repository file.
        #[cfg(windows)]
        for path in [
            self.root.join("package/vendor/google/conformance_fds.bin"),
            self.root.join("package/out/conformance_fds.bin"),
        ] {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let mut permissions = metadata.permissions();
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(&path, permissions);
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn actual_build_script_repeats_read_only_fallback_and_watches_existing_inputs() {
    let fixture = Fixture::new();
    let package = fixture.root.join("package");
    let vendor = package.join("vendor/google/conformance_fds.bin");
    let out = package.join("out");
    std::fs::create_dir_all(vendor.parent().unwrap()).unwrap();
    std::fs::create_dir(&out).unwrap();
    std::fs::write(&vendor, DESCRIPTORS).unwrap();
    let mut permissions = std::fs::metadata(&vendor).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&vendor, permissions).unwrap();
    let source_permissions = std::fs::metadata(&vendor).unwrap().permissions();
    let output = out.join("conformance_fds.bin");

    for legacy_read_only in [false, true, false] {
        if legacy_read_only {
            let mut permissions = std::fs::metadata(&output).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&output, permissions).unwrap();
        }
        let result = fixture.run(&package, &out, true);
        assert!(
            result.status.success(),
            "fallback: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(std::fs::read(&output).unwrap(), DESCRIPTORS);
        assert!(!std::fs::metadata(&output).unwrap().permissions().readonly());
        assert_eq!(std::fs::read(&vendor).unwrap(), DESCRIPTORS);
        assert_eq!(
            std::fs::metadata(&vendor).unwrap().permissions(),
            source_permissions
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap(),
            "cargo:rerun-if-changed=vendor/google/conformance_fds.bin\n"
        );
    }

    std::fs::create_dir(package.join("third_party")).unwrap();
    let result = fixture.run(&package, &out, true);
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "cargo:rerun-if-changed=third_party\ncargo:rerun-if-changed=vendor/google/conformance_fds.bin\n"
    );

    std::fs::create_dir(package.join("third_party/protobuf")).unwrap();
    std::fs::create_dir(package.join("proto")).unwrap();
    std::fs::write(package.join("proto/person.proto"), "").unwrap();
    let result = fixture.run(&package, &out, true);
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "cargo:rerun-if-changed=proto/person.proto\ncargo:rerun-if-changed=third_party/protobuf\ncargo:rerun-if-changed=vendor/google/conformance_fds.bin\n"
    );

    let missing = fixture.root.join("missing-vendor");
    std::fs::create_dir(&missing).unwrap();
    let result = fixture.run(&missing, &out, true);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains(
        "missing vendor/google/conformance_fds.bin; refusing to write an empty conformance descriptor set"
    ));
    assert_eq!(std::fs::read(&output).unwrap(), DESCRIPTORS);

    let disabled_out = missing.join("disabled-out");
    let result = fixture.run(&missing, &disabled_out, false);
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "cargo:rerun-if-changed=build.rs\n"
    );
    assert!(!disabled_out.exists());
}
