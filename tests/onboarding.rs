//! Advertised quickstarts as fresh-directory consumers of the path crates.

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

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HELLO_PROTO: &str = r#"syntax = "proto3";
package helloworld;

service Greeter {
  rpc SayHello (HelloRequest) returns (HelloReply);
}

message HelloRequest { string name = 1; }
message HelloReply { string message = 1; }
"#;

const NATIVE_MAIN: &str = r#"include!(concat!(env!("OUT_DIR"), "/hello.rs"));
use pbrs_grpc::{Request, Response, Status};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or_default();
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {name}"));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        GreeterServer::new(Echo).serve_listener(listener).await.ok();
    });
    let mut last = Status::unavailable("connect");
    let mut client = None;
    for _ in 0..80 {
        match GreeterClient::connect(addr).await {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    let client = client.ok_or(last)?;
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let reply = client.say_hello(Request::new(req)).await?;
    println!("{}", reply.get_ref().message().to_str().unwrap_or_default());
    Ok(())
}
"#;

const TONIC_MAIN: &str = r#"include!(concat!(env!("OUT_DIR"), "/hello.rs"));
use std::time::Duration;
use tokio::net::TcpListener;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or_default();
        let mut reply = HelloReply::new();
        reply.set_message(format!("hello {name}"));
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        Server::builder()
            .add_service(GreeterServer::new(Echo))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let mut last = String::from("connect");
    let mut client = None;
    for _ in 0..80 {
        match Channel::from_shared(format!("http://{addr}"))
            .expect("uri")
            .connect()
            .await
        {
            Ok(ch) => {
                client = Some(GreeterClient::new(ch));
                break;
            }
            Err(e) => {
                last = e.to_string();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    let mut client = client.unwrap_or_else(|| panic!("connect: {last}"));
    let mut req = HelloRequest::new();
    req.set_name("ada");
    let resp = client.say_hello(Request::new(req)).await.expect("unary");
    println!("{}", resp.into_inner().message().to_str().unwrap_or_default());
}
"#;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = repo_root().join("target").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    dir
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
    // `/usr/bin/env` honors this Command's PATH, unlike a bare `protoc` lookup
    // that can still resolve via the parent process PATH.
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

fn cargo_run(dir: &Path, path: Option<&OsStr>, quiet: bool) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run").arg("--offline");
    if quiet {
        cmd.arg("--quiet");
    }
    cmd.current_dir(dir).env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cmd.output().expect("cargo run")
}

fn cargo_check_pkg(pkg: &str, target_dir: &Path, path: Option<&OsStr>) -> Output {
    // Unique CARGO_TARGET_DIR: a cached workspace OUT_DIR is not proof that
    // build.rs ran without protoc.
    let mut cmd = Command::new("cargo");
    cmd.arg("check")
        .arg("-p")
        .arg(pkg)
        .arg("--offline")
        .arg("--lib")
        .current_dir(repo_root())
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    cmd.output().expect("cargo check")
}

fn assert_build_failed_without_protoc(out: &Output) {
    let text = dump(out);
    assert!(!out.status.success(), "must fail without protoc:\n{text}");
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

fn assert_stdout_hello_ada(out: &Output) {
    assert!(out.status.success(), "consumer failed:\n{}", dump(out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim(),
        "hello ada",
        "expected SayHello reply content, got:\n{}",
        dump(out)
    );
    println!("{}", stdout.trim());
}

fn write_hello_proto(dir: &Path) {
    std::fs::write(dir.join("hello.proto"), HELLO_PROTO).unwrap();
}

fn write_native_consumer(dir: &Path) {
    let root = repo_root();
    write_hello_proto(dir);
    std::fs::write(
        dir.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::compile_protos(&["hello.proto"], &["."]).expect("compile_protos");
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-onboarding-native\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\npbrs-grpc = {{ path = \"{root}/pbrs-grpc\" }}\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\", \"time\", \"sync\"] }}\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), NATIVE_MAIN).unwrap();
}

fn write_tonic_consumer(dir: &Path) {
    let root = repo_root();
    write_hello_proto(dir);
    std::fs::write(
        dir.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::Config::new()
        .emit_tonic_stubs(true)
        .compile_protos(&["hello.proto"], &["."])
        .expect("compile_protos");
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-onboarding-tonic\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\nprotobuf-tonic = {{ path = \"{root}/protobuf-tonic\" }}\ntonic = {{ version = \"0.14\", default-features = false, features = [\"transport\", \"codegen\", \"router\", \"gzip\"] }}\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\", \"time\", \"sync\"] }}\ntokio-stream = {{ version = \"0.1\", features = [\"net\"] }}\nhttp = \"1\"\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), TONIC_MAIN).unwrap();
}

const FOUR_SHAPES_MAIN: &str = r#"include!(concat!(env!("OUT_DIR"), "/hello.rs"));
use pbrs_grpc::{Request, Response, Router, Status, Streaming};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

struct MyGreeter;

fn name_of(req: &HelloRequest) -> String {
    req.name().to_str().unwrap_or_default().to_owned()
}

fn reply(msg: impl Into<String>) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(msg.into());
    r
}

impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = name_of(request.get_ref());
        Ok(Response::new(reply(format!("hello {name}"))))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(req) = stream.message().await? {
            names.push(name_of(&req));
        }
        Ok(Response::new(reply(format!("hello {}", names.join(", ")))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = name_of(request.get_ref());
        let (tx, stream) = Streaming::channel(4);
        tokio::spawn(async move {
            for i in 1..=3 {
                if tx.send(reply(format!("hello {name} #{i}"))).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(4);
        tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                let name = name_of(&req);
                if tx.send(reply(format!("hello {name}"))).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(stream))
    }
}

#[tokio::main]
async fn main() -> Result<(), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(MyGreeter))
            .serve_with_shutdown(listener, async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    let mut client = None;
    for _ in 0..80 {
        if let Ok(c) = GreeterClient::connect(addr).await {
            client = Some(c);
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let client = client.ok_or_else(|| Status::unavailable("connect failed"))?;

    // 1. Unary
    let mut req1 = HelloRequest::new();
    req1.set_name("ada");
    let resp1 = client.say_hello(Request::new(req1)).await?;
    println!("[unary] {}", resp1.get_ref().message().to_str().unwrap_or_default());

    // 2. Client-streaming
    let (tx2, call2) = client.client_hello(Request::new(()));
    let mut req2a = HelloRequest::new();
    req2a.set_name("grace");
    tx2.send(req2a).await?;
    let mut req2b = HelloRequest::new();
    req2b.set_name("alan");
    tx2.send(req2b).await?;
    tx2.close();
    let resp2 = call2.await?;
    println!("[client_streaming] {}", resp2.get_ref().message().to_str().unwrap_or_default());

    // 3. Server-streaming
    let mut req3 = HelloRequest::new();
    req3.set_name("edsger");
    let mut stream3 = client.server_hello(Request::new(req3)).await?.into_inner();
    let mut msgs3 = Vec::new();
    while let Some(msg) = stream3.message().await? {
        msgs3.push(msg.message().to_str().unwrap_or_default().to_owned());
    }
    println!("[server_streaming] {}", msgs3.join(", "));

    // 4. Bidi-streaming
    let (tx4, call4) = client.stream_hello(Request::new(()));
    let mut stream4 = call4.await?.into_inner();
    let mut req4 = HelloRequest::new();
    req4.set_name("barbara");
    tx4.send(req4).await?;
    tx4.close();
    let mut msgs4 = Vec::new();
    while let Some(msg) = stream4.message().await? {
        msgs4.push(msg.message().to_str().unwrap_or_default().to_owned());
    }
    println!("[bidi_streaming] {}", msgs4.join(", "));

    // Clean server shutdown
    let _ = shutdown_tx.send(());
    let _ = server.await;
    println!("[shutdown] clean shutdown complete");

    Ok(())
}
"#;

const PACKAGED_GREETER_CONSUMER_MAIN: &str = r#"
use pbrs_grpc_example_greeter::{
    greeter, say_hello, say_hello_bidi_stream, say_hello_server_stream, say_hello_stream, serve,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let live = serve().await?;
    let client = greeter(live.addr).await?;

    let unary = say_hello(&client, "ada").await?;
    println!("[unary] {unary}");

    let upload = say_hello_stream(&client, ["grace", "alan"]).await?;
    println!("[client_streaming] {upload}");

    let download = say_hello_server_stream(&client, "edsger").await?;
    println!("[server_streaming] {}", download.join(", "));

    let bidi = say_hello_bidi_stream(&client, ["barbara"]).await?;
    println!("[bidi_streaming] {}", bidi.join(", "));

    live.shutdown().await;
    println!("[shutdown] clean shutdown complete");
    Ok(())
}
"#;

fn write_four_shapes_consumer(dir: &Path) {
    let root = repo_root();
    let proto_content = std::fs::read_to_string(root.join("examples/greeter/proto/hello.proto"))
        .expect("read greeter hello.proto");
    std::fs::write(dir.join("hello.proto"), proto_content).unwrap();
    std::fs::write(
        dir.join("build.rs"),
        r#"fn main() {
    pbrs::codegen::compile_protos(&["hello.proto"], &["."]).expect("compile_protos");
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-onboarding-four-shapes\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\npbrs-grpc = {{ path = \"{root}/pbrs-grpc\" }}\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\", \"time\", \"sync\"] }}\n[build-dependencies]\npbrs = {{ path = \"{root}\" }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), FOUR_SHAPES_MAIN).unwrap();
}

fn write_packaged_greeter_consumer(dir: &Path) {
    let root = repo_root();
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"pbrs-onboarding-packaged-greeter\"\nversion = \"0.0.1\"\nedition = \"2021\"\n[workspace]\n[dependencies]\npbrs = {{ path = \"{root}\" }}\npbrs-grpc = {{ path = \"{root}/pbrs-grpc\" }}\npbrs-grpc-example-greeter = {{ path = \"{root}/examples/greeter\" }}\ntokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\", \"time\", \"sync\"] }}\n",
            root = root.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.rs"), PACKAGED_GREETER_CONSUMER_MAIN).unwrap();
}

fn generate_hello(out: &Path, mut cfg: pbrs::codegen::Config) -> String {
    std::fs::create_dir_all(out).unwrap();
    let proto_dir = out.join("proto");
    std::fs::create_dir_all(&proto_dir).unwrap();
    std::fs::write(proto_dir.join("hello.proto"), HELLO_PROTO).unwrap();
    cfg.out_dir(out)
        .compile_protos(&[&proto_dir.join("hello.proto")], &[&proto_dir])
        .expect("compile_protos");
    std::fs::read_to_string(out.join("hello.rs")).expect("hello.rs")
}

#[test]
fn native_grpc_consumer_says_hello_ada() {
    let tmp = scratch("pbrs-onboarding-native");
    write_native_consumer(&tmp);
    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    assert_build_failed_without_protoc(&cargo_run(&tmp, Some(&no_protoc), false));
    assert_stdout_hello_ada(&cargo_run(&tmp, None, true));
}

#[test]
fn tonic_consumer_says_hello_ada() {
    let tmp = scratch("pbrs-onboarding-tonic");
    write_tonic_consumer(&tmp);
    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    assert_build_failed_without_protoc(&cargo_run(&tmp, Some(&no_protoc), false));
    assert_stdout_hello_ada(&cargo_run(&tmp, None, true));
}

#[test]
fn core_builds_without_protoc() {
    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    let target = scratch("pbrs-onboarding-core-noprotoc");
    let out = cargo_check_pkg("pbrs", &target, Some(&no_protoc));
    assert!(
        out.status.success(),
        "pbrs must build from bundled FDS without protoc:\n{}",
        dump(&out)
    );
    assert!(!env_protoc_runs(&no_protoc));
}

#[test]
fn adapters_require_protoc() {
    let no_protoc = path_without_protoc();
    assert_filtered_path(&no_protoc);
    for pkg in ["pbrs-grpc", "protobuf-tonic"] {
        let target = scratch(&format!("pbrs-onboarding-{pkg}-protoc"));
        let fail = cargo_check_pkg(pkg, &target, Some(&no_protoc));
        assert_build_failed_without_protoc(&fail);
        let ok = cargo_check_pkg(pkg, &target, None);
        assert!(
            ok.status.success(),
            "{pkg} must check when protoc is on PATH:\n{}",
            dump(&ok)
        );
    }
}

#[test]
fn codegen_stub_flavours_are_explicit() {
    let root = scratch("pbrs-onboarding-codegen");

    let kernel = generate_hello(&root.join("kernel"), pbrs::codegen::Config::new());
    assert!(
        kernel.contains("// --- gRPC stubs (pbrs-grpc kernel) ---"),
        "default compile_protos must emit kernel stubs"
    );
    assert!(kernel.contains("::pbrs_grpc"), "{kernel}");
    assert!(kernel.contains("Channel"), "{kernel}");
    assert!(
        !kernel.contains("protobuf_tonic::ProtobufCodec"),
        "default stubs must not be tonic"
    );

    let mut tonic_cfg = pbrs::codegen::Config::new();
    tonic_cfg.emit_tonic_stubs(true);
    let tonic = generate_hello(&root.join("tonic"), tonic_cfg);
    assert!(
        tonic.contains("protobuf_tonic::ProtobufCodec"),
        "emit_tonic_stubs(true) must use ProtobufCodec"
    );
    assert!(tonic.contains("tonic::client::Grpc"), "{tonic}");
    assert!(
        tonic.contains("// --- gRPC stubs (protobuf-tonic, not tonic-prost) ---"),
        "{tonic}"
    );
    assert!(
        !tonic.contains("// --- gRPC stubs (pbrs-grpc kernel) ---"),
        "tonic stubs must not include kernel stubs"
    );
    assert!(
        !tonic.contains("prost::Message"),
        "tonic stubs are not a prost::Message drop-in"
    );

    let mut none_cfg = pbrs::codegen::Config::new();
    none_cfg.emit_kernel_stubs(false);
    let none = generate_hello(&root.join("none"), none_cfg);
    assert!(none.contains("HelloRequest"), "{none}");
    assert!(
        !none.contains("protobuf_tonic::ProtobufCodec"),
        "messages-only must not emit tonic stubs"
    );
    assert!(
        !none.contains("// --- gRPC stubs (pbrs-grpc kernel) ---"),
        "messages-only must not emit kernel stubs"
    );
    assert!(!none.contains("tonic::client::Grpc"), "{none}");
}

#[test]
fn committed_hello_and_wkt_copies_match() {
    let root = repo_root();
    let groups: &[&[&str]] = &[
        &[
            "proto/hello.proto",
            "pbrs-grpc/proto/hello.proto",
            "protobuf-tonic/proto/hello.proto",
            "examples/greeter/proto/hello.proto",
        ],
        &[
            "proto/google/protobuf/any.proto",
            "pbrs-grpc/proto/google/protobuf/any.proto",
        ],
        &[
            "proto/google/protobuf/duration.proto",
            "pbrs-grpc/proto/google/protobuf/duration.proto",
        ],
        &[
            "proto/google/protobuf/empty.proto",
            "pbrs-grpc/proto/google/protobuf/empty.proto",
        ],
        &[
            "proto/google/protobuf/timestamp.proto",
            "pbrs-grpc/proto/google/protobuf/timestamp.proto",
        ],
        &[
            "proto/google/protobuf/wrappers.proto",
            "pbrs-grpc/proto/google/protobuf/wrappers.proto",
        ],
    ];
    for files in groups {
        let first = std::fs::read(root.join(files[0])).unwrap();
        for rel in files.iter().skip(1) {
            let other = std::fs::read(root.join(rel)).unwrap();
            assert_eq!(first, other, "{} drifted from {}", rel, files[0]);
        }
    }
}

#[test]
fn tonic_readme_selects_stubs_explicitly() {
    let readme = include_str!("../protobuf-tonic/README.md");
    assert!(
        readme.contains("emit_tonic_stubs(true)"),
        "tonic README must select tonic stubs; default compile_protos is kernel"
    );
    assert!(
        readme.contains("prost::Message"),
        "tonic README must say these types are not prost::Message"
    );
}

#[test]
fn native_grpc_consumer_exercises_all_four_rpc_shapes() {
    let tmp = scratch("pbrs-onboarding-four-shapes");
    write_four_shapes_consumer(&tmp);
    let out = cargo_run(&tmp, None, true);
    assert!(out.status.success(), "consumer failed:\n{}", dump(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[unary] hello ada"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[client_streaming] hello grace, alan"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[server_streaming] hello edsger #1, hello edsger #2, hello edsger #3"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[bidi_streaming] hello barbara"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[shutdown] clean shutdown complete"),
        "got stdout:\n{stdout}"
    );
}

#[test]
fn packaged_greeter_example_consumer_runs_all_four_shapes() {
    let tmp = scratch("pbrs-onboarding-packaged-greeter");
    write_packaged_greeter_consumer(&tmp);
    let out = cargo_run(&tmp, None, true);
    assert!(
        out.status.success(),
        "packaged consumer failed:\n{}",
        dump(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[unary] hello ada"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[client_streaming] hello grace, alan"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[server_streaming] hello edsger #1, hello edsger #2, hello edsger #3"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[bidi_streaming] hello barbara"),
        "got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[shutdown] clean shutdown complete"),
        "got stdout:\n{stdout}"
    );
}

#[test]
fn greeter_example_binary_runs_and_prints_hello_world() {
    let greeter_dir = repo_root().join("examples").join("greeter");
    let out = cargo_run(&greeter_dir, None, true);
    assert!(
        out.status.success(),
        "greeter binary failed:\n{}",
        dump(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), "hello world", "got stdout:\n{stdout}");
}
