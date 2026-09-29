# devloop: deterministic dev-loop measurement harness (SB-03)

`devloop` is an excluded standalone crate for quick, deterministic local
performance checks. It measures codec operations and loopback remote procedure
call (RPC) shapes; the usual command is
`scripts/devloop.sh --out /tmp/now.json`. Read the JSON by cell: measured
metrics compare against thresholds, while `not_run` metrics are skipped.

Metrics:

| Metric | Source | Role |
|---|---|---|
| Retired instructions | Linux `perf`, else valgrind/callgrind, else `not_run`; reported as `(2N - N) / N` | Primary when available |
| Heap allocations and bytes | Counting `GlobalAlloc` | Exact allocation signal |
| Syscalls per RPC | `strace -c`, else `not_run` | Linux diagnostic |
| Wall time | Built-in timing | Secondary signal |

## Usage

```sh
# Full matrix, JSON report to stdout (release build).
scripts/devloop.sh --out /tmp/now.json

# Subset, more repeats.
scripts/devloop.sh --cells codec.pbrs.cached_encode,rpc.pbrs.unary --repeats 5

# Compare against a baseline (exits 1 on regression).
scripts/devloop.sh --baseline /tmp/before.json --out /tmp/now.json
```

Each cell runs in a child process (`devloop run-cell <id>`), which reports
exact allocations plus wall time. The parent wraps repeats in the available
tools and aggregates medians into versioned JSON (`schema: "devloop/1"`).
Instruction counts are loop-only differential counts: the parent runs the
same cell at N and 2N measured-loop iterations with identical preparation and
reports `(instructions_2N - instructions_N) / N`. The JSON
`instruction_method` field records the collection method; older reports that
lack it are legacy whole-process counts. `not_run` never passes or fails a
comparison; it skips.

Local comparison defaults follow the scoreboard guidance: instructions or
allocations fall ≥2% on targeted cells; no primary cell regresses
>1% (2% for RPC cells). The CI lane is advisory. SB-20 must calibrate blocking
thresholds from at least 30 usable measured runs; missing metrics and error
placeholder reports are not successful measurements.

## CI comparisons and retained evidence

The performance workflow runs on relevant pushes to `main`, relevant pull
requests, or an explicit dispatch with full `base_sha` and `head_sha`
commit IDs. Base and head build in isolated source directories on the same
runner with the same compiler. Regression thresholds remain advisory;
broken measurement infrastructure fails visibly.

Each `devloop-<run-id>` artifact contains base/head JSON, their logs,
`validation.json`, the readable summary and the comparison output when
available. Only `validation.json` with `qualified_for_noise: true` is a
candidate input for SB-20. Its `eligible_metrics` names the comparable
measurements; missing instruction/syscall counters are excluded, and added
or removed cells are reported separately. Do not count an uploaded error
report, empty cell list, mismatched source SHA, or green upload step as a
successful measurement.

The wrapper preserves an output report even when compilation or comparison
fails. Local regression comparisons still exit nonzero. CI uses
`scripts/perf-report.py` to validate provenance, host/tool metadata, sample
counts, metric units and finite values before interpreting a comparison.

## Cell families

The matrix has expanded beyond the original 16 cells. Run
`bench/devloop/target/release/devloop list` after building for the exact
inventory of the current revision.

Codec cells share one populated `TestAllTypesProto3` specimen: pbrs
builds it by hand, prost and protobuf v4 parse the same wire bytes
(also proving wire compatibility), and touch checksums must agree.

- `codec.{pbrs,prost,v4}.{fresh_encode,cached_encode,owned_decode,parse_touch}`
- `rpc.{pbrs,tonic}.{unary,server_stream}` — closed-loop loopback
  (concurrency 1) over 127.0.0.1, 1 KiB payloads, 4 replies per
  server-stream RPC.
- Additional codec rows cover small-message losses, packed/repeated fields,
  and large-byte ownership/copy costs.
- RPC rows include native compression and explicit prost-codec variants.
- `lb.*.pick` rows measure steady-state load-balancer picker cost.

The original pbrs/tonic RPC rows use different generated services and codecs
(pbrs hello versus prost echo), so their cross-stack delta does not isolate
transport. SB-01 repaired the separate `rpc-bench` comparator; that does not
automatically qualify these dev-loop cells. Prefer within-stack before/after
comparisons and inspect the cell's codec and settings before interpreting it.

## Known measurement limits

- v4/upb decode cells report 0 Rust-heap allocations: upb allocates
  from C arenas invisible to the counting `GlobalAlloc`. Encode
  cells count the returned Rust `Vec` only.
- RPC allocation counts jitter by ~1 across processes (ephemeral
  port digits, hash seeds); RPC cells report medians while codec
  cells assert bit-exact totals.
- Instructions and syscalls need Linux `perf`/`strace` (or
  valgrind); macOS reports `not_run` for both. Instruction counts exclude
  process startup, setup, warmup, and JSON printing by using the differential
  N/2N method above.

## Historical variance (2026-09-27, Apple M4 Pro, rustc 1.98.1)

The original 16 cells × 3 repeats, `--iters 2000` (RPC capped at 200/RPC-run):

- Codec allocations: bit-exact across repeats on all 12 cells.
- Codec wall CV: 0.6–4.1% (secondary metric; instructions are
  primary on Linux hosts).
- RPC wall CV: 0.4–1.5% except `rpc.pbrs.unary` at 36% on one run
  (cold-start outlier; wall is secondary).
- Instructions/syscalls: `not_run` (no perf/strace/valgrind on
  macOS). The <1% codec / <3% RPC instruction-variance bar must be
  validated on a Linux host with `perf` (see SB-04 CI lane).
