# RX-02: thread-per-core server and client modes

## What

Opt-in per-core mode: N pinned current-thread runtimes on
`SO_REUSEPORT`-sharded listeners, per-core connection ownership, and
per-core client pools.

- `pbrs-grpc/src/rt/per_core.rs` (new): shard listener setup
  (`reuseport_listener`) and thread pinning (`pin_current_thread_to`).
- `pbrs-grpc/src/server/accept.rs`: `Server::serve_per_core` /
  `serve_tls_per_core` / `bind_per_core` / `serve_per_core_on` /
  `serve_tls_per_core_on`, plus the same five methods on `Router` so
  multi-service servers (including the bench dual router) can use the
  mode. Both delegate to one shared supervisor
  (`serve_per_core_shards`) and one shared binder
  (`bind_reuseport_shards`): each shard converts its own std listener
  to Tokio on its own thread, runs `accept_loop_with_slots` on its own
  dispatch clone, and reports to a shared supervisor that stops every
  shard when one accept loop fails. The connection budget is shared
  across shards (exact aggregate); `max_concurrent_rpcs` divides into
  `max(1, limit / cores)` per shard. Shutdown/drain/error behavior is
  the shared accept loop, so parity with the default mode is
  structural, including EMFILE killing the serve in both modes.
- `pbrs-grpc/src/client/pool.rs`: `Channel::connect_per_core` /
  `connect_per_core_with` / TLS variants: per-core client pools.
- `pbrs-grpc/tests/per_core.rs` (new): 11 tests covering unary serve
  on `Server` and `Router` shards, zero-core rejection, drain waiting
  for in-flight, RPC-limit division, exact shared connection limit,
  per-core client channels, connect churn, creator-reactor
  independence, and TLS roundtrip.
- `rpc-bench`: `server --cores N` (native only; rejects tonic) and
  `load --connections N` (native pool / tonic `balance_list`, so peer
  comparisons stop measuring client framing).

## Accept mapping

- Scaling beats work-stealing and the best peer: saturated
  empty-unary cell, 14-core host, 32 client conns, 400k offered/s,
  10 s, 3 rounds, median sustained QPS:

  | server | median QPS | rounds |
  |---|---|---|
  | tonic | 36,568 | 37561 / 36495 / 36568 |
  | native work-stealing | 35,665 | 35665 / 35666 / 35641 |
  | native per-core 1 | 78,607 | 78610 / 78607 / 74512 |
  | native per-core 2 | 77,351 | 76362 / 78800 / 77351 |
  | native per-core 4 | 77,491 | 77662 / 77491 / 75456 |
  | native per-core 8 | 78,467 | 78970 / 78467 / 78150 |
  | native per-core 14 | 79,919 | 80773 / 79919 / 72084 |

  Per-core beats work-stealing ~2.2x and tonic ~2.1x at every shard
  count 1-14 (host max; 16 untestable on 14 cores). The curve is flat
  above the default mode: the server is no longer the bottleneck
  (client/host-loopback ceiling; a dual-client run only added
  contention). Sporadic failures at extreme oversaturation are
  client-side `DEADLINE_EXCEEDED` plus h2 `too_many_data_frames` load
  protection; server logs show zero errors on every row.
- Identical shutdown/drain/limits: per-core tests pin drain,
  limit division/sharing, and shutdown above; the mode reuses the
  same accept loop and serve future as the default path.

## Gates

lib 399, serving 1098, per_core 11, rpc 11, bench bin 81, clippy
`-D warnings` clean (lib), fmt clean, stack-matrix smoke PASS,
interop self-only PASS.

Raw probe logs: /tmp/percore-scale/ (rows.txt, per-round client/server
logs); dual-client ceiling check: /tmp/percore-scale/dual/.
