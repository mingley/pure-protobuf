//! Optional OpenTelemetry tracing (GF-02, A72): W3C TraceContext
//! propagation plus server spans with a safe-by-default attribute policy.
//!
//! Install [`ClientTracing`] on a channel to inject `traceparent` /
//! `tracestate` from the ambient [`Context`], and [`ServerTracing`] on a
//! server or router to extract the remote parent and record one server
//! span per RPC. Span names follow the RPC semantic conventions
//! (`{service}/{method}`); attributes are exactly `rpc.system`,
//! `rpc.service`, `rpc.method`, and (on the success path)
//! `rpc.grpc.status_code`. Metadata, payloads, and peer identity never
//! become span attributes.
//!
//! Propagation is plain W3C Trace Context, the same bytes the Go and
//! Java OTel SDKs emit, implemented against the API crate only so this
//! module needs no SDK dependency.
//!
//! Boundaries (GF-02b): automatic client spans need a per-call client
//! completion hook, which the channel does not expose — `ClientTracing`
//! injects the ambient context, so wrap the call in your own span (see
//! the module test). Error-path server spans end without a status code
//! attribute: the response hook that reports success never runs for
//! handler errors, and the drop fallback cannot see the status.
//! Streaming server spans end when the handler future completes, before
//! the response drain finishes.

#![allow(
    clippy::disallowed_types,
    reason = "span handoff inside sync interceptor hooks; never held across await"
)]

use crate::{
    ClientInterceptor, Interceptor, Metadata, Outgoing, ResponseInterceptor, ResponseParts, Rpc,
    Status,
};
use opentelemetry::propagation::text_map_propagator::FieldIter;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::{Span, SpanId, SpanKind, TraceContextExt, TraceFlags, TraceId, Tracer};
use opentelemetry::{Context, KeyValue};
use std::sync::Mutex;

/// W3C Trace Context header names.
pub mod header {
    /// `traceparent` carrying version, trace id, parent span id, flags.
    pub const TRACEPARENT: &str = "traceparent";
    /// `tracestate` carrying the vendor list.
    pub const TRACESTATE: &str = "tracestate";
}

/// Span attribute keys (RPC semantic conventions).
pub mod attr {
    /// Always `grpc`.
    pub const RPC_SYSTEM: &str = "rpc.system";
    /// Service half of the path (e.g. `helloworld.Greeter`).
    pub const RPC_SERVICE: &str = "rpc.service";
    /// Method half of the path (e.g. `SayHello`).
    pub const RPC_METHOD: &str = "rpc.method";
    /// Numeric gRPC status code; set on the success path only.
    pub const RPC_GRPC_STATUS_CODE: &str = "rpc.grpc.status_code";
}

/// `rpc.system` value recorded on every span.
const RPC_SYSTEM_GRPC: &str = "grpc";

/// Maximum `tracestate` members kept on extract (W3C cap is 32).
const MAX_TRACESTATE_MEMBERS: usize = 32;

/// W3C Trace Context propagator over [`TextMapPropagator`].
///
/// Extract is lenient the way interop requires: a missing or malformed
/// `traceparent` yields the input context unchanged (the RPC keeps a
/// root span), and invalid `tracestate` members are skipped. Inject
/// writes nothing when the context holds no valid span.
#[derive(Clone, Debug)]
pub struct W3CPropagator {
    fields: [String; 2],
}

impl Default for W3CPropagator {
    fn default() -> Self {
        Self::new()
    }
}

impl W3CPropagator {
    /// Build the propagator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fields: [
                header::TRACEPARENT.to_owned(),
                header::TRACESTATE.to_owned(),
            ],
        }
    }
}

impl TextMapPropagator for W3CPropagator {
    fn inject_context(&self, cx: &Context, injector: &mut dyn Injector) {
        let span_cx = cx.span().span_context().clone();
        if !span_cx.is_valid() {
            return;
        }
        injector.set(
            header::TRACEPARENT,
            format!(
                "00-{:032x}-{:016x}-{:02x}",
                span_cx.trace_id(),
                span_cx.span_id(),
                span_cx.trace_flags(),
            ),
        );
        let state = span_cx.trace_state().header();
        if !state.is_empty() {
            injector.set(header::TRACESTATE, state);
        }
    }

    fn extract_with_context(&self, cx: &Context, extractor: &dyn Extractor) -> Context {
        let Some(remote) = extract_remote(extractor) else {
            return cx.clone();
        };
        cx.with_remote_span_context(remote)
    }

    fn fields(&self) -> FieldIter<'_> {
        FieldIter::new(&self.fields)
    }
}

/// Parse `traceparent` (+ optional `tracestate`) into a remote span context.
fn extract_remote(extractor: &dyn Extractor) -> Option<opentelemetry::trace::SpanContext> {
    let header = extractor.get(header::TRACEPARENT)?;
    let parts: Vec<&str> = header.split('-').collect();
    let version = *parts.first()?;
    if version.len() != 2 || !is_hex(version) || version.eq_ignore_ascii_case("ff") {
        return None;
    }
    if version == "00" && parts.len() != 4 {
        return None;
    }
    if parts.len() < 4 {
        return None;
    }
    let trace_hex = parts.get(1)?;
    let span_hex = parts.get(2)?;
    let flags_hex = parts.get(3)?;
    if trace_hex.len() != 32 || !is_hex(trace_hex) {
        return None;
    }
    if span_hex.len() != 16 || !is_hex(span_hex) {
        return None;
    }
    let flags_field = if version == "00" {
        if flags_hex.len() != 2 {
            return None;
        }
        *flags_hex
    } else {
        flags_hex.get(0..2)?
    };
    if !is_hex(flags_field) {
        return None;
    }
    let trace_id = TraceId::from_hex(trace_hex).ok()?;
    let span_id = SpanId::from_hex(span_hex).ok()?;
    if trace_id == TraceId::INVALID || span_id == SpanId::INVALID {
        return None;
    }
    let flags = u8::from_str_radix(flags_field, 16).ok()?;
    let state = extractor
        .get(header::TRACESTATE)
        .map(parse_tracestate)
        .unwrap_or_default();
    Some(opentelemetry::trace::SpanContext::new(
        trace_id,
        span_id,
        TraceFlags::new(flags),
        true,
        state,
    ))
}

/// Whether every byte is ASCII hex (either case parses).
fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse a `tracestate` header leniently: trim optional whitespace,
/// skip invalid members, keep the first 32.
fn parse_tracestate(header: &str) -> opentelemetry::trace::TraceState {
    let mut members = Vec::new();
    for member in header.split(',') {
        if members.len() >= MAX_TRACESTATE_MEMBERS {
            break;
        }
        let member = member.trim_matches(|c| c == ' ' || c == '\t');
        let Some((key, value)) = member.split_once('=') else {
            continue;
        };
        if !is_tracestate_key(key) || !is_tracestate_value(value) {
            continue;
        }
        members.push((key.to_owned(), value.to_owned()));
    }
    opentelemetry::trace::TraceState::from_key_value(members).unwrap_or_default()
}

/// W3C `tracestate` key shape (single or multi-tenant `tenant@vendor`).
fn is_tracestate_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 256 {
        return false;
    }
    let valid_simple = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && s.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'_' | b'-' | b'*' | b'/')
            })
    };
    match key.split_once('@') {
        None => valid_simple(key),
        Some((tenant, vendor)) => {
            !tenant.is_empty()
                && tenant.len() <= 241
                && tenant
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && !vendor.contains('@')
                && valid_simple(vendor)
        }
    }
}

/// W3C `tracestate` value shape: printable ASCII except `,` and `=`.
fn is_tracestate_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| (0x20..0x7e).contains(&b) && b != b',' && b != b'=')
}

/// [`Extractor`] over gRPC metadata.
struct MetadataExtractor<'a>(&'a Metadata);

impl Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)
    }

    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(key, _)| key).collect()
    }
}

/// [`Injector`] into gRPC metadata. Header values the propagator emits
/// are always valid metadata; a set failure never fails the RPC.
struct MetadataInjector<'a>(&'a mut Metadata);

impl Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.set(key, value).ok();
    }
}

/// Client-side trace propagation: inject `traceparent` / `tracestate`
/// from the ambient [`Context`] into outbound metadata.
///
/// Automatic client spans are GF-02b (the channel exposes no per-call
/// completion hook): wrap the call in your own span and this injector
/// parents the server span under it.
#[derive(Clone, Debug, Default)]
pub struct ClientTracing {
    propagator: W3CPropagator,
}

impl ClientTracing {
    /// Build the injector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClientInterceptor for ClientTracing {
    fn intercept(&self, call: &mut Outgoing<'_>) -> Result<(), Status> {
        let cx = Context::current();
        self.propagator
            .inject_context(&cx, &mut MetadataInjector(call.metadata_mut()));
        Ok(())
    }
}

/// Server-side tracing: extract the remote parent from inbound metadata
/// and record one server span per RPC.
///
/// Install with `Server::intercept`, `Router::intercept`, or
/// `ServiceExt::intercept`. The span ends when the handler future
/// completes on the success path (reporting `rpc.grpc.status_code` 0);
/// on error paths a drop fallback ends it without a status attribute.
pub struct ServerTracing<T> {
    tracer: T,
    propagator: W3CPropagator,
}

impl<T: Tracer> ServerTracing<T> {
    /// Build server tracing around `tracer`.
    #[must_use]
    pub fn new(tracer: T) -> Self {
        Self {
            tracer,
            propagator: W3CPropagator::new(),
        }
    }
}

impl<T> Clone for ServerTracing<T>
where
    T: Tracer + Clone,
{
    fn clone(&self) -> Self {
        Self {
            tracer: self.tracer.clone(),
            propagator: self.propagator.clone(),
        }
    }
}

impl<T> std::fmt::Debug for ServerTracing<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerTracing").finish_non_exhaustive()
    }
}

impl<T> Interceptor for ServerTracing<T>
where
    T: Tracer + Send + Sync + 'static,
    T::Span: Send + Sync + 'static,
{
    fn intercept(&self, rpc: &mut Rpc) -> Result<(), Status> {
        let parent = self.propagator.extract(&MetadataExtractor(rpc.metadata()));
        let service = rpc.service().to_owned();
        let method = rpc.method().to_owned();
        let builder = self
            .tracer
            .span_builder(format!("{service}/{method}"))
            .with_kind(SpanKind::Server)
            .with_attributes([
                KeyValue::new(attr::RPC_SYSTEM, RPC_SYSTEM_GRPC),
                KeyValue::new(attr::RPC_SERVICE, service),
                KeyValue::new(attr::RPC_METHOD, method),
            ]);
        let span = self.tracer.build_with_context(builder, &parent);
        rpc.push_response_hook(std::sync::Arc::new(FinishSpan::new(span)));
        Ok(())
    }
}

/// Ends the server span. Runs on the success path, reporting status 0;
/// the [`Drop`] fallback covers handler errors, where the hook never runs.
struct FinishSpan<S: Span> {
    span: Mutex<Option<S>>,
}

impl<S: Span> FinishSpan<S> {
    fn new(span: S) -> Self {
        Self {
            span: Mutex::new(Some(span)),
        }
    }

    /// Take the span unless a poisoning thread left the lock behind; a
    /// poisoned lock still ends the span best-effort via the guard.
    fn take(&self) -> Option<S> {
        self.span.lock().ok().and_then(|mut slot| slot.take())
    }
}

impl<S> ResponseInterceptor for FinishSpan<S>
where
    S: Span + Send + Sync + 'static,
{
    fn intercept(&self, _parts: &mut ResponseParts) -> Result<(), Status> {
        if let Some(mut span) = self.take() {
            span.set_attribute(KeyValue::new(attr::RPC_GRPC_STATUS_CODE, 0i64));
            span.set_status(opentelemetry::trace::Status::Ok);
            span.end();
        }
        Ok(())
    }
}

impl<S: Span> Drop for FinishSpan<S> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.span.lock() {
            if let Some(mut span) = slot.take() {
                span.end();
            }
        }
    }
}
