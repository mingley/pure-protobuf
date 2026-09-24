use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(super) fn write_consumer(dir: &Path, generated: &str, main_rs: &str) {
    let scenario = dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .expect("generated consumer scenario directory");
    // Distinct package names prevent binary collisions in the shared Cargo target.
    let package_name = format!("{scenario}-consumer");
    std::fs::create_dir_all(dir.join("src")).expect("create generated consumer source");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{}\" }}\n",
            repo_root().display()
        ),
    )
    .expect("write generated consumer manifest");
    std::fs::write(dir.join("src/main.rs"), format!("{generated}\n{main_rs}"))
        .expect("write generated consumer source");
}

pub(super) fn run_consumer(dir: &Path) -> String {
    let run = Command::new("cargo")
        .args(["run", "--offline", "--quiet"])
        .current_dir(dir)
        .env(
            "CARGO_TARGET_DIR",
            repo_root().join("target/integration-consumers"),
        )
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .expect("cargo run generated consumer");
    assert!(
        run.status.success(),
        "consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}
