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
fn qg20_registry_tuples(lock: &[u8]) -> std::collections::BTreeSet<(String, String, String, String)> {
    std::str::from_utf8(lock)
        .unwrap()
        .split("[[package]]")
        .filter_map(|block| {
            let value = |key: &str| {
                block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix(key))
                    .map(str::to_owned)
            };
            let source = value("source = ")?;
            Some((
                value("name = ").expect("locked provider package name"),
                value("version = ").expect("locked provider package version"),
                source,
                value("checksum = ").unwrap_or_default(),
            ))
        })
        .collect()
}

#[cfg(feature = "codegen")]
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "retained actual child command, raw provider/seed/accepted locks and metadata fallback are one bounded driver record"
)]
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
    let records = consumer.join("qg20-run-records");
    std::fs::create_dir_all(&records).unwrap();
    let mut index = 0u32;
    let record = loop {
        let path = records.join(format!("run-{index:04}"));
        match std::fs::create_dir(&path) {
            Ok(()) => break path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                index = index
                    .checked_add(1)
                    .expect("consumer record index overflow");
            }
            result => result.unwrap(),
        }
    };
    match std::fs::read(consumer.join("Cargo.lock")) {
        Ok(previous) => {
            std::fs::write(record.join("previous-replay.Cargo.lock"), previous).unwrap()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        result => {
            result.unwrap();
        }
    }
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
    let root_seed = std::fs::read(root.join("Cargo.lock")).unwrap();
    std::fs::write(record.join("root-seed.Cargo.lock"), &root_seed).unwrap();
    let mut lock = String::from_utf8(root_seed.clone()).unwrap();
    lock.push_str("\n[[package]]\nname = \"qg20-map-value-consumer\"\nversion = \"0.0.0\"\ndependencies = [\n \"pbrs\",\n]\n");
    std::fs::write(record.join("seed-before-normalization.Cargo.lock"), lock.as_bytes()).unwrap();
    let native_cargo =
        std::env::var_os("PBRS_QG20_CONSUMER_CARGO").unwrap_or_else(|| "cargo".into());
    if let Some(accepted_path) = std::env::var_os("PBRS_QG20_ACCEPTED_CONSUMER_LOCK") {
        let accepted = std::fs::read(&accepted_path).expect("prefrozen accepted child lock");
        std::fs::write(record.join("accepted.Cargo.lock"), &accepted).unwrap();
        std::fs::write(
            record.join("accepted-lock-origin.txt"),
            format!("prefrozen accepted path = {accepted_path:?}\nactive lock must already equal accepted bytes; driver does not overwrite it\n"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(consumer.join("Cargo.lock")).unwrap(),
            accepted,
            "prepared active child lock differs from immutable accepted lock"
        );
    } else {
        // Standard standalone invocation: explicit graph preparation is separate
        // from the actual locked test, never silently treated as locked evidence.
        std::fs::write(consumer.join("Cargo.lock"), lock.as_bytes()).unwrap();
        let mut metadata = Command::new(&native_cargo);
        metadata
            .env("CARGO_BUILD_JOBS", "1")
            .env("CARGO_INCREMENTAL", "0")
            .args(["metadata", "--offline", "--format-version=1"])
            .current_dir(&consumer);
        std::fs::write(
            record.join("fallback-metadata-command.txt"),
            format!(
                "program = {:?}\nargv = {:#?}\ncwd = {:?}\nexplicit_env = {:#?}\npolicy = explicit offline graph preparation without --locked; actual test remains --locked\n",
                metadata.get_program(),
                metadata.get_args().collect::<Vec<_>>(),
                metadata.get_current_dir(),
                metadata.get_envs().collect::<Vec<_>>()
            ),
        )
        .unwrap();
        let prepared = metadata.output().unwrap();
        std::fs::write(record.join("fallback-metadata.stdout"), &prepared.stdout).unwrap();
        std::fs::write(record.join("fallback-metadata.stderr"), &prepared.stderr).unwrap();
        std::fs::write(
            record.join("fallback-metadata-exit.txt"),
            format!("exit_code = {:?}\nstatus = {:?}\n", prepared.status.code(), prepared.status),
        )
        .unwrap();
        assert!(prepared.status.success(), "offline child graph preparation failed");
        std::fs::write(
            record.join("accepted.Cargo.lock"),
            std::fs::read(consumer.join("Cargo.lock")).unwrap(),
        )
        .unwrap();
    }
    let before = std::fs::read(consumer.join("Cargo.lock")).unwrap();
    std::fs::write(record.join("before.Cargo.lock"), &before).unwrap();
    assert!(
        qg20_registry_tuples(&before).is_subset(&qg20_registry_tuples(&root_seed)),
        "normalized child selected a version/source/checksum outside the frozen provider"
    );
    for path in ["Cargo.toml", "src/qg20.rs", "src/vectors.rs", "src/lib.rs"] {
        let retained = record.join(path);
        std::fs::create_dir_all(retained.parent().unwrap()).unwrap();
        std::fs::copy(consumer.join(path), retained).unwrap();
    }
    let mut command = Command::new(&native_cargo);
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
    std::fs::write(
        record.join("command.txt"),
        format!(
            "program = {:?}\nargv = {:#?}\ncwd = {:?}\nexplicit_env = {:#?}\ninherited_env_provenance = root ordinary runner\n",
            command.get_program(),
            command.get_args().collect::<Vec<_>>(),
            command.get_current_dir(),
            command.get_envs().collect::<Vec<_>>()
        ),
    )
    .unwrap();
    let result = command.output().unwrap();
    std::fs::write(record.join("stdout"), &result.stdout).unwrap();
    std::fs::write(record.join("stderr"), &result.stderr).unwrap();
    std::fs::write(
        record.join("process-exit.txt"),
        format!(
            "exit_code = {:?}\nstatus = {:?}\n",
            result.status.code(),
            result.status
        ),
    )
    .unwrap();
    let after = std::fs::read(consumer.join("Cargo.lock")).unwrap();
    let root_after = std::fs::read(root.join("Cargo.lock")).unwrap();
    std::fs::write(record.join("after.Cargo.lock"), &after).unwrap();
    std::fs::write(record.join("accepted-after-test.Cargo.lock"), &after).unwrap();
    std::fs::write(record.join("root-after.Cargo.lock"), &root_after).unwrap();
    std::fs::write(
        record.join("lock-byte-equality.txt"),
        format!(
            "child_before_after_equal = {}\nroot_seed_after_equal = {}\ncryptographic_sha256 = computed and checked from retained bytes by root ordinary runner and proof verifier\n",
            before == after,
            root_seed == root_after
        ),
    )
    .unwrap();
    std::fs::write(consumer.join("stdout"), &result.stdout).unwrap();
    std::fs::write(consumer.join("stderr"), &result.stderr).unwrap();
    assert_eq!(before, after, "child --locked invocation changed its lock");
    assert_eq!(
        before,
        std::fs::read(record.join("accepted.Cargo.lock")).unwrap(),
        "actual test did not use the retained accepted lock"
    );
    assert_eq!(
        root_seed, root_after,
        "child invocation changed the root seed lock"
    );
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
