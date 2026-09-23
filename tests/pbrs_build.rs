//! Downstream `build.rs` uses configured `pbrs::codegen` build APIs (not `scripts/gen.sh`).

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
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[allow(
    clippy::disallowed_types,
    reason = "synchronous tests serialize process-wide environment changes"
)]
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[allow(
    clippy::disallowed_types,
    reason = "synchronous child Cargo runs share a cache and never hold this lock across await"
)]
static CARGO_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn apply_cargo_home(cmd: &mut Command) {
    if let Some(h) = std::env::var_os("CARGO_HOME") {
        cmd.env("CARGO_HOME", h);
    }
}

/// Keep the real PATH so `cc`/`cargo`/`rustc` stay resolvable. Prepend a
/// unique directory with a `protoc` shim that exits 127 so
/// `Command::new("protoc")` finds the shim first. Unique dir per call so
/// parallel tests never share a truncated shim. Does not rewrite HOME /
/// CARGO_HOME / RUSTUP_HOME.
fn path_without_protoc() -> OsString {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SHIM_SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SHIM_SEQ.fetch_add(1, Ordering::Relaxed);
    let shim_dir =
        std::env::temp_dir().join(format!("pbrs-hide-protoc-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&shim_dir).expect("hide-protoc dir");
    let shim = shim_dir.join("protoc");
    let tmp = shim_dir.join(format!(".protoc.{n}.tmp"));
    std::fs::write(&tmp, "#!/bin/sh\nexit 127\n").expect("write protoc shim");
    let mut perms = std::fs::metadata(&tmp)
        .expect("shim metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmp, perms).expect("chmod protoc shim");
    std::fs::rename(&tmp, &shim).expect("install protoc shim");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs = vec![shim_dir];
    dirs.extend(std::env::split_paths(&path));
    std::env::join_paths(dirs).expect("join PATH")
}

/// First PATH entry is the one `/usr/bin/env protoc` finds.
fn path_has_protoc(path: &OsStr) -> bool {
    std::env::split_paths(path)
        .next()
        .map(|dir| dir.join("protoc").exists() || dir.join("protoc.exe").exists())
        .unwrap_or(false)
}

fn env_protoc_runs(path: &OsStr) -> bool {
    Command::new("/usr/bin/env")
        .arg("protoc")
        .arg("--version")
        .env("PATH", path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn env_cc_runs(path: &OsStr) -> bool {
    Command::new("/usr/bin/env")
        .arg("cc")
        .arg("--version")
        .env("PATH", path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn assert_filtered_path(path: &OsStr) {
    assert!(
        std::env::split_paths(path).any(|d| d.join("cargo").exists() || d.join("rustc").exists()),
        "PATH filter dropped cargo/rustc"
    );
    assert!(
        path_has_protoc(path),
        "shim PATH must start with a protoc shim"
    );
    assert!(
        !env_protoc_runs(path),
        "protoc still runs on the filtered PATH"
    );
    assert!(
        env_cc_runs(path)
            || std::env::split_paths(path)
                .any(|d| d.join("cc").exists() || d.join("cc.exe").exists()),
        "PATH hid cc; keep the real PATH and prepend a failing protoc shim"
    );
}

fn dump(out: &Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn cargo_output(command: &mut Command, context: &str) -> Output {
    let _guard = CARGO_MUTEX.lock().expect("consumer Cargo lock");
    command
        .output()
        .unwrap_or_else(|err| panic!("{context}: {err}"))
}

fn cargo_run(dir: &Path, path: Option<&OsStr>, quiet: bool) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--offline");
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.current_dir(dir)
        .env(
            "CARGO_TARGET_DIR",
            repo_root().join("target/integration-consumers"),
        )
        .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cargo_output(&mut cmd, "cargo run")
}

fn assert_build_failed_without_protoc(out: &Output) {
    let text = dump(out);
    assert!(
        !out.status.success(),
        "consumer must fail without protoc:\n{text}"
    );
    assert!(
        !text.contains("linker `cc` not found"),
        "failure should be the build script / protoc path, not a skipped test:\n{text}"
    );
    assert!(
        text.contains("failed to run custom build command")
            || text.contains("compile_protos")
            || text.contains("codegen")
            || text.contains("parse error"),
        "failure should be the build script / protoc path, not a skipped test:\n{text}"
    );
}

#[test]
fn compile_protos_consumer_parses_ada() {
    let tmp = repo_root().join("target").join("pbrs-build-test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("name.proto"),
        "syntax = \"proto3\";\npackage pbrsbuild;\nmessage Name { string name = 1; }\n",
    )
    .unwrap();
    // Messages-only: no service, and stubs explicitly off so a later service
    // addition cannot silently emit kernel/tonic RPC types.
    std::fs::write(
        tmp.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(false)
        .compile_protos(&["name.proto"], &["."])
        .expect("compile_protos");
}
"#,
    )
    .unwrap();
    let root = repo_root();
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

    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    assert_build_failed_without_protoc(&cargo_run(&tmp, Some(&no_protoc), false));

    let run = cargo_run(&tmp, None, true);
    assert!(
        run.status.success(),
        "pbrs-build consumer failed:\n{}",
        dump(&run)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    assert_eq!(out.trim(), "ada");
    println!("{}", out.trim());
}

#[test]
fn compile_protos_defaults_to_kernel_stubs() {
    assert_eq!(
        pbrs::codegen::Stubs::default(),
        pbrs::codegen::Stubs::Kernel
    );
}

fn test_temp_dir(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_SEQ: AtomicU64 = AtomicU64::new(0);
    let n = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root()
        .join("target")
        .join(format!("{name}-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_descriptor_set(path: &Path, protos: &[&Path], includes: &[&Path], source_info: bool) {
    let mut cmd = Command::new("protoc");
    cmd.arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", path.display()));
    if source_info {
        cmd.arg("--include_source_info");
    }
    for include in includes {
        cmd.arg("-I").arg(include);
    }
    for proto in protos {
        cmd.arg(proto);
    }
    let out = cmd.output().expect("protoc descriptor set");
    assert!(out.status.success(), "protoc failed:\n{}", dump(&out));
}

fn generated_files(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut pending = vec![dir.to_path_buf()];
    let mut files = std::collections::BTreeMap::new();
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.insert(
                    path.strip_prefix(dir).unwrap().to_path_buf(),
                    std::fs::read(&path).unwrap(),
                );
            }
        }
    }
    files
}

fn plugin_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_protoc-gen-pbrs") {
        return PathBuf::from(p);
    }
    repo_root().join("target/debug/protoc-gen-pbrs")
}

#[test]
fn error_missing_import_identifies_cause_and_path() {
    let tmp = test_temp_dir("missing-import-test");
    let proto_path = tmp.join("failing_import.proto");
    std::fs::write(
        &proto_path,
        "syntax = \"proto3\";\npackage test;\nimport \"nonexistent_dependency.proto\";\nmessage Foo { string s = 1; }\n",
    )
    .unwrap();
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&[&proto_path], &[&tmp])
        .unwrap_err();

    match &err {
        pbrs::codegen::CodegenError::MissingImport {
            import,
            proto,
            detail,
        } => {
            assert!(
                import.contains("nonexistent_dependency.proto"),
                "expected import name in error, got: {import}"
            );
            assert!(
                proto.display().to_string().contains("failing_import.proto"),
                "expected importing proto in error, got: {}",
                proto.display()
            );
            assert!(
                detail.contains("nonexistent_dependency.proto"),
                "expected protoc stderr in detail, got: {detail}"
            );
        }
        other => panic!("expected MissingImport error variant, got: {other:?}"),
    }

    let msg = err.to_string();
    assert!(msg.contains("nonexistent_dependency.proto"));
    assert!(msg.contains("failing_import.proto"));
    assert!(!msg.contains("parse error"));
    assert_eq!(err.path(), Some(proto_path.as_path()));
}

#[test]
fn error_syntax_error_identifies_cause_and_path() {
    let tmp = test_temp_dir("syntax-error-test");
    let proto_path = tmp.join("syntax_error.proto");
    std::fs::write(
        &proto_path,
        "syntax = \"proto3\";\nthis is invalid protobuf content;\n",
    )
    .unwrap();
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&[&proto_path], &[&tmp])
        .unwrap_err();

    match &err {
        pbrs::codegen::CodegenError::ProtocExecution {
            status,
            stderr,
            protos,
        } => {
            assert!(!status.success());
            assert!(
                stderr.contains("syntax_error.proto"),
                "expected stderr to mention failing proto, got: {stderr}"
            );
            assert_eq!(protos.as_slice(), std::slice::from_ref(&proto_path));
        }
        other => panic!("expected ProtocExecution error variant, got: {other:?}"),
    }

    let msg = err.to_string();
    assert!(msg.contains("syntax_error.proto"));
    assert!(msg.contains("protoc failed with"));
    assert_eq!(err.path(), Some(proto_path.as_path()));
    assert!(err.stderr().unwrap().contains("syntax_error.proto"));
}

#[test]
fn error_malformed_descriptor_identifies_cause() {
    let err = pbrs::codegen::generate_from_code_generator_request(&[0xff, 0xff]).unwrap_err();
    match &err {
        pbrs::codegen::CodegenError::MalformedDescriptor { detail, .. } => {
            assert!(detail.contains("CodeGeneratorRequest"));
        }
        other => panic!("expected MalformedDescriptor, got: {other:?}"),
    }
    assert!(err.to_string().contains("malformed protobuf descriptor"));

    let err_fds = pbrs::codegen::generate_from_file_descriptor_set(
        &[0x0a, 0x05, 0xff],
        &["test.proto".to_string()],
    )
    .unwrap_err();
    assert!(matches!(
        err_fds,
        pbrs::codegen::CodegenError::MalformedDescriptor { .. }
    ));
}

#[test]
fn descriptor_set_errors_identify_input_and_preserve_output() {
    let tmp = test_temp_dir("descriptor-errors");
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let prior = out_dir.join("hello.rs");
    std::fs::write(&prior, "prior complete output").unwrap();

    let missing = tmp.join("missing.fds");
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&missing, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::Io { path, source }
            if path == &missing && source.kind() == std::io::ErrorKind::NotFound
    ));
    assert_eq!(err.path(), Some(missing.as_path()));
    assert!(err.source().is_some());
    assert!(err.to_string().contains(&missing.display().to_string()));

    let malformed = tmp.join("malformed.fds");
    std::fs::write(&malformed, [0x0a, 0x05, 0xff]).unwrap();
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&malformed, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::MalformedDescriptor { path: Some(path), detail }
            if path == &malformed && !detail.is_empty()
    ));
    assert_eq!(err.path(), Some(malformed.as_path()));
    assert!(err.to_string().contains(&malformed.display().to_string()));

    let malformed_file = tmp.join("malformed_file.fds");
    std::fs::write(&malformed_file, [0x0a, 0x01, 0xff]).unwrap();
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&malformed_file, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::MalformedDescriptor { path: Some(path), .. }
            if path == &malformed_file
    ));

    let empty = tmp.join("empty.fds");
    std::fs::write(&empty, []).unwrap();
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&empty, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::MalformedDescriptor { path: Some(path), .. }
            if path == &empty
    ));

    let no_files = tmp.join("no_files.fds");
    std::fs::write(&no_files, [0x10, 0x01]).unwrap();
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&no_files, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::MalformedDescriptor { path: Some(path), .. }
            if path == &no_files
    ));

    let nameless = tmp.join("nameless.fds");
    std::fs::write(&nameless, [0x0a, 0x00]).unwrap();
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&nameless, &["hello.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::MalformedDescriptor { path: Some(path), .. }
            if path == &nameless
    ));

    let known = repo_root().join("tests/fixtures/differential/differential.fds");
    let err = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_descriptor_set(&known, &["missing_target.proto"], &["."])
        .unwrap_err();
    assert!(matches!(
        &err,
        pbrs::codegen::CodegenError::UnknownFile { file, available }
            if file == "missing_target.proto"
                && available.iter().any(|(name, _)| name == "differential_proto3.proto")
    ));
    assert_eq!(
        std::fs::read_to_string(&prior).unwrap(),
        "prior complete output"
    );
    assert!(!out_dir.join("mod.rs").exists());
    assert!(
        generated_files(&out_dir)
            .keys()
            .all(|p| !p.to_string_lossy().contains(".tmp"))
    );
}

#[test]
fn error_unwritable_output_identifies_cause_and_path() {
    let tmp = test_temp_dir("unwritable-test");
    let proto_path = tmp.join("name.proto");
    std::fs::write(
        &proto_path,
        "syntax = \"proto3\";\npackage test;\nmessage Name { string s = 1; }\n",
    )
    .unwrap();

    // Create a regular file where the out directory is supposed to be.
    let blocking_file = tmp.join("blocking_file");
    std::fs::write(&blocking_file, "blocking").unwrap();
    let unwritable_dir = blocking_file.join("sub");

    let err = pbrs::codegen::Config::new()
        .out_dir(&unwritable_dir)
        .compile_protos(&[&proto_path], &[&tmp])
        .unwrap_err();

    match &err {
        pbrs::codegen::CodegenError::UnwritableOutput { path, source: _ } => {
            assert_eq!(path, &unwritable_dir);
        }
        other => panic!("expected UnwritableOutput error variant, got: {other:?}"),
    }

    let msg = err.to_string();
    assert!(msg.contains("failed to write codegen output"));
    assert!(msg.contains(&unwritable_dir.display().to_string()));
    assert_eq!(err.path(), Some(unwritable_dir.as_path()));
}

#[test]
fn error_missing_out_dir_identifies_cause() {
    if std::env::var_os("PBRS_BUILD_TEST_MISSING_OUT_DIR_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "error_missing_out_dir_identifies_cause"])
            .env_remove("OUT_DIR")
            .env("PBRS_BUILD_TEST_MISSING_OUT_DIR_CHILD", "1")
            .output()
            .expect("run missing OUT_DIR check in a child");
        assert!(output.status.success(), "{}", dump(&output));
        return;
    }
    // When out_dir is not specified and OUT_DIR is not set.
    assert!(std::env::var_os("OUT_DIR").is_none());
    let tmp = test_temp_dir("missing-out-dir-test");
    let proto_path = tmp.join("test.proto");
    std::fs::write(&proto_path, "syntax = \"proto3\";\n").unwrap();

    let res = pbrs::codegen::Config::new().compile_protos(&[&proto_path], &[&tmp]);
    let err = res.unwrap_err();
    assert!(matches!(err, pbrs::codegen::CodegenError::MissingOutDir));
    assert!(err.to_string().contains("OUT_DIR"));
}

#[test]
fn codegen_error_converts_to_parse_error() {
    let err = pbrs::codegen::CodegenError::MissingOutDir;
    let parse_err: pbrs::ParseError = err.into();
    assert_eq!(parse_err, pbrs::ParseError);
}

#[test]
fn encode_code_generator_response_error_wire_format() {
    let msg = "failed to parse descriptor";
    let encoded = pbrs::codegen::encode_code_generator_response_error(msg);
    // CodeGeneratorResponse field 1 is string error: tag (1 << 3) | 2 = 10 (0x0a)
    assert_eq!(encoded[0], 0x0a);
    let len = encoded[1] as usize;
    let s = std::str::from_utf8(&encoded[2..2 + len]).unwrap();
    assert_eq!(s, msg);
}

#[test]
fn protoc_plugin_binary_outputs_error_response_on_invalid_stdin() {
    use std::io::Write;
    let mut child = Command::new(plugin_bin())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn protoc-gen-pbrs");

    let stdin = child.stdin.as_mut().expect("stdin");
    // Send invalid protobuf bytes to trigger MalformedDescriptor
    stdin.write_all(&[0xff, 0xff]).expect("write stdin");
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("wait on plugin");
    // Plugin must write CodeGeneratorResponse with error to stdout without corrupting stdout
    assert!(!output.stdout.is_empty(), "stdout must have response");
    assert_eq!(output.stdout[0], 0x0a, "first tag must be field 1 (error)");
    let len = output.stdout[1] as usize;
    let err_str = std::str::from_utf8(&output.stdout[2..2 + len]).unwrap();
    assert!(
        err_str.contains("malformed protobuf descriptor"),
        "error must identify malformed descriptor, got: {err_str}"
    );
}

#[test]
fn compile_protos_multi_file_stem_collision_and_cross_package_references() {
    let tmp = test_temp_dir("multi-file-collision-test");
    let root = repo_root();

    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("build.rs"),
        format!(
            r#"fn main() {{
    let root = std::path::PathBuf::from(r"{root}");
    let fixture = root.join("tests/fixtures/codegen-layout/proto");
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(false)
        .compile_protos(
            &[
                fixture.join("pkg_a/common.proto"),
                fixture.join("pkg_b/common.proto"),
                fixture.join("pkg_b/service.proto"),
            ],
            &[&fixture],
        )
        .expect("compile_protos");
}}
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        tmp.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-build-multi-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/mod.rs"));
use pkg::a::CommonMsg as ACommonMsg;
use pkg::a::CommonEnum as ACommonEnum;
use pkg::b::CommonMsg as BCommonMsg;
use pkg::b::CommonEnum as BCommonEnum;
use pkg::b::ServiceRequest;

fn main() {
    let mut a = ACommonMsg::new();
    a.set_a_name("alice");
    a.set_a_code(42);
    a.set_status_a(ACommonEnum::AActive);

    let mut b = BCommonMsg::new();
    b.set_b_id(100);
    b.set_status_b(BCommonEnum::BInitialized);

    let mut req = ServiceRequest::new();
    req.set_a_msg(a);
    req.set_b_msg(b);
    req.set_a_status(ACommonEnum::AActive);
    req.set_b_status(BCommonEnum::BInitialized);

    assert_eq!(req.a_msg().a_name(), "alice");
    assert_eq!(req.a_msg().a_code(), 42);
    assert_eq!(req.b_msg().b_id(), 100);
    assert_eq!(req.a_status(), ACommonEnum::AActive);
    assert_eq!(req.b_status(), BCommonEnum::BInitialized);

    let mut nested_a = pkg::a::common_msg::NestedA::new();
    nested_a.set_note_a("note_from_a");
    req.set_nested_a(nested_a);
    assert_eq!(req.nested_a().note_a(), "note_from_a");

    let mut nested_b = pkg::b::common_msg::NestedB::new();
    nested_b.set_count_b(888);
    req.set_nested_b(nested_b);
    assert_eq!(req.nested_b().count_b(), 888);

    println!("build multi-file collision-safe ok");
}
"#,
    )
    .unwrap();

    let run = cargo_run(&tmp, None, true);
    assert!(
        run.status.success(),
        "pbrs-build multi-file consumer failed:\n{}",
        dump(&run)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "build multi-file collision-safe ok"
    );
}

#[test]
fn ambiguous_stem_request_fails_with_diagnostic() {
    let tmp = test_temp_dir("ambiguous-stem-test");
    let root = repo_root();
    let fixture_proto = root.join("tests/fixtures/codegen-layout/proto");

    let fds_path = tmp.join("test.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", fds_path.display()))
        .arg("-I")
        .arg(&fixture_proto)
        .arg(fixture_proto.join("pkg_a/common.proto"))
        .arg(fixture_proto.join("pkg_b/common.proto"))
        .status()
        .expect("run protoc");
    assert!(status.success());
    let bytes = std::fs::read(&fds_path).expect("read fds");

    // Test with "common.proto"
    let res =
        pbrs::codegen::generate_from_file_descriptor_set(&bytes, &["common.proto".to_string()]);
    let err = res.expect_err("ambiguous common.proto must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("ambiguous proto stem 'common' across multiple files:"),
        "error message should contain stem diagnostic:\n{msg}"
    );
    assert!(
        msg.contains("pkg_a/common.proto (package pkg.a)"),
        "error message should detail pkg.a:\n{msg}"
    );
    assert!(
        msg.contains("pkg_b/common.proto (package pkg.b)"),
        "error message should detail pkg.b:\n{msg}"
    );
    assert!(
        msg.contains("Use the hierarchical path or include the root mod.rs instead."),
        "error message should suggest solution:\n{msg}"
    );

    // Test with "common.rs"
    let res_rs =
        pbrs::codegen::generate_from_file_descriptor_set(&bytes, &["common.rs".to_string()]);
    let err_rs = res_rs.expect_err("ambiguous common.rs must fail");
    let msg_rs = err_rs.to_string();
    assert!(
        msg_rs.contains("ambiguous proto stem 'common' across multiple files:"),
        "error message should contain stem diagnostic:\n{msg_rs}"
    );
}

#[test]
fn unknown_requested_file_fails_with_diagnostic() {
    let tmp = test_temp_dir("unknown-file-test");
    let root = repo_root();
    let fixture_proto = root.join("tests/fixtures/codegen-layout/proto");

    let fds_path = tmp.join("test.fds");
    let status = Command::new("protoc")
        .arg("--include_imports")
        .arg(format!("--descriptor_set_out={}", fds_path.display()))
        .arg("-I")
        .arg(&fixture_proto)
        .arg(fixture_proto.join("pkg_a/common.proto"))
        .arg(fixture_proto.join("pkg_b/common.proto"))
        .status()
        .expect("run protoc");
    assert!(status.success());
    let bytes = std::fs::read(&fds_path).expect("read fds");

    let res = pbrs::codegen::generate_from_file_descriptor_set(
        &bytes,
        &["nope/missing.proto".to_string()],
    );
    let err = res.expect_err("unknown nope/missing.proto must fail");
    assert!(
        matches!(err, pbrs::codegen::CodegenError::UnknownFile { .. }),
        "expected UnknownFile variant, got: {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("unknown proto file 'nope/missing.proto'"),
        "error message should name the unknown file:\n{msg}"
    );
    assert!(
        msg.contains("pkg_a/common.proto (package pkg.a)"),
        "error message should list pkg_a/common.proto:\n{msg}"
    );
    assert!(
        msg.contains("pkg_b/common.proto (package pkg.b)"),
        "error message should list pkg_b/common.proto:\n{msg}"
    );

    // A known canonical target from the same set still succeeds.
    let ok = pbrs::codegen::generate_from_file_descriptor_set(
        &bytes,
        &["pkg_a/common.proto".to_string()],
    )
    .expect("known target must succeed");
    assert!(
        ok.iter().any(|(name, _)| name == "pkg_a/common.rs"),
        "expected pkg_a/common.rs in {ok:?}"
    );
}

#[test]
fn transitive_public_import_chain_compiles() {
    let tmp = test_temp_dir("transitive-reexport-test");
    let root = repo_root();

    std::fs::create_dir_all(tmp.join("src")).unwrap();
    std::fs::write(
        tmp.join("build.rs"),
        format!(
            r#"fn main() {{
    let root = std::path::PathBuf::from(r"{root}");
    let fixture = root.join("tests/fixtures/codegen-layout/proto");
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(false)
        .compile_protos(
            &[
                fixture.join("reexport/grandparent.proto"),
                fixture.join("reexport/parent.proto"),
                fixture.join("reexport/child.proto"),
            ],
            &[&fixture],
        )
        .expect("compile_protos reexport");
}}
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        tmp.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-build-reexport-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        tmp.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/mod.rs"));
use reexport::child::{ChildData, GrandparentData, GrandparentLevel, ParentData};

fn main() {
    let mut gp = GrandparentData::new();
    gp.set_origin("earth");
    gp.set_level(GrandparentLevel::Root);

    let mut p = ParentData::new();
    p.set_lineage("family");
    p.set_direct_ref(gp);

    let mut c = ChildData::new();
    c.set_tag("child_tag");
    c.set_parent_data(p);

    assert_eq!(c.tag(), "child_tag");
    assert_eq!(c.parent_data().lineage(), "family");
    assert_eq!(c.parent_data().direct_ref().origin(), "earth");
    assert_eq!(c.parent_data().direct_ref().level(), GrandparentLevel::Root);

    println!("reexport ok");
}
"#,
    )
    .unwrap();

    let run = cargo_run(&tmp, None, true);
    assert!(
        run.status.success(),
        "pbrs-build reexport consumer failed:\n{}",
        dump(&run)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "reexport ok");
}

fn find_real_protoc() -> PathBuf {
    if let Some(p) = std::env::var_os("PROTOC").filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("protoc");
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("protoc")
}

fn cargo_build_verbose(dir: &Path, path: Option<&OsStr>) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("--offline").arg("-vv");
    cmd.current_dir(dir)
        .env(
            "CARGO_TARGET_DIR",
            repo_root().join("target/integration-consumers"),
        )
        .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cargo_output(&mut cmd, "cargo build -vv")
}

#[test]
fn custom_protoc_path_configuration() {
    if let Some(dir) = std::env::var_os("PBRS_BUILD_TEST_CUSTOM_PROTOC_CHILD") {
        let tmp = PathBuf::from(dir);
        let custom_bin = tmp.join("my-custom-protoc");
        let proto_path = tmp.join("test.proto");
        let out_dir = tmp.join("out");
        let res = pbrs::codegen::Config::new()
            .protoc_path(&custom_bin)
            .out_dir(&out_dir)
            .emit_kernel_stubs(false)
            .compile_protos(&[&proto_path], &[&tmp]);
        assert!(
            res.is_ok(),
            "compile with custom protoc_path must succeed: {:?}",
            res.err()
        );
        return;
    }
    let tmp = test_temp_dir("custom-protoc-test");
    let real_protoc = find_real_protoc();
    assert!(
        real_protoc.exists(),
        "real protoc must exist on test machine"
    );

    let custom_bin = tmp.join("my-custom-protoc");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_protoc, &custom_bin).expect("symlink protoc");
    #[cfg(not(unix))]
    std::fs::copy(&real_protoc, &custom_bin).expect("copy protoc");

    let proto_path = tmp.join("test.proto");
    std::fs::write(
        &proto_path,
        "syntax = \"proto3\";\npackage test;\nmessage CustomMsg { string val = 1; }\n",
    )
    .unwrap();
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut config = pbrs::codegen::Config::new();
    config.protoc_path(&custom_bin);

    assert_eq!(config.get_protoc_path(), Some(custom_bin.as_path()));
    assert_eq!(config.selected_protoc_path(), custom_bin);

    let version = config.protoc_version().expect("protoc_version query");
    assert!(
        version.contains("libprotoc") || version.contains("protoc"),
        "expected protoc version, got: {version}"
    );

    let no_protoc_path = path_without_protoc();
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "custom_protoc_path_configuration"])
        .env("PBRS_BUILD_TEST_CUSTOM_PROTOC_CHILD", &tmp)
        .env("PATH", &no_protoc_path)
        .output()
        .expect("run custom protoc check in a child");
    assert!(
        output.status.success(),
        "custom protoc subprocess failed:\n{}",
        dump(&output)
    );
    assert!(out_dir.join("test.rs").exists());
}

#[test]
fn error_invalid_protoc_path_diagnostics() {
    let tmp = test_temp_dir("invalid-protoc-test");
    let proto_path = tmp.join("test.proto");
    std::fs::write(&proto_path, "syntax = \"proto3\";\nmessage Dummy {}\n").unwrap();
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Non-existent protoc executable path.
    let nonexistent = tmp.join("nonexistent_protoc_binary");
    let err = pbrs::codegen::Config::new()
        .protoc_path(&nonexistent)
        .out_dir(&out_dir)
        .compile_protos(&[&proto_path], &[&tmp])
        .unwrap_err();

    match &err {
        pbrs::codegen::CodegenError::MissingProtoc { path, source } => {
            assert_eq!(path, &nonexistent);
            let s = source.to_string();
            assert!(
                s.contains("No such file") || s.contains("not found"),
                "expected not found in source error, got: {s}"
            );
        }
        other => panic!("expected MissingProtoc, got: {other:?}"),
    }
    let msg = err.to_string();
    assert!(msg.contains("protoc executable not found or failed to execute at"));
    assert!(msg.contains(&nonexistent.display().to_string()));
    assert_eq!(err.path(), Some(nonexistent.as_path()));
    assert!(err.source().is_some());

    // 2. protoc_version on nonexistent path.
    let ver_err = pbrs::codegen::Config::new()
        .protoc_path(&nonexistent)
        .protoc_version()
        .unwrap_err();
    assert!(matches!(
        ver_err,
        pbrs::codegen::CodegenError::MissingProtoc { .. }
    ));

    // 3. Failing protoc script exiting with 127.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let failing_shim = tmp.join("failing_shim_127");
        std::fs::write(&failing_shim, "#!/bin/sh\nexit 127\n").unwrap();
        let mut perms = std::fs::metadata(&failing_shim).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&failing_shim, perms).unwrap();

        let shim_err = pbrs::codegen::Config::new()
            .protoc_path(&failing_shim)
            .out_dir(&out_dir)
            .compile_protos(&[&proto_path], &[&tmp])
            .unwrap_err();

        match &shim_err {
            pbrs::codegen::CodegenError::MissingProtoc { path, source } => {
                assert_eq!(path, &failing_shim);
                assert!(source.to_string().contains("127"));
            }
            other => panic!("expected MissingProtoc for exit 127 shim, got: {other:?}"),
        }
        assert!(
            shim_err
                .to_string()
                .contains(&failing_shim.display().to_string())
        );
    }
}

#[test]
fn transitive_proto_rebuild_directives_emitted() {
    let tmp = test_temp_dir("transitive-rebuild-test");
    let root = repo_root();

    // Create three separate include roots containing spaces!
    let inc1 = tmp.join("include search root 1");
    let inc2 = tmp.join("include search root 2");
    let inc3 = tmp.join("include search root 3");
    std::fs::create_dir_all(inc1.join("pkg_root")).unwrap();
    std::fs::create_dir_all(inc2.join("dep_mid")).unwrap();
    std::fs::create_dir_all(inc3.join("sub_leaf")).unwrap();

    let leaf_proto = inc3.join("sub_leaf/leaf.proto");
    std::fs::write(
        &leaf_proto,
        r#"syntax = "proto3";
package test.rebuild;
message LeafMsg {
    string leaf_name = 1;
}
"#,
    )
    .unwrap();

    let mid_proto = inc2.join("dep_mid/middle.proto");
    std::fs::write(
        &mid_proto,
        r#"syntax = "proto3";
package test.rebuild;
import public "sub_leaf/leaf.proto";
message MiddleMsg {
    int32 count = 1;
    LeafMsg leaf = 2;
}
"#,
    )
    .unwrap();

    let main_proto = inc1.join("pkg_root/main.proto");
    std::fs::write(
        &main_proto,
        r#"syntax = "proto3";
package test.rebuild;
import public "dep_mid/middle.proto";
message MainMsg {
    string title = 1;
    MiddleMsg middle = 2;
}
"#,
    )
    .unwrap();

    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();

    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "pbrs-rebuild-consumer"
version = "0.0.1"
edition = "2021"
[workspace]
[dependencies]
pbrs = {{ path = "{root}" }}
[build-dependencies]
pbrs = {{ path = "{root}" }}
"#,
            root = root.display()
        ),
    )
    .unwrap();

    std::fs::write(
        consumer.join("build.rs"),
        format!(
            r#"fn main() {{
    let main_proto = std::path::PathBuf::from(r"{main_proto}");
    let inc1 = std::path::PathBuf::from(r"{inc1}");
    let inc2 = std::path::PathBuf::from(r"{inc2}");
    let inc3 = std::path::PathBuf::from(r"{inc3}");
    pbrs::codegen::Config::new()
        .emit_kernel_stubs(false)
        .compile_protos(&[&main_proto], &[&inc1, &inc2, &inc3])
        .expect("compile_protos with transitive imports");
}}
"#,
            main_proto = main_proto.display(),
            inc1 = inc1.display(),
            inc2 = inc2.display(),
            inc3 = inc3.display()
        ),
    )
    .unwrap();

    std::fs::write(
        consumer.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/mod.rs"));
use test::rebuild::{LeafMsg, MiddleMsg, MainMsg};

fn main() {
    let mut leaf = LeafMsg::new();
    leaf.set_leaf_name("autumn");
    let mut mid = MiddleMsg::new();
    mid.set_count(7);
    mid.set_leaf(leaf);
    let mut m = MainMsg::new();
    m.set_title("tree");
    m.set_middle(mid);

    assert_eq!(m.title(), "tree");
    assert_eq!(m.middle().count(), 7);
    assert_eq!(m.middle().leaf().leaf_name(), "autumn");
    println!("initial run ok");
}
"#,
    )
    .unwrap();

    // 1. Initial build: build with verbose output to inspect cargo rerun directives.
    let build_out = cargo_build_verbose(&consumer, None);
    assert!(
        build_out.status.success(),
        "consumer initial build failed:\n{}",
        dump(&build_out)
    );
    let build_text = dump(&build_out);

    // Verify directives are emitted for main proto and ALL transitive imported proto files!
    assert!(
        build_text.contains("cargo:rerun-if-changed="),
        "missing rerun-if-changed in build output:\n{build_text}"
    );
    assert!(
        build_text.contains(&main_proto.display().to_string()),
        "missing rerun-if-changed for main proto in build output:\n{build_text}"
    );
    assert!(
        build_text.contains(&mid_proto.display().to_string()),
        "missing rerun-if-changed for middle proto in build output:\n{build_text}"
    );
    assert!(
        build_text.contains(&leaf_proto.display().to_string()),
        "missing rerun-if-changed for transitive leaf proto in build output:\n{build_text}"
    );
    assert!(
        build_text.contains("cargo:rerun-if-env-changed=PROTOC"),
        "missing rerun-if-env-changed=PROTOC in build output:\n{build_text}"
    );
    assert!(
        build_text.contains("cargo:rerun-if-env-changed=PURE_PROTOBUF_"),
        "missing rerun-if-env-changed=PURE_PROTOBUF_ in build output:\n{build_text}"
    );

    // 2. Initial run verifies generated types work end-to-end.
    let run1 = cargo_run(&consumer, None, false);
    assert!(
        run1.status.success(),
        "consumer initial run failed:\n{}",
        dump(&run1)
    );
    assert_eq!(
        String::from_utf8_lossy(&run1.stdout).trim(),
        "initial run ok"
    );

    // 3. Update ONLY the transitive dependency leaf.proto (second-level dependency).
    // Ensure mtime advances so Cargo detects the change.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        &leaf_proto,
        r#"syntax = "proto3";
package test.rebuild;
message LeafMsg {
    string leaf_name = 1;
    int64 version_id = 2;
}
"#,
    )
    .unwrap();

    // Update src/main.rs to assert the new field added to leaf.proto.
    std::fs::write(
        consumer.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/mod.rs"));
use test::rebuild::{LeafMsg, MiddleMsg, MainMsg};

fn main() {
    let mut leaf = LeafMsg::new();
    leaf.set_leaf_name("autumn");
    leaf.set_version_id(4242);
    let mut mid = MiddleMsg::new();
    mid.set_count(7);
    mid.set_leaf(leaf);
    let mut m = MainMsg::new();
    m.set_title("tree");
    m.set_middle(mid);

    assert_eq!(m.middle().leaf().version_id(), 4242);
    println!("regenerated after leaf change ok");
}
"#,
    )
    .unwrap();

    // 4. Re-run cargo: Cargo must detect leaf.proto changed and re-run build.rs!
    let run2 = cargo_run(&consumer, None, false);
    assert!(
        run2.status.success(),
        "consumer rebuild after transitive change failed:\n{}",
        dump(&run2)
    );
    assert_eq!(
        String::from_utf8_lossy(&run2.stdout).trim(),
        "regenerated after leaf change ok"
    );
}

#[test]
fn mtime_preservation_for_identical_content() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let tmp = test_temp_dir("mtime-preservation");
    let proto_path = tmp.join("test.proto");
    std::fs::write(
        &proto_path,
        r#"syntax = "proto3";
package test.mtime;
message StableMsg {
    string name = 1;
    int32 count = 2;
}
"#,
    )
    .unwrap();

    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Initial compilation
    pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .emit_kernel_stubs(false)
        .compile_protos(&[&proto_path], &[&tmp])
        .expect("initial compile_protos");

    let test_rs = out_dir.join("test.rs");
    let mod_rs = out_dir.join("mod.rs");
    assert!(test_rs.exists(), "test.rs must exist");
    assert!(mod_rs.exists(), "mod.rs must exist");

    let mtime_test_1 = std::fs::metadata(&test_rs).unwrap().modified().unwrap();
    let mtime_mod_1 = std::fs::metadata(&mod_rs).unwrap().modified().unwrap();

    // Small delay to ensure timestamp resolution can advance if written
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 2. Re-compilation with IDENTICAL inputs
    pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .emit_kernel_stubs(false)
        .compile_protos(&[&proto_path], &[&tmp])
        .expect("second compile_protos with identical inputs");

    let mtime_test_2 = std::fs::metadata(&test_rs).unwrap().modified().unwrap();
    let mtime_mod_2 = std::fs::metadata(&mod_rs).unwrap().modified().unwrap();

    assert_eq!(
        mtime_test_1, mtime_test_2,
        "mtime of test.rs must be preserved when content is unchanged"
    );
    assert_eq!(
        mtime_mod_1, mtime_mod_2,
        "mtime of mod.rs must be preserved when content is unchanged"
    );

    // 3. Update proto input so content actually changes
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(
        &proto_path,
        r#"syntax = "proto3";
package test.mtime;
message StableMsg {
    string name = 1;
    int32 count = 2;
    string updated_field = 3;
}
"#,
    )
    .unwrap();

    pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .emit_kernel_stubs(false)
        .compile_protos(&[&proto_path], &[&tmp])
        .expect("compile_protos with changed inputs");

    let mtime_test_3 = std::fs::metadata(&test_rs).unwrap().modified().unwrap();
    assert!(
        mtime_test_3 > mtime_test_1,
        "mtime of test.rs must be updated when content changed"
    );
}

#[test]
fn changed_input_updates_only_affected_outputs() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let tmp = test_temp_dir("affected-outputs");
    let alpha_proto = tmp.join("alpha.proto");
    let beta_proto = tmp.join("beta.proto");
    std::fs::write(
        &alpha_proto,
        "syntax = \"proto3\";\npackage test.affected;\nmessage Alpha {\n    string a = 1;\n}\n",
    )
    .unwrap();
    std::fs::write(
        &beta_proto,
        "syntax = \"proto3\";\npackage test.affected;\nmessage Beta {\n    string b = 1;\n}\n",
    )
    .unwrap();

    // no_reflect removes the shared embedded FileDescriptorSet so each
    // output depends only on its own input; with default reflection every
    // output embeds the whole set and is legitimately affected by any change.
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .emit_kernel_stubs(false)
        .no_reflect(true)
        .compile_protos(&[&alpha_proto, &beta_proto], &[&tmp])
        .expect("initial compile");

    let alpha_rs = out_dir.join("alpha.rs");
    let beta_rs = out_dir.join("beta.rs");
    let alpha_before = std::fs::read(&alpha_rs).expect("read alpha");
    let beta_before = std::fs::read(&beta_rs).expect("read beta");
    assert!(
        !String::from_utf8_lossy(&alpha_before).contains("FILE_DESCRIPTOR_SET"),
        "no_reflect output must not embed shared descriptor bytes"
    );
    let alpha_mtime = std::fs::metadata(&alpha_rs).unwrap().modified().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(
        &beta_proto,
        "syntax = \"proto3\";\npackage test.affected;\nmessage Beta {\n    string b = 1;\n    int32 added = 2;\n}\n",
    )
    .unwrap();
    pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .emit_kernel_stubs(false)
        .no_reflect(true)
        .compile_protos(&[&alpha_proto, &beta_proto], &[&tmp])
        .expect("recompile after beta change");

    let alpha_after = std::fs::read(&alpha_rs).expect("reread alpha");
    let beta_after = std::fs::read(&beta_rs).expect("reread beta");
    assert_eq!(
        alpha_before, alpha_after,
        "alpha.rs bytes must be identical when only beta.proto changed"
    );
    assert_eq!(
        std::fs::metadata(&alpha_rs).unwrap().modified().unwrap(),
        alpha_mtime,
        "alpha.rs mtime must be preserved when only beta.proto changed"
    );
    assert_ne!(
        beta_before, beta_after,
        "beta.rs bytes must change when beta.proto changed"
    );
    assert!(
        String::from_utf8_lossy(&beta_after).contains("added"),
        "beta.rs must contain the new field"
    );
    for entry in std::fs::read_dir(&out_dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".tmp"),
            "temporary file left behind in output directory: {name}"
        );
    }
}

#[test]
fn atomic_write_on_failure_preserves_prior_output() {
    let tmp = test_temp_dir("atomic-failure-test");
    let out_dir = tmp.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let prior_file = out_dir.join("existing.rs");
    let prior_content = "// prior complete successful output\npub struct Existing;\n";
    std::fs::write(&prior_file, prior_content).unwrap();

    // Attempt compilation of invalid proto (syntax error)
    let bad_proto = tmp.join("syntax_error.proto");
    std::fs::write(&bad_proto, "this is not valid proto syntax;;;;\n").unwrap();

    let res = pbrs::codegen::Config::new()
        .out_dir(&out_dir)
        .compile_protos(&[&bad_proto], &[&tmp]);

    assert!(res.is_err(), "compile of invalid proto must fail");

    // Verify prior complete output is completely intact
    assert!(prior_file.exists(), "prior output must still exist");
    let current_content = std::fs::read_to_string(&prior_file).unwrap();
    assert_eq!(
        current_content, prior_content,
        "prior output must not be modified or truncated on failure"
    );

    // Verify no temporary .tmp files left in out_dir
    let entries = std::fs::read_dir(&out_dir).unwrap();
    for entry in entries {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.contains(".tmp"),
            "temporary file left behind in output directory: {name}"
        );
    }
}

#[test]
fn byte_stability_across_input_permutations_and_no_host_paths() {
    let _lock = ENV_MUTEX.lock().unwrap();
    let tmp = test_temp_dir("byte-stability-permutations");
    let alpha_proto = tmp.join("alpha.proto");
    let beta_proto = tmp.join("beta.proto");

    std::fs::write(
        &alpha_proto,
        r#"syntax = "proto3";
package test.stability;

message AlphaMsg {
    string a_name = 1;
    int32 a_id = 2;
    oneof payload {
        string text = 3;
        int64 num = 4;
    }
}

enum AlphaEnum {
    ALPHA_UNKNOWN = 0;
    ALPHA_ACTIVE = 1;
}

service AlphaService {
    rpc ZMethod (AlphaMsg) returns (AlphaMsg);
    rpc AMethod (AlphaMsg) returns (AlphaMsg);
}
"#,
    )
    .unwrap();

    std::fs::write(
        &beta_proto,
        r#"syntax = "proto3";
package test.stability;

message BetaMsg {
    string b_name = 1;
}

enum BetaEnum {
    BETA_UNKNOWN = 0;
    BETA_READY = 1;
}
"#,
    )
    .unwrap();

    let out1 = tmp.join("out1");
    let out2 = tmp.join("out2");
    std::fs::create_dir_all(&out1).unwrap();
    std::fs::create_dir_all(&out2).unwrap();

    // Order 1: alpha then beta
    pbrs::codegen::Config::new()
        .out_dir(&out1)
        .emit_kernel_stubs(true)
        .compile_protos(&[&alpha_proto, &beta_proto], &[&tmp])
        .expect("compile order 1");

    // Order 2: beta then alpha (permuted input order!)
    pbrs::codegen::Config::new()
        .out_dir(&out2)
        .emit_kernel_stubs(true)
        .compile_protos(&[&beta_proto, &alpha_proto], &[&tmp])
        .expect("compile order 2");

    // Read generated files
    let alpha1 = std::fs::read(out1.join("alpha.rs")).expect("read alpha1");
    let alpha2 = std::fs::read(out2.join("alpha.rs")).expect("read alpha2");
    assert_eq!(
        alpha1, alpha2,
        "alpha.rs must be byte-for-byte identical across input permutations"
    );

    let beta1 = std::fs::read(out1.join("beta.rs")).expect("read beta1");
    let beta2 = std::fs::read(out2.join("beta.rs")).expect("read beta2");
    assert_eq!(
        beta1, beta2,
        "beta.rs must be byte-for-byte identical across input permutations"
    );

    let mod1 = std::fs::read(out1.join("mod.rs")).expect("read mod1");
    let mod2 = std::fs::read(out2.join("mod.rs")).expect("read mod2");
    assert_eq!(
        mod1, mod2,
        "mod.rs must be byte-for-byte identical across input permutations"
    );

    // Verify no absolute host paths in output
    let alpha_str = String::from_utf8(alpha1).unwrap();
    assert!(
        !alpha_str.contains(&tmp.display().to_string()),
        "generated code must not contain absolute host path: {}",
        tmp.display()
    );
    assert!(
        !alpha_str.contains("/Users/") && !alpha_str.contains("/home/"),
        "generated code must not contain host paths"
    );

    // Verify method sorting in service
    let pos_a = alpha_str
        .find("fn a_method")
        .expect("must contain a_method");
    let pos_z = alpha_str
        .find("fn z_method")
        .expect("must contain z_method");
    assert!(
        pos_a < pos_z,
        "service methods must be emitted in deterministic sorted order"
    );
}

#[test]
fn descriptor_set_matches_compile_protos_layout_and_stub_modes() {
    let tmp = test_temp_dir("descriptor-equivalence");
    let proto_dir = repo_root().join("proto");
    let proto = proto_dir.join("hello.proto");
    let fds = tmp.join("hello.fds");
    write_descriptor_set(&fds, &[&proto], &[&proto_dir], true);

    for (mode, stubs) in [
        ("messages", pbrs::codegen::Stubs::None),
        ("native", pbrs::codegen::Stubs::Kernel),
        ("tonic", pbrs::codegen::Stubs::Tonic),
    ] {
        let from_protos = tmp.join(format!("{mode}-protos"));
        let from_descriptor = tmp.join(format!("{mode}-descriptor"));
        pbrs::codegen::Config::new()
            .out_dir(&from_protos)
            .stubs(stubs)
            .include_source_info(true)
            .compile_protos(&[&proto], &[&proto_dir])
            .expect("compile_protos");
        pbrs::codegen::Config::new()
            .out_dir(&from_descriptor)
            .stubs(stubs)
            .include_source_info(true)
            .compile_descriptor_set(&fds, &[&proto], &[&proto_dir])
            .expect("compile_descriptor_set");

        let expected = generated_files(&from_protos);
        let actual = generated_files(&from_descriptor);
        assert!(actual.contains_key(&PathBuf::from("hello.rs")), "{mode}");
        assert!(actual.contains_key(&PathBuf::from("mod.rs")), "{mode}");
        assert_eq!(
            actual, expected,
            "{mode}: descriptor output differs from protoc"
        );
        let hello = std::fs::read_to_string(from_descriptor.join("hello.rs")).unwrap();
        assert_eq!(
            hello.contains("pub struct GreeterClient"),
            stubs != pbrs::codegen::Stubs::None
        );
        assert_eq!(
            hello.contains("::pbrs_grpc"),
            stubs == pbrs::codegen::Stubs::Kernel
        );
        assert_eq!(
            hello.contains("ProtobufCodec"),
            stubs == pbrs::codegen::Stubs::Tonic
        );
        assert!(
            hello.contains("FILE_DESCRIPTOR_SET"),
            "{mode}: reflection lost"
        );
    }

    let fixture = repo_root().join("tests/fixtures/codegen-layout/proto");
    let protos = [
        fixture.join("pkg_a/common.proto"),
        fixture.join("pkg_b/common.proto"),
        fixture.join("pkg_b/service.proto"),
    ];
    let fds = tmp.join("multi.fds");
    write_descriptor_set(
        &fds,
        &[&protos[0], &protos[1], &protos[2]],
        &[&fixture],
        true,
    );
    let from_protos = tmp.join("multi-protos");
    let from_descriptor = tmp.join("multi-descriptor");
    pbrs::codegen::Config::new()
        .out_dir(&from_protos)
        .stubs(pbrs::codegen::Stubs::None)
        .include_source_info(true)
        .compile_protos(&protos, &[&fixture])
        .expect("compile_protos with colliding stems");
    pbrs::codegen::Config::new()
        .out_dir(&from_descriptor)
        .stubs(pbrs::codegen::Stubs::None)
        .include_source_info(true)
        .compile_descriptor_set(&fds, &protos, &[&fixture])
        .expect("compile_descriptor_set with colliding stems");
    let expected = generated_files(&from_protos);
    let actual = generated_files(&from_descriptor);
    assert!(actual.contains_key(&PathBuf::from("pkg_a/common.rs")));
    assert!(actual.contains_key(&PathBuf::from("pkg_b/common.rs")));
    assert!(actual.contains_key(&PathBuf::from("pkg_b/service.rs")));
    assert!(!actual.contains_key(&PathBuf::from("common.rs")));
    assert_eq!(actual, expected, "hierarchical layout differs from protoc");
}

#[test]
fn checked_descriptor_messages_consumer_builds_without_protoc() {
    let tmp = test_temp_dir("descriptor-messages-consumer");
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    let root = repo_root();
    let fds = root.join("tests/fixtures/differential/differential.fds");
    std::fs::write(
        tmp.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-descriptor-messages-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        tmp.join("build.rs"),
        format!(
            r#"fn main() {{
    pbrs::codegen::Config::new()
        .protoc_path("nonexistent-protoc")
        .stubs(pbrs::codegen::Stubs::None)
        .compile_descriptor_set(r"{fds}", &["differential_proto3.proto"], &["."])
        .expect("generate from checked-in descriptor");
}}
"#,
            fds = fds.display()
        ),
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/main.rs"),
        r#"include!(concat!(env!("OUT_DIR"), "/differential_proto3.rs"));
use pbrs::Parse;
fn main() {
    let message = Proto3Presence::parse(&[0x08, 0x2a]).expect("parse");
    assert_eq!(message.implicit_int32(), 42);
    println!("descriptor messages ok");
}
"#,
    )
    .unwrap();

    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    let build = cargo_build_verbose(&tmp, Some(&no_protoc));
    let log = dump(&build);
    assert!(
        build.status.success(),
        "fresh messages build failed:\n{log}"
    );
    assert!(
        log.contains(&format!("cargo:rerun-if-changed={}", fds.display())),
        "descriptor path missing from Cargo rebuild metadata:\n{log}"
    );
    let run = cargo_run(&tmp, Some(&no_protoc), true);
    assert!(
        run.status.success(),
        "messages consumer failed:\n{}",
        dump(&run)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "descriptor messages ok"
    );
}

#[test]
fn descriptor_set_native_and_tonic_consumers_build_without_protoc() {
    let tmp = test_temp_dir("descriptor-stubs-consumer");
    std::fs::create_dir_all(tmp.join("src")).unwrap();
    let root = repo_root();
    let proto_dir = root.join("proto");
    let proto = proto_dir.join("hello.proto");
    let fds = tmp.join("hello.fds");
    write_descriptor_set(&fds, &[&proto], &[&proto_dir], true);
    std::fs::write(
        tmp.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-descriptor-stubs-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\npbrs-grpc = {{ path = \"{root}/pbrs-grpc\" }}\nprotobuf-tonic = {{ path = \"{root}/protobuf-tonic\" }}\ntonic = {{ version = \"0.14\", default-features = false, features = [\"transport\", \"codegen\", \"router\", \"gzip\"] }}\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\", \"time\", \"sync\"] }}\ntokio-stream = {{ version = \"0.1\", features = [\"net\"] }}\nhttp = \"1\"\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(tmp.join("build.rs"), "fn main() {}\n").unwrap();
    std::fs::write(tmp.join("src/main.rs"), "fn main() {}\n").unwrap();

    // These existing adapters compile their own protos in build.rs. Warm them
    // before hiding protoc; this card only removes the consumer's protoc need.
    let warm = cargo_run(&tmp, None, true);
    assert!(
        warm.status.success(),
        "adapter prebuild failed:\n{}",
        dump(&warm)
    );

    std::fs::write(
        tmp.join("build.rs"),
        r#"fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    for (mode, stubs) in [
        ("native", pbrs::codegen::Stubs::Kernel),
        ("tonic", pbrs::codegen::Stubs::Tonic),
    ] {
        pbrs::codegen::Config::new()
            .out_dir(out.join(mode))
            .protoc_path("nonexistent-protoc")
            .stubs(stubs)
            .compile_descriptor_set("hello.fds", &["hello.proto"], &["."])
            .expect("generate stubs from descriptor");
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.join("src/main.rs"),
        r#"mod native {
    include!(concat!(env!("OUT_DIR"), "/native/hello.rs"));
}
mod tonic_mode {
    include!(concat!(env!("OUT_DIR"), "/tonic/hello.rs"));
}
use pbrs::Parse;
fn main() {
    let wire = [0x0a, 0x03, b'a', b'd', b'a'];
    assert_eq!(native::HelloRequest::parse(&wire).expect("native").name(), "ada");
    assert_eq!(tonic_mode::HelloRequest::parse(&wire).expect("tonic").name(), "ada");
    let _ = std::any::type_name::<native::GreeterClient>();
    let _ = std::any::type_name::<tonic_mode::GreeterClient<tonic::transport::Channel>>();
    println!("descriptor native and tonic ok");
}
"#,
    )
    .unwrap();

    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    let build = cargo_build_verbose(&tmp, Some(&no_protoc));
    let log = dump(&build);
    assert!(
        build.status.success(),
        "stubs build without protoc failed:\n{log}"
    );
    assert!(
        log.contains("cargo:rerun-if-changed=hello.fds"),
        "descriptor rebuild metadata missing:\n{log}"
    );
    let run = cargo_run(&tmp, Some(&no_protoc), true);
    assert!(
        run.status.success(),
        "stubs consumer failed:\n{}",
        dump(&run)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "descriptor native and tonic ok"
    );
}

#[test]
fn descriptor_set_tracks_transitive_source_provenance() {
    let tmp = test_temp_dir("descriptor-provenance");
    let consumer = tmp.join("consumer");
    std::fs::create_dir_all(consumer.join("src")).unwrap();
    let root = repo_root();
    let fixture = root.join("tests/fixtures/codegen-layout/proto");
    let child = fixture.join("reexport/child.proto");
    let parent = fixture.join("reexport/parent.proto");
    let grandparent = fixture.join("reexport/grandparent.proto");
    let fds = consumer.join("reexport.fds");
    write_descriptor_set(&fds, &[&child], &[&fixture], true);

    std::fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-descriptor-provenance-consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(
        consumer.join("build.rs"),
        format!(
            r#"fn main() {{
    pbrs::codegen::Config::new()
        .stubs(pbrs::codegen::Stubs::None)
        .compile_descriptor_set("reexport.fds", &["reexport/child.proto"], &[r"{fixture}"])
        .expect("descriptor with transitive imports");
}}
"#,
            fixture = fixture.display()
        ),
    )
    .unwrap();
    std::fs::write(consumer.join("src/main.rs"), "fn main() {}\n").unwrap();

    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    let build = cargo_build_verbose(&consumer, Some(&no_protoc));
    let log = dump(&build);
    assert!(build.status.success(), "provenance consumer failed:\n{log}");
    for path in [&fds, &child, &parent, &grandparent] {
        let entry = if path == &fds {
            "cargo:rerun-if-changed=reexport.fds".to_string()
        } else {
            format!("cargo:rerun-if-changed={}", path.display())
        };
        assert!(
            log.contains(&entry),
            "missing {entry} in build output:\n{log}"
        );
    }
}
