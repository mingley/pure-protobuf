# Parity proof attempt, October 27

The full tonic/prost parity and performance claim fails this audit. This record
contains concrete counterexamples, two transport repairs, their checks, and
source-pinned local measurements. It does not certify production readiness or
an overall performance ranking.

Start with the [short proof summary](../../parity.md) or [benchmark results](../../benchmarks.md).
Run `python3 check.py` in this directory to verify `results.json` and every
member of `raw.tar.gz` against `manifest.json`. The archive includes failed
baseline attempts, successful controls, exact commands, generated bindings,
source hashes, independent reviews and all raw measurements. It excludes
compiled executables; their hashes and build logs remain.

## Sources and peers

| Artifact | Source |
|---|---|
| Original baseline and unchanged codec runtime/generator | `73e954c0610906f0f789ba3276241e3b3863a27b` |
| Transport repair and frozen RPC executable | `c1f7e9b433ddbbbd86a025e85d578c090c2f987f` |
| Committed proof suites and final correctness checks | `e14cccb5` (full revision in `results.json`) |
| Independent Rust peers | tonic 0.14.6, prost/prost-build 0.14.4 |
| Tools | Rust 1.99.1, protoc 35.1; Rust 1.85.0 for native MSRV checks |

The codec probe was built while unrelated gRPC changes were in progress.
Its before/build/after hash maps confirm its runtime and generator sources
match the baseline. The generated Rust, schema, standalone lock and binary
hashes are retained. RPC uses a clean sparse worktree at the full repair
revision, copied release executable, locked dependencies and counters disabled.
The final validation runs from a clean committed checkout and records its source.

## Compatibility results

The protobuf differential exercises 330 wire inputs through 654 proto3 parse
and merge operations. Exactly 652 agree. Both differing operations use `0a00`
for an `int32` field: pbrs keeps the wrong-wire-type field as unknown; prost
rejects it. A separate proto2 control shows pbrs rejects an absent required
field while prost accepts it. Closed-enum value 987 roundtrips in both.

The default strict executable exits 1. Its explicit known-differences mode
exits 0 only when those exact differences recur, including their input,
operation, acceptance direction and retained wire. That regression result
never sets `parity_equivalent` true. The two policy differences have not been
classified as pbrs protobuf-conformance defects.

Nine generated consumer cases pass: imports, nested types, enums, oneof,
optional presence, maps, bytes, Timestamp and external mappings. Six separate
configuration probes reproduce absent pbrs build methods, with configured
prost output as a positive control. These describe API/representation migration
work, not malformed generated wire. The initial WKT compile failure was an
incorrect fixture module placement; its log is retained and the final consumer
uses the documented crate-root layout.

The alloc-only runtime control compiles prost with default features disabled
for `thumbv7em-none-eabi`. The pbrs equivalent, also with default features off,
fails because it requires `std`. The installed target and successful reference
control exclude a missing toolchain target as the explanation. This is a real
functional deployment gap; neither consumer runs on embedded hardware here.

The raw-wire gRPC baseline has four differing request/response cases. The
repair distinguishes a missing message from a framed zero-length protobuf.
Single-request shapes and successful single-response shapes require a message;
zero-message streaming inputs remain valid. Non-OK peer status keeps precedence.
All 18 comparisons match tonic after the fix.

The original mixed-peer run passes 20 tests and fails 21. A serial rerun
reproduces the connection-info failure. The optional tonic service adapter now
materializes native TCP/TLS extensions before forwarding the HTTP request.
All 41 mixed-peer tests then pass. The first after logs precede a five-line
binary-log status-ordering correction; the final committed-source rerun covers it.

## Codec measurements

Eight cases each have parse-only and parse/read-all rows. Each row has five
randomized process pairs and three samples per process: 160 shipping processes,
480 CPU samples, and 32 separate allocation controls. All semantic prechecks,
wire fingerprints and read-all checksums agree. Measurements include dropping
the owned result and exclude fixture generation and output formatting.

The complete table is retained in `work/proof-codec/notes.md` inside the archive
and in machine-readable `results.json`. Two important losses:

| Read all fields | pbrs CPU ns/op | Prost CPU ns/op | Median paired ratio | pbrs allocations / bytes | Prost allocations / bytes |
|---|---:|---:|---:|---:|---:|
| Nested depth 8 | 671.45 | 149.31 | 4.473 | 8 / 608 | 7 / 112 |
| Nested depth 64 | 28,914.35 | 1,722.49 | 16.921 | 64 / 5,344 | 63 / 1,008 |

The depth-64 parse-only ratio is 0.481. Reading its deferred fields reverses
the result. Independent source review confirms repeated descendant validation:
63 + 62 + … + 1 = 2,016 visits along this chain, plus materialization. This is
source attribution, not an instruction profile or a shipped parser repair.
A future optimization must preserve malformed-input, recursion, required-field
and merge checks. Scalar, sparse and packed-field wins remain in the full table.

Allocation bytes are cumulative Rust allocator requests, not RSS or peak live
memory. CPU uses `CLOCK_PROCESS_CPUTIME_ID` in a release executable without
allocation counters. Five pairs run on a shared x86_64 KVM host with CPU 0
pinned, five visible CPUs and a four-CPU quota; no additional cgroup throttling
occurred during the 20.6-second codec campaign. Some measured batches are only
1.70 ms. The descriptive bootstrap intervals do not qualify a ranking.

## RPC measurements

The first campaign completes 90 captures and 454,950 successful RPCs across
three cross-peer combinations, five repeats and N/2N process pairs. It covers
1 KiB unary, 64 × 1 KiB pipelined bidi, and four-message gzip bidi with 1 MiB
noise bodies. Native/pbrs zero-mode peers and tonic/prost peers check lengths
and counts, not every payload byte. The native/pbrs server uses the richer
Interop handler, so its server CPU includes unequal handler work. Native and
tonic Prost noise peers validate complete payload bodies.

An independent review found that original pipeline slopes include zero or
negative values. Their ratios remain null. Before collecting new data, the
follow-up plan increases pipeline counts from 100/200 to 1,000/2,000 and pins
Prost on both sides. This holds the codec and valid-path handler logic fixed.
The second campaign completes 60 captures and 495,000 successful RPCs. It
retains all five pairs; it does not replace or filter the original campaign.

| Same-Prost-codec cell | Endpoint | Native CPU µs/RPC | Tonic CPU µs/RPC | Median paired native/tonic ratio | Pair range |
|---|---|---:|---:|---:|---:|
| 1 KiB unary | Client | 21 | 66 | 0.303 | 0.212–0.579 |
| 1 KiB unary | Server | 46 | 31 | 1.515 | 1.097–1.704 |
| 64 × 1 KiB pipeline | Client | 200 | 320 | 0.625 | 0.340–2.625 |
| 64 × 1 KiB pipeline | Server | 250 | 350 | 0.800 | 0.694–1.917 |

Displayed CPU values are separate medians. The paired ratio is the median of
each repeat's ratio, so dividing the displayed medians can differ. Pipeline
ranges cross one and remain inconclusive. Native unary server CPU loses in
all five local pairs even with the codec and handler logic matched.

In the original full-body noise/gzip cell, median paired client/server CPU
ratios are 1.036/1.009, with ranges 0.855–1.194 and 0.973–1.065. These are mixed
local results, not a compression speed win. All original unary and pipeline
rows, including the richer-handler limitation, remain in `results.json`.

These 150 captures contain 949,950 successful RPCs and zero terminal errors.
They use counters-off release builds, two Tokio workers per endpoint, one
connection, one call in flight and plaintext loopback. Client CPU is lifetime
process CPU; server CPU covers post-READY through client exit. N/2N differences
approximate incremental cost using 100 Hz `/proc` counters; they are not direct
steady-state CPU windows. Startup, connection handling, scheduling and cleanup
can affect them. Source config matches HTTP/2 settings, but the load captures
have no effective FAIRNESS/socket attestation. Compression checks verify the
encoding header, not captured per-frame flags. The shared VM, absent dedicated
separate hosts, short closed-loop runs and only one comparator prevent
qualification. No latency, sustainable throughput or overall ranking is claimed.


## Correctness and evidence checks

At clean committed source `e14cccb5`, the full all-feature workspace passes
3,921 tests with zero failures and nine existing ignores. Strict core and
transport linting pass. The final 18-comparison raw-wire proof and 41-test
mixed-peer suite pass, including the final binary-log status ordering.

Both Rust 1.85 native and Tower library checks pass. Official protobuf v35.1
conformance completes two required runs and one recommended run: each has
5,631 binary/JSON and 909 text successes, zero skips and zero unexpected
failures, with maximum Edition 2023. The codegen audit, two differential
regressions and 13 evidence-checker tests pass. A final documentation run
passes all 25 link, contract and snippet checks; workflow YAML parses and the
405-card plan passes dependency validation. The strict protobuf comparison
exits 1 on the exact recorded differences; the driver verifies that expected
failed-equivalence outcome separately from the successful regression checks.

The new RPC CPU preflight supports named C8/D4 comparisons. It validates
required peers, hashes, source/build pins, effective settings, raw paired runs,
call conservation, warmup/duration/sample floors and CPU/p99/RSS bounds. It
recomputes intervals instead of trusting a winner label. Thirteen tests retain
CPU/tail/memory losses, uncertainty, missing requirements, malformed inputs and
synthetic fixtures. Synthetic fixtures can test the checker but cannot qualify
as measurements. Published diagnostic summaries and this local RPC report do
not have its claim-grade raw schema; their rejection does not reinterpret
historical diagnostics as qualified comparisons or losses.

The host audit and preflight explicitly leave independent provenance review,
dedicated separate hosts on both architectures, broader peers, equivalent work,
real-network load, holdouts, scaling and sustained resource gates open. The
[benchmark contract](../../benchmark-contract.md) defines the full scope.
The compiler audit adds no generator-speed or consumer-build ranking.
