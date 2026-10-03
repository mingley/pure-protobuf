//! Fetch the official conformance FileDescriptorSet for the runner child.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "build.rs is a sync compile-time script; panic fails the build"
)]

use std::path::Path;
use std::process::Command;

fn main() {
    if std::env::var_os("CARGO_FEATURE_CONFORMANCE").is_none() {
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }
    if Path::new("proto/person.proto").exists() {
        println!("cargo:rerun-if-changed=proto/person.proto");
    }
    let root = Path::new("third_party/protobuf");
    if root.exists() {
        println!("cargo:rerun-if-changed=third_party/protobuf");
    } else if Path::new("third_party").exists() {
        println!("cargo:rerun-if-changed=third_party");
    }
    // Missing watched paths make Cargo rerun forever. Packaged builds have no
    // SDK or parent; installing one there requires a normal rebuild trigger.
    println!("cargo:rerun-if-changed=vendor/google/conformance_fds.bin");
    let out = std::env::var("OUT_DIR").unwrap();
    let fds = Path::new(&out).join("conformance_fds.bin");
    let vendored = Path::new("vendor/google/conformance_fds.bin");
    let src = root.join("src");
    if src.exists() && try_protoc(&fds, root, &src) {
        println!("cargo:warning=wrote conformance descriptor set");
        return;
    }
    // Never write an empty FDS: that produced 2090 unexpected JsonOutput failures.
    if vendored.exists() {
        copy_vendored_fds(vendored, &fds).unwrap_or_else(|e| {
            panic!("failed to copy vendor/google/conformance_fds.bin: {e}");
        });
        return;
    }
    panic!(
        "missing vendor/google/conformance_fds.bin; \
         refusing to write an empty conformance descriptor set"
    );
}

fn copy_vendored_fds(vendored: &Path, fds: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(vendored)?;
    // fs::copy also copies permissions: a read-only vendor file makes OUT_DIR
    // read-only and breaks subsequent builds. Recreate only our output file,
    // including outputs left read-only by older build scripts.
    match std::fs::remove_file(fds) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        #[cfg(windows)]
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            let metadata = std::fs::symlink_metadata(fds)?;
            let mut permissions = metadata.permissions();
            if metadata.file_type().is_symlink() || !permissions.readonly() {
                return Err(error);
            }
            permissions.set_readonly(false);
            std::fs::set_permissions(fds, permissions)?;
            std::fs::remove_file(fds)?;
        }
        Err(error) => return Err(error),
    }
    let mut output = std::fs::File::create(fds)?;
    std::io::copy(&mut input, &mut output)?;
    Ok(())
}

fn try_protoc(fds: &Path, root: &Path, src: &Path) -> bool {
    let mut inputs: Vec<std::path::PathBuf> = Vec::new();
    for rel in [
        "google/protobuf/test_messages_proto3.proto",
        "google/protobuf/test_messages_proto2.proto",
        "google/protobuf/any.proto",
        "google/protobuf/duration.proto",
        "google/protobuf/timestamp.proto",
        "google/protobuf/struct.proto",
        "google/protobuf/wrappers.proto",
        "google/protobuf/field_mask.proto",
        "google/protobuf/empty.proto",
    ] {
        let p = src.join(rel);
        if p.exists() {
            inputs.push(p);
        }
    }
    for rel in [
        "conformance/test_protos/test_messages_edition2023.proto",
        "conformance/test_protos/test_messages_edition_unstable.proto",
        "editions/golden/test_messages_proto2_editions.proto",
        "editions/golden/test_messages_proto3_editions.proto",
    ] {
        let p = root.join(rel);
        if p.exists() {
            inputs.push(p);
        }
    }
    if inputs.is_empty() {
        return false;
    }
    let mut cmd = Command::new("protoc");
    cmd.arg("--include_imports")
        .arg("--descriptor_set_out")
        .arg(fds)
        .arg("-I")
        .arg(src)
        .arg("-I")
        .arg(root);
    for p in &inputs {
        cmd.arg(p);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}
