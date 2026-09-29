# Production Service Configuration and Lifecycle

Use this guide to run the tested TLS and mutual TLS (mTLS) production-style
greeter recipe. You need the repository checkout, existing Rust dependencies,
`protoc`, and the public TLS fixtures already in this repo. Bottom line: this
is a **bounded, loopback-only teaching recipe**, not a deployment preset.

The executable [greeter TLS service and client](../../examples/greeter/src/production.rs)
uses generated stubs, `rustls` TLS, health, and a finite drain. Its
[binary entry point](../../examples/greeter/src/main.rs) runs the service and
client in one process; the original no-argument `cargo run` still prints
`hello world`. The [onboarding consumer](../../tests/onboarding.rs) compiles
the same recipe from a fresh crate and asserts the outcomes below. This guide
links to tested source instead of duplicating Rust snippets.

<a id="tls"></a>
## 1. Run the TLS and mTLS recipes

From the repository root, run the two **local-fixture-only** exercises:

```bash
cargo run --offline -p pbrs-grpc-example-greeter -- --tls-demo pbrs-grpc/tests/tls_data
cargo run --offline -p pbrs-grpc-example-greeter -- --mtls-demo pbrs-grpc/tests/tls_data
```

Expected output:

| Mode | Success output |
|---|---|
| TLS | `[Tls] overload, readiness and bounded drain verified` |
| mTLS | `[Mtls] overload, readiness and bounded drain verified` |

Failures exit nonzero. The fixture directory contains **public test
credentials**, including `server.key` and `client.key`. The demo reads them
from their original paths only while running. It never includes them in the
binary, copies them into the consumer, or prints them. Do not reuse the keys,
certificate authority (CA), or `localhost` identity for a deployment.

The [actual constructors](../../examples/greeter/src/production.rs) use:

- `Identity::from_pem`;
- `ServerTls::new(identity)` for TLS;
- `ServerTls::mtls(identity, client_ca_pem)` for mTLS;
- `ClientTls::ca("localhost", ca_pem)` or `ClientTls::ca_mtls` on the client.

`ServerTls::optional_mtls(identity, client_ca_pem)` is also available when a
listener should request, but not require, a verified client certificate. A
client that presents no certificate is allowed through with no peer identity;
a client that does present a certificate must chain to `client_ca_pem`.

The client verifies the certificate's `localhost` name independently of its
loopback TCP address. TLS requires Application-Layer Protocol Negotiation
(ALPN) `h2`; there is no tonic-style `assume_http2` mode that skips ALPN, and
certificate verification cannot be disabled. The exercise confirms that a
wrong CA fails in TLS mode and missing client identity fails in mTLS mode with
`UNAUTHENTICATED`.

Production trust is separate from this sample:

- Supply a maintained CA and server identity for the real Domain Name System
  (DNS) name.
- Enable the optional `native-roots` feature and use
  `ClientTls::native_roots` only when the operating system trust store is the
  reviewed trust policy for that client.
- Restrict access to private keys.
- Define issuance, renewal, and connection-restart procedures.
- Do not expect a live certificate-rotation or secret-provisioning API here.
- Use `ServerTls::key_log_file()` / `ClientTls::key_log_file()` only for local
  packet-decryption diagnostics with `SSLKEYLOGFILE`; key logs expose traffic
  secrets and must not be enabled in production.

mTLS authentication proves possession of a CA-issued client certificate.
Ordinary TLS requires no client certificate. Authorization still needs an
application policy that maps verified `Request::peer_identity` /
`Rpc::peer_identity` DER certificate chains to per-method permissions.
Trusting a CA or exposing a certificate does not automatically authorize its
holder. Keep the service on loopback unless that policy and network exposure
have been reviewed.

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

The demo proves this lifecycle:

1. Keep one upload active.
2. Verify the next unary call is rejected by the **server** with
   `RESOURCE_EXHAUSTED: too many concurrent RPCs`.
3. Complete the upload and verify a later call succeeds.
4. Mark readiness false.
5. Leave an upload in flight and call `ProductionLive::shutdown()`.

`Router::serve_tls_with_shutdown` stops accepting, sends HTTP/2 GOAWAY, and
allows existing requests to finish for at most `max_connection_age_grace`
(250 ms here) before force-closing. The demo asserts termination inside 2 s,
rejected new connections, zero live application stream tasks, and zero tracked
server transport bytes. Always await `shutdown()` and handle its `Result`.
Dropping the handle aborts the server task; it does not perform a graceful
drain.

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

Inbound gzip and deflate are accepted by default; optional zstd is available
with the `zstd` feature. Outbound compression is opt-in through
`ServerConfig::send_compressed(true)` or
`ChannelConfig::send_compressed(true)`. The demo leaves outbound compression
off, and its uncompressed message caps still apply if compression is enabled.
See the [native TLS/compression tests](../../pbrs-grpc/tests/tls.rs) for every
RPC shape.
