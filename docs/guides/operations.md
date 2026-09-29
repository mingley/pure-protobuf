# Operations, Health Checking, and Diagnostics

Use this guide to add readiness checks, reflection, keepalive, error details,
and telemetry to a generated service. Start with health and resource limits;
enable other diagnostics as your deployment needs them. Replace TLS material
by constructing new server/client objects.

The named Rust recipes below use the generated types and `MyGreeter` from
the [greeter example](../../examples/greeter/src/lib.rs). The
[onboarding tests](../../tests/onboarding.rs) extract these recipes, compile
them as a fresh consumer, and exercise their RPC behavior. They use the default
`pbrs-grpc` feature set and Tokio's `rt-multi-thread`, `macros`, `net`, `time`,
`sync`, and `io-util` features; health, reflection and rich errors need no
optional crate feature. The [production recipe](production-service.md) supplies
the separately compiled TLS/mTLS and resource-limit example.

---

<a id="health-checks"></a>
## 1. Health Checking (`grpc.health.v1`)

Mount the built-in official gRPC Health Checking protocol
(`grpc.health.v1`) when clients or load balancers need readiness state.

### Mounting the Health Service

```rust recipe=health
use pbrs_grpc::health;
use pbrs_grpc::Router;
use pbrs_grpc_example_greeter::{GreeterServer, MyGreeter};

pub fn health_router() -> (Router, health::HealthReporter) {
    let (health_service, health_reporter) = health::service();
    health_reporter.set_serving("");
    health_reporter.set_serving(GreeterServer::<MyGreeter>::NAME);
    let router = Router::new()
        .add_service(GreeterServer::new(MyGreeter))
        .add_service(health_service);
    (router, health_reporter)
}
```

Serve the returned router with your chosen listener and shutdown signal;
retain the reporter for readiness updates.

### Dynamic Status Updates

Update status during startup, shutdown, or health probes:

```rust recipe=readiness
use pbrs_grpc::health::HealthReporter;
use pbrs_grpc_example_greeter::{GreeterServer, MyGreeter};

pub fn mark_not_ready(health_reporter: &HealthReporter) {
    health_reporter.set_not_serving(GreeterServer::<MyGreeter>::NAME);
    // Mark the entire process not serving during graceful drain.
    health_reporter.shutdown();
}
```

Clients can run unary `Check` calls or subscribe to long-lived streaming
`Watch` calls. `Health::list` returns a snapshot of all registered service
names.

---

<a id="reflection"></a>
## 2. Server Reflection (`grpc.reflection.v1`)

Enable server reflection when developer tools such as `grpcurl` or Postman may
inspect and call services without local `.proto` files:

```rust recipe=reflection
use pbrs_grpc::{reflection, Router, Status};
use pbrs_grpc_example_greeter::{FILE_DESCRIPTOR_SET, GreeterServer, MyGreeter};

pub fn reflection_router() -> Result<Router, Status> {
    // FILE_DESCRIPTOR_SET is emitted alongside the generated service code.
    let reflection_service = reflection::service([FILE_DESCRIPTOR_SET])?;
    Ok(Router::new()
        .add_service(GreeterServer::new(MyGreeter))
        .add_service(reflection_service))
}
```

### Inspecting with `grpcurl`

Use plaintext only for local or otherwise authorized endpoints:

```bash
# List available services
grpcurl -plaintext 127.0.0.1:50051 list

# Describe a specific service
grpcurl -plaintext 127.0.0.1:50051 describe hello.Greeter

# Invoke an RPC directly
grpcurl -plaintext -d '{"name": "Ada"}' 127.0.0.1:50051 hello.Greeter/SayHello
```

Mount `pbrs_grpc::reflection::v1alpha_service` after the v1 reflection service
when older tools need `grpc.reflection.v1alpha.ServerReflection`; it uses the
same wire-compatible reflection messages through a distinct service route.

---

<a id="keepalive"></a>
<a id="tuning"></a>
## 3. Keepalive and Socket Tuning

Network middleboxes, such as firewalls or Network Address Translation (NAT)
gateways, can terminate idle TCP connections. `pbrs-grpc` provides HTTP/2
keepalive and operating-system TCP keepalive knobs.

### HTTP/2 Protocol PINGs

Configure protocol PINGs with `keep_alive_interval` and
`keep_alive_timeout`:

- Sends HTTP/2 `PING` frames on idle connections.
- If the peer does not acknowledge within `keep_alive_timeout`, the connection
  is closed. A later eligible call can redial; replay still follows the
  [retry safety contract](../retry-contract.md).

```rust
use pbrs_grpc::ChannelConfig;
use std::time::Duration;

let config = ChannelConfig::new()
    .keep_alive_interval(Duration::from_secs(30))
    .keep_alive_timeout(Duration::from_secs(10));

let client = GreeterClient::connect_with("127.0.0.1:50051", config).await?;
```

### OS-Level TCP Keepalive

Configure these on `ChannelConfig` or `ServerConfig`:

| Setting | Effect |
|---|---|
| `tcp_keepalive_interval` | Sets the `TCP_KEEPINTVL` interval between keepalive probes |
| `tcp_keepalive_retries` | Sets `TCP_KEEPCNT`, the number of unacknowledged probes before the operating system drops the socket |
| `TCP_NODELAY` | Enabled by default to minimize RPC latency |

---

<a id="status-codes"></a>
## 4. Status Codes and Rich Error Details

### Standard Status Codes

Use `pbrs_grpc::Status` constructors for standard gRPC status codes:

```rust
Status::ok();
Status::invalid_argument("missing user id");
Status::not_found("record does not exist");
Status::permission_denied("insufficient privileges");
Status::resource_exhausted("rate limit exceeded");
Status::internal("database connection failed");
Status::unavailable("service temporarily overloaded");
```

### Rich Errors with `ErrorDetails`

Use `ErrorDetails` when a server needs to return structured
`google.rpc.Status` payloads in the `grpc-status-details-bin` trailer:

```rust recipe=rich_error
use pbrs_grpc::pb::{BadRequest, ErrorDetails, ErrorInfo, RetryInfo};
use pbrs_grpc::{Code, Status};
use std::time::Duration;

pub fn quota_status() -> Result<Status, Status> {
    let details = ErrorDetails::new()
        .with_error_info(ErrorInfo::with_reason("RATE_LIMITED", "api.example.com"))
        .with_retry_info(RetryInfo::with_retry_delay(Duration::from_secs(5)))
        .with_bad_request(BadRequest::with_field("user_id", "must be positive"));
    Status::from_error_details(Code::ResourceExhausted, "quota exceeded", &details)
}
```

On the client, inspect the unpacked details from the returned `Status`:

```rust recipe=inspect_error
pub fn invalid_fields(status: &pbrs_grpc::Status) -> Vec<String> {
    status.bad_request().map_or_else(Vec::new, |bad_request| {
        bad_request.field_violations().iter()
            .map(|violation| violation.field().to_str().unwrap_or_default().to_owned())
            .collect()
    })
}
```

---

<a id="lifecycle-metrics"></a>
## 5. Lifecycle Metrics and Bounded Labels

`Server::observer`, `Router::observer`, and `Channel::observer` expose
`LifecycleObserver` events for calls, attempts, queue delay, bytes, reconnects,
rejections, and cancellations.

Treat observer identity as untrusted input:

- Callbacks receive **raw** RPC paths and authorities.
- An inbound peer can supply arbitrarily many distinct values.
- Never use `CallLabels::path()`, `CallLabels::authority()`,
  `ReconnectEvent::target`, or the numeric attempt index directly as metric
  dimensions.

Classify raw call identity with a reviewed static allowlist:

```rust
use pbrs_grpc::{CallLabels, CallRole, MetricLabelPolicy, OTHER_METRIC_LABEL};

let policy = MetricLabelPolicy::new(&["/helloworld.Greeter/SayHello"], &[])
    .expect("reviewed method allowlist");
let raw = CallLabels::new(
    "/unknown.Service/Method123",
    Some("peer-supplied-authority"),
    CallRole::Server,
);
assert_eq!(policy.call(&raw).rpc(), OTHER_METRIC_LABEL);
```

For an exporter, implement `MetricSink::record(MetricEvent)` and register
`BoundedMetricObserver::new(policy, sink)` with `Server::observer`,
`Router::observer`, or `Channel::observer`. The
[compiled API example](../../pbrs-grpc/src/telemetry.rs) shows this adapter.

The bounded observer forwards only:

- allowlisted or `_other` RPC/target labels;
- a bounded initial/retry/invalid attempt class;
- status, rejection, and cancellation enums;
- durations and byte counts.

It never forwards raw status messages, authorities, metadata, payloads, or
diagnostic telemetry contexts. Direct `LifecycleObserver` implementations still
receive raw identity and require explicit classification before exporting.

Configuration rejects:

- more than 256 RPC paths;
- more than 16 reconnect targets;
- malformed paths;
- duplicate entries;
- labels over 256 bytes.

The policy itself has no exporter dependency or per-call label allocation.
Enabled observers may still copy identity across async lifecycles.

Server queue wait measures post-admission scheduling until the dispatch task
starts. It does **not** measure full listener or transport queue delay.
Pre-admission rejections have their own event. The optional `otel` feature
provides OpenTelemetry observers; the application supplies its SDK/exporter
configuration and installs those observers explicitly.

### Safe Diagnostic Formatting

Default `Debug` output is redacted. Use it first during incident triage.

| Type | Default redaction behavior |
|---|---|
| `Request`, `Response`, split `Parts` / `ResponseParts`, server `Rpc`, `TelemetryContext` | Mask unverified path and authority fields |
| `Response` and `ResponseParts` | Also mask received `grpc-encoding` text |
| `Request`, `Parts`, and `Rpc` | Also mask remote/local socket addresses, peer certificate identity, Unix peer credentials, untrusted scheme, and `grpc-encoding` text |
| `Request` and `Parts` | Also mask user-agent override |
| Peer fields | Keep presence visible as `Some("[REDACTED]")` |
| `Outgoing` | Keep the application-defined static RPC path; mask destination authority and user-agent |
| `Channel` | Mask authority, endpoint, and user-agent |
| `ConnectionInfo` | Mask peer addresses, certificate identity, Unix credentials, and scheme |
| `TelemetryContext` | Keep status **code** and metadata key names; mask all peer-supplied values, including `x-request-id`, `traceparent`, and the free-form status message |
| `Status::Debug` | Keep code, transport evidence, and value-redacted metadata; mask message and source error; report only the length of binary rich details |
| Metadata default formatting | Show at most 64 entries and 256 bytes per key name; never show a value |

Use explicit consent for controlled diagnostics:

- To reveal raw identity on a `Request`, `Parts`, `Response`, or
  `ResponseParts`, use `DiagnosticConfig::with_consent(true)` with
  `with_raw_identity(true)`.
- To apply that setting to an inbound RPC and its handler `Request` / split
  `Parts`, call `Rpc::set_diagnostic_config`.
- To include status text in a telemetry context, enable
  `with_status_message(true)` separately.
- To inspect a `Status` message without changing default `Debug`, format
  `status.diagnostic_debug(&config)` with both `with_consent(true)` and
  `with_status_message(true)`.
- To inspect metadata values, call `Metadata::safe_debug` explicitly.

Even in the controlled `Status` diagnostic view, binary details and the source
error stay hidden, and all metadata values stay masked, even if unclassified
metadata disclosure is separately permitted. Revealed identity and status text
are truncated on UTF-8 boundaries to `with_max_value_length` bytes, 256 by
default, plus a truncation marker. Certificate `Debug` reveals only a bounded
certificate count, never DER bytes.

Consent switches are independent:

- `with_sensitive_headers(true)` and `with_payload(true)` do not follow from
  consent to inspect peer identity.
- `Metadata::safe_debug` reveals byte-bounded unclassified ASCII values only
  with both `with_consent(true)` and `with_metadata_values(true)`.
- A peer can place secrets under *any* custom header name, so use metadata
  value disclosure only for controlled diagnostics.
- Inbound `user-agent`, credentials, and binary metadata remain redacted.
- Showing sensitive ASCII values also requires `with_sensitive_headers(true)`.
- Showing binary values requires both `with_sensitive_headers(true)` and
  `with_binary_metadata(true)`.
- Unclassified-value, sensitive-value, identity, and payload permissions are
  independent.

Invalid non-ASCII header values never print raw bytes. The opt-in view defaults
to 64 entries and 256 bytes per value. Raising those limits also accepts the
extra log-volume and disclosure risk.

Direct `Status::Display` and raw getters (`message()`, `details()`, and
`Error::source()`) remain application-controlled and may expose untrusted
content. `Channel`, `ConnectionInfo`, and `Outgoing` do not provide a consent
switch for their masked `Debug` fields; use explicit getters under your own
logging policy. Do not put credentials in status messages, and do not log raw
peer fields without explicit consent. The policy boundary is that Display and
raw getters stay application-controlled, while the kernel puts no peer
credential values into messages.

---

<a id="credential-refresh"></a>
## 6. Certificate and Trust Replacement

Do not edit PEM files and expect live TLS state to change. There is no live
certificate or certificate authority (CA) reload on `ServerTls`, `ClientTls`,
or an existing `Channel`.

The `rustls` configuration is built from the supplied identity and trust
material. Editing a PEM file later does not change that configuration. An
already negotiated TLS connection is not reverified. Use a bounded replacement
procedure instead:

1. Set health to `NOT_SERVING`, let upstream routing observe it, then drain and
   stop the old TLS listener with `serve_tls_with_shutdown`. A readiness update
   by itself does not reject new calls or close existing connections.
2. Obtain replacement material through an approved secret source; create a new
   `Identity` and `ServerTls::new` or `ServerTls::mtls`, and start a new listener.
   Never log the key, certificate bytes, peer metadata, or status message.
3. Construct fresh `ClientTls` and `Channel` instances with the new CA and, for
   mTLS, client identity. Retire old channels; they retain their original
   connector even when they reconnect. Do not assume the new policy applies to
   connections that were never closed.
4. Probe with an authorized new client, reject an untrusted CA and a missing
   client identity, then restore `SERVING`. If probes fail, keep readiness off
   and roll back the listener/credentials through the same drain path.

The [loopback policy-change test](../../pbrs-grpc/tests/tls.rs) exercises a
restart from ordinary TLS to mTLS: the old credential-less channel cannot
bypass the new policy on redial, while a newly constructed mTLS client works
across all four RPC shapes. This proves new-connection behavior with public
test fixtures, **not** live rotation of a server certificate or a production
rollout. The [production recipe](production-service.md) covers readiness,
drain and the fixture boundaries.

For incident triage, distinguish a dial failure (`UNAVAILABLE`), failed TLS
trust/client identity (`UNAUTHENTICATED`), protocol negotiation errors (which
can also surface as `UNAVAILABLE` or `INTERNAL`), deadline exhaustion
(`DEADLINE_EXCEEDED`), and admission overload (`RESOURCE_EXHAUSTED`). Use
redacted `Debug` and bounded observer labels from section 5; raw `Status`
messages and metadata are application-controlled and may contain secrets.
Keep reflection disabled on exposed production listeners unless access to
descriptors is separately authorized.

---

<a id="testing"></a>
## 7. Testing with In-Memory Channels (`from_io`)

Use `Channel::from_io` for deterministic service tests that should not open TCP
sockets or manage ports:

```rust recipe=from_io
use pbrs_grpc::{Channel, Request, Server, Status};
use pbrs_grpc_example_greeter::{GreeterClient, GreeterServer, HelloRequest, MyGreeter};

pub async fn greeter_in_process() -> Result<(), Status> {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);

    // Run server in background task
    let server = tokio::spawn(async move {
        Server::new(GreeterServer::new(MyGreeter))
            .serve_connection(server_io)
            .await
    });

    let result = async {
        let channel = Channel::from_io(client_io, "in-process").await?;
        let client = GreeterClient::new(channel);
        let mut req = HelloRequest::new();
        req.set_name("Test");
        let reply = client.say_hello(Request::new(req)).await?;
        assert_eq!(reply.get_ref().message().to_str().unwrap(), "hello Test");
        Ok(())
    }.await;
    // This one-connection fixture has no listener to drain; stop and join it.
    server.abort();
    let _ = server.await;
    result
}
```
