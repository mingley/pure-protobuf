# gRFC Coverage

This matrix tracks implementation coverage for gRPC Request for Comments
(gRFCs). It is a feature inventory, not a claim that every upstream
qualification suite has passed. See [project status](status.md) for those
gates. Language-specific (`L`) and process (`P`) proposals are out of scope.

The matrix covers cross-language (`A`) and protocol-level (`G`) proposals in
[grpc/proposal](https://github.com/grpc/proposal). Status values:

- **shipped:** implemented and tested;
- **partial:** a subset ships;
- **in progress:** committed work is underway;
- **planned:** accepted, not started;
- **boundary:** deliberately not in the kernel, with the reason recorded.

## Core RPC behavior

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A6 | Client retries | partial | Transparent retry, unary policy retry/hedging, pushback, and pre-commit server-streaming policy retry ship. Client-streaming/bidi require call-site retries; streaming throttling accounting is approximate. See the [retry contract](retry-contract.md) and `tests/policy_retry.rs`. |
| A8 | Client-side keepalive | shipped | `keep_alive_interval` / `keep_alive_timeout`, idle PINGs. |
| A9 | Server-side connection management | shipped | `max_connection_age`/`idle`, GOAWAY drain, `serve_with_shutdown`. |
| A15 | Promote reflection | shipped | `grpc.reflection.v1` server. |
| A17 | Client-side health checking | shipped | Server `Check`/`Watch`/`HealthReporter` plus client gating: per-subchannel same-conn Watch, unhealthy skips rotation (RR) / fails over (PF), UNIMPLEMENTED treated healthy, service-config opt-in with channel switch (CH-05). No channel-trace hookup yet. |
| A90 | Health `List` method | shipped | `Health::list`. |
| A18 | TCP user timeout | shipped | `PBRS_TCP_USER_TIMEOUT_MS` applied on every dial via Linux-only raw `TCP_USER_TIMEOUT` setsockopt (`proxy::set_user_timeout`; socket2 exposes no API); round-trip unit test runs on Linux, no-op elsewhere (CH-09). `ChannelConfig` surface is a follow-up. |
| A61 | IPv4/IPv6 dualstack backends | shipped | Hostname resolution plus Happy-Eyeballs racing: 250ms-staggered full dials, fast-failure advance, first READY wins, family interleave after shuffle (CH-04). No RFC-6724 sort in the DNS resolver; no channel-arg delay knob. |
| A101 | SNI setting and SNI/SAN validation | partial | `ClientTls` constructors take an explicit server name for SNI and certificate verification, independent of the dial address. xDS-driven name handling is not implemented. |
| A105 | `max_concurrent_streams` connection scaling | planned | Grow the pool when the server lowers the stream cap. |
| G1 | True binary metadata | shipped | `-bin` values base64 on the wire, padded/unpadded accepted; official `custom_metadata` passes both directions against grpc-go and pinned C++ (`grpc/grpc@d1487957`, CH-09). |

## Name resolution, balancing, routing

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A2 | Service configs in DNS | shipped | `_grpc_config.` TXT service config in the DNS resolver (UDP, single nameserver, no TCP fallback; failures/empty keep last config). |
| A10 | Avoid grpclb/service-config for localhost and IP literals | shipped | Literals resolve statically without DNS; localhost and literals skip TXT (`skips_txt_lookup`). |
| A21 | Service-config error handling | shipped | Eager validation; invalid initial TXT fails the channel, invalid updates keep the last good document. |
| A24 | LB policy config | shipped | First-registered-wins `loadBalancingConfig` selection via `LbPolicyRegistry`; `pick_first` (FL-03), `round_robin` (FL-04), `weighted_round_robin` (CH-06) registered. |
| A62 | pick_first | shipped | Sticky first-ready selection, in-order TF failover, shuffleAddressList, 1s×1.6^r±20%/120s-cap backoff (`lb/pick_first.rs`, FL-03). |
| A113 | pick_first weighted shuffling | shipped | Efraimidis–Spirakis `u^(1/weight)` sort under `shuffleAddressList` with per-endpoint weights defaulting to 1 (CH-04). CDS-side normalized weight computation arrives with xDS. |
| round_robin | (core policy, no gRFC number) | shipped | Strict rotation over ready endpoints, per-address backoff, graceful drain on removal, and optional client-side health gating (`lb/round_robin.rs`, CH-05). |
| A58 | Client-side weighted round robin | shipped | EDF scheduler over ORCA weights (`lb/wrr.rs`, CH-06): UpdateWeight/GetWeight with blackout/expiration, lazy rebuilds (period/ready-set/weight-move), <2 weighted degrades to RR, error penalty, health gating; per-call ingestion in unary/hedged loops, OOB pump per subchannel (UNIMPLEMENTED stops silently). WRR config snapshots at channel build; streaming per-call ingestion deferred to OOB. |
| A114 | WRR metric names for computing utilization | shipped | `metricNamesForComputingUtilization` parsed + max-over-hits selection with A58 app-then-cpu fallback (`orca::utilization`, CH-06). |
| A42/A76 | Ring hash LB policy | shipped | `lb/ring_hash.rs` (CH-07): vendored seeded XXH64 (Go-verified vectors), grpc-go-identical ring build (`key_idx` entries, normalized scale, sorted), hash walk with failover locality, `requestHashHeader` (validated, `-bin` rejected) with random-hash fallback, fail-fast without a hash source, health gating + backoff. `ring_hash_experimental` alias normalized at parse. Endpoint weights all 1 (xDS attributes lane). |
| A56 | Priority LB policy | shipped | `lb/priority.rs` (CH-08): lazy named children with named-slot decoupling, P0-first/pending-hold/CONNECTING/last-child selection, 10s failover + 15m deactivation timers evaluated lazily (no background tasks), atomic config updates, flat resolver updates feed P0 (per-priority membership via `update_priorities` until resolvers carry hierarchy), sync hash-header derivation for ring children, full signal delegation. E2E: `tests/lb_priority.rs` (serving, dead-skip, ring affinity); exact timelines in fake-clock unit tests. |
| A115 | Remove priority-LB child-policy cache | shipped | No retention cache: children removed from the config destroy immediately in `update_config` (CH-08); kept children with changed policy lists rebuild lazily. |
| A68 | Random subsetting | shipped | `lb/subset.rs` (CH-07): seeded-XXH64 rendezvous top-N (grpc-go construction), first-registered child via shared `instantiate()`, full delegation (pick/hash/health/ORCA/OOB/load-track); nesting parse-rejected. |
| A5/A26 | grpclb in DNS / selection | boundary | Superseded by xDS; not implemented. |

## Proxies and transports

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A1 | HTTP CONNECT proxy support | shipped | Env-driven CONNECT tunneling (`HTTPS_PROXY`/`NO_PROXY` with uppercase-wins, `*`/suffix/IP/CIDR bypass, basic auth, TLS end-to-end above the tunnel) consulted on every dial (`proxy.rs`, CH-09). `tests/proxy.rs` 7/7. No per-channel config surface yet. |
| A86 | xDS HTTP CONNECT | planned | With the xDS client. |
| G2 | HTTP/3 protocol | boundary | QUIC transport is a separate transport project, not this kernel. |

## Retries, hedging, overload (client policy)

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A6 hedging | Hedging (part of A6) | shipped | Unary `hedgingPolicy` ships: bounded by `maxAttempts`, opt-in per method, first OK/fatal commits, non-fatal waits (`tests/policy_retry.rs`). Streaming hedging is not applicable (grpc-go is unary-only too). |
| A44 | xDS retry | planned | With the xDS client (route-level retry policy). |
| A45 | Retry stats | planned | Per-call retry attempt counters. |
| A96 | Retry OTel stats | planned | With OTel metrics. |

## Observability

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A3 | Channel tracing | shipped | Bounded per-entity channel traces in the channelz registry; descriptions are capped and owner drop unregisters entities (`channelz::Trace`, `DEFAULT_MAX_TRACE_EVENTS`, `tests/channelz.rs`). |
| A14 | Channelz | shipped | Process-global registry for channels, subchannels, servers and sockets plus `grpc.channelz.v1.Channelz` service (`ChannelzService::shared_global`, `tests/channelz.rs`). |
| A16 | Binary logging | shipped | `binlog::{BinaryLogger, BinaryLogFilter, Sink}`; `Channel`/`Server`/`Router::binary_logger`; `{h;m}` caps; attempts share one call id; client peer + `grpc-trace-bin` omitted at taps. |
| A38 | Admin interface API | planned | Admin server exposing channelz/CSDS. |
| A40 | CSDS support | planned | With the xDS client. |
| A59 | Audit logging | shipped | `authz::{AuditEvent, AuditLogger, AuditLoggerFactory, StdoutAuditLogger}` + `register_audit_logger_factory`; `audit_logging_options` (NONE/ON_DENY/ON_ALLOW/ON_DENY_AND_ALLOW, `is_optional`); records are exactly the five A59 fields + timestamp, no metadata (OB-03). |
| A66 | OTel stats | shipped | Optional `otel` feature: `otel::Metrics` observer records client attempt started/duration, call duration, and server started/duration with method/target/status labels (GF-01, `tests/otel.rs`). |
| A72 | OpenTelemetry tracing | partial | W3C propagation both directions (`ClientTracing` inject / `ServerTracing` extract) + server spans with safe-by-default RPC attributes (GF-02, `tests/otel_trace.rs`). Automatic client spans need a per-call client completion hook; error-path spans end without a status code (GF-02b). |
| A78 | gRPC metrics for WRR/PF/xDS | partial | WRR hooks ship: `weights_snapshot` + `WrrStats` (accepted/ignored/rebuilds) for polling (CH-06). OTel instrument mapping rides the OTel bridge (A66); pick_first/xDS instruments with their lanes. |
| A79 | Non-per-call metrics architecture | planned | With the OTel bridge. |
| A80 | TCP telemetry | partial | Per-connection socket addrs + handshake timing ride the TLS handshake observer (`TcpStats` in `tls.rs`, GF-06). Kernel TCP_INFO (RTT/cwnd/retransmits) is a boundary: no safe API in `socket2` 0.6 and `tls.rs` is `forbid(unsafe_code)`. |
| A94 | Subchannel OTel metrics | partial | Connection-attempt counters ride `on_reconnect`, which fires for redials only; initial dials stay invisible until the pool gains dial hooks (GF-01b). |
| A96 | Retry OTel stats | planned | With retry stats. |
| A108 | OTel custom per-call labels | partial | Static channel-level attributes via `with_custom_attributes`; dynamic per-RPC values need a tags channel (GF-01b). |
| A118 | TLS telemetry | shipped | `ServerTls`/`ClientTls::with_handshake_observer` delivers `TlsHandshakeInfo` (version, suite, resumed, ALPN, peer cert count, duration) after every successful handshake; failures are not reported (GF-06, in-`tls.rs` tests). |

## Security

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A29 | xDS TLS security | planned | With the xDS client (SDS-delivered roots/identities). |
| A41 | xDS RBAC | planned | RBAC filter from xDS route config. |
| A43 | gRPC authorization API | shipped | `authz::{Policy, StaticDataProvider, FileWatcherProvider, AuthzInterceptor}`; `Server`/`Router::authorization_policy`; deny-first/default-deny, SAN/subject principals, header matching; denied calls are `PERMISSION_DENIED` without handler execution. |
| A65 | xDS mTLS creds in bootstrap | planned | With xDS bootstrap. |
| A69 | CRL enhancements | shipped | Fail-closed CRL checking on pinned-CA constructors (`mtls_with_crl`, `ca_with_crl`, `ca_mtls_with_crl`): full-chain status required, unknown status and expired CRLs reject; fixtures cross-checked with `openssl verify -crl_check` (GF-06, in-`tls.rs` tests). |
| A82 | xDS system root certs | planned | With xDS bootstrap. |
| A83 | xDS GCP authn filter | boundary | GCP-only; not in the portable kernel. |
| A87 | mTLS SPIFFE support | shipped | Exact-match SPIFFE ID constraint on the leaf URI SAN, checked after WebPKI verification (`mtls_spiffe`, `ca_spiffe`, `ca_mtls_spiffe`); can only reject more, never a skip-verify path (GF-06, in-`tls.rs` tests). |
| A97 | xDS JWT call creds | planned | JWT call credentials (also usable without xDS). |
| A107 | TLS private-key offloading | boundary | Requires HSM/key-provider integration; not portable. |
| A120 | Post-quantum cryptography | boundary | Pinned `rustls-graviola` 0.2.1 negotiates classical KX only (X25519/P-256/P-384); tracked in code by `post_quantum_key_exchange_available` with a provider-change tripwire test. Re-evaluate at `rustls-graviola` 0.4 (pure-Rust `X25519MLKEM768`, needs rustc 1.89 > MSRV 1.85). |
| ALTS | (no single gRFC; L126/L18x touch it) | boundary | Google-internal transport; JWT/mTLS cover portable auth. |

## Fault tolerance and traffic policy

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A31 | xDS timeout support and config selector | planned | With the xDS client. |
| A32 | xDS circuit breaking | planned | Cluster circuit breakers in the xDS balancer. |
| A33 | Fault injection | planned | Delay/abort injection as an opt-in filter. |
| A48 | xDS least-request LB | shipped | `lb/least_request.rs` (CH-07): choice_count sampling (parse: reject <2, clamp >10), least in-flight wins (first sampled breaks ties); RAII guards count unary attempts (abort-safe); streams untracked pending completion plumbing. `least_request_experimental` alias normalized at parse. |
| A50 | xDS outlier detection | shipped | `lb/outlier.rs` (CH-08): success-rate (mean−stdev·factor) + failure-percentage detectors over per-address call outcomes, ejection cap (floor, resolver-order trim), `base × total_ejections` backoff capped at `maxEjectionTime` (default max(300s, base)), lazy sweep/unejection on picks and observations, `try_lock` call reporting (never blocks; Cancelled dropped); dial failures feed only child backoff. Unary + hedged attempts report via `ingest_call_status` (streaming pending, same coverage as per-call ORCA). E2E: `tests/lb_priority.rs` (eject/uneject a failing backend); detectors pinned by fake-clock unit tests. |
| A52 | xDS custom LB policies | planned | Plugin registry for LB policies. |
| A53 | xDS ignore resource deletion | planned | With the xDS client. |
| A54 | Restrict control-plane status codes | planned | With the xDS client. |
| A55/A60 | xDS stateful session affinity | planned | With the xDS client. |
| A57 | xDS client failure-mode behavior | planned | With the xDS client. |
| A63 | xDS string matcher in header matching | planned | With xDS route matching. |
| A64/A85 | LRS custom metrics | planned | With xDS load reporting. |
| A71 | xDS fallback | planned | With the xDS client. |
| A74 | xDS config tears | planned | With the xDS client. |
| A75 | xDS aggregate-cluster behavior fixes | planned | With xDS clusters. |
| A81 | xDS authority rewriting | planned | With the xDS client. |
| A88 | xDS data error handling | planned | With the xDS client. |
| A89 | Backend service metric label | planned | With ORCA/LRS. |
| A91 | Outlier-detection metrics | shipped | `OutlierMetrics` hooks (CH-08): per-method enforced + unenforced (enforcement-roll / cap-overflow) counters, cumulative and never pruned; OTel export (names, units, target labels) lands with the OTel metrics lane. |
| A95 | xDS endpoint fallback | planned | With the xDS client. |

## xDS control plane

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A27 | xDS global load balancing | planned | Bootstrap + ADS + EDS in the xDS client. |
| A28 | xDS traffic splitting and routing | planned | LDS/RDS route table in the xDS client. |
| A30 | xDS v3 | planned | v3 API surface for the resources above. |
| A36 | xDS for servers | planned | Server-side xDS listener/filter-chain matching. |
| A37 | xDS aggregate and logical-DNS clusters | planned | With xDS clusters. |
| A39 | xDS HTTP filters | planned | Filter chain: RBAC, fault injection. |
| A46 | xDS NACK semantics improvement | planned | With the ADS client. |
| A47 | xDS federation | planned | With the xDS client. |
| A102 | xDS gRPC service | planned | With the xDS client. |
| A103 | xDS composite filter | planned | With xDS HTTP filters. |
| A110 | Child-channel plugins | planned | With the LB plugin registry. |

## Load reporting

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A51 | Custom backend metrics (ORCA) | shipped | `orca/` (CH-06): vendored v3 protos, `OrcaRecorder` (server/request + merge precedence), `OrcaResponseHook` per-call trailers on all reply shapes, builtin `OpenRcaService` (immediate snapshot + clamped interval + cost filtering), client decode + OOB pump. Wire-proven both directions against pinned grpc-go (byte-identical golden). One pump per address owned by WRR; cross-policy OOB subscription sharing arrives with xDS. |

## Out of scope by category

- `L*` proposals change another language's API or platform support; they do
  not apply to this Rust implementation.
- `P*` proposals change gRPC project process, not implementation behavior.
- A5/A26 (grpclb), A83 (GCP authn filter), A107 (key offloading), A120 (PQC),
  ALTS, and G2 (HTTP/3) are recorded above as explicit boundaries with reasons.

## Evidence

Rows move to **shipped** only with committed behavior tests and, where a peer
exists, cross-implementation evidence. The [task cards](plan/tasks.json)
(FL/EX/OB lanes) carry the per-feature acceptance criteria.
