# xDS implementation contract

This is the proposed design for proxyless xDS; it is not an available feature.
The control plane publishes versioned configuration snapshots. Application
traffic continues from the last good snapshot when the control plane fails,
using separate connections and credentials.

**Contract version:** 0.1 (draft). **Status:** Draft under
[xDS build versus reuse](decisions/xds-build-vs-reuse.md) (XD-01,
proposed for maintainer review); no xDS code ships with this document.
**Baseline:** `2598aa12` on 2026-09-28. **Supersedes:** the scope
portion of EX-07. **Depends on:** [the channel
architecture](decisions/channel-architecture.md) (CH-01) for the
resolver, LB tree, picker, and subchannel seams; no xDS-only channel
path exists. Version 0.1 covers the client, server (A36), load
reporting, and admin surface; each XD card may extend this contract in
its own PR, bumping the minor version with a dated changelog entry at
the foot of this file.

## Identity separation

Four identifiers must never be conflated:

| Identifier | Source | May xDS change it? |
|---|---|---|
| Control-plane authority | Bootstrap `server_uri` (A27) | Only by re-bootstrap / fallback (A71, XD-03b). Never derived from a data-plane target. |
| Control-plane credentials | Bootstrap `channel_creds` (A27/A65) | Only by re-bootstrap. Data-plane call creds never authenticate ADS. |
| Data-plane `:authority` | The `xds:///` target (A28/A81 rules) | Only by LDS/RDS route update, never by the control-plane address. |
| Data-plane TLS identity | SDS / cert providers (A29) or channel config | Only by SDS update or provider rotation. |

Consequences: ADS runs on its own connection(s) with its own TLS
session and backoff; losing ADS never closes data-plane connections
(XD-03b failure modes, A57); data-plane RPCs never carry the
control-plane identity.

## Bootstrap (XD-03; A27, A65, A82)

- Sources in precedence order: programmatic config, `GRPC_XDS_BOOTSTRAP`
  file, `GRPC_XDS_BOOTSTRAP_CONFIG` inline JSON. Unknown top-level
  fields are ignored; unknown `channel_creds` types are skipped, and
  bootstrap fails only if no usable entry remains.
- Supported `channel_creds`: `tls` (A65 system roots via A82),
  `mtls`/`file-watcher` identity as XD-06 delivers, `insecure` for
  local fixtures only and never as a silent fallback (A29: no
  insecure fallback). GCP-only credential types are a boundary
  ([gRFC A83](grfc.md)).
- `node.id`, `cluster`, `locality`, and `metadata` (notably
  `GENERATOR = "grpc"`) are set from bootstrap; `user_agent_name`
  identifies `pbrs-grpc` with its version.
- Federation (A47), fallback client config (A71), and
  `server_features` arrive in XD-03b; v0.1 bootstrap targets one
  management server.

## ADS lifecycle (XD-03/XD-03b; A27, A30, A46, A53, A54, A57, A88)

- One ADS stream per management server (Aggregated Discovery Service,
  State of the World). Reconnect uses truncated exponential backoff
  with jitter on a bounded budget; reconnect re-sends subscriptions
  with the last ACKed version per type.
- **Version/nonce/ACK/NACK:** every response carries `version_info`
  and `nonce`. A fully accepted response is ACKed by echoing both in
  the next request for that type. A rejected response is NACKed with
  the last accepted `version_info`, the new `nonce`, and an
  `error_detail` naming the failure (A46). Decoding is two-phase
  (A46/A88): deserialize-then-validate, so a validation failure
  reports the resource name when it is known and a top-level error
  otherwise.
- **Stale behavior:** responses for a type older than the last
  accepted version are ignored without ACK/NACK. Data-plane
  snapshots are generationed: a failed update keeps serving the
  last good snapshot; in-flight RPCs are never migrated (CH-01
  rule 2).
- **Invalid behavior:** an invalid resource fails only its own
  watchers (resource error) unless the response is undecodable
  (top-level error, whole type NACKed). Unknown fields inside a
  known message are ignored; unknown `type_url` entries fail the
  response.
- **Removal behavior:** per A53, LDS/CDS responses always carry the
  full set, so an absent subscribed Listener/Cluster means deleted
  and its watchers fire resource-not-found; RDS/EDS allow partial
  responses, so absence keeps the cached RouteConfiguration/
  LoadAssignment. XD-03b adds the ignore-deletion option (A53).
- Control-plane status codes are restricted per A54; unexpected
  codes surface as ambient errors that keep the cached config
  (A57 failure-mode behavior, XD-03b).
- **v3 only** (A30). The `v2` API is not implemented and its type
  URLs are rejected.

## Resources and routing

### Supported resource types (v3)

| Type | Delivers | Card |
|---|---|---|
| `Listener` (LDS) | Route table inline or RDS name, filter chain (A39), fault injection (A33), RBAC (A41) | XD-04, XD-06b |
| `RouteConfiguration` (RDS) | Path/header matchers (A63), weighted clusters, timeouts (A31), retries (A44), authority rewrite (A81) | XD-04 |
| `Cluster` (CDS) | LB policy + config (A52), circuit breakers (A32), outlier detection (A50), aggregate / logical-DNS (A37, A75) | XD-05, XD-05b, XD-05c |
| `ClusterLoadAssignment` (EDS) | Localities, endpoints, priorities (A56), endpoint fallback (A95) | XD-05, XD-05b |
| Discovery / ADS envelopes, `Node`, `Any` packing | ADS framing (A27/A30) | XD-03 |
| SDS secrets, cert-provider config | TLS roots/identities (A29), SPIFFE (A87), SNI/SAN (A101) | XD-06 |
| LRS load reports | Client load reporting (A64/A85), backend labels (A89) | XD-08 |
| CSDS dumps | Config status (A40) behind the admin server (A38) | XD-09 |
| HTTP CONNECT / `GrpcService` / child-channel options | Management-server transport options (A86/A102/A110) | XD-12 |

XD-02 vendors exactly the protos needed for these rows plus their
compile closure, generated by `pbrs` codegen (the ORCA pattern).

### Explicitly unsupported (boundaries, not gaps)

- v2 type URLs (A30); grpclb (A5/A26); GCP authn filter (A83);
  key offloading (A107); PQC (A120); ALTS; HTTP/3 (G2) — all
  recorded as boundaries in [the gRFC matrix](grfc.md).
- Envoy-only resources outside the gRPC xDS feature set (proxy
  filters, ext_authz, rate-limit configs): never subscribed to; if
  one arrives inside a subscribed type it fails the response per
  the invalid rules above.
- A subset implementation must never be labeled full xDS (EX-07).

### Routing and clusters (XD-04/XD-05; A28, A31, A37, A44, A56, A63, A74, A75, A81)

- Route selection runs per RPC before the LB pick: longest-prefix
  path match, header matchers with A63 string semantics, weighted
  cluster split, then per-cluster config (timeouts, retry policy,
  hash policies for ring hash). Config tears (A74) apply
  atomically: a route-table update swaps whole generations, never
  half a table.
- Clusters map onto the CH-01 tree: one child policy per
  cluster/endpoint-group, EDS locality+priority driving the
  shipped `priority` policy (A56), endpoint health from CH-05.
- Reused CH-lane policies (already shipped, wired by XD-05/XD-05b/
  XD-05c, not reimplemented): `pick_first` (A62/A113),
  `round_robin`, ring hash (A42/A76), least request (A48), random
  subsetting (A68), WRR+ORCA (A58/A114), priority (A56/A115),
  outlier detection (A50/A91).
- Session affinity (A55/A60) and custom LB config (A52) land in
  XD-05c; circuit breaking (A32) and endpoint fallback (A95) in
  XD-05b.

### Security (XD-06/XD-06b/XD-06c; A29, A33, A39, A41, A65, A82, A87, A97, A101, A103)

- SDS-delivered roots and identities rotate without dropping
  data-plane connections; rotation failures keep the last good
  credentials and emit ambient errors. File-watcher providers
  bound reload frequency.
- RBAC (A41) denies as `PERMISSION_DENIED` before handler
  execution, reusing the shipped authz engine; fault injection
  (A33) is opt-in per filter chain; composite filters (A103)
  compose RBAC + fault injection in chain order.
- JWT call credentials (A97) work with and without xDS and
  coordinate with the legacy EX-03..EX-05 credential cards (which
  stay authoritative for provider shape).
- mTLS bootstrap (A65), system roots (A82), SPIFFE verification
  (A87), and SNI/SAN validation (A101) follow the gRFC defaults;
  insecure-channel fallback is forbidden.

### Server side (XD-07; A36)

- Listeners select filter chains by destination port, SNI, and
  transport/protocol match; non-matching connections fail closed.
- Serving state follows the resource: listener removal drains
  within the bounded grace period (CH-01 rule 4); no new RPC is
  admitted on a removed listener.
- Per-connection security (TLS/mTLS/RBAC) applies from the
  matched chain before routing to a handler.

### Load reporting and admin (XD-08/XD-09; A38, A40, A64, A85, A89)

- LRS streams per-cluster/per-locality load with ORCA-fed custom
  metrics (A64/A85) and backend-service labels (A89); reporting
  intervals are server-driven within client-side min/max bounds,
  and a dead LRS stream never affects RPC serving.
- CSDS (A40) exposes the accepted per-type versions plus ACK/NACK
  state; the admin server (A38) combines CSDS with channelz on a
  loopback-only listener by default.

## Bounded API surface

- Public entry: the `xds` scheme in `ResolverRegistry`
  (`xds:///target`, CH-01) plus bootstrap config; everything else
  is internal until XD-10's interop clients prove the shape. No
  new crate: `pbrs-grpc/src/xds/` behind an opt-in `xds` feature
  (placement pending maintainer approval in XD-01).
- Bounds every xDS card must enforce: resource-cache entry caps,
  subscription caps, ADS message-size caps, snapshot generation
  counters, watcher counts, LRS queue caps, and drain grace
  periods — all under `ChannelConfig` knobs with documented
  defaults. Unbounded growth on a hostile or chatty control plane
  is a defect.
- Hot-path rule (XD-11): xDS must not change pick cost. Route
  lookup is table-driven off-path state; per-RPC work stays O(1)
  with no locks and no allocation beyond the pick's single `Arc`
  clone. Scale targets: 10k clusters / 100k endpoints for update
  CPU and memory.

## Test contracts

| Card (legacy) | Contract |
|---|---|
| XD-02 | Vendored protos generate with `pbrs` codegen (byte-stable, no `protoc`); pinned local ADS fixture (in-repo fake or pinned `go-control-plane` — a test tool, never shipped); `tests/xds_local.rs` harness online. |
| XD-03 (EX-08, EX-07 remainder) | Bootstrap precedence/source tests; ADS version/nonce ACK/NACK timelines on fake clocks; stale/invalid/removal unit suites per §ADS; `tests/xds_local/client.rs`. |
| XD-03b | Federation/fallback/failure-mode suites; A53/A54/A57/A71/A88 fixtures; `client_hardening.rs`. |
| XD-04 (EX-10) | Table-driven route fixtures (path/header/weight/timeout/retry/authority); config-tear atomicity; `routing.rs`. |
| XD-05 (EX-09, EX-11) | CDS/EDS snapshot→subchannel tests incl. A37/A75/A56; priority failover timelines; `cluster.rs`. |
| XD-05b (EX-13, EX-14) | Breaker budgets, ejection/recovery with fake time and deterministic sampling; `cluster_policies.rs`. |
| XD-05c | Custom LB config + affinity fixtures; `lb_config.rs`. |
| XD-06 (EX-12 part) | SDS rotation fixture; mTLS/SPIFFE/SNI suites; never-fallback-to-insecure test; `security.rs`. |
| XD-06b (EX-12 part) | RBAC deny/allow, fault-injection delay/abort, composite order; `filters.rs`. |
| XD-06c | JWT sign/verify with fixed synthetic keys; `jwt.rs`. |
| XD-07 | Listener/chain-match matrix, serving-state transitions, per-connection security; `server.rs`. |
| XD-08 | LRS interval/label suites against the fixture; `lrs.rs`. |
| XD-09 | CSDS dump golden tests, admin loopback default; `tests/admin.rs`. |
| XD-12 | CONNECT/GCP-service/child-channel option suites; `transport.rs`. |
| XD-10 (EX-15, EX-16) | `LoadBalancerStatsService` + `XdsUpdateClientConfigureService` adapters, strict upstream CLI flags, pinned `psm-interop` cases in CI; writes `tests/interop/xds-cases.json` (the case inventory EX-07 scoped but XD-01 cannot write). GKE/cloud runs are operator actions. |
| XD-11 | Scale cells (10k/100k) + picker-cost parity in `bench/devloop/cells/xds_scale.rs`; record in `docs/evidence/xds-scale.md`. |

Every row needs committed behavior tests; rows move to shipped in
[the gRFC matrix](grfc.md) only with tests plus, where a peer
exists, cross-implementation evidence.

## Changelog

- 0.1 (2026-09-28, XD-01 draft): initial contract from EX-07 scope;
  case inventory deferred to XD-10.
