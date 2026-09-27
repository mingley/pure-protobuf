# Channel architecture (CH-01)

**Status:** Proposed for maintainer review; no resolver or balancer ships
with this document. **Baseline:** `10b0ba1a` on 2026-09-26.
**Supersedes:** the FL-01 scope of [the resolver
contract](../resolver-contract.md) (kept as the DNS-profile detail:
timeouts, staleness, deterministic timelines t0–t7).

**Review gate:** the maintainer must approve the registries, the state
model, the performance rules, and the opt-in API before CH-02 closes or
unblocks CH-03+. A design note is not production evidence.

## Shape

One `Channel` owns a pipeline, left to right:

```
target URI → Resolver → Resolution snapshot ─┐
                                             ├→ LB policy tree → Picker → RPC
service config ──────────────────────────────┘        ↑
                                                      Subchannels (one per address)
```

- **Resolver** watches a target (`dns:///`, `passthrough:///`,
  later `xds:///`) and publishes `Resolution` snapshots: an ordered,
  deduplicated address list plus an optional service config.
- **Service config** selects the LB policy and its config (`A24`),
  validated eagerly with `InvalidArgument` on errors (`A21`).
- **LB policy tree** owns one child policy per cluster/endpoint group
  and produces a **Picker**. Policies are plugins behind a registry.
- **Subchannel** owns one address: connection lifecycle, connectivity
  state, per-address backoff, and (CH-05) health-check state.
- **Picker** maps one RPC to a ready subchannel. Picks are O(1),
  lock-free, and allocation-free (see Performance rules).

## Connectivity states

Subchannels move `Idle → Connecting → Ready`, with `TransientFailure`,
`Draining`, and `Shutdown` as explicit transitions (per the resolver
contract). The channel aggregates: `Ready` when the picker can serve,
`TransientFailure` when every child reports failure, `Connecting`
while any child connects, else `Idle`. `Shutdown` is terminal and
cancels the resolver, all backoffs, and all subchannel tasks; after
shutdown no task, connection, or byte remains (t7).

## Plugin registries

Two init-time registries, both following the `authz` factory pattern
(name → factory, last registration wins, parse-time reads only):

- `ResolverRegistry`: scheme → `ResolverFactory`
  (`dns`, `passthrough` ship in CH-02; `xds` arrives with XD-02).
- `LbPolicyRegistry`: policy name → `PolicyFactory`
  (`pick_first`, `round_robin`, `ring_hash`, `least_request`,
  `weighted_round_robin`, xDS policies).

Policy configs come from `loadBalancingConfig`; unknown policy names
fall through to the next entry, and an empty/unusable list fails the
channel to `TransientFailure` with `InvalidArgument` naming the entry
(A21/A24).

## Performance rules (hard)

1. **Per-call pick is O(1), lock-free, allocation-free.** The picker
   holds an `Arc`-swapped snapshot (ready-subchannel table + atomic
   cursor); the hot path clones one `Arc` and advances atomics.
2. **Policy updates never block RPCs.** Resolver and LB updates build
   a new snapshot off-path and publish it atomically by generation;
   in-flight RPCs keep their old snapshot until completion.
3. **No per-RPC allocation in the resolver path.** Refresh work is one
   task per channel with latest-value notification, as in the resolver
   contract; no unbounded queues, no per-call timers beyond the call's
   own absolute deadline.
4. **Bounded everything.** Addresses, subchannels, connections, pending
   picks, and queued wait-for-ready calls all sit under the existing
   connection, RPC, byte, and deadline budgets. Removed addresses
   drain within a bounded grace period; nothing migrates mid-RPC.

## Passthrough default

`Channel::connect`, `connect_tls`, `connect_lazy`, Unix, and `from_io`
keep their exact behavior: one authority, DNS-at-dial, no refresh, no
balancer. `Target` keeps rejecting URI-shaped strings. Resolver-managed
channels require an explicit opt-in (`Channel::connect_uri` in CH-02,
plus `ChannelConfig` knobs for bounds); the direct path never pays for
the balancer (no extra tasks, no snapshot machinery).

## gRFC map

| gRFC | Behavior | Component | Proved by |
|---|---|---|---|
| A5/A26 | grpclb | Boundary: superseded by xDS, not implemented | grfc.md row stays `boundary` |
| A10 | Literals skip DNS-TXT service-config lookup | `resolver` (dns): IP literals never issue TXT | CH-02 unit tests |
| A17 | Client-side health checking | `subchannel` health state + `HealthReporter` wiring | CH-05 e2e (existing `Check`/`Watch` stay) |
| A21 | Service-config error handling | `service_config` validation at resolver delivery | CH-03 invalid-document tests |
| A24 | LB policy + config selection | `balancer` tree + `LbPolicyRegistry` | CH-03 selection/fallthrough tests |
| A61 | Dualstack racing | `subchannel` Happy-Eyeballs dial (CH-04) | CH-04 race/churn tests |
| A62 | pick_first | `lb/pick_first.rs` (FL-03, done) | `tests/resolver.rs` stick/failover/drain/backoff/shuffle + ported `pick_first_unary` procedure |
| A105 | Scale on `max_concurrent_streams` | `pool` growth signals from subchannel caps | CH-08 scaling tests |
| A113 | pick_first weighted shuffling | `lb/pick_first.rs` shuffle (CH-04) | CH-04 shuffle distribution tests |

Follow-ups land in the same tree: ring hash / least request /
subsetting (CH-07), ORCA + weighted round robin (CH-06), proxy + user
timeout (CH-09), picker-cost qualification (CH-11).

## xDS plugin points (later, no divergence)

xDS reuses the same three seams: an `xds` resolver (A27 bootstrap +
ADS), EDS-driven address updates (A56 priority), and LB policies
(ring hash A42, least-request A48, outlier detection A50, weighted
round robin A58). No xDS-only channel path: the control plane only
publishes snapshots and configs through the registries above.

## Module map (post-MX-02 client tree)

Existing files keep their jobs (`channel.rs` facade, `call.rs`,
`unary.rs`, `streaming.rs`, `retry.rs`, `pool.rs`, `config_glue.rs`).
New components land in:

- `resolver/` — target URI parsing, `ResolverRegistry`,
  `Resolution` snapshots, `dns` + static providers (CH-02; top-level
  per the card, not `client/resolver.rs`).
- `client/subchannel.rs` — per-address state machine, dial/backoff,
  health input, drain (CH-03/CH-04/CH-05).
- `client/balancer.rs` — policy tree, snapshot publication, `Picker`
  trait + registry (CH-03).
- `client/balancer/pick_first.rs`, `round_robin.rs`, … — one file per
  policy as its card lands (CH-04/CH-06/CH-07/CH-09…).

Tests: `tests/resolver.rs` (fake provider + paused clock, t0–t7),
`tests/balancer.rs` (distribution, churn, O(1)-pick soak), extended by
each policy card. Docs: this file plus the resolver contract.

## Open decisions for the maintainer

1. Opt-in spelling: `Channel::connect_uri` vs config-only. Proposed:
   both (constructor for URIs, `ChannelConfig` for bounds).
2. Default LB policy for resolver-managed channels: `pick_first`
   (gRPC default) — confirm.
3. DNS defaults: `min_refresh`/`max_refresh`/`refresh_without_ttl`/
   `retry_min`/`retry_max`/`max_stale` values (the contract approves
   no numbers yet).
