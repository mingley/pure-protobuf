# gRFC Coverage

This page tracks `pbrs-grpc` support for the cross-language (`A`) and
protocol-level (`G`) gRFCs in [grpc/proposal](https://github.com/grpc/proposal).
Language-specific (`L`) proposals for other languages and process (`P`)
proposals are out of scope by definition; the table below records that once
instead of repeating it per row.

Status values: **shipped** (implemented, tested), **partial** (subset shipped),
**in progress** (committed work underway), **planned** (accepted, not started),
**boundary** (deliberately not in the kernel, with the reason).

## Core RPC behavior

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A6 | Client retries | partial | Transparent retry ships ([retry contract](retry-contract.md)). Unary service-config `retryPolicy`, `retryThrottling`, `hedgingPolicy`, per-attempt timeouts, and server pushback ship (`Channel::service_config`, `tests/policy_retry.rs`). Server-streaming policy retry is next; client-streaming/bidi stay call-site retries (no replay buffer). |
| A8 | Client-side keepalive | shipped | `keep_alive_interval` / `keep_alive_timeout`, idle PINGs. |
| A9 | Server-side connection management | shipped | `max_connection_age`/`idle`, GOAWAY drain, `serve_with_shutdown`. |
| A15 | Promote reflection | shipped | `grpc.reflection.v1` server. |
| A17 | Client-side health checking | shipped | `grpc.health.v1` `Check`/`Watch`, `HealthReporter`. |
| A90 | Health `List` method | shipped | `Health::list`. |
| A18 | TCP user timeout | planned | `tcp_user_timeout` via socket2 on Linux. |
| A61 | IPv4/IPv6 dualstack backends | partial → in progress | Hostname resolution ships; Happy-Eyeballs racing in progress. |
| A101 | SNI setting and SNI/SAN validation | partial → in progress | rustls sends SNI from the name; explicit server-name override in progress. |
| A105 | `max_concurrent_streams` connection scaling | planned | Grow the pool when the server lowers the stream cap. |
| G1 | True binary metadata | planned | Interop-verified binary metadata handling. |

## Name resolution, balancing, routing

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A2 | Service configs in DNS | planned | TXT-record service config in the DNS resolver. |
| A10 | Avoid grpclb/service-config for localhost and IP literals | planned | Folded into the resolver: literals skip DNS-TXT lookup. |
| A21 | Service-config error handling | planned | With the JSON service-config parser. |
| A24 | LB policy config | planned | `loadBalancingConfig` selection in service config. |
| A62 | pick_first | planned | Default LB policy with sticky transient-failure handling. |
| A113 | pick_first weighted shuffling | planned | With pick_first. |
| round_robin | (core policy, no gRFC number) | planned | Per-subchannel ready-list rotation. |
| A58 | Client-side weighted round robin | planned | With ORCA utilization input (A114 names). |
| A114 | WRR metric names for computing utilization | planned | With WRR. |
| A42/A76 | Ring hash LB policy | planned | Request-hash ring with bounded state. |
| A56 | Priority LB policy | planned | Prioritized failover across localities. |
| A115 | Remove priority-LB child-policy cache | planned | With priority LB. |
| A68 | Random subsetting | planned | Bounded subset selector for large endpoint sets. |
| A5/A26 | grpclb in DNS / selection | boundary | Superseded by xDS; not implemented. |

## Proxies and transports

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A1 | HTTP CONNECT proxy support | planned | CONNECT tunneling, `HTTPS_PROXY`/`NO_PROXY` mapping. |
| A86 | xDS HTTP CONNECT | planned | With the xDS client. |
| G2 | HTTP/3 protocol | boundary | QUIC transport is a separate transport project, not this kernel. |

## Retries, hedging, overload (client policy)

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A6 hedging | Hedging (part of A6) | planned | Bounded, opt-in, idempotency-gated. |
| A44 | xDS retry | planned | With the xDS client (route-level retry policy). |
| A45 | Retry stats | planned | Per-call retry attempt counters. |
| A96 | Retry OTel stats | planned | With OTel metrics. |

## Observability

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A3 | Channel tracing | planned | Bounded in-memory channel trace API. |
| A14 | Channelz | planned | Channelz data model + `grpc.channelz.v1` service. |
| A16 | Binary logging | planned | `grpc.binarylog.v1` sink with size caps. |
| A38 | Admin interface API | planned | Admin server exposing channelz/CSDS. |
| A40 | CSDS support | planned | With the xDS client. |
| A59 | Audit logging | planned | Authz-decision audit sink. |
| A66 | OTel stats | planned | Optional `opentelemetry` metrics bridge. |
| A72 | OpenTelemetry tracing | planned | Optional OTel trace propagation + spans. |
| A78 | gRPC metrics for WRR/PF/xDS | planned | With WRR/pick_first/xDS metrics. |
| A79 | Non-per-call metrics architecture | planned | With the OTel bridge. |
| A80 | TCP telemetry | planned | TCP_INFO-based per-connection stats where available. |
| A94 | Subchannel OTel metrics | planned | With the OTel bridge. |
| A96 | Retry OTel stats | planned | With retry stats. |
| A108 | OTel custom per-call labels | planned | With the OTel bridge. |
| A118 | TLS telemetry | planned | Handshake/session telemetry hooks. |

## Security

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A29 | xDS TLS security | planned | With the xDS client (SDS-delivered roots/identities). |
| A41 | xDS RBAC | planned | RBAC filter from xDS route config. |
| A43 | gRPC authorization API | planned | Authz policy engine + server/client enforcement. |
| A65 | xDS mTLS creds in bootstrap | planned | With xDS bootstrap. |
| A69 | CRL enhancements | planned | CRL revocation checking in the TLS verifier. |
| A82 | xDS system root certs | planned | With xDS bootstrap. |
| A83 | xDS GCP authn filter | boundary | GCP-only; not in the portable kernel. |
| A87 | mTLS SPIFFE support | planned | SPIFFE ID constraint verification. |
| A97 | xDS JWT call creds | planned | JWT call credentials (also usable without xDS). |
| A107 | TLS private-key offloading | boundary | Requires HSM/key-provider integration; not portable. |
| A120 | Post-quantum cryptography | boundary | Blocked on a pure-Rust PQ TLS provider; re-evaluate when rustls/Graviola ships one. |
| ALTS | (no single gRFC; L126/L18x touch it) | boundary | Google-internal transport; JWT/mTLS cover portable auth. |

## Fault tolerance and traffic policy

| gRFC | Title | Status | Notes |
|---|---|---|---|
| A31 | xDS timeout support and config selector | planned | With the xDS client. |
| A32 | xDS circuit breaking | planned | Cluster circuit breakers in the xDS balancer. |
| A33 | Fault injection | planned | Delay/abort injection as an opt-in filter. |
| A48 | xDS least-request LB | planned | With the xDS client. |
| A50 | xDS outlier detection | planned | Success-rate ejection in the xDS balancer. |
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
| A91 | Outlier-detection metrics | planned | With outlier detection. |
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
| A51 | Custom backend metrics (ORCA) | planned | Per-RPC + out-of-band load reports, WRR input. |

## Out of scope by category

- `L*` proposals change another language's API or platform support; they do
  not apply to this Rust implementation.
- `P*` proposals change gRPC project process, not implementation behavior.
- A5/A26 (grpclb), A83 (GCP authn filter), A107 (key offloading), A120 (PQC),
  ALTS, and G2 (HTTP/3) are recorded above as explicit boundaries with reasons.

## Evidence

Each row moves to **shipped** only with committed behavior tests and, where a
peer exists, cross-implementation evidence. The [task cards](plan/tasks.json)
(FL/EX/OB lanes) carry the per-feature acceptance criteria.
