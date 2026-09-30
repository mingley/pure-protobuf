//! An isolated application supplies ring-backed rustls configs to the kernel.

#![allow(clippy::expect_used, reason = "integration fixture command")]

#[test]
fn caller_ring_and_tonic_matrix() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("target"), std::path::PathBuf::from)
        .join("caller-rustls");
    let output = std::process::Command::new(env!("CARGO"))
        .args(["test", "--locked", "--manifest-path"])
        .arg(root.join("tests/caller-rustls/Cargo.toml"))
        .arg("--target-dir")
        .arg(target)
        .output()
        .expect("run caller rustls application fixture");
    assert!(
        output.status.success(),
        "caller rustls fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
