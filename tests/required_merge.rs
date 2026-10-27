//! Required-field initialization after singular-message merging.
#![cfg(feature = "codegen")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "synchronous generated-code regression consumer writes isolated source and captured process logs"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn field(
    name: &str,
    number: u64,
    label: u64,
    kind: u64,
    target: Option<&str>,
    oneof: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 1, name.as_bytes());
    for (tag, value) in [(3, number), (4, label), (5, kind)] {
        pbrs::rt::encode_tag(&mut out, tag, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut out, value);
    }
    if let Some(target) = target {
        pbrs::rt::encode_len_field(&mut out, 6, target.as_bytes());
    }
    if oneof {
        pbrs::rt::encode_tag(&mut out, 9, pbrs::rt::WIRE_VARINT);
        pbrs::rt::encode_varint(&mut out, 0);
    }
    out
}

fn descriptor() -> Vec<u8> {
    let mut child = Vec::new();
    pbrs::rt::encode_len_field(&mut child, 1, b"Child");
    for (name, n) in [("a", 1), ("b", 2)] {
        pbrs::rt::encode_len_field(&mut child, 2, &field(name, n, 2, 5, None, false));
    }
    pbrs::rt::encode_len_field(&mut child, 2, &field("payload", 3, 1, 12, None, false));
    let mut parent = Vec::new();
    pbrs::rt::encode_len_field(&mut parent, 1, b"Parent");
    for (name, n, label, kind, target, oneof) in [
        ("child", 1, 1, 11, Some(".required_merge.Child"), false),
        ("children", 2, 3, 11, Some(".required_merge.Child"), false),
        ("selected", 3, 1, 11, Some(".required_merge.Child"), true),
        ("other", 4, 1, 5, None, true),
        ("g", 5, 1, 10, Some(".required_merge.Parent.G"), false),
        (
            "by_key",
            6,
            3,
            11,
            Some(".required_merge.Parent.ByKeyEntry"),
            false,
        ),
    ] {
        pbrs::rt::encode_len_field(&mut parent, 2, &field(name, n, label, kind, target, oneof));
    }
    let mut oneof = Vec::new();
    pbrs::rt::encode_len_field(&mut oneof, 1, b"choice");
    pbrs::rt::encode_len_field(&mut parent, 8, &oneof);
    let mut group = Vec::new();
    pbrs::rt::encode_len_field(&mut group, 1, b"G");
    for (name, n) in [("a", 1), ("b", 2)] {
        pbrs::rt::encode_len_field(&mut group, 2, &field(name, n, 2, 5, None, false));
    }
    pbrs::rt::encode_len_field(&mut parent, 3, &group);
    let mut entry = Vec::new();
    pbrs::rt::encode_len_field(&mut entry, 1, b"ByKeyEntry");
    pbrs::rt::encode_len_field(&mut entry, 2, &field("key", 1, 1, 5, None, false));
    pbrs::rt::encode_len_field(
        &mut entry,
        2,
        &field("value", 2, 1, 11, Some(".required_merge.Child"), false),
    );
    pbrs::rt::encode_len_field(&mut entry, 7, &[0x38, 1]);
    pbrs::rt::encode_len_field(&mut parent, 3, &entry);
    let mut envelope = Vec::new();
    pbrs::rt::encode_len_field(&mut envelope, 1, b"Envelope");
    pbrs::rt::encode_len_field(
        &mut envelope,
        2,
        &field("inner", 1, 1, 11, Some(".required_merge.Parent"), false),
    );
    let mut cold = Vec::new();
    pbrs::rt::encode_len_field(&mut cold, 1, b"ColdRequired");
    for (name, n, label, kind, target) in [
        ("tag", 1, 2, 5, None),
        ("name", 2, 2, 9, None),
        ("data", 3, 1, 12, None),
        ("child", 4, 1, 11, Some(".required_merge.Child")),
    ] {
        pbrs::rt::encode_len_field(&mut cold, 2, &field(name, n, label, kind, target, false));
    }
    let mut wide = Vec::new();
    pbrs::rt::encode_len_field(&mut wide, 1, b"Wide");
    for n in 1..=65 {
        pbrs::rt::encode_len_field(&mut wide, 2, &field(&format!("f{n}"), n, 2, 5, None, false));
    }
    let mut file = Vec::new();
    pbrs::rt::encode_len_field(&mut file, 1, b"required_merge.proto");
    pbrs::rt::encode_len_field(&mut file, 2, b"required_merge");
    let mut message_set = Vec::new();
    pbrs::rt::encode_len_field(&mut message_set, 1, b"MessageSetHost");
    pbrs::rt::encode_len_field(&mut message_set, 7, &[8, 1]);
    let mut range = vec![8, 4, 16];
    pbrs::rt::encode_varint(&mut range, 536_870_912);
    pbrs::rt::encode_len_field(&mut message_set, 5, &range);
    for message in [child, parent, envelope, cold, wide, message_set] {
        pbrs::rt::encode_len_field(&mut file, 4, &message);
    }
    let mut extension = field(
        "extension_child",
        100,
        1,
        11,
        Some(".required_merge.Child"),
        false,
    );
    pbrs::rt::encode_len_field(&mut extension, 2, b".required_merge.MessageSetHost");
    pbrs::rt::encode_len_field(&mut file, 7, &extension);
    pbrs::rt::encode_len_field(&mut file, 12, b"proto2");
    let mut out = Vec::new();
    pbrs::rt::encode_len_field(&mut out, 1, &file);
    out
}

fn run_logged(command: &mut Command, root: &Path, label: &str) -> std::process::Output {
    let output = command.output().unwrap();
    std::fs::write(root.join(format!("{label}.stdout")), &output.stdout).unwrap();
    std::fs::write(root.join(format!("{label}.stderr")), &output.stderr).unwrap();
    output
}

#[test]
fn generated_required_fields_validate_after_singular_merge() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let consumer = std::env::var_os("PBRS_REQUIRED_MERGE_CONSUMER_DIR").map_or_else(
        || {
            repo.join("target")
                .join(format!("required-merge-{}", std::process::id()))
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let descriptor = descriptor();
    std::fs::write(consumer.join("schema.fds"), &descriptor).unwrap();
    let generated = pbrs::codegen::generate_from_file_descriptor_set(
        &descriptor,
        &["required_merge.proto".into()],
    )
    .unwrap();
    let (_, source) = generated
        .into_iter()
        .find(|(name, _)| name == "required_merge.rs")
        .unwrap();
    std::fs::write(consumer.join("src/required_merge.rs"), source).unwrap();
    std::fs::write(
        consumer.join("src/lib.rs"),
        include_str!("fixtures/required_merge_consumer.rs"),
    )
    .unwrap();
    test_consumer(&consumer, &repo, "required-merge-consumer", 30);
}

fn test_consumer(consumer: &Path, repo: &Path, package: &str, expected_tests: usize) {
    std::fs::write(consumer.join("Cargo.toml"), format!("[package]\nname=\"{package}\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\npbrs={{path={:?}}}\n", repo.to_str().unwrap())).unwrap();
    std::fs::copy(
        repo.join("Cargo.lock"),
        consumer.join("provider.Cargo.lock"),
    )
    .unwrap();
    std::fs::copy(repo.join("Cargo.lock"), consumer.join("Cargo.lock")).unwrap();
    let mut metadata = Command::new("cargo");
    metadata
        .args(["metadata", "--offline", "--format-version=1"])
        .current_dir(consumer);
    assert!(
        run_logged(&mut metadata, consumer, "prepare-lock")
            .status
            .success(),
        "explicit offline child lock preparation"
    );
    let accepted = std::fs::read(consumer.join("Cargo.lock")).unwrap();
    std::fs::write(consumer.join("accepted.Cargo.lock"), &accepted).unwrap();
    let mut command = Command::new("cargo");
    command
        .args([
            "test",
            "--offline",
            "--locked",
            "--lib",
            "--",
            "--test-threads=1",
        ])
        .env("CARGO_BUILD_JOBS", "1")
        .current_dir(consumer);
    let output = run_logged(&mut command, consumer, "test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let passed_summary = format!("test result: ok. {expected_tests} passed; 0 failed;");
    assert!(
        stdout.contains(&passed_summary),
        "expected {expected_tests} generated tests for {package}:\n{stdout}"
    );
    // Retain the actual executable before a later consumer is built.
    if let Some(executable) = String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| line.trim_start().starts_with("Running unittests"))
        .and_then(|line| line.rsplit_once(" ("))
        .and_then(|(_, path)| path.strip_suffix(')'))
    {
        std::fs::copy(executable, consumer.join("test-elf")).unwrap();
    }
    assert_eq!(
        std::fs::read(consumer.join("Cargo.lock")).unwrap(),
        accepted
    );
    assert!(
        output.status.success(),
        "fresh generated consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn parallel_targets_preserve_imported_required_initialization() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = std::env::var_os("PBRS_REQUIRED_MERGE_CONSUMER_DIR").map_or_else(
        || {
            repo.join("target")
                .join(format!("required-merge-{}", std::process::id()))
        },
        PathBuf::from,
    );
    let consumer = base.with_file_name(format!(
        "{}-parallel",
        base.file_name().unwrap().to_str().unwrap()
    ));
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let mut descriptor = descriptor();
    let mut imported = Vec::new();
    pbrs::rt::encode_len_field(&mut imported, 1, b"ImportedParent");
    pbrs::rt::encode_len_field(
        &mut imported,
        2,
        &field("child", 1, 1, 11, Some(".required_merge.Child"), false),
    );
    let mut file = Vec::new();
    pbrs::rt::encode_len_field(&mut file, 1, b"required_import.proto");
    pbrs::rt::encode_len_field(&mut file, 2, b"required_import");
    pbrs::rt::encode_len_field(&mut file, 3, b"required_merge.proto");
    pbrs::rt::encode_len_field(&mut file, 4, &imported);
    pbrs::rt::encode_len_field(&mut file, 12, b"proto2");
    pbrs::rt::encode_len_field(&mut descriptor, 1, &file);
    std::fs::write(consumer.join("schema.fds"), &descriptor).unwrap();
    let targets = [
        "required_merge.proto".into(),
        "required_import.proto".into(),
    ];
    println!(
        "multi-target required control: available_parallelism={:?}",
        std::thread::available_parallelism()
    );
    for (name, source) in
        pbrs::codegen::generate_from_file_descriptor_set(&descriptor, &targets).unwrap()
    {
        std::fs::write(consumer.join("src").join(name), source).unwrap();
    }
    std::fs::write(
        consumer.join("src/lib.rs"),
        include_str!("fixtures/required_merge_parallel_consumer.rs"),
    )
    .unwrap();
    test_consumer(&consumer, &repo, "required-merge-parallel-consumer", 3);
}
