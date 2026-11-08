# devloop: local performance measurement harness (SB-03)

`devloop` is an excluded standalone crate for quick local performance checks
with repeatable inputs. It measures codec operations and loopback remote
procedure call (RPC) shapes; the usual command is
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

Before comparing any cells, `compare` rejects duplicate IDs, blank cell IDs,
kinds or codecs, zero iteration or repeat counts, and non-finite or negative
measured values in either report. It
also rejects empty reports and pairs with no shared cells. For
each shared cell, it requires matching kind, codec, iteration count and repeat
count. For two measured values
it checks that units match and are nonempty; measured instruction values must
also have the same nonempty collection method other than `not_run`.
Incompatible values print `INCOMPARABLE`, are not classified as improvements
or regressions, and make the command exit nonzero. A thresholded metric that
increases from zero is a regression without a percentage. Local comparison does
not require matching hosts. For historical reports without an instruction method,
the local command retains the `whole_process_legacy` default; the CI verifier
requires an explicit method for measured instructions. If either metric is
`not_run`, it remains skipped and cannot establish a pass. Wall time is
recorded for inspection and
its units are validated, but the command does not apply a wall-time threshold.
The timed child may itself run under perf or Callgrind; such wall observations
do not establish native latency.

Fresh-encode rows prepare separate message sets for warmup and measurement.
Each measured object is encoded once, and preparation stays outside the
window. New reports carry `fresh_encode_profile: "independent-warmup-v1"`;
collection requires it, and comparison rejects a corrected row paired with
an older lifecycle. Historical rows could reuse warmed packed-field caches.
The [repair record](../../docs/evidence/fresh-encode-warmup-20261106/README.md)
retains the detecting allocation counts and all codec controls. This lifecycle
is distinct from cached encoding and construction-plus-encoding cells.

Adoption codec and RPC rows retain `input_wire_fingerprint`. The collector
requires it to match across N/2N children and repeats; comparison rejects a
missing or different fingerprint when either report supplies one. Older
reports without the field remain readable. Frozen adoption map inputs use
deterministic entry ordering during preparation; see the
[corpus documentation](adoption/README.md) for the historical measurement
limit. The fingerprint does not make legacy randomized map measurements
comparable with new ones.

RPC rows also retain the optional `input_wire_bytes`. New captures check both
identity and length across N/2N children and repeats. The fingerprint identifies
the common wire used to prepare both messages. Subsequent encodes of unordered
maps can use a different entry order; full decoded values and equal encoded
lengths remain separate required checks. Older reports still decode, but a
missing identity or length cannot qualify against a new RPC capture. The local
comparator keeps ordinary legacy pairs with both sides absent readable; any
fingerprint that is present must contain non-whitespace characters. Bridge
cells require matching nonblank fingerprints. CI noise qualification requires
nonblank fingerprints on both sides of every adoption cell and explicit prepared
input lengths on both sides of RPC cells, even when both legacy values are
absent. Valid `not_run` counters remain excluded.

The standalone transport collector derives coverage from its pinned specimen
table. Current equal-wire inventories name
`bench/devloop/adoption/evidence/rpc-inventory-equal-wire.json` in their
`specimen_inventory_path`; they require all 512 states. Historical inventories
retain the older table and its 24 blocked map states. Each new capture retains
the selected table's path and hash, and requires prepared-input lengths in
both child and aggregate records. Missing states or counters remain visible.

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

Counter parsers require the tool's actual output format; a present executable
does not prove a usable counter. Missing or malformed output stays `not_run`,
including partial-repeat measurements. The legacy `locks` JSON field counts
futex-family syscalls for the whole child process, divided by its iteration
count. This includes wakes, unsuccessful waits and setup/teardown overhead;
it does not isolate blocking waits or picker activity. Small changes in syscall
and futex counts may reflect scheduling and setup noise. Its corrected unit is
deliberately incompatible with older reports labeled as blocking waits; do not
mix those series.

## Cell families

The matrix has expanded beyond the original 16 cells. Run
`bench/devloop/target/release/devloop list` after building for the exact
inventory of the current revision.

Codec cells share one populated `TestAllTypesProto3` specimen: pbrs
builds it by hand, prost and protobuf v4 parse the same wire bytes,
and touch checksums must agree.

Preparation also checks exact native/Prost fixture values using private
copies: all nine populated TAT fields, repeated strings, map cardinality and
the zero-key entry. Ordered values retain their order; maps can change wire
order or omit default entry subfields. These checks leave measured messages
and fresh-encode pools untouched. They cover the registered fixture fields;
v4 retains the checksum check. The [perf workflow](../../.github/workflows/perf.yml)
runs the binary unit tests and comparison diagnostics before measurement.

- `codec.{pbrs,prost,v4}.{fresh_encode,cached_encode,owned_decode,parse_touch}`
- `rpc.{pbrs,tonic}.{unary,server_stream}` — closed-loop loopback
  (concurrency 1) over 127.0.0.1, approximately 1 KiB requests and four
  replies per server-stream RPC. Reply sizes differ between stacks below.
- Additional codec rows cover small-message losses, packed/repeated fields,
  and large-byte ownership/copy costs.
- RPC rows include native compression and explicit prost-codec variants.
- `lb.*.pick` rows measure steady-state load-balancer picker cost.

The original pbrs/tonic RPC rows use different generated services and codecs
(pbrs hello versus prost echo). Native streaming splits its request into four
roughly 256-byte replies; tonic sends four 1,024-byte replies. Their cross-stack
delta therefore compares different work and cannot establish transport parity
or a win. SB-01 repaired the separate `rpc-bench` comparator; that does not
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
