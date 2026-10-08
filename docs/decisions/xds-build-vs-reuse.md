# xDS build versus reuse (XD-01)

Decision: build the xDS client in-repo on the native `pbrs-grpc` transport
with `pbrs`-generated v3 protos, and do not take a dependency on
`xds-client`, `tonic-xds`, `xds-api`, or `envoy-types`. Treat
`grpc/grpc-rust`'s `xds-client` as the upstream collaboration target
(concrete ask: unseal `TransportStream`) and `tonic-xds`/`envoy-types`
as behavior and coverage references.

**Status:** Draft for maintainer review; no code ships with this
document. **Baseline:** `2598aa12` on 2026-09-28. **Supersedes:** the
scope portion of EX-07 (the versioned contract now lives in
[the xDS contract](../xds-contract.md)). **Companion:**
[the xDS contract](../xds-contract.md) (XD-01's second deliverable).
**Depends on:** [the channel architecture](channel-architecture.md)
(CH-01).

**Review gate:** the maintainer must approve the build decision, the
crate placement, and the upstream-collaboration asks before XD-02
closes or unblocks XD-03+.

## What was evaluated

Pinned crates.io versions, fetched 2026-09-28 to `/tmp` scratch copies
for reading only (no manifest change, no build):

| Crate | Pinned version | sha256 of `.crate` | License | MSRV | Repo (observed) |
|---|---|---|---|---|---|
| `xds-client` | 0.1.0-alpha.4 (2026-09-23) | `91eaa823…8a4da906` | MIT | 1.88 | `grpc/grpc-rust` (manifest still says `hyperium/tonic`; that repo redirects) |
| `tonic-xds` | 0.1.0-alpha.4 (2026-09-23) | `f5d9b164…ccbb5dc` | MIT | 1.88 | `grpc/grpc-rust` (same redirect note) |
| `xds-api` | 0.2.0 (2025-03-21) | `178c1014…4523bac` | Apache-2.0 | 1.81 | `junction-labs/xds-api` (last push 2025-08-18) |
| `envoy-types` | 0.7.7 (2026-09-22) | `e1fdd3fa…ecc39c0` | Apache-2.0 | workspace | `flemosr/envoy-types` |

(`envoy-types` is not named in the card but both `grpc-rust` crates
depend on it, so it is evaluated as a transitive choice.)

The full `.crate` sha256 values are recorded in the worker return note,
not here, to keep this record readable.

## Option 1: reuse `xds-client` (rejected as a dependency)

`xds-client` is a protocol-agnostic ADS core: stream management,
reconnect, subscription/watch plumbing, version/nonce tracking and
ACK/NACK (~5.9k lines). Its own docs state what it does **not** do:
LDS-RDS-CDS-EDS cascading, gRPC resource validation, or service-config
generation. That excluded middle is most of XD-03 through XD-08, so
reusing it saves only the ADS envelope while the gRPC-specific bulk is
built either way — against a foreign alpha API.

Three concrete blockers, any one of which is enough to reject:

1. **The transport trait is sealed.** `TransportStream` is
   `sealed::Sealed` with the only out-of-crate implementor being
   `tonic::TonicAdsStream`. `pbrs-grpc` cannot plug in its own ADS
   stream. Reuse therefore forces the default features
   (`transport-tonic` + `codegen-prost`), i.e. the ADS control plane
   runs over tonic while the data plane runs `pbrs-grpc`: two HTTP/2
   stacks and two TLS stacks in one process.
2. **MSRV 1.88 exceeds `pbrs-grpc`'s 1.85.** Adoption forces a
   maintainer-level MSRV bump for the whole native crate.
3. **Marginal payoff.** The reusable half (ADS envelope) is the
   smaller half of XD-03; everything above it (bootstrap semantics,
   cascading watches, validation, CH-01 snapshot publication) must be
   written against `xds-client`'s watcher/`Resource` API instead of
   directly against CH-01's seams, adding coupling without removing
   work.

The `XdsCodec` and `Resource` traits are public and unsealed (a `pbrs`
codec would be feasible), and the two-phase decode design (A46/A88) is
sound and worth mirroring. But without an unsealed transport, reuse
means adopting tonic for the control plane. This is the collaboration
item instead (see below).

Minimal no-default-features graph would be `bytes`, `rand 0.10`,
`thiserror`, `tokio`: small, but unusable without a transport, and
`rand 0.10` is a second `rand` major next to the locked `rand 0.9.5`.

## Option 2: reuse `tonic-xds` (rejected)

`tonic-xds` (~22.4k lines) is a full client-side xDS implementation for
tonic: bootstrap, ADS via `xds-client`, routing, clusters, security,
retry, circuit breaking, outlier detection. Its entry point is
`XdsChannelGrpc: tonic::client::GrpcService`, a Tower service. Reuse
would mean routing xDS RPCs through tonic's channel stack, which:

- bypasses CH-01 entirely (no resolver registry, no LB policy tree,
  no O(1) lock-free allocation-free picker);
- duplicates LB policies this repo already ships and qualifies
  (ring hash, least request, subsetting, priority, outlier detection,
  WRR/ORCA per [the gRFC matrix](../grfc.md));
- pulls tonic 0.14 with **default features** plus `prost`,
  `envoy-types`, `tower`, `dashmap`, `regex`, `url`, `arc-swap`,
  `backoff`, `fastrand`, `xxhash-rust`, `indexmap`,
  `shared_http_body` and more into the `pbrs-grpc` shipping profile.
  `pbrs-grpc` is tonic-free today (tonic appears in the lockfile only
  via the `protobuf-tonic` adapter, examples, and dev-dependencies);
  `dashmap`, `url`, `arc-swap`, `backoff`, `xxhash-rust`,
  `shared_http_body` and `envoy-types` are all new supply-chain
  crates;
- conflicts with the TLS dependency policy: its `tls-ring`/`tls-aws-lc`
  features map to `ring`/`aws-lc`, and `ring` is in the `C_DENY` set
  of `scripts/pure-rust-audit.sh`. `pbrs-grpc` standardizes on
  Graviola;
- is client-only: no A36 server-side support (the `xds_server`
  example is a fake control plane for tests), no CSDS/LRS/federation
  markers in source. XD-07/XD-08/XD-09 would still be built in-repo;
- carries alpha churn (0.1.0-alpha.4) and MSRV 1.88.

`tonic-xds` remains useful as a behavior reference for gRFC
interpretations (its source cites A27/A28/A29/A32/A42/A44/A48/A50/A57/
A63/A65/A78/A88), and `psm-interop` results against it are comparable
evidence. That is a reading relationship, not a dependency.

## Option 3: reuse `xds-api` bindings (rejected)

`junction-labs/xds-api` 0.2.0 is generated prost/tonic bindings for the
Envoy xDS API. Reject:

- **Stale:** released 2025-03-21, last repo push 2025-08-18, against
  an old data-plane-api pin (`786c93c`). xDS moves monthly;
  `envoy-types` cut four releases in 2026 alone.
- **Pin conflict:** requires `prost 0.13` and `tonic 0.12`, a second
  prost major and a second tonic major next to the locked
  `prost 0.14.4` / `tonic 0.14.6`.
- **Wrong scope and codegen:** whole-Envoy surface (including Envoy-
  only APIs gRPC never touches) generated by prost, giving up the
  codec ownership and performance the PK lane exists to win.

## Option 4: reuse `envoy-types` bindings (rejected as a dependency)

`envoy-types` 0.7.7 is the live alternative to `xds-api`: actively
released, version-aligned (`prost 0.14`, `tonic 0.14`), Apache-2.0.
Reject as a dependency:

- pulls `tonic` (with the `transport` feature, i.e. the hyper stack)
  and `tonic-prost` into the `pbrs-grpc` shipping graph, plus
  prost-based codegen for ~8 MB of generated sources covering the
  whole Envoy API (including `ext_authz` and other non-gRPC
  surfaces) where gRPC needs about a dozen resources;
- is a single-maintainer crate outside the gRPC org, so its release
  cadence and Envoy-pin choices are not governed by gRPC's xDS
  needs;
- cannot serve the native path: at best it could sit behind the
  optional `prost` feature, but xDS must work with `pbrs` messages
  on the default path.

Use it as a coverage cross-check instead: when XD-02 selects the
proto subset to vendor, diff the selection against `envoy-types`
(and `tonic-xds`'s resource modules) so a missing message is a
deliberate boundary, not an accident.

## Option 5: build in-repo (chosen)

Vendor the gRPC-relevant xDS v3 protos under `pbrs-grpc/proto/xds/`,
generate them with the in-repo `pbrs` codegen through the existing
`build.rs` + `.fds` pipeline, implement the ADS client over the
native `pbrs-grpc` transport, and publish results through the CH-01
seams (`xds` resolver, EDS address updates, `LbPolicyRegistry`). This
is already the shape XD-02 through XD-12 assume (`pbrs-grpc/src/xds/`
write scopes, local control-plane fixture, `psm-interop` in XD-10).

Reasons for this choice:

- **No new dependencies.** No tonic/prost in the
  default graph, no MSRV bump (stays 1.85), nothing for QG-04 to
  flag. The ORCA precedent already ships: `xds/data/orca/v3` and
  `xds/service/orca/v3` protos are vendored and generated by `pbrs`
  today (`pbrs-grpc/build.rs`, `orca_report.fds`/`orca.fds`).
- **Performance.** Control-plane parsing uses the kernel the PK lane
  is optimizing rather than prost; the data plane keeps CH-01's
  lock-free picker with no xDS-only hot path (XD-11 proves picker
  cost is unchanged).
- **API fit.** There is no xDS-only channel path: bootstrap/ADS is a
  resolver input, EDS feeds the existing subchannel lifecycle, and
  xDS selects among the already-shipped LB policies. Reuse options
  1–2 invert this by dragging a foreign channel model in.
- **Dogfooding.** Running ADS over the native transport exercises
  the stack the program is trying to qualify; a tonic-based control
  plane would leave that evidence on the table.
- **Scope control.** Vendoring only the gRPC-relevant subset
  (A30 v3) keeps gencode small and makes every unsupported resource
  an explicit, NACK-correct boundary per
  [the xDS contract](../xds-contract.md).

The cost is real: the full client (XD-03..XD-06, XD-08, XD-12), the
server side (XD-07), and admin (XD-09) are written here. But options
1–2 leave most of that work in place anyway (cascading, validation,
server side, admin), while adding dependency, MSRV, TLS, and
API-fit penalties. The differential cost of building is small; the
differential benefit (ownership, purity, fit) is large.

## Crate placement (for maintainer confirmation)

`pbrs-grpc/src/xds/` as an opt-in `xds` feature of the existing
`pbrs-grpc` crate. No new crate, consistent with TC-01 and the
three-crate release shape. Rationale: xDS is a resolver/LB input to
the channel, not a standalone product; a feature keeps the default
graph and gencode size unchanged while the module map in CH-01
already reserves the seams. A split-out later stays possible if the
gencode footprint demands it.

## Upstream collaboration (no external action without approval)

Per the worker protocol, each of these needs explicit maintainer
approval before anyone files or posts it:

1. **Ask `grpc/grpc-rust` to unseal `TransportStream`** (or accept a
   public bytes-oriented adapter), so a non-tonic transport such as
   a `pbrs-grpc` ADS stream can drive `xds-client`. This is the
   single change that would make option 1 viable in the future.
   Re-evaluate reuse if it lands: the trigger is a released
   `xds-client` whose core (no default features) can run an ADS
   stream without tonic/prost in the graph.
2. **Share gRFC interpretations, not code.** Where `tonic-xds`
   behavior is ambiguous, resolve from `grpc/proposal` text and
   `psm-interop` outcomes, and report discrepancies upstream as
   issues with interop evidence.
3. **Watch, do not fork.** Track `xds-client` toward a stable API
   and its MSRV; a future MSRV-1.85-compatible, transport-open
   release reopens the decision. Do not vendor or fork the alpha.

## Consequences

- Unblocks XD-02 (proto vendoring + fixture), which unblocks the
  whole XD lane. XD-10 runs `psm-interop` locally in CI; GKE/cloud
  runs stay operator actions.
- The `xds` feature must pass `scripts/pure-rust-audit.sh` like
  every other feature combo (`--all-features` keeps the gate
  strict).
- `docs/grfc.md` xDS rows stay `planned` until their XD card ships
  with tests; nothing in this decision moves a row.

## Open questions for maintainer

1. Approve building in-repo per option 5 (no `xds-client` /
   `tonic-xds` / `xds-api` / `envoy-types` dependency)?
2. Approve crate placement: `pbrs-grpc/src/xds/` behind an opt-in
   `xds` feature, no new crate?
3. Approve filing the upstream ask to unseal `xds-client`'s
   `TransportStream` (question 1's collaboration item), and if so,
   who posts it and under which account?
4. Confirm the re-evaluation trigger: revisit reuse only when a
   released `xds-client` can run without tonic/prost in the graph,
   rather than tracking alphas?
5. Record approval here (maintainer name + date) so XD-02 can close
   the review gate. Approval status: **pending**.
