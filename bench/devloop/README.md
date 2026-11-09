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
| Heap allocation requests and bytes | Counting `GlobalAlloc` | Process-wide diagnostic |
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
allocation requests plus wall time. The parent wraps repeats in the available
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

## Opt-in mixed-byte ownership diagnostic

This separate binary compares construction and serialization of the same
`BlobMixed` message: eight 4 KiB byte fields and an integer ID. For both
pbrs and Prost, it measures a copied borrowed slice and a moved vector, each
with exact or spare source capacity. Payload construction, serialization and
drop are inside the counted window. This includes allocation of the source
vector; it does not model a pre-existing borrowed slice. A moved
spare-capacity vector can retain its extra allocation; use the exact and spare
rows separately.

```sh
cargo test --locked --release --manifest-path bench/devloop/Cargo.toml \
  --features mixed-ownership-diagnostic --bin mixed-ownership

cargo run --locked --release --manifest-path bench/devloop/Cargo.toml \
  --features mixed-ownership-diagnostic --bin mixed-ownership -- \
  --iterations 1000 --warmup 100
```

Before counting, the binary checks the complete wire bytes, both decoders,
all payload bytes, ID boundaries and owned-buffer pointer transfer through
cloning and the repeated-field setter. A separately armed allocation control
checks the counter, and zero-allocation rows are rejected. During each
operation the complete encoded slice passes through `black_box`.

The JSON reports process-wide allocation requests through a wrapped System
allocator. Its relaxed atomic counters can include allocations by other
threads. Reallocations count as allocations and report the full new size;
requested bytes are not live or peak memory, retained capacity or RSS. The
loop's ID is its iteration index, so output lengths can change at varint
boundaries. The devloop dependency graph enables pbrs copy-count hooks. The
diagnostic does not measure CPU time, latency or RPC performance. These new
rows do not replace or change the existing `devloop/1` cell IDs.

## Opt-in server-stream burst diagnostic

The separate `stream-burst` binary measures 4, 64, or 128 cloned Prost
responses of the existing sparse-v0 fixture. It also offers the smaller
query-d3-v0 fixture as a control. It does not add or alter any `devloop/1`
cells. Build and run it explicitly:

```sh
# From the repository root; requires cmake and a C++ compiler.
scripts/build-pinned-protoc.sh
export PATH="$PWD/target/pinned-protoc-build:$PATH"
cargo build --locked --release --manifest-path bench/devloop/Cargo.toml \
  --features stream-burst-diagnostic --bin stream-burst
bench/devloop/target/release/stream-burst sparse-v0 live8 4 100 100
bench/devloop/target/release/stream-burst sparse-v0 live8 64 100 100
bench/devloop/target/release/stream-burst sparse-v0 ready128 128 100 100
```

Arguments are case (`sparse-v0` or `query-d3-v0`), producer (`live8`
or `ready128`), reply count (4, 64, or 128), measured RPCs, and warmup
RPCs. Measured iterations must be 1–400 and warmup 0–1000, allowing an N/2N
pair up to 200/400. `live8` matches the existing four-reply fixture's spawned
producer and channel depth; `ready128` fills and closes a 128-slot channel before returning
headers, forcing a full available burst. A preflight checks every decoded
response for full equality before warmup. The timed loop retains the original
read checksum and response-count checks. Identical replies cannot establish
ordering; the separate server-stream integration tests use numbered replies.

The JSON is a `devloop/stream-burst/1` diagnostic, not a `devloop/1` report.
It records raw process-wide allocation counts and requested bytes, including
both client and server; reallocations count their full new size. Counter updates
are checked for overflow; a sampled overflow rejects the sample. Counter fields
and `overflow_sampled` are read separately with relaxed atomics while recording
is armed. Hooks can update between or after reads, so a false sampled flag does
not prove that no racing hook overflowed. Asynchronous work near the window
boundaries can be included or missed; these are non-atomic, process-wide
diagnostic samples, not exact per-side totals. Wall time is instrumented and
secondary. The devloop crate enables copy counters, so this is not a
counters-off latency measurement. Setup, input validation, warmup,
printing, and teardown are outside the allocation window.

For a comparison, use isolated baseline and candidate source trees. Apply the
identical manifest addition and diagnostic source to each tree; do not replace
an older manifest that differs. Build each with `--locked`, separate target
directories, the same Rust/protoc versions and flags, and no dependency
changes. Worktrees do not automatically contain the ignored
`third_party/protobuf` checkout; each needs the verified pinned source or a
link to that same checkout. Preserve source commit IDs, any candidate diff,
build commands, toolchain versions, and binary hashes. The binary requires `git` and
`sha256sum`; it records embedded harness, manifest, lock, corpus and server
drain hashes, plus runtime Git head/dirty and diff observations. Runtime Git
state is not a substitute for the source tree used at compilation.

Compare only matching case IDs, full input wire bytes, message and item sizes,
reply counts, producer mode, warmup, embedded harness/manifest/lock/corpus
hashes and build settings. The subject drain hash and tracked diff should
differ when testing a drain patch. Capture repeated matched N and 2N runs;
treat unavailable instruction counters as unavailable. These diagnostics are
not directly comparable to historical adoption rows or a controlled ranking.

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
