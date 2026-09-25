# Operations, Health Checking, and Diagnostics

This guide covers operational practices for operating, monitoring, and debugging `pbrs-grpc` microservices.

---

<a id="health-checks"></a>
## 1. Health Checking (`grpc.health.v1`)

`pbrs-grpc` includes built-in support for the official gRPC Health Checking protocol (`grpc.health.v1`).

### Mounting the Health Service
```rust
use pbrs_grpc::health::{HealthReporter, HealthServer, ServingStatus};
use pbrs_grpc::Router;

let (health_reporter, health_service) = HealthReporter::new();

// Set initial serving status
health_reporter.set_serving_status("", ServingStatus::Serving);
health_reporter.set_serving_status("hello.Greeter", ServingStatus::Serving);

let router = Router::new()
    .add_service(GreeterServer::new(MyGreeter))
    .add_service(health_service);

router.serve("0.0.0.0:50051").await?;
```

### Dynamic Status Updates
Update status during startup, shutdown, or health check probes:
```rust
// Mark a specific service unhealthy
health_reporter.set_serving_status("hello.Greeter", ServingStatus::NotServing);

// Mark the entire process not serving during graceful drain
health_reporter.shutdown();
```

Clients can execute unary `Check` calls or subscribe to long-lived streaming `Watch` calls. `Health::list` returns a snapshot of all registered service names.

---

<a id="reflection"></a>
## 2. Server Reflection (`grpc.reflection.v1`)

Enable server reflection so developer tools like `grpcurl` or Postman can dynamically inspect and call your services without manual `.proto` files:

```rust
use pbrs_grpc::reflection::ServerReflectionServer;

let reflection_service = ServerReflectionServer::new();

let router = Router::new()
    .add_service(GreeterServer::new(MyGreeter))
    .add_service(reflection_service);

router.serve("0.0.0.0:50051").await?;
```

### Inspecting with `grpcurl`
```bash
# List available services
grpcurl -plaintext 127.0.0.1:50051 list

# Describe a specific service
grpcurl -plaintext 127.0.0.1:50051 describe hello.Greeter

# Invoke an RPC directly
grpcurl -plaintext -d '{"name": "Ada"}' 127.0.0.1:50051 hello.Greeter/SayHello
```

Both `v1` and `v1alpha` paths are automatically served as aliases for compatibility with older tools.

---

<a id="keepalive"></a>
<a id="tuning"></a>
## 3. Keepalive and Socket Tuning

Network middleboxes (such as firewalls or NAT gateways) can terminate idle TCP connections. `pbrs-grpc` provides two separate keepalive mechanisms:

### HTTP/2 Protocol PINGs
Configured via `keep_alive_interval` and `keep_alive_timeout`:
- Sends HTTP/2 `PING` frames on idle connections.
- If the peer does not acknowledge within `keep_alive_timeout`, the connection is torn down and redialed.

```rust
use pbrs_grpc::ChannelConfig;
use std::time::Duration;

let config = ChannelConfig::new()
    .keep_alive_interval(Duration::from_secs(30))
    .keep_alive_timeout(Duration::from_secs(10));

let client = GreeterClient::connect_with("127.0.0.1:50051", config).await?;
```

### OS-Level TCP Keepalive
Configured on `ChannelConfig` or `ServerConfig`:
- `tcp_keepalive_interval`: Sets the `TCP_KEEPINTVL` interval between keepalive probes.
- `tcp_keepalive_retries`: Sets `TCP_KEEPCNT` (how many unacknowledged probes before the OS drops the socket).
- `TCP_NODELAY` is enabled by default to minimize RPC latency.

---

<a id="status-codes"></a>
## 4. Status Codes and Rich Error Details

### Standard Status Codes
`pbrs_grpc::Status` provides constructors for all standard gRPC status codes:
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
`pbrs-grpc` supports unpacking structured `google.rpc.Status` payloads from the `grpc-status-details-bin` trailer:

```rust
use pbrs_grpc::status::{BadRequest, ErrorDetails, ErrorInfo, FieldViolation, RetryInfo};
use std::time::Duration;

// Constructing rich error details on the server:
let mut details = ErrorDetails::new();
details.add_error_info(ErrorInfo::with_reason("RATE_LIMITED", "api.example.com"));
details.add_retry_info(RetryInfo::with_retry_delay(Duration::from_secs(5)));
details.add_bad_request(BadRequest::with_field("user_id", "must be positive"));

let status = Status::resource_exhausted("quota exceeded")
    .with_error_details(details);
```

On the client side:
```rust
match client.say_hello(req).await {
    Ok(resp) => { /* ... */ }
    Err(status) => {
        if let Some(details) = status.error_details() {
            for bad_req in details.bad_request() {
                for violation in bad_req.field_violations() {
                    println!("Invalid field '{}': {}", violation.field(), violation.description());
                }
            }
        }
    }
}
```

---

<a id="lifecycle-metrics"></a>
## 5. Lifecycle Metrics and Bounded Labels

`Server::observer`, `Router::observer`, and `Channel::observer` expose
`LifecycleObserver` events for calls, attempts, queue delay, bytes, reconnects,
rejections, and cancellations. Callbacks receive **raw** RPC paths and
authorities: an inbound peer can supply arbitrarily many distinct values.
Never use `CallLabels::path()`, `CallLabels::authority()`,
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
It forwards only allowlisted or `_other` RPC/target labels, a bounded
initial/retry/invalid attempt class, status/rejection/cancellation enums,
durations and byte counts. It never forwards raw status messages,
authorities, metadata, payloads, or diagnostic telemetry contexts. Direct
`LifecycleObserver` implementations still receive raw identity and require
explicit classification before exporting. Configuration rejects more than
256 RPC paths or 16 reconnect targets, malformed paths, duplicate entries,
and labels over 256 bytes. The policy itself has no exporter dependency or
per-call label allocation; enabled observers may still copy identity across
async lifecycles.
Server queue wait measures post-admission scheduling until the dispatch task
starts, **not** full listener or transport queue delay; pre-admission
rejections have their own event. OpenTelemetry export is not built in.

### Safe Diagnostic Formatting

Default `Debug` formatting for `Request`, `Response`, their split `Parts` and
`ResponseParts`, server `Rpc`, and `TelemetryContext` masks unverified
path/authority fields. `Response` and `ResponseParts` also mask the received
`grpc-encoding` text.
`Request`, `Parts`, and `Rpc` also mask remote/local socket addresses, peer
certificate identity, Unix peer credentials, and untrusted scheme and
`grpc-encoding` text. `Request` and `Parts` mask their user-agent override.
The presence of a peer field remains visible as `Some("[REDACTED]")`.
`Outgoing` retains its application-defined static RPC path but masks the
destination authority and user-agent. `Channel` masks its authority, endpoint,
and user-agent; `ConnectionInfo` masks peer addresses, certificate identity,
Unix credentials, and scheme. A telemetry context keeps the status **code**
and metadata key names visible while masking all peer-supplied values,
including `x-request-id` and `traceparent`, and the free-form status message.
Direct `Status::Debug` also retains the code, transport evidence, and
value-redacted metadata, but masks the message and source error and
reports only the length of binary rich details. Default metadata formatting
shows at most 64 entries and 256 bytes per key name, never a value.

For a controlled diagnostic, use `DiagnosticConfig::with_consent(true)` with
`with_raw_identity(true)` on a `Request`, `Parts`, `Response`, or
`ResponseParts`, or call
`Rpc::set_diagnostic_config` on an inbound RPC (the setting carries to its
handler `Request` and split `Parts`). For status text in a telemetry context,
enable `with_status_message(true)` separately. To inspect a `Status` message
without changing its default `Debug`, format
`status.diagnostic_debug(&config)`, where `config` has both
`with_consent(true)` and `with_status_message(true)`. That view still hides
binary details and the source error and keeps all metadata values masked,
even if unclassified metadata disclosure is separately permitted. Inspect
`Metadata::safe_debug` explicitly for a controlled metadata diagnostic.
Revealed identity and status text is truncated on UTF-8 boundaries to
`with_max_value_length` bytes (256 by default) plus a truncation marker.
Certificate `Debug` reveals only a bounded certificate count, never DER
bytes. These switches are independent of `with_sensitive_headers(true)` and
`with_payload(true)`: consent to inspect peer details must not also reveal
metadata credentials or payloads.

Direct `Status::Display` and raw getters (`message()`, `details()`, and
`Error::source()`) remain application-controlled and may expose untrusted
content. `Channel`, `ConnectionInfo`, and `Outgoing` do not provide a consent
switch for their masked Debug fields; use explicit getters under your own
logging policy. `Metadata::safe_debug` reveals byte-bounded unclassified
ASCII values only with both `with_consent(true)` and
`with_metadata_values(true)`; a peer can place secrets under *any* custom
header name, so use that switch only for controlled diagnostics. Inbound
`user-agent`, credentials and binary metadata remain redacted: showing
sensitive ASCII values additionally requires `with_sensitive_headers(true)`,
and binary values require both that switch and `with_binary_metadata(true)`.
Unclassified-value, sensitive-value, identity and payload permissions are
independent. Invalid non-ASCII header values never print raw bytes. The opt-in
view defaults to 64 entries and 256 bytes per value; callers who raise those
limits also accept the resulting log-volume and disclosure risk.
Do not put credentials in status messages or log raw peer fields without one;
OB-03 remains open.

---

<a id="credential-refresh"></a>
## 6. Certificate and Trust Replacement

There is no live certificate or CA reload on `ServerTls`, `ClientTls`, or an
existing `Channel`. The rustls configuration is built from the supplied
identity/trust material; editing a PEM file later does not change that
configuration, and an already negotiated TLS connection is not reverified.
Use a bounded replacement procedure instead:

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

To test services deterministically without opening TCP sockets or managing ports:

```rust
#[tokio::test]
async fn test_greeter_in_process() {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);

    // Run server in background task
    tokio::spawn(async move {
        Server::new(GreeterServer::new(MyGreeter))
            .serve_connection(server_io)
            .await
            .unwrap();
    });

    // Connect client directly to the in-memory duplex stream
    let channel = Channel::from_io(client_io);
    let client = GreeterClient::new(channel);

    let mut req = HelloRequest::new();
    req.set_name("Test");
    let reply = client.say_hello(Request::new(req)).await.unwrap();
    assert_eq!(reply.get_ref().message(), "Hello, Test!");
}
```
