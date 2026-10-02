# TC-30a: unchanged tonic-generated servers on the native kernel

TC-30a adds `pbrs_grpc::tonic_server::{TonicServer, TonicServerExt}` behind
the existing opt-in `tonic` feature. An ordinary tonic-generated named
server can use `.into_pbrs_service()` and join `Server` or a mixed native
`Router`. `.layer(layer)` retains its generated routing name while accepting
ordinary Tower layers whose services do not implement `NamedService`.
Generated interceptors and their response-body wrappers are supported.
No shipping or fixture manifest, lockfile, dependency version, generated
client/server source, crypto provider or default feature is changed.

## Kernel ownership and opaque policies

The adapter hands HTTP/2 DATA `Bytes` and trailers directly between the
existing native transport and the tonic service. It does not decode
protobuf, collect a body or create a second HTTP/2 connection. Native
transport validation, TLS, inbound authentication, connection settings,
RPC concurrency permits and graceful draining remain in the kernel.
Post-interceptor user metadata replaces the original user headers, so an
authentication interceptor's removed authorization cannot be resurrected.
Kernel-stamped extensions are retained.

One effective native deadline covers Tower readiness, the handler and the
response body. Pre-header reset drops pending readiness/handler work;
post-header reset drops the response producer. A cancellation watch remains
owned by incoming-body readers, including a detached reader after the
handler returns. Native deadlines and the response's cancellation guard wake
that reader and stop its open request upload. Response-body errors retain
tonic status, public details and metadata. Terminal status determines native
channelz success; a missing status is not counted as success.

Tonic owns opaque framing, protobuf codecs, its default 4 MiB decode limit,
outbound message limits and configured gzip. Nondefault native message or
compression policies, finite native byte budgets, response hooks, matching
binary logging, lifecycle observers and grpc-web fail with
`FAILED_PRECONDITION` before Tower readiness or business dispatch. A native
send-buffer setter that implicitly installs a finite byte budget is also
rejected. Default native message/compression settings are not a second codec
policy over tonic. **TC-30b remains open** for full native opaque-body policy
enforcement; this functional adapter does not satisfy that work.

## Actual connection information

Built-in native TCP acceptance stamps tonic's actual TCP connect-info type
alongside the native type. At the single successful, verified native TLS
handshake, the acceptor retains tonic's `Connected::connect_info` from the
actual accepted TLS stream. The original native identity/address extensions
remain present; the retained tonic certificate `Arc` is shared across RPCs.
No second handshake or fabricated private tonic TLS value is involved.

On Unix, the existing accepted socket provides tonic's exact `UdsConnectInfo`
and Tokio credential value. Native portable credentials and peer addresses
remain present. Additive feature-gated `ConnectionInfo::with_tonic_tls` and
`with_tonic_uds` let a custom `Incoming::peer` retain information captured from
its actual accepted stream while preserving its native fields. The custom
acceptor still supplies native fields and performs its own TLS verification.
Native certificate/credential facts alone cannot manufacture tonic's private
TLS or credential values. `serve_connection` deliberately supplies no peer
facts; custom acceptors that have them use `serve_with_incoming`.

## Performance boundary

The leaf forwards existing DATA without payload copies. It adds a service
clone, cancellation watch, incoming-body cancellation future and normal
tonic body boxing. Tonic's codecs are unchanged: upstream tonic 0.14.6 still
copies received DATA into its `BytesMut` decoding buffer, and compression
requires its own processing. These source facts do not establish a latency,
allocation or throughput improvement. **SB-32 remains open** for the frozen
unmodified-codegen transport matrix; the existing handwritten native-prost
profiles do not qualify this adapter. Parent TC-30 requires TC-30b and that
performance qualification as well.

## Validation

The source commit is `d2a7bc467c31b68c2647d9343258a7fa509b65db`, based on
`1fc4c855dbd254098c385fc96d46e9b270b78e3f`. Each gate's exercised source/test
bytes match that commit. The unchanged TC-22 RouteGuide generator pipeline
provides unary, server-streaming, client-streaming and bidirectional services.
All four execute with complete response-value checks, native authentication,
generated interceptors, an ordinary tonic Tower interceptor layer and gzip
through both original tonic and native client transports. The generated
consumer also proves mixed native routing, codec limits, mTLS certificate
identity, real Unix UID/GID/PID authentication, a custom Unix `Incoming`,
deadlines, cancellation and graceful drain of an active bidi RPC.

| Gate | Local result |
|---|---|
| New native server transport tests | 12 passed |
| TC-29 client transport regression tests | 9 passed |
| Standalone unchanged-generated consumer suite | 26 passed: 3 existing main, 4 TC-22, 8 TC-29 and 11 TC-30a |
| Native hostile/retry/Tower/TLS/connect-info regressions | 97 passed: connect-info 4, hostile 40, retry 18, TLS 26, tonic server 2, Tower client 7 |
| Strict scoped native and standalone all-target Clippy | Passed with `-D warnings` |
| Strict opt-in tonic rustdoc | Passed with `RUSTDOCFLAGS=-Dwarnings` |
| Feature-off compile and normal dependency graph | Passed; tree excludes tonic, Tower and hyper |
| Scoped formatting and whitespace | Passed |

These are local correctness and compatibility gates, not whole-workspace CI,
minimum-supported-Rust qualification, a production soak or performance
evidence. Raw commands, UTC times, results, source hashes, tool pins and full
stdout/stderr are retained in [validation.json](tc-30a/validation.json) and
the neighboring logs. Independent read-only peer reviews by GN-12 and the
resource-review lane found no blocking source issue in the final bounded
leaf, context seams or fixtures; live execution is recorded separately.
