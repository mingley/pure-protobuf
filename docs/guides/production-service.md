# Production Service Configuration and Lifecycle

This is a **bounded, loopback-only teaching recipe**, not a deployment preset.
The executable [greeter TLS service and client](../../examples/greeter/src/production.rs)
use generated stubs, `rustls` TLS, health and a finite drain. Its
[binary entry point](../../examples/greeter/src/main.rs) runs the service and
client in one process; the original no-argument `cargo run` still prints
`hello world`. The [onboarding consumer](../../tests/onboarding.rs) compiles
the same recipe from a fresh crate and asserts the outcomes below. The guide
links to this tested source instead of maintaining separate Rust snippets.

<a id="tls"></a>
## 1. Run the TLS and mTLS recipes

From the repository root, with the existing Rust dependencies and `protoc`
available, run the two **local-fixture-only** exercises:

```bash
cargo run --offline -p pbrs-grpc-example-greeter -- --tls-demo pbrs-grpc/tests/tls_data
cargo run --offline -p pbrs-grpc-example-greeter -- --mtls-demo pbrs-grpc/tests/tls_data
```

The respective output is `[Tls] overload, readiness and bounded drain
verified` or `[Mtls] overload, readiness and bounded drain verified`; failures
exit nonzero. The fixture directory contains **public test credentials**,
including `server.key` and `client.key`. They are read from their original
paths only when running the demo, never included in the binary, copied into
the consumer, or printed. Do not reuse the keys, CA, or `localhost` identity
for a deployment.

The [actual constructors](../../examples/greeter/src/production.rs) use
`Identity::from_pem`, `ServerTls::new(identity)` for TLS or
`ServerTls::mtls(identity, client_ca_pem)` for mTLS, and
`ClientTls::ca("localhost", ca_pem)` or `ClientTls::ca_mtls` on the client.
The client verifies the certificate's `localhost` name independently of its
loopback TCP address; TLS requires ALPN `h2`, and certificate verification
cannot be disabled. The exercise confirms a wrong CA fails in TLS mode and
missing client identity fails in mTLS mode with `UNAUTHENTICATED`.

**Production trust** is a separate operational decision: supply a
maintained CA and server identity for the real DNS name, restrict access to
private keys, and define issuance, renewal and connection-restart procedures.
This sample has no live certificate-rotation or secret-provisioning API.
**Authentication** in mTLS verifies possession of a CA-issued client
certificate; in ordinary TLS no client certificate is required.
**Authorization** still requires an application policy mapping verified
`Request::peer_identity` / `Rpc::peer_identity` (DER certificate chain) to
per-method permissions. Neither trusting a CA nor exposing a certificate
automatically authorizes its holder. Keep the service on loopback unless that
policy and network exposure have been reviewed.

<a id="deadlines"></a>
<a id="timeouts"></a>
<a id="wait-for-ready"></a>
## 2. Bound connections, bytes, streams and time

The [server and client configurations](../../examples/greeter/src/production.rs)
set these explicit **example** limits; size them against the
[resource-budget model](../resource-budgets.md) and your actual load before
deployment:

| Budget | Server | Client |
|---|---|---|
| Connections and RPCs | 4 concurrent connections; 1 RPC process-wide; 4 HTTP/2 streams per connection | 1 pooled connection; 4 concurrent RPCs |
| Transport and messages | 64 KiB process-wide transport byte budget; 1 KiB inbound/outbound uncompressed message caps | 1 KiB inbound/outbound uncompressed message caps |
| Flow control and metadata | 64 KiB connection / 32 KiB stream windows; 32 KiB send buffer; 4 KiB header list; 1 KiB HPACK table | Same windows, send buffer, header list and HPACK table |
| Application streams | At most 4 upload/bidi names, 128 bytes each; 3 download replies, channel capacity 2 | 2-message sender buffer; reads terminate at the declared counts and final status |
| Handshake and RPC | 2 s per TLS/HTTP2 handshake stage; server RPC cap 6 s | 3 s whole dial; 5 s default RPC cap; explicit 1–2 s call timeouts |
| Lifecycle | 10 s idle connection limit; 250 ms in-flight drain grace | No wait-for-ready queue; failures are explicit |

`Request::set_timeout` supplies `grpc-timeout` on individual calls; the
client/channel and server timeout overlays also bound calls that omit it.
`ChannelConfig::connect_timeout` bounds dialing, **not** an RPC's handler.
The demo leaves `wait_for_ready` off; enabling it without a finite deadline
can queue indefinitely. With one process-wide RPC slot, an in-flight upload
can also reject a health probe as `RESOURCE_EXHAUSTED`. Real services must
budget probe capacity separately rather than treating this tiny test cap as
a universal production value. These are application and transport caps, **not
a claim of a strict process RSS ceiling**: TLS, socket buffers and unrelated
application work require separate budgets.

<a id="graceful-shutdown"></a>
<a id="connection-age"></a>
## 3. Readiness, overload and graceful drain

The router mounts the generated greeter and `health::service()` on the same
TLS listener, advertises `SERVING` once bound, and verifies `HealthClient::check`
over TLS. `ProductionLive::mark_not_ready()` calls
`HealthReporter::shutdown()`, making both the named service and process `""`
`NOT_SERVING`; that is **a readiness signal**, not an admission firewall.
Wait for the environment's load balancer to observe the change before
triggering shutdown in a real deployment.

The demo keeps one upload active, proves the next unary call is rejected by
the **server** with `RESOURCE_EXHAUSTED: too many concurrent RPCs`, completes
the upload and proves a subsequent call succeeds. It then marks readiness
false, leaves an upload in flight and calls `ProductionLive::shutdown()`.
`Router::serve_tls_with_shutdown` stops accepting, sends HTTP/2 GOAWAY and
allows existing requests to finish for at most `max_connection_age_grace`
(250 ms here) before force-closing. The demo asserts termination inside 2 s,
rejected new connections, zero live application stream tasks and zero tracked
server transport bytes. Always await `shutdown()` and handle its `Result`;
dropping the handle instead aborts its server task, not a graceful drain.

`ServerConfig::max_connection_idle` is 10 s here; the example does not set
`max_connection_age`. If you opt into an age limit, connection aging and
GOAWAY are separate from application readiness and share the configured grace
policy. Deadlines also bound unending streams, but neither a deadline nor
GOAWAY automatically cancels unrelated tasks spawned by your application.

<a id="router"></a>
## 4. Routing and reflection exposure

The TLS recipe deliberately mounts **greeter plus health, not reflection**.
The original plaintext [greeter service](../../examples/greeter/src/lib.rs)
mounts reflection for local learning. Enabling reflection on a reachable
endpoint exposes service names and protobuf descriptors; decide who may
discover them and enforce that policy independently of TLS/mTLS. An unmatched
service returns `UNIMPLEMENTED`.

<a id="unix-sockets"></a>
<a id="in-process"></a>
## 5. Other transports

Unix sockets (`serve_unix_unlink` / `connect_unix`) and in-process
`serve_connection` / `Channel::from_io` are separate, **non-TLS** paths;
see the [native lifecycle tests](../../pbrs-grpc/tests/lifecycle.rs).
Do not treat filesystem permissions or Unix peer credentials as equivalent
to the certificate and authorization policy above.

<a id="compression"></a>
## 6. Compression

Inbound gzip is accepted by default; outbound gzip is opt-in through
`ServerConfig::send_compressed(true)` or
`ChannelConfig::send_compressed(true)`. The demo leaves outbound compression
off, and its uncompressed message caps still apply if gzip is enabled. See
the [native TLS/compression tests](../../pbrs-grpc/tests/tls.rs) for every RPC
shape.
