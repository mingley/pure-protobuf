//! Integration tests for generated-code and runtime evolution (CG-11).
//!
//! Validates schema evolution across versioned proto definitions (v1 and v2):
//! - Forward compatibility: v1 code reading v2 wire data preserves unknown fields.
//! - Backward compatibility: v2 code reading v1 wire data populates default values and handles field absence.
//! - Presence semantics, mutation, and serialized size cache invalidation.
//! - Client and server stubs across versions for all four call shapes (Unary, Client-streaming, Server-streaming, Bidi-streaming).
//! - Incompatible generated/runtime pairs fail clearly.
//! - Crate identity (pbrs) and application-level v4 compatibility are distinct from Google internal C/upb ABI.

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
    reason = "integration tests are sync; generated fixtures live in test crate"
)]

use pbrs::codegen::{CodegenError, Config, Stubs};
use pbrs::{Clear, Message, Parse, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Keep frozen generated consumers as evidence of their original output.
#[rustfmt::skip]
#[path = "fixtures/codegen-compat/v1_generated.rs"]
mod v1;

#[rustfmt::skip]
#[path = "fixtures/codegen-compat/v2_generated.rs"]
mod v2;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = repo_root().join("target").join(format!(
        "pbrs-compat-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn apply_cargo_home(cmd: &mut Command) {
    if let Some(h) = std::env::var_os("CARGO_HOME") {
        cmd.env("CARGO_HOME", h);
    }
}

fn dump(out: &Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn cargo_run_consumer(dir: &Path) {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--offline", "--quiet"]);
    cmd.current_dir(dir).env("CARGO_TERM_COLOR", "never");
    apply_cargo_home(&mut cmd);
    let out = cmd.output().expect("cargo run consumer");
    assert!(
        out.status.success(),
        "isolated consumer in {} failed:\n{}",
        dir.display(),
        dump(&out)
    );
}

// ---------------------------------------------------------------------------
// 1. Forward Compatibility: v1 code reading v2 wire data preserves unknown fields
// ---------------------------------------------------------------------------

#[test]
fn test_forward_compatibility_preserves_unknown_fields() {
    let mut msg2 = v2::CompatMessage::new();
    msg2.set_id(42);
    msg2.set_name("alpha-v2");
    msg2.set_description("explicit-presence-desc");
    msg2.set_priority(10);
    msg2.tags_mut().push("tag1");
    msg2.tags_mut().push("tag2");
    msg2.scores_mut().push(100);
    msg2.scores_mut().push(200);
    msg2.properties_mut().insert("env", "prod");
    msg2.properties_mut().insert("tier", "frontend");
    // Enum variant added in v2: STATUS_SUSPENDED (value 3)
    msg2.set_status(v2::Status::Suspended);
    msg2.set_text_payload("payload-data");

    // Evolved fields in v2 that do not exist in v1 schema:
    msg2.set_extra_info("extra-v2-metadata");
    msg2.set_timestamp(1_726_700_000);
    msg2.categories_mut().push("security");
    msg2.categories_mut().push("observability");
    msg2.flags_mut().insert("rate_limit", 500);

    let v2_bytes = msg2.serialize().expect("serialize v2 message");

    // Deserialize into v1 consumer:
    let msg1 = <v1::CompatMessage as Parse>::parse(&v2_bytes).expect("parse as v1");

    // 1. All shared fields are parsed accurately by v1:
    assert_eq!(msg1.id(), 42);
    assert_eq!(msg1.name(), "alpha-v2");
    assert!(msg1.has_description());
    assert_eq!(msg1.description(), "explicit-presence-desc");
    assert!(msg1.has_priority());
    assert_eq!(msg1.priority(), 10);
    assert_eq!(
        msg1.tags()
            .iter()
            .map(|t| t.to_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["tag1", "tag2"]
    );
    assert_eq!(msg1.scores().iter().collect::<Vec<_>>(), vec![100, 200]);
    assert!(msg1.properties().get("env").is_some_and(|s| s == "prod"));
    assert!(
        msg1.properties()
            .get("tier")
            .is_some_and(|s| s == "frontend")
    );
    assert_eq!(msg1.status().0, 3); // Raw value preserved in open proto3 enum
    assert_eq!(msg1.text_payload(), "payload-data");

    // 2. Unknown fields (tags 12, 13, 14, 15) must be captured in v1:
    let v1_reserialized = msg1.serialize().expect("reserialize v1 message");

    // 3. Round-trip back to v2: All v2 fields must be recovered completely intact
    let msg2_recovered =
        <v2::CompatMessage as Parse>::parse(&v1_reserialized).expect("parse recovered v2");
    assert_eq!(msg2, msg2_recovered);
    assert_eq!(msg2_recovered.extra_info(), "extra-v2-metadata");
    assert!(msg2_recovered.has_timestamp());
    assert_eq!(msg2_recovered.timestamp(), 1_726_700_000);
    assert_eq!(
        msg2_recovered
            .categories()
            .iter()
            .map(|c| c.to_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["security", "observability"]
    );
    assert_eq!(msg2_recovered.flags().get("rate_limit"), Some(500));
}

// ---------------------------------------------------------------------------
// 2. Backward Compatibility: v2 code reading v1 wire data populates defaults
// ---------------------------------------------------------------------------

#[test]
fn test_backward_compatibility_defaults_and_absence() {
    let mut msg1 = v1::CompatMessage::new();
    msg1.set_id(99);
    msg1.set_name("v1-source");
    // Leave optional fields description and priority absent
    msg1.tags_mut().push("legacy-tag");
    msg1.set_status(v1::Status::Active);
    msg1.set_number_payload(777);
    // Legacy field present in v1, retired and reserved in v2:
    msg1.set_legacy_field(12345);

    let v1_bytes = msg1.serialize().expect("serialize v1");

    // Deserialize into v2 consumer:
    let msg2 = <v2::CompatMessage as Parse>::parse(&v1_bytes).expect("parse as v2");

    // 1. Shared fields match:
    assert_eq!(msg2.id(), 99);
    assert_eq!(msg2.name(), "v1-source");
    assert_eq!(
        msg2.tags()
            .iter()
            .map(|t| t.to_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        vec!["legacy-tag"]
    );
    assert_eq!(msg2.status(), v2::Status::Active);
    assert_eq!(msg2.number_payload(), 777);

    // 2. Absent fields in v1 correctly report absence and defaults in v2:
    assert!(!msg2.has_description());
    assert_eq!(msg2.description(), "");
    assert!(!msg2.has_priority());
    assert_eq!(msg2.priority(), 0);
    assert!(!msg2.has_timestamp());
    assert_eq!(msg2.timestamp(), 0);
    assert_eq!(msg2.extra_info(), "");
    assert!(msg2.categories().is_empty());
    assert!(msg2.flags().is_empty());
    assert!(!msg2.boolean_payload());

    // 3. Tag 11 (retired in v2) is preserved as an unknown field and survives round-trip back to v1:
    let v2_bytes = msg2.serialize().expect("serialize v2");
    let msg1_recovered =
        <v1::CompatMessage as Parse>::parse(&v2_bytes).expect("parse recovered v1");
    assert_eq!(msg1_recovered.legacy_field(), 12345);
    assert_eq!(msg1, msg1_recovered);
}

// ---------------------------------------------------------------------------
// 3. Enum Evolution across Versions
// ---------------------------------------------------------------------------

#[test]
fn test_enum_evolution_open_proto3_semantics() {
    // v2 uses new enum variant: STATUS_ARCHIVED (4)
    let mut msg2 = v2::CompatMessage::new();
    msg2.set_status(v2::Status::Archived);
    let bytes = msg2.serialize().expect("serialize");

    let msg1 = <v1::CompatMessage as Parse>::parse(&bytes).expect("parse v1");
    assert_eq!(msg1.status().0, 4);

    let reserialized = msg1.serialize().expect("serialize v1");
    let msg2_back = <v2::CompatMessage as Parse>::parse(&reserialized).expect("parse v2");
    assert_eq!(msg2_back.status(), v2::Status::Archived);
    assert_eq!(msg2_back.status().0, 4);
}

// ---------------------------------------------------------------------------
// 4. Oneof Evolution across Versions
// ---------------------------------------------------------------------------

#[test]
fn test_oneof_evolution_across_versions() {
    // v2 sets the newly added boolean_payload alternative (tag 16)
    let mut msg2 = v2::CompatMessage::new();
    msg2.set_boolean_payload(true);
    let bytes = msg2.serialize().expect("serialize v2");

    // v1 parses message: neither text_payload nor number_payload is set
    let msg1 = <v1::CompatMessage as Parse>::parse(&bytes).expect("parse v1");
    assert_eq!(msg1.text_payload(), "");
    assert_eq!(msg1.number_payload(), 0);

    // When re-serialized from v1, tag 16 is preserved in unknown fields
    let v1_bytes = msg1.serialize().expect("serialize v1");
    let msg2_back = <v2::CompatMessage as Parse>::parse(&v1_bytes).expect("parse v2");
    assert!(msg2_back.boolean_payload());
}

// ---------------------------------------------------------------------------
// 5. Presence Semantics across Versions
// ---------------------------------------------------------------------------

#[test]
fn test_presence_semantics_across_versions() {
    let mut msg = v1::CompatMessage::new();
    assert!(!msg.has_description());
    assert!(!msg.has_priority());

    msg.set_description("present-desc");
    assert!(msg.has_description());
    assert_eq!(msg.description(), "present-desc");

    msg.set_priority(5);
    assert!(msg.has_priority());
    assert_eq!(msg.priority(), 5);

    msg.clear_description();
    assert!(!msg.has_description());
    assert_eq!(msg.description(), "");

    msg.clear_priority();
    assert!(!msg.has_priority());
    assert_eq!(msg.priority(), 0);

    // Oneof presence: setting one variant clears the other
    msg.set_text_payload("hello");
    assert_eq!(msg.text_payload(), "hello");
    assert_eq!(msg.number_payload(), 0);

    msg.set_number_payload(42);
    assert_eq!(msg.text_payload(), "");
    assert_eq!(msg.number_payload(), 42);
}

// ---------------------------------------------------------------------------
// 6. Mutation and Serialized Size Cache Invalidation
// ---------------------------------------------------------------------------

#[test]
fn test_mutation_and_cache_invalidation() {
    let mut msg = v1::CompatMessage::new();
    assert_eq!(msg.compute_size(), 0);

    // Scalar mutation invalidates cached size:
    msg.set_id(1);
    let s1 = msg.compute_size();
    assert!(s1 > 0);

    // Calling compute_size again returns cached size:
    assert_eq!(msg.compute_size(), s1);

    // Mutating with a larger value invalidates and recomputes:
    msg.set_name("lengthy-string-payload-exceeding-small-threshold");
    let s2 = msg.compute_size();
    assert!(s2 > s1);

    // Repeated field mutation invalidates:
    msg.tags_mut().push("item-1");
    let s3 = msg.compute_size();
    assert!(s3 > s2);

    // Map field mutation invalidates:
    msg.properties_mut().insert("k", "v");
    let s4 = msg.compute_size();
    assert!(s4 > s3);

    // Clear resets cached size to 0:
    msg.clear();
    assert_eq!(msg.compute_size(), 0);
    let empty_bytes = msg.serialize().expect("serialize empty");
    assert!(empty_bytes.is_empty());
}

// ---------------------------------------------------------------------------
// 7. Codegen Stub Signatures for All Four Call Shapes
// ---------------------------------------------------------------------------

#[test]
fn test_codegen_stub_signatures_all_four_call_shapes() {
    let out_v1 = scratch_dir("stub-v1");
    let out_v2 = scratch_dir("stub-v2");

    let proto_v1 = repo_root().join("tests/fixtures/codegen-compat/v1/compat.proto");
    let proto_v2 = repo_root().join("tests/fixtures/codegen-compat/v2/compat.proto");

    // 1. Generate Kernel stubs for v1
    Config::new()
        .out_dir(&out_v1)
        .stubs(Stubs::Kernel)
        .compile_protos(&[&proto_v1], &[proto_v1.parent().unwrap()])
        .expect("compile v1 kernel stubs");

    let v1_src = std::fs::read_to_string(out_v1.join("compat.rs")).expect("read v1 compat.rs");

    // Verify all 4 call shape signatures in the generated trait:
    assert!(
        v1_src.contains("pub trait CompatService: Send + Sync + 'static"),
        "missing CompatService trait"
    );
    // Unary:
    assert!(v1_src.contains("fn unary_call"), "missing unary_call in v1");
    assert!(
        v1_src.contains("request: ::pbrs_grpc::Request<CompatRequest>"),
        "missing unary request signature"
    );
    assert!(
        v1_src.contains("Result<::pbrs_grpc::Response<CompatResponse>, ::pbrs_grpc::Status>"),
        "missing unary response signature"
    );
    // Client streaming:
    assert!(
        v1_src.contains("fn client_stream_call"),
        "missing client_stream_call in v1"
    );
    assert!(
        v1_src.contains("request: ::pbrs_grpc::Request<::pbrs_grpc::Streaming<CompatRequest>>"),
        "missing client streaming request signature"
    );
    // Server streaming:
    assert!(
        v1_src.contains("fn server_stream_call"),
        "missing server_stream_call in v1"
    );
    assert!(
        v1_src.contains(
            "Result<::pbrs_grpc::Response<::pbrs_grpc::Streaming<CompatResponse>>, ::pbrs_grpc::Status>"
        ),
        "missing server streaming response signature"
    );
    // Bidi streaming:
    assert!(
        v1_src.contains("fn bidi_stream_call"),
        "missing bidi_stream_call in v1"
    );

    // Verify Client and Server types exist:
    assert!(
        v1_src.contains("pub struct CompatServiceClient"),
        "missing client struct"
    );
    assert!(
        v1_src.contains("pub struct CompatServiceServer<T>"),
        "missing server struct"
    );

    // 2. Generate Kernel stubs for v2
    Config::new()
        .out_dir(&out_v2)
        .stubs(Stubs::Kernel)
        .compile_protos(&[&proto_v2], &[proto_v2.parent().unwrap()])
        .expect("compile v2 kernel stubs");

    let v2_src = std::fs::read_to_string(out_v2.join("compat.rs")).expect("read v2 compat.rs");

    // Verify existing 4 call shapes preserved + new methods added:
    assert!(v2_src.contains("fn unary_call"));
    assert!(v2_src.contains("fn client_stream_call"));
    assert!(v2_src.contains("fn server_stream_call"));
    assert!(v2_src.contains("fn bidi_stream_call"));
    assert!(
        v2_src.contains("fn new_unary_call"),
        "v2 must contain added new_unary_call"
    );
    assert!(
        v2_src.contains("fn new_server_stream_call"),
        "v2 must contain added new_server_stream_call"
    );

    // 3. Generate Tonic stubs:
    let out_tonic = scratch_dir("stub-tonic");
    Config::new()
        .out_dir(&out_tonic)
        .stubs(Stubs::Tonic)
        .compile_protos(&[&proto_v1], &[proto_v1.parent().unwrap()])
        .expect("compile tonic stubs");

    let tonic_src =
        std::fs::read_to_string(out_tonic.join("compat.rs")).expect("read tonic compat.rs");
    assert!(
        tonic_src.contains("use protobuf_tonic::ProtobufCodec;"),
        "tonic stubs must use ProtobufCodec"
    );

    // 4. Generate with Stubs::None:
    let out_none = scratch_dir("stub-none");
    Config::new()
        .out_dir(&out_none)
        .stubs(Stubs::None)
        .compile_protos(&[&proto_v1], &[proto_v1.parent().unwrap()])
        .expect("compile without stubs");

    let none_src =
        std::fs::read_to_string(out_none.join("compat.rs")).expect("read none compat.rs");
    assert!(
        !none_src.contains("pub trait CompatService"),
        "stubs=none must omit service stubs"
    );
    assert!(
        !none_src.contains("CompatServiceClient"),
        "stubs=none must omit client struct"
    );
}

// ---------------------------------------------------------------------------
// 8. Live Cross-Version Client and Server Stubs (All 4 Call Shapes)
// ---------------------------------------------------------------------------

#[test]
fn test_live_cross_version_stubs_and_unimplemented_behavior() {
    let consumer_dir = scratch_dir("grpc-cross-compat");
    let src_dir = consumer_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let root = repo_root();
    let pbrs_path = root.display().to_string();
    let grpc_path = root.join("pbrs-grpc").display().to_string();
    let v1_stubs = root
        .join("tests/fixtures/codegen-compat/v1_with_stubs.rs")
        .display()
        .to_string();
    let v2_stubs = root
        .join("tests/fixtures/codegen-compat/v2_with_stubs.rs")
        .display()
        .to_string();

    std::fs::write(
        consumer_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "grpc-cross-compat-consumer"
version = "0.0.1"
edition = "2021"

[workspace]

[dependencies]
pbrs = {{ path = "{pbrs_path}" }}
pbrs-grpc = {{ path = "{grpc_path}" }}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros", "net", "time"] }}
"#
        ),
    )
    .unwrap();

    std::fs::write(
        src_dir.join("main.rs"),
        format!(
            r#"#[path = "{v1_stubs}"]
mod v1;

#[path = "{v2_stubs}"]
mod v2;

use pbrs_grpc::{{Request, Response, Status}};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

struct V2Server;

impl v2::CompatService for V2Server {{
    async fn unary_call(
        &self,
        request: Request<v2::CompatRequest>,
    ) -> Result<Response<v2::CompatResponse>, Status> {{
        let q = request.get_ref().query().to_str().unwrap_or_default();
        let mut resp = v2::CompatResponse::new();
        resp.set_result(format!("echo:{{q}}"));
        resp.set_code(200);
        resp.details_mut().push("v2-extra-detail");
        Ok(Response::new(resp))
    }}

    async fn client_stream_call(
        &self,
        request: Request<pbrs_grpc::Streaming<v2::CompatRequest>>,
    ) -> Result<Response<v2::CompatResponse>, Status> {{
        let mut stream = request.into_inner();
        let mut count = 0;
        let mut queries = Vec::new();
        while let Some(msg) = stream.message().await? {{
            count += 1;
            queries.push(msg.query().to_str().unwrap_or_default().to_string());
        }}
        let mut resp = v2::CompatResponse::new();
        resp.set_result(format!("collected:{{count}}:{{}}", queries.join(",")));
        resp.set_code(200);
        Ok(Response::new(resp))
    }}

    async fn server_stream_call(
        &self,
        request: Request<v2::CompatRequest>,
    ) -> Result<Response<pbrs_grpc::Streaming<v2::CompatResponse>>, Status> {{
        let q = request.get_ref().query().to_str().unwrap_or_default().to_string();
        let (tx, rx) = pbrs_grpc::Streaming::channel(4);
        tokio::spawn(async move {{
            for i in 1..=3 {{
                let mut resp = v2::CompatResponse::new();
                resp.set_result(format!("{{q}}-{{i}}"));
                resp.set_code(i);
                if tx.send(resp).await.is_err() {{
                    break;
                }}
            }}
        }});
        Ok(Response::new(rx))
    }}

    async fn bidi_stream_call(
        &self,
        request: Request<pbrs_grpc::Streaming<v2::CompatRequest>>,
    ) -> Result<Response<pbrs_grpc::Streaming<v2::CompatResponse>>, Status> {{
        let mut in_stream = request.into_inner();
        let (tx, rx) = pbrs_grpc::Streaming::channel(4);
        tokio::spawn(async move {{
            while let Ok(Some(msg)) = in_stream.message().await {{
                let q = msg.query().to_str().unwrap_or_default().to_string();
                let mut resp = v2::CompatResponse::new();
                resp.set_result(format!("bidi:{{q}}"));
                resp.set_code(1);
                if tx.send(resp).await.is_err() {{
                    break;
                }}
            }}
        }});
        Ok(Response::new(rx))
    }}
}}

struct V1Server;
impl v1::CompatService for V1Server {{}}

#[tokio::main]
async fn main() -> Result<(), Status> {{
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_task = tokio::spawn(async move {{
        pbrs_grpc::Router::new()
            .add_service(v2::CompatServiceServer::new(V2Server))
            .serve_with_shutdown(listener, async {{
                let _ = shutdown_rx.await;
            }})
            .await
            .ok();
    }});

    let mut client = None;
    for _ in 0..80 {{
        if let Ok(c) = v1::CompatServiceClient::connect(addr).await {{
            client = Some(c);
            break;
        }}
        tokio::time::sleep(Duration::from_millis(5)).await;
    }}
    let client = client.ok_or_else(|| Status::unavailable("connect failed"))?;

    // 1. Unary Call
    let mut req1 = v1::CompatRequest::new();
    req1.set_query("hello");
    let resp1 = client.unary_call(Request::new(req1)).await?;
    assert_eq!(resp1.get_ref().result().to_str().unwrap(), "echo:hello");
    assert_eq!(resp1.get_ref().code(), 200);

    // 2. Client Streaming Call
    let (tx2, call2) = client.client_stream_call(Request::new(()));
    let mut req2a = v1::CompatRequest::new();
    req2a.set_query("alpha");
    tx2.send(req2a).await?;
    let mut req2b = v1::CompatRequest::new();
    req2b.set_query("beta");
    tx2.send(req2b).await?;
    tx2.close();
    let resp2 = call2.await?;
    assert_eq!(resp2.get_ref().result().to_str().unwrap(), "collected:2:alpha,beta");

    // 3. Server Streaming Call
    let mut req3 = v1::CompatRequest::new();
    req3.set_query("chunk");
    let mut stream3 = client.server_stream_call(Request::new(req3)).await?.into_inner();
    let mut items = Vec::new();
    while let Some(msg) = stream3.message().await? {{
        items.push(msg.result().to_str().unwrap().to_string());
    }}
    assert_eq!(items, vec!["chunk-1", "chunk-2", "chunk-3"]);

    // 4. Bidirectional Streaming Call
    let (tx4, call4) = client.bidi_stream_call(Request::new(()));
    let mut stream4 = call4.await?.into_inner();
    let mut req4 = v1::CompatRequest::new();
    req4.set_query("msg1");
    tx4.send(req4).await?;
    let bidi_resp = stream4.message().await?.expect("bidi response");
    assert_eq!(bidi_resp.result().to_str().unwrap(), "bidi:msg1");
    tx4.close();

    // Clean shutdown of v2 server
    let _ = shutdown_tx.send(());
    let _ = server_task.await;

    // 5. Test that an incompatible / unimplemented RPC fails clearly:
    let listener_v1 = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr_v1 = listener_v1.local_addr()?;
    let (shutdown_v1_tx, shutdown_v1_rx) = tokio::sync::oneshot::channel::<()>();
    let server_v1_task = tokio::spawn(async move {{
        pbrs_grpc::Router::new()
            .add_service(v1::CompatServiceServer::new(V1Server))
            .serve_with_shutdown(listener_v1, async {{
                let _ = shutdown_v1_rx.await;
            }})
            .await
            .ok();
    }});

    let mut client_v2 = None;
    for _ in 0..80 {{
        if let Ok(c) = v2::CompatServiceClient::connect(addr_v1).await {{
            client_v2 = Some(c);
            break;
        }}
        tokio::time::sleep(Duration::from_millis(5)).await;
    }}
    let client_v2 = client_v2.ok_or_else(|| Status::unavailable("connect failed"))?;
    let mut req_new = v2::CompatRequest::new();
    req_new.set_query("calling-new-method-on-v1-server");
    let err = client_v2.new_unary_call(Request::new(req_new)).await.unwrap_err();
    assert_eq!(err.code(), pbrs_grpc::Code::Unimplemented);

    let _ = shutdown_v1_tx.send(());
    let _ = server_v1_task.await;

    println!("all cross-version gRPC assertions passed");
    Ok(())
}}
"#
        ),
    )
    .unwrap();

    cargo_run_consumer(&consumer_dir);
}

// ---------------------------------------------------------------------------
// 9. Incompatible Generated / Runtime Pairs Fail Clearly
// ---------------------------------------------------------------------------

#[test]
fn test_incompatible_generated_runtime_pairs_fail_clearly() {
    let out_dir = scratch_dir("incompat");
    let proto = repo_root().join("tests/fixtures/codegen-compat/v1/compat.proto");

    // Invalid plugin parameters fail with specific CodegenError:
    let res1 = Config::new()
        .out_dir(&out_dir)
        .extern_path("invalid", "")
        .compile_protos(&[&proto], &[proto.parent().unwrap()]);
    assert!(
        matches!(res1, Err(CodegenError::InvalidParameter { .. })),
        "expected InvalidParameter, got: {res1:?}"
    );

    // Missing input proto fails with execution error:
    let res_missing = Config::new()
        .out_dir(&out_dir)
        .compile_protos(&["nonexistent_schema.proto"], &[proto.parent().unwrap()]);
    assert!(res_missing.is_err(), "missing proto must fail compilation");

    // Malformed descriptors fail cleanly:
    let bad_fds = [0xFF, 0xFF, 0xFF, 0xFF];
    let res2 = pbrs::codegen::generate_from_file_descriptor_set(&bad_fds, &["test.proto".into()]);
    assert!(
        matches!(res2, Err(CodegenError::MalformedDescriptor { .. })),
        "expected MalformedDescriptor, got: {res2:?}"
    );

    // Malformed / truncated wire data fails cleanly with ParseError:
    let truncated_varint = [0x08, 0x80, 0x80]; // Incomplete varint tag 1
    let res3 = <v1::CompatMessage as Parse>::parse(&truncated_varint);
    assert!(res3.is_err(), "truncated varint must fail parse");

    let truncated_len = [0x12, 0x10, 0x61, 0x62]; // Tag 2 LEN specifies 16 bytes, but only 2 provided
    let res4 = <v1::CompatMessage as Parse>::parse(&truncated_len);
    assert!(res4.is_err(), "truncated length delimited must fail parse");
}

// ---------------------------------------------------------------------------
// 10. Crate Identity and Application-Level v4 Compatibility vs Universal C ABI
// ---------------------------------------------------------------------------

#[test]
fn test_crate_names_and_application_level_v4_vs_universal_c_abi() {
    // pure-protobuf is published as `pbrs`, not crates.io `protobuf` 4.x (upb/C FFI).
    // The runtime provides application-level v4 compatibility (Parse, Serialize, Clear, proto! macro),
    // but explicitly does not provide C arena memory layouts or upb C FFI pointers.
    let msg = v1::CompatMessage::new();

    // Application traits are implemented:
    fn assert_app_traits<
        T: Message + Parse + Serialize + Clear + Default + Clone + std::fmt::Debug,
    >() {
    }
    assert_app_traits::<v1::CompatMessage>();
    assert_app_traits::<v2::CompatMessage>();

    // gencode version assertion accepts v4 releases without error:
    pbrs::__internal::assert_compatible_gencode_version("4.35.1-release");
    pbrs::__internal::assert_compatible_gencode_version("4.29.3");

    // Pure-Rust messages are safe and standard Rust structs:
    assert!(std::mem::size_of_val(&msg) > 0);
    assert_eq!(v1::CompatMessage::FULL_NAME, "compat.CompatMessage");
}
