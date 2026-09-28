//! gRFC A43 authorization policies enforced on the server path.
//!
//! Allowed calls reach the handler; denied calls fail with
//! `PERMISSION_DENIED` without handler execution, on every call shape.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    unreachable_pub,
    reason = "integration tests"
)]

mod common;

use common::{ServerGuard, greeter_client, reply, req};
use pbrs_grpc::authz::{
    AuditEvent, AuditLogger, AuditLoggerFactory, FileWatcherProvider, PolicyError,
    StaticDataProvider, format_record, register_audit_logger_factory,
};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{
    ClientTls, Code, Identity, Request, Response, Router, Server, ServerTls, Status, Streaming,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("tls_data/server.crt");
const SERVER_KEY: &str = include_str!("tls_data/server.key");
const CLIENT_CERT: &str = include_str!("tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("tls_data/client.key");

fn server_identity() -> Identity {
    Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity")
}

fn client_identity() -> Identity {
    Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity")
}

/// Echo Greeter that counts handler executions per method.
#[derive(Clone)]
struct Counting {
    unary: Arc<AtomicUsize>,
    server_stream: Arc<AtomicUsize>,
}

impl Counting {
    fn new() -> Self {
        Self {
            unary: Arc::new(AtomicUsize::new(0)),
            server_stream: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Greeter for Counting {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        self.unary.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(reply(request.get_ref().name().to_string())))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        self.server_stream.fetch_add(1, Ordering::SeqCst);
        let (tx, stream) = Streaming::channel(4);
        let name = request.get_ref().name().to_string();
        drop(tokio::spawn(async move {
            let _ = tx.send(reply(name)).await;
        }));
        Ok(Response::new(stream))
    }
}

async fn bind() -> (SocketAddr, TcpListener) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

fn permission_denied(status: &Status) {
    assert_eq!(
        status.code(),
        Code::PermissionDenied,
        "expected PERMISSION_DENIED, got {status:?}"
    );
}

#[tokio::test]
async fn allows_one_path_and_denies_the_rest_without_handler() {
    let service = Counting::new();
    let unary = Arc::clone(&service.unary);
    let streams = Arc::clone(&service.server_stream);
    let provider = StaticDataProvider::new(
        r#"{"name":"paths","allow_rules":[
            {"name":"unary-only","request":{"paths":["/helloworld.Greeter/SayHello"]}}
        ]}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(service))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;

    let response = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("allowed unary");
    assert_eq!(response.get_ref().message(), "ada");
    assert_eq!(unary.load(Ordering::SeqCst), 1);

    let denied = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("unlisted path is denied");
    permission_denied(&denied);
    assert!(
        denied.message().contains("no allow rule matched"),
        "unexpected message: {denied:?}"
    );
    assert_eq!(streams.load(Ordering::SeqCst), 0, "handler must not run");
}

#[tokio::test]
async fn deny_rule_beats_allow() {
    let service = Counting::new();
    let streams = Arc::clone(&service.server_stream);
    let provider = StaticDataProvider::new(
        r#"{"name":"deny-wins","allow_rules":[
            {"name":"all","request":{"paths":["/helloworld.Greeter/*"]}}
        ],"deny_rules":[
            {"name":"no-streams","request":{"paths":["*/ServerHello"]}}
        ]}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(service))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;

    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary allowed");
    let denied = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("deny rule wins");
    permission_denied(&denied);
    assert!(
        denied.message().contains("no-streams"),
        "unexpected message: {denied:?}"
    );
    assert_eq!(streams.load(Ordering::SeqCst), 0, "handler must not run");
}

#[tokio::test]
async fn header_gated_rule() {
    let service = Counting::new();
    let unary = Arc::clone(&service.unary);
    let provider = StaticDataProvider::new(
        r#"{"name":"tenant","allow_rules":[
            {"name":"tenant-only","request":{
                "paths":["/helloworld.Greeter/SayHello"],
                "headers":[{"key":"x-tenant","values":["acme*"]}]
            }}
        ]}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(service))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;

    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("missing header is denied");
    permission_denied(&denied);
    assert_eq!(unary.load(Ordering::SeqCst), 0);

    let mut request = Request::new(req("ada"));
    request
        .metadata_mut()
        .insert("x-tenant", "acme-1")
        .expect("header");
    client.say_hello(request).await.expect("header allowed");
    assert_eq!(unary.load(Ordering::SeqCst), 1);

    let mut request = Request::new(req("ada"));
    request
        .metadata_mut()
        .insert("x-tenant", "globex")
        .expect("header");
    let denied = client
        .say_hello(request)
        .await
        .expect_err("wrong tenant is denied");
    permission_denied(&denied);
    assert_eq!(unary.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn router_enforces_on_every_mount() {
    let provider =
        StaticDataProvider::new(r#"{"name":"closed","allow_rules":[]}"#).expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(Counting::new()))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("empty allow list denies everything");
    permission_denied(&denied);
}

#[test]
fn invalid_policies_are_rejected() {
    assert!(StaticDataProvider::new(r#"{"name":"x"}"#).is_err());
    assert!(StaticDataProvider::new(r#"{"name":"x","allow_rules":[],"bogus":1}"#).is_err());
    assert!(StaticDataProvider::new("not json").is_err());
}

#[tokio::test]
async fn file_watcher_reloads_and_keeps_last_valid() {
    let path = std::env::temp_dir().join(format!("pbrs-authz-{}.json", std::process::id()));
    std::fs::write(&path, r#"{"name":"open","allow_rules":[{"name":"all"}]}"#).expect("write");
    let provider =
        FileWatcherProvider::new(&path, Duration::from_millis(10)).expect("watcher starts");
    assert!(provider.last_error().is_none());

    let (addr, listener) = bind().await;
    let worker = provider.clone();
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Counting::new()))
            .authorization_policy(worker)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client: GreeterClient = greeter_client(addr).await;
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("open policy allows");

    // A broken reload keeps the last valid policy and records the error.
    std::fs::write(&path, r#"{"name":"broken""#).expect("write");
    assert!(!provider.refresh(), "broken reload changes nothing");
    assert!(provider.last_error().is_some());
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("still open after broken reload");

    // A valid reload takes effect on the next call.
    std::fs::write(&path, r#"{"name":"closed","allow_rules":[]}"#).expect("write");
    assert!(provider.refresh(), "valid reload applies");
    assert!(provider.last_error().is_none());
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("closed policy denies");
    permission_denied(&denied);

    // The background poller picks up a later change without refresh().
    std::fs::write(&path, r#"{"name":"open","allow_rules":[{"name":"all"}]}"#).expect("write");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if client.say_hello(Request::new(req("ada"))).await.is_ok() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "poller never picked up the reopened policy"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn mtls_principal_match() {
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let provider = StaticDataProvider::new(
        r#"{"name":"spiffe","allow_rules":[
            {"name":"client-cert","source":{"principals":["pbrs-grpc-test-client"]},
             "request":{"paths":["/helloworld.Greeter/SayHello"]}}
        ]}"#,
    )
    .expect("policy");
    let service = Counting::new();
    let unary = Arc::clone(&service.unary);
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(service))
            .authorization_policy(provider)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client(addr, client_tls).await;
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("DNS SAN principal allowed");
    assert_eq!(unary.load(Ordering::SeqCst), 1);

    // A policy naming a different principal denies the same peer.
    let tls = ServerTls::mtls(server_identity(), CA).expect("mtls server");
    let provider = StaticDataProvider::new(
        r#"{"name":"spiffe","allow_rules":[
            {"name":"stranger","source":{"principals":["someone-else"]},
             "request":{"paths":["/helloworld.Greeter/SayHello"]}}
        ]}"#,
    )
    .expect("policy");
    let service = Counting::new();
    let unary = Arc::clone(&service.unary);
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(service))
            .authorization_policy(provider)
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client_tls = ClientTls::ca_mtls("localhost", CA, client_identity()).expect("mtls client");
    let client = tls_client(addr, client_tls).await;
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("wrong principal is denied");
    permission_denied(&denied);
    assert_eq!(unary.load(Ordering::SeqCst), 0, "handler must not run");
}

/// Channel-backed test logger: `log` never blocks, tests drain events.
struct Recorder {
    name: &'static str,
    tx: std::sync::mpsc::Sender<AuditEvent>,
}

impl AuditLogger for Recorder {
    fn name(&self) -> &str {
        self.name
    }

    fn log(&self, event: &AuditEvent) {
        self.tx.send(event.clone()).ok();
    }
}

struct RecorderFactory {
    name: &'static str,
    tx: std::sync::mpsc::Sender<AuditEvent>,
}

impl AuditLoggerFactory for RecorderFactory {
    fn name(&self) -> &str {
        self.name
    }

    fn build(&self, _config: &serde_json::Value) -> Result<Box<dyn AuditLogger>, PolicyError> {
        Ok(Box::new(Recorder {
            name: self.name,
            tx: self.tx.clone(),
        }))
    }
}

fn drain(rx: &std::sync::mpsc::Receiver<AuditEvent>) -> Vec<AuditEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn audit_logs_allow_and_deny_with_context() {
    let (tx, rx) = std::sync::mpsc::channel();
    register_audit_logger_factory(Arc::new(RecorderFactory {
        name: "test_recorder",
        tx,
    }));
    let provider = StaticDataProvider::new(
        r#"{"name":"audited","allow_rules":[
            {"name":"unary-only","request":{"paths":["/helloworld.Greeter/SayHello"]}}
        ],"deny_rules":[
            {"name":"no-streams","request":{"paths":["*/ServerHello"]}}
        ],"audit_logging_options":{
            "audit_condition":"ON_DENY_AND_ALLOW",
            "audit_logger":[{"name":"test_recorder"}]
        }}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Counting::new()))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;

    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("allowed unary");
    let denied = client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("deny rule wins");
    permission_denied(&denied);

    let events = drain(&rx);
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0],
        AuditEvent {
            rpc_method: "/helloworld.Greeter/SayHello".to_owned(),
            principal: String::new(),
            policy_name: "audited".to_owned(),
            matched_rule: "unary-only".to_owned(),
            authorized: true,
        }
    );
    assert_eq!(
        events[1],
        AuditEvent {
            rpc_method: "/helloworld.Greeter/ServerHello".to_owned(),
            principal: String::new(),
            policy_name: "audited".to_owned(),
            matched_rule: "no-streams".to_owned(),
            authorized: false,
        }
    );
}

#[tokio::test]
async fn audit_default_deny_reports_empty_rule() {
    let (tx, rx) = std::sync::mpsc::channel();
    register_audit_logger_factory(Arc::new(RecorderFactory {
        name: "test_recorder_default",
        tx,
    }));
    let provider = StaticDataProvider::new(
        r#"{"name":"closed","allow_rules":[],
            "audit_logging_options":{
            "audit_condition":"ON_DENY",
            "audit_logger":[{"name":"test_recorder_default"}]
        }}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Counting::new()))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;
    let denied = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect_err("empty allow list denies");
    permission_denied(&denied);

    let events = drain(&rx);
    assert_eq!(events.len(), 1);
    assert!(!events[0].authorized);
    assert!(events[0].matched_rule.is_empty());
}

#[tokio::test]
async fn audit_condition_filters_events() {
    let (tx, rx) = std::sync::mpsc::channel();
    register_audit_logger_factory(Arc::new(RecorderFactory {
        name: "test_recorder_deny_only",
        tx,
    }));
    // ON_DENY: the allowed call audits nothing, the denied call audits.
    let provider = StaticDataProvider::new(
        r#"{"name":"deny-only","allow_rules":[
            {"name":"unary-only","request":{"paths":["/helloworld.Greeter/SayHello"]}}
        ],"audit_logging_options":{
            "audit_condition":"ON_DENY",
            "audit_logger":[{"name":"test_recorder_deny_only"}]
        }}"#,
    )
    .expect("policy");
    let (addr, listener) = bind().await;
    let handle = tokio::spawn(async move {
        Server::new(GreeterServer::new(Counting::new()))
            .authorization_policy(provider)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _guard = ServerGuard(handle);
    let client = greeter_client(addr).await;
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("allowed unary");
    assert!(drain(&rx).is_empty());
    client
        .server_hello(Request::new(req("ada")))
        .await
        .expect_err("unlisted path denied");
    assert_eq!(drain(&rx).len(), 1);
}

#[test]
fn audit_record_carries_no_metadata() {
    // OB-03: the stdout record is exactly the five A59 fields plus the
    // timestamp; headers and metadata can never appear in it.
    let record = format_record(
        &AuditEvent {
            rpc_method: "/s/M".to_owned(),
            principal: String::new(),
            policy_name: "p".to_owned(),
            matched_rule: String::new(),
            authorized: false,
        },
        std::time::SystemTime::UNIX_EPOCH,
    );
    let value: serde_json::Value = serde_json::from_str(&record).expect("JSON");
    let entry = value.get("grpc_audit_log").expect("entry");
    assert_eq!(entry.as_object().expect("object").len(), 6);
    for key in ["headers", "metadata", "authorization", "cookie"] {
        assert!(entry.get(key).is_none(), "must not emit {key}");
    }
}

async fn tls_client(addr: SocketAddr, tls: ClientTls) -> GreeterClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match GreeterClient::connect_tls(addr, tls.clone()).await {
            Ok(client) => return client,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}
