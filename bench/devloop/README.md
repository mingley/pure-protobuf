# devloop: deterministic dev-loop measurement harness (SB-03)

Excluded standalone crate. Measures codec operations and loopback RPC
shapes with deterministic metrics: retired instructions (Linux
`perf`, else valgrind/callgrind, else `not_run`), exact heap
allocations/bytes (counting `GlobalAlloc`), syscalls per RPC
(`strace -c`, else `not_run`), and wall time (secondary).

## Usage

```sh
# Full matrix, JSON report to stdout (release build).
scripts/devloop.sh --out /tmp/now.json

# Subset, more repeats.
scripts/devloop.sh --cells codec.pbrs.cached_encode,rpc.pbrs.unary --repeats 5

# Compare against a baseline (exits 1 on regression).
scripts/devloop.sh --baseline /tmp/before.json --out /tmp/now.json
```

Each cell runs in a child process (`devloop run-cell <id>`), which
reports exact allocations plus wall time; the parent wraps repeats in
the available tools and aggregates medians into versioned JSON
(`schema: "devloop/1"`). `not_run` never passes or fails a
comparison; it skips.

Thresholds follow the scoreboard win rules: instructions or
allocations fall ≥2% on targeted cells; no primary cell regresses
>1% (2% for RPC cells).

## Cells (16)

Codec cells share one populated `TestAllTypesProto3` specimen: pbrs
builds it by hand, prost and protobuf v4 parse the same wire bytes
(also proving wire compatibility), and touch checksums must agree.

- `codec.{pbrs,prost,v4}.{fresh_encode,cached_encode,owned_decode,parse_touch}`
- `rpc.{pbrs,tonic}.{unary,server_stream}` — closed-loop loopback
  (concurrency 1) over 127.0.0.1, 1 KiB payloads, 4 replies per
  server-stream RPC.

Cross-stack RPC deltas are NOT fair transport comparisons: each
stack uses its natural codec (pbrs hello vs prost echo) until SB-01
qualifies the tonic setup. Within-stack repeats are exact.

## Known measurement limits

- v4/upb decode cells report 0 Rust-heap allocations: upb allocates
  from C arenas invisible to the counting `GlobalAlloc`. Encode
  cells count the returned Rust `Vec` only.
- RPC allocation counts jitter by ~1 across processes (ephemeral
  port digits, hash seeds); RPC cells report medians while codec
  cells assert bit-exact totals.
- Instructions and syscalls need Linux `perf`/`strace` (or
  valgrind); macOS reports `not_run` for both.

## Observed variance (2026-09-27, Apple M4 Pro, rustc 1.98.1)

16 cells × 3 repeats, `--iters 2000` (RPC capped at 200/RPC-run):

- Codec allocations: bit-exact across repeats on all 12 cells.
- Codec wall CV: 0.6–4.1% (secondary metric; instructions are
  primary on Linux hosts).
- RPC wall CV: 0.4–1.5% except `rpc.pbrs.unary` at 36% on one run
  (cold-start outlier; wall is secondary).
- Instructions/syscalls: `not_run` (no perf/strace/valgrind on
  macOS). The <1% codec / <3% RPC instruction-variance bar must be
  validated on a Linux host with `perf` (see SB-04 CI lane).
