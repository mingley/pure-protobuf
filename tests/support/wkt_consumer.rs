//! Small GN-11 generated-consumer helpers shared with documentation contracts.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(crate) fn generate(
    name: &str,
    protos: &[&str],
    mapping: Option<&str>,
) -> Result<PathBuf, pbrs::codegen::CodegenError> {
    let fixture = root().join("tests/fixtures/codegen-wkt-sharing");
    let output = root().join("target/gn11-tests").join(name);
    if let Err(error) = std::fs::remove_dir_all(&output) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
    let sources: Vec<_> = protos.iter().map(PathBuf::from).collect();
    let descriptor = if protos.first() == Some(&"local/descriptor.proto") {
        "alias.fds"
    } else if protos
        .first()
        .is_some_and(|proto| proto.starts_with("opt_"))
    {
        "options.fds"
    } else {
        "wkts.fds"
    };
    let mut config = pbrs::codegen::Config::new();
    config.out_dir(&output).emit_kernel_stubs(false);
    if let Some(mapping) = mapping {
        config.extern_path(".google.protobuf", mapping);
    }
    config.compile_descriptor_set(fixture.join(descriptor), &sources, &[fixture])?;
    Ok(output)
}

pub(crate) fn compile(output: &Path, name: &str, source: &str) {
    let consumer = output.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    std::fs::write(consumer.join("Cargo.toml"), format!(
        "[package]\nname = \"gn11-{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = {:?} }}\n", root()
    )).unwrap();
    std::fs::write(consumer.join("src/main.rs"), source).unwrap();
    let result = Command::new("cargo")
        .args(["run", "--offline", "--quiet"])
        .current_dir(&consumer)
        .env(
            "CARGO_TARGET_DIR",
            root().join("target/integration-consumers"),
        )
        .env("CARGO_BUILD_JOBS", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
