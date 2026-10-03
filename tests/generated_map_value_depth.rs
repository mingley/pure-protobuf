//! QG20 test-only baseline. Over-limit/malformed known-value assertions are NOT_RUN.
#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    unreachable_pub,
    reason = "source-pinned actual consumer driver; root owns the bounded ordinary gate"
)]

#[path = "support/generated_map_value_depth.rs"]
#[allow(
    dead_code,
    reason = "wire builders are also compiled into the actual child consumer"
)]
mod vectors;

#[cfg(feature = "codegen")]
#[test]
fn fresh_generated_map_value_depth_consumer() {
    use std::path::PathBuf;
    use std::process::Command;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let retained = std::env::var_os("PBRS_QG20_CONSUMER_DIR");
    let consumer = retained.as_ref().map_or_else(
        || {
            root.join("target")
                .join(format!("qg20-consumer-{}", std::process::id()))
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let files = pbrs::codegen::generate_from_file_descriptor_set(
        &vectors::descriptor_set(),
        &["qg20.proto".into()],
    )
    .unwrap();
    let (_, source) = files
        .into_iter()
        .find(|(name, _)| name == "qg20.rs")
        .unwrap();
    std::fs::write(consumer.join("src/qg20.rs"), source).unwrap();
    std::fs::write(
        consumer.join("src/vectors.rs"),
        include_str!("support/generated_map_value_depth.rs"),
    )
    .unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        include_str!("fixtures/generated_map_value_depth_consumer.rs"),
    )
    .unwrap();
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname=\"qg20-map-value-consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\npbrs={{path={:?}}}\n", root.to_str().unwrap()
    )).unwrap();
    // Preserve every registry tuple/checksum. Add only the actual local child root.
    // Locked resolution must pass before a child red is called semantic evidence.
    let mut lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    lock.push_str("\n[[package]]\nname = \"qg20-map-value-consumer\"\nversion = \"0.0.0\"\ndependencies = [\n \"pbrs\",\n]\n");
    std::fs::write(consumer.join("Cargo.lock"), lock).unwrap();
    let mut command = Command::new(
        std::env::var_os("PBRS_QG20_CONSUMER_CARGO").unwrap_or_else(|| "cargo".into()),
    );
    command
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_INCREMENTAL", "0")
        .args([
            "test",
            "--offline",
            "--locked",
            "--quiet",
            "--lib",
            "--",
            "--test-threads=1",
        ])
        .current_dir(&consumer);
    // Qualification must run under root's timeout/process-group/resource helper.
    // A normal standalone invocation gets a distinct cache, never historical GN caches.
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        command.env("CARGO_TARGET_DIR", root.join("target/qg20-consumer-cache"));
    }
    let result = command.output().unwrap();
    std::fs::write(consumer.join("stdout"), &result.stdout).unwrap();
    std::fs::write(consumer.join("stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "generated map-value consumer failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("15 passed; 0 failed"));
    if retained.is_none() {
        std::fs::remove_dir_all(consumer).unwrap();
    }
}
