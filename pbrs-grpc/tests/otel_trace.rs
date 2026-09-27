//! OpenTelemetry tracing tests (GF-02, A72): W3C propagation
//! inject/extract against spec vectors, end-to-end parent/child spans,
//! all four call shapes, error-path span completion, and the
//! safe-by-default attribute policy (no metadata or payloads).
//!
//! The in-memory tracer implements the API-crate traits directly, so
//! these tests need no SDK trace dependency.

#![cfg(feature = "otel")]
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
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

use common::{ServerGuard, name_of, req};
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry::trace::{
    Span, SpanContext, SpanId, SpanKind, Status as SpanStatus, TraceContextExt, TraceFlags,
    TraceId, TraceState, Tracer, TracerProvider,
};
use opentelemetry::{Context, InstrumentationScope, KeyValue, Value};
use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::otel::trace::{ClientTracing, ServerTracing, W3CPropagator, attr};
use pbrs_grpc::{Metadata, Request, Response, Router, Status, Streaming};
use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// In-memory tracer (API traits only).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct FinishedSpan {
    name: String,
    kind: SpanKind,
    context: SpanContext,
    parent: SpanContext,
    attributes: Vec<KeyValue>,
    status: SpanStatus,
}

#[derive(Debug, Default)]
struct MemState {
    spans: Mutex<Vec<FinishedSpan>>,
    next_id: AtomicU64,
}

#[derive(Clone, Debug, Default)]
struct MemProvider {
    state: Arc<MemState>,
}

#[derive(Clone, Debug)]
struct MemTracer {
    state: Arc<MemState>,
}

#[derive(Debug)]
struct MemSpan {
    state: Arc<MemState>,
    name: String,
    kind: SpanKind,
    context: SpanContext,
    parent: SpanContext,
    attributes: Vec<KeyValue>,
    status: SpanStatus,
    recording: bool,
}

impl MemProvider {
    fn finished(&self) -> Vec<FinishedSpan> {
        self.state.spans.lock().expect("spans").clone()
    }
}

impl TracerProvider for MemProvider {
    type Tracer = MemTracer;

    fn tracer_with_scope(&self, _scope: InstrumentationScope) -> Self::Tracer {
        MemTracer {
            state: self.state.clone(),
        }
    }
}

impl Tracer for MemTracer {
    type Span = MemSpan;

    fn build_with_context(
        &self,
        builder: opentelemetry::trace::SpanBuilder,
        parent: &Context,
    ) -> Self::Span {
        let parent_cx = parent.span().span_context().clone();
        // Deterministic ids: tests assert exact parenting, not randomness.
        let trace_id = if parent_cx.is_valid() {
            parent_cx.trace_id()
        } else {
            TraceId::from(u128::from(
                self.state.next_id.fetch_add(1, Ordering::SeqCst) + 1,
            ))
        };
        let span_id = SpanId::from(self.state.next_id.fetch_add(1, Ordering::SeqCst) + 1);
        MemSpan {
            state: self.state.clone(),
            name: builder.name.into_owned(),
            kind: builder.span_kind.unwrap_or(SpanKind::Internal),
            context: SpanContext::new(
                trace_id,
                span_id,
                TraceFlags::SAMPLED,
                false,
                TraceState::NONE,
            ),
            parent: parent_cx,
            attributes: builder.attributes.unwrap_or_default(),
            status: SpanStatus::Unset,
            recording: true,
        }
    }
}

impl Span for MemSpan {
    fn span_context(&self) -> &SpanContext {
        &self.context
    }

    fn is_recording(&self) -> bool {
        self.recording
    }

    fn set_attribute(&mut self, attribute: KeyValue) {
        self.attributes.push(attribute);
    }

    fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    fn update_name<T>(&mut self, new_name: T)
    where
        T: Into<Cow<'static, str>>,
    {
        self.name = new_name.into().into_owned();
    }

    fn add_event_with_timestamp<T>(
        &mut self,
        _name: T,
        _timestamp: SystemTime,
        _attributes: Vec<KeyValue>,
    ) where
        T: Into<Cow<'static, str>>,
    {
    }

    fn add_link(&mut self, _span_context: SpanContext, _attributes: Vec<KeyValue>) {}

    fn end_with_timestamp(&mut self, _timestamp: SystemTime) {
        if !self.recording {
            return;
        }
        self.recording = false;
        self.state.spans.lock().expect("spans").push(FinishedSpan {
            name: self.name.clone(),
            kind: self.kind.clone(),
            context: self.context.clone(),
            parent: self.parent.clone(),
            attributes: self.attributes.clone(),
            status: self.status.clone(),
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

/// Echo service where unary `boom` fails (error-path spans).
struct TraceEcho;

fn reply(text: &str) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(text);
    r
}

impl Greeter for TraceEcho {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        if name == "boom" {
            return Err(Status::invalid_argument("boom"));
        }
        Ok(Response::new(reply(&name)))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut inbound = request.into_inner();
        let mut names = Vec::new();
        while let Some(msg) = inbound.message().await? {
            names.push(msg.name().to_str().unwrap_or("").to_owned());
        }
        Ok(Response::new(reply(&names.join(","))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = request.get_ref().name().to_str().unwrap_or("").to_owned();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(reply(part)).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        drop(tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                let name = msg.name().to_str().unwrap_or("").to_owned();
                if tx.send(reply(&name)).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

async fn serve(provider: &MemProvider) -> (SocketAddr, ServerGuard) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let tracing = ServerTracing::new(provider.tracer("test-server"));
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(GreeterServer::new(TraceEcho))
            .intercept(tracing)
            .serve_listener(listener)
            .await
            .ok();
    });
    (addr, ServerGuard(handle))
}

fn str_attr(span: &FinishedSpan, key: &str) -> Option<String> {
    span.attributes.iter().find_map(|kv| {
        if kv.key.as_str() == key {
            match &kv.value {
                Value::String(value) => Some(value.to_string()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn i64_attr(span: &FinishedSpan, key: &str) -> Option<i64> {
    span.attributes.iter().find_map(|kv| {
        if kv.key.as_str() == key {
            match &kv.value {
                Value::I64(value) => Some(*value),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn metadata_with(pairs: &[(&str, &str)]) -> Metadata {
    let mut md = Metadata::new();
    for (key, value) in pairs {
        md.set(key, value).expect("set");
    }
    md
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

/// End to end: ambient client span injects, server span parents under it.
#[tokio::test]
async fn trace_parenting_end_to_end() {
    let provider = MemProvider::default();
    let (addr, _guard) = serve(&provider).await;

    let channel = pbrs_grpc::Channel::connect_lazy(addr)
        .expect("lazy")
        .intercept(ClientTracing::new());
    let client = GreeterClient::new(channel);

    let tracer = provider.tracer("test-client");
    let client_span = tracer.start("client-work");
    let client_cx = client_span.span_context().clone();
    assert!(client_cx.is_valid());
    // The attach and the first poll share one thread with no await
    // between them, so injection sees this context.
    let ambient = Context::current_with_span(client_span);
    let _attached = ambient.attach();
    let resp = client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    assert_eq!(name_of(&resp.into_inner()), "ada");

    let spans = provider.finished();
    assert_eq!(spans.len(), 1, "one server span");
    let span = &spans[0];
    assert_eq!(span.name, "helloworld.Greeter/SayHello");
    assert_eq!(span.kind, SpanKind::Server);
    assert_eq!(span.context.trace_id(), client_cx.trace_id());
    assert_eq!(span.parent.span_id(), client_cx.span_id());
    assert!(span.parent.is_remote());
    assert!(span.parent.is_sampled());
    assert_eq!(str_attr(span, attr::RPC_SYSTEM).as_deref(), Some("grpc"));
    assert_eq!(
        str_attr(span, attr::RPC_SERVICE).as_deref(),
        Some("helloworld.Greeter")
    );
    assert_eq!(
        str_attr(span, attr::RPC_METHOD).as_deref(),
        Some("SayHello")
    );
    assert_eq!(i64_attr(span, attr::RPC_GRPC_STATUS_CODE), Some(0));
    assert!(matches!(span.status, SpanStatus::Ok));
}

/// W3C Trace Context vectors: the spec examples plus rejects.
#[test]
fn trace_w3c_vectors() {
    let propagator = W3CPropagator::new();

    // Spec example (trace-context §3.2.1): ids, flags, state survive.
    let md = metadata_with(&[
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("tracestate", "rojo=00f067aa0ba902b7,congo=t61rcWkgMzE"),
    ]);
    let cx = propagator.extract(&MetadataExtractor(&md));
    let remote = cx.span().span_context().clone();
    assert!(remote.is_valid());
    assert!(remote.is_remote());
    assert!(remote.is_sampled());
    assert_eq!(
        format!("{:032x}", remote.trace_id()),
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(format!("{:016x}", remote.span_id()), "00f067aa0ba902b7");
    assert_eq!(remote.trace_state().get("rojo"), Some("00f067aa0ba902b7"));
    assert_eq!(remote.trace_state().get("congo"), Some("t61rcWkgMzE"));

    // Unsampled flag parses.
    let md = metadata_with(&[(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
    )]);
    let remote = propagator
        .extract(&MetadataExtractor(&md))
        .span()
        .span_context()
        .clone();
    assert!(remote.is_valid());
    assert!(!remote.is_sampled());

    // Future version with extra fields stays forward-compatible.
    let md = metadata_with(&[(
        "traceparent",
        "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-ignored",
    )]);
    let remote = propagator
        .extract(&MetadataExtractor(&md))
        .span()
        .span_context()
        .clone();
    assert!(remote.is_valid());

    // Every malformed header yields no remote context.
    for bad in [
        "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", // bad version
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01", // zero trace
        "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01", // zero span
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-x", // 00 + extra
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",    // short
        "00-zzf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", // bad hex
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0g", // bad flags
        "not-a-header",
        "",
    ] {
        let md = metadata_with(&[("traceparent", bad)]);
        let remote = propagator
            .extract(&MetadataExtractor(&md))
            .span()
            .span_context()
            .clone();
        assert!(!remote.is_valid(), "rejects {bad:?}");
    }

    // Missing headers extract nothing.
    let remote = propagator
        .extract(&MetadataExtractor(&Metadata::new()))
        .span()
        .span_context()
        .clone();
    assert!(!remote.is_valid());

    // Invalid tracestate members drop; valid ones survive.
    let md = metadata_with(&[
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("tracestate", "BAD KEY=value,ok=1,tenant@vendor=v"),
    ]);
    let remote = propagator
        .extract(&MetadataExtractor(&md))
        .span()
        .span_context()
        .clone();
    assert!(remote.is_valid());
    assert_eq!(remote.trace_state().get("ok"), Some("1"));
    assert_eq!(remote.trace_state().get("tenant@vendor"), Some("v"));
    assert_eq!(remote.trace_state().get("BAD KEY"), None);
}

/// Inject writes exact W3C bytes and round-trips through extract.
#[test]
fn trace_inject_round_trip() {
    let propagator = W3CPropagator::new();
    let provider = MemProvider::default();
    let span = provider.tracer("t").start("work");
    let cx = Context::current_with_span(span);
    let want_trace = format!("{:032x}", cx.span().span_context().trace_id());
    let want_span = format!("{:016x}", cx.span().span_context().span_id());

    let mut md = Metadata::new();
    propagator.inject_context(&cx, &mut MetadataInjector(&mut md));
    assert_eq!(
        md.get("traceparent"),
        Some(format!("00-{want_trace}-{want_span}-01").as_str())
    );

    let back = propagator
        .extract(&MetadataExtractor(&md))
        .span()
        .span_context()
        .clone();
    assert_eq!(back.trace_id(), cx.span().span_context().trace_id());
    assert_eq!(back.span_id(), cx.span().span_context().span_id());
    assert!(back.is_sampled());

    // Empty context injects nothing.
    let mut md = Metadata::new();
    propagator.inject_context(&Context::new(), &mut MetadataInjector(&mut md));
    assert_eq!(md.get("traceparent"), None);
}

/// Handler errors still end the span (without a status attribute: the
/// success hook never runs, so the drop fallback cannot see the code).
#[tokio::test]
async fn trace_error_span_still_ends() {
    let provider = MemProvider::default();
    let (addr, _guard) = serve(&provider).await;
    let channel = pbrs_grpc::Channel::connect_lazy(addr)
        .expect("lazy")
        .intercept(ClientTracing::new());
    let client = GreeterClient::new(channel);

    let err = client
        .say_hello(Request::new(req("boom")))
        .await
        .expect_err("boom fails");
    assert_eq!(err.code(), pbrs_grpc::Code::InvalidArgument);

    let spans = provider.finished();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].name, "helloworld.Greeter/SayHello");
    assert_eq!(i64_attr(&spans[0], attr::RPC_GRPC_STATUS_CODE), None);
    assert!(matches!(spans[0].status, SpanStatus::Unset));
}

/// Safe by default: only the four RPC attributes exist, and neither
/// metadata values nor payload bytes appear in any of them.
#[tokio::test]
async fn trace_safe_attributes() {
    let provider = MemProvider::default();
    let (addr, _guard) = serve(&provider).await;
    let channel = pbrs_grpc::Channel::connect_lazy(addr)
        .expect("lazy")
        .intercept(ClientTracing::new());
    let client = GreeterClient::new(channel);

    let mut request = Request::new(req("secret-payload-ada"));
    request
        .metadata_mut()
        .set("authorization", "Bearer hunter2-topsecret")
        .expect("auth");
    request
        .metadata_mut()
        .set("cookie", "session=abcdef-topsecret")
        .expect("cookie");
    client.say_hello(request).await.expect("unary");

    let spans = provider.finished();
    assert_eq!(spans.len(), 1);
    for kv in &spans[0].attributes {
        assert!(
            [
                attr::RPC_SYSTEM,
                attr::RPC_SERVICE,
                attr::RPC_METHOD,
                attr::RPC_GRPC_STATUS_CODE,
            ]
            .contains(&kv.key.as_str()),
            "unexpected attribute {}",
            kv.key
        );
        let rendered = format!("{:?}", kv.value);
        assert!(
            !rendered.contains("secret") && !rendered.contains("hunter2"),
            "sensitive value leaked into {rendered}"
        );
    }
}

/// Without an ambient span nothing is injected and the server span is
/// a root; the RPC itself is unaffected.
#[tokio::test]
async fn trace_untraced_client_stays_root() {
    let provider = MemProvider::default();
    let (addr, _guard) = serve(&provider).await;

    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let channel = pbrs_grpc::Channel::connect_lazy(addr)
        .expect("lazy")
        .intercept(ClientTracing::new())
        .intercept(move |call: &mut pbrs_grpc::Outgoing<'_>| {
            seen_clone
                .lock()
                .expect("seen")
                .push(call.metadata().get("traceparent").map(str::to_owned));
            Ok(())
        });
    let client = GreeterClient::new(channel);
    // No ambient span attached: ThreadLocal context is empty here.
    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");

    assert_eq!(*seen.lock().expect("seen"), vec![None]);
    let spans = provider.finished();
    assert_eq!(spans.len(), 1);
    assert!(!spans[0].parent.is_valid(), "server span is a root");
}

/// All four call shapes record server spans.
#[tokio::test]
async fn trace_all_call_shapes() {
    let provider = MemProvider::default();
    let (addr, _guard) = serve(&provider).await;
    let channel = pbrs_grpc::Channel::connect_lazy(addr)
        .expect("lazy")
        .intercept(ClientTracing::new());
    let client = GreeterClient::new(channel);

    client
        .say_hello(Request::new(req("ada")))
        .await
        .expect("unary");
    let mut stream = client
        .server_hello(Request::new(req("a,b")))
        .await
        .expect("server stream")
        .into_inner();
    let mut seen = Vec::new();
    while let Some(msg) = stream.message().await.expect("message") {
        seen.push(name_of(&msg));
    }
    assert_eq!(seen, vec!["a".to_owned(), "b".to_owned()]);

    let (tx, call) = client.client_hello(Request::new(()));
    tx.send(req("ada")).await.expect("send");
    tx.send(req("bob")).await.expect("send");
    drop(tx);
    assert_eq!(name_of(&call.await.expect("rpc").into_inner()), "ada,bob");

    let (tx, call) = client.stream_hello(Request::new(()));
    tx.send(req("x")).await.expect("send");
    drop(tx);
    let mut stream = call.await.expect("rpc").into_inner();
    let mut seen = Vec::new();
    while let Some(msg) = stream.message().await.expect("message") {
        seen.push(name_of(&msg));
    }
    assert_eq!(seen, vec!["x".to_owned()]);

    let mut names: Vec<String> = provider
        .finished()
        .iter()
        .map(|span| span.name.clone())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "helloworld.Greeter/ClientHello",
            "helloworld.Greeter/SayHello",
            "helloworld.Greeter/ServerHello",
            "helloworld.Greeter/StreamHello",
        ]
    );
    for span in provider.finished() {
        assert_eq!(span.kind, SpanKind::Server);
        assert_eq!(i64_attr(&span, attr::RPC_GRPC_STATUS_CODE), Some(0));
    }
}

// ---------------------------------------------------------------------------
// Test-local metadata carrier adapters (the lib keeps its own private).
// ---------------------------------------------------------------------------

struct MetadataExtractor<'a>(&'a Metadata);

impl opentelemetry::propagation::Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(key, _)| key).collect()
    }
}

struct MetadataInjector<'a>(&'a mut Metadata);

impl opentelemetry::propagation::Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.set(key, value).ok();
    }
}
