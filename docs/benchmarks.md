# Benchmarks

This page explains the local benchmark records for `pbrs`, `pbrs-grpc`, and
the tonic adapter. It is for Rust developers who want to understand where the
project is fast, where it loses, and which numbers are only diagnostic.

The recorded runs show workload-specific wins and important losses. They do
not prove universal performance leadership or production readiness.

## Start here

For current evidence status, read the [scoreboard](scoreboard.md). To run a
specific measurement, choose a harness below. The detailed tables later on
this page preserve historical captures; they are not a fresh run of HEAD.

| Question | Tool and starting point | What a successful run proves |
|---|---|---|
| Did this change reduce local instruction or allocation cost? | [Dev-loop harness](../bench/devloop/README.md) and [profiling guide](profiling.md) | A local before/after diagnostic, subject to metric availability and noise |
| How do codec peers compare on real schemas? | [Corpora](../bench/corpora/README.md), [C++](../bench/xlang/cpp/README.md), [upb](../bench/xlang/upb/README.md), and [Go](../bench/xlang/go/README.md) peers | Supported operations with recorded semantics; dynamic C++/upb baselines do not establish peak generated-code performance |
| What does code generation cost users? | [Codegen matrix](../bench/codegen/README.md) | Generation and downstream consumer cost for the selected corpora and features |
| Does the official worker protocol work? | [QPS worker](../rpc-bench/README.md) | Selected scenario compatibility and result accounting |
| Which client or server limits throughput? | [Cross-stack matrix](evidence/stack-matrix-sb11.md) | Open-loop probes with separate processes; smoke does not find every ceiling |
| How does connection count, RTT, or overload affect behavior? | [Scenario guide](../bench/stack-matrix/scenarios/README.md) | The supported diagnostic cells; invalid/unsupported rows remain incomplete |
| Can we publish a comparative claim? | [Benchmark contract](benchmark-contract.md) | Only a complete controlled campaign meeting its statistical and correctness rules |

### Evidence update: 2026-09-29

Several tools have advanced beyond the historical tables on this page:

- The [SB-09 codegen matrix](evidence/sb09-codegen-matrix.md) includes
  165 completed cell-runs across seeded, realistic, and service-stub corpora.
  Its five-repeat diagnostics report pbrs losses on 179 of 240 peer-metric
  comparisons, mainly downstream compilation and memory. Later GN changes
  have narrower before/after evidence; they do not replace a full matrix rerun.
- The [official QPS worker](evidence/qps-sb10.md),
  [cross-stack smoke](evidence/stack-matrix-sb11.md), and
  [public grpc_bench entry](evidence/grpc-bench.md) have run. They establish
  useful compatibility and harness evidence, not performance leadership.
- [Claim scenario definitions](evidence/sb21-claims-scenarios.md) exist,
  but the worker's schedule-relative latency export, repeat-order execution,
  and claim-scenario loader remain integration work. A checked-in scenario
  does not mean a claim campaign can yet run unattended.
- The perf CI lane is advisory. Its repair needs a successful measured
  base/head run; blocking thresholds need SB-20's noise analysis. A green job
  containing only error placeholders is not performance evidence.

The [evidence index](evidence/README.md) collects the detailed records and
their limits. The [execution plan](plan/world-class/README.md) owns the next
tasks.

## How to read this page

- **Read the caveats first.** These are historical local measurements. They do
  not include complete dated source revisions and per-run raw artifacts for
  every table.
- **Check the unit in each header.** Lower is better for time, latency,
  memory, resident set size (RSS), and binary size. Higher is better for
  throughput, queries per second (QPS), messages per second, and ratios.
- **Do not compare owned decoders to borrowed views as equals.** Owned decoders
  produce messages independent of the input buffer. Borrowed views can retain
  slices of the input wire bytes.
- **Treat gates as smoke checks.** A process gate means the local harness exits
  non-zero for that loss. It is not the same as claim-grade comparative
  evidence.
- **Use the contract for stronger claims.** The
  [benchmark contract](benchmark-contract.md) defines the evidence required
  before promoting a comparative performance claim.

## Summary

| Area | Where pbrs looked faster | Where pbrs looked slower or open | What the numbers do not prove |
|---|---|---|---|
| Core codec `bench` | All twelve gated cases still won encode and owned decode against prost, v4 upb, and buffa owned in the recorded Apple M4 Pro capture. | `tat_populated` and `person` were in a ~3% band behind buffa view and are not process-gated. Large packed-fixed results are only reported, not gated. | No universal codec ranking or view-versus-owned equivalence; retained cells are Rust-heap only (v4 arena excluded). |
| Large payloads | pbrs, v4 upb, and buffa owned were in the same band for owned bytes encode/decode at 1-5 MiB. | buffa `decode_view` on bytes does not copy. v4 encode was a bit faster for 5 MiB packed fixed32. | No zero-copy claim from these rows: they time the copying `parse(&[u8])` path. The newer shared-buffer path (`parse_bytes`) is covered in [Large payloads / zero-copy](zero-copy.md). |
| tonic codec survey | Most common-shape cached-encode-plus-parse rows beat prost and v4 in the historical capture. | pbrs loses some small common-shape rows versus prost (`empty`, `id`, `name_80`) and has diagnostic mutation/first-encode caveats. | No claim that historical "fresh" encode cells are direct first-encode timings. |
| Native gRPC transport | Fair-loopback results show pbrs-grpc ahead on p50 latency, throughput, server-streaming, and upload. | tonic leads empty-unary p99 and ping-pong in the fair rerun; old Xeon tables are superseded by SB-01. | No production tail-latency, network, or multicore leadership claim. |
| Codegen and downstream compile | SB-09 records generation-time wins on some corpus/peer pairs; the earlier small pair favored pbrs binary size. | SB-09 records broad generated-size, check/build, and memory losses; subsequent GN improvements need the same full matrix rerun. | No controlled-host qualified codegen claim. |

## Provenance and scope

The numeric tables below are historical local measurements, not a current
performance-leadership claim. Rerun commands alone cannot recreate their host
conditions or supply uncertainty intervals.

The page keeps every recorded caveat visible: missing raw artifacts, diagnostic
versus claim-grade status, local host sensitivity, owned-versus-view
differences, and known losses.

## Core codec method

The historical kernel comparisons target the same `.proto` per row. Most cases
use `TestAllTypesProto3` (TAT), Google's kitchen-sink conformance message.
pbrs types in the historical kernel table are plugin-generated, except
`person`, which uses the handwritten `pbrs::testdata::Person`.

That handwritten type has no typed `extras` field (tag 16). The Person
specimens leave the field empty, so this is populated-field equivalence rather
than full-schema equivalence.

Competitors are prost 0.13 (`prost-build` of that proto), crates.io
`protobuf` 4.35.1-release (`protoc --rust_out kernel=upb`), buffa 0.9.1
owned, and buffa `decode_view` where it exists.

### Workload taxonomy and semantic-bias controls

The benchmark contract's BM-01 and BM-03 rules require workloads to avoid
semantic bias across layout, buffer ownership, caching, and access patterns:

1. **Handwritten vs. generated schemas.** `person` uses handwritten
   `pbrs::testdata::Person`, which stores up to four repeated
   `ProtoString` values inline with `InlineVec<ProtoString, 4>`. All other
   historical cases use compiler-generated `TestAllTypesProto3` structures.
   The `person_generated` row uses the generated layout
   (`Repeated<LazyStr>`, `Map<LazyStr, i32>`) to show layout differences. The
   current `tonic-bench` also reports both Person layouts in a same-input
   diagnostic; that does not replace this historical table.
2. **Owned vs. view decode.** Owned decoders (`pbrs`, `prost`, v4 upb, and
   buffa owned) produce messages independent of the caller's input lifetime.
   pbrs can retain its own wire backing and defer field materialization; prost
   eagerly owns fields; v4 uses an upb Arena. Borrowed view decoders
   (`buffa view`) borrow slices directly from the input wire bytes, so they
   stay in a separate column. Per-case retained Rust-heap bytes and
   allocation counts are reported for every decoder; see Retained-Memory
   Footprint for the scope limits.
3. **Fresh, cached, and mutated encode.** Cached encode re-serializes a
   message whose length (`cached_size`) and canonical packed varint bytes
   (`Packed::encoded`) are already computed. Fresh encode serializes a freshly
   parsed message once, before canonical caches are warmed; parsing and
   preparation are outside the timed interval. `fresh_encode_iters` is capped
   at 10,000 and an estimated 32 MiB of prepared inputs per sample in `bench`.
   The current `bench` and `tonic-bench` executables directly time this path,
   but the historical tonic tables below predate that fix. Fresh construction
   (`new` plus field setters, no parse) is timed as a separate column in
   `tonic-bench` for every row: each codec builds the same specimen its other
   columns encode, and a rebuilt message must serialize to the checked wire
   before the row is accepted. `bench` does not time construction. Mutated
   encode alternates a field before every encode to include cache invalidation
   and size recomputation: `bench` alternates one scalar per case, while
   `tonic-bench` reports separate three-codec `Person` ID and variable-length
   name-mutation rows for handwritten and generated pbrs. Those rows are
   diagnostic and do not replace cached encode rows.
4. **Parse-only vs. parse-and-touch.** Parse-only deserializes wire bytes and
   drops the message without field access. Parse-and-touch recursively accesses
   string, bytes, and collection fields on the parsed message so deferred
   materialization and accessor costs show up. In `tonic-bench` the touch
   checksum reads every populated field and every string/bytes payload byte,
   and must match across codecs before timing.

Decode uses pbrs wire bytes. `./bench` from `bench/` runs 40000 iterations and
reports the median of 15 after warmup. Builds are release, thin link-time
optimization (LTO), one codegen unit. `size_of::<TestAllTypesProto3>()` is
648. Default is ~19 ns.

In the next two codec tables, each implementation cell is `encode / decode` in
nanoseconds. Payload is encoded size in bytes. Lower is better. Buffa view has
no encode, so that side is `n/a`.

`./bench` exits non-zero if a gated case loses encode or owned decode to prost,
v4, or buffa owned. Twelve cases are gated: the original nine plus
`packed_fixed64_256`, `packed_float_256`, and `repeated_nested_8`. Buffa view
is gated except `tat_populated`, `person`, and the packed-fixed rows. The view
does not build an owned `Vec`; `person` and `tat_populated` sit in a ~3% band
versus buffa view and are not process gates. These legacy owned-versus-view
gates are smoke checks only, not comparable codec evidence under the benchmark
contract; replacing them requires BM-13.

Per-case JSON also carries `"workload"` (primary/holdout), per-codec
retained Rust-heap bytes/allocation counts, and `"raw_samples"`: the raw
per-sample values behind the gated cached-encode and owned-decode medians,
in sample order, so medians can be recomputed and uncertainty bounded
without rerunning. Raw samples are fixed-order sequential runs, not
interleaved or randomized pairs.

JSON, text, proto2 required, maps larger than 64, and well-known types (WKT)
are not gated. 1 MiB and 5 MiB rows are reported below and are not gated.
Iteration counts drop with payload (120x9 at 1 MiB, 40x7 at 5 MiB) so the
timer stays memcpy-bound rather than a 40k-iteration wall clock.

Numbers below are from one Apple M4 Pro. Two consecutive
`./target/release/bench` runs were captured; the second capture is below. Both
runs exited 0, so all twelve gated cases still win encode and owned decode.

## Gated

Lower is better. Each implementation cell is `encode / decode` in nanoseconds.

| case | payload (bytes) | pbrs encode/decode (ns) | prost encode/decode (ns) | v4 upb encode/decode (ns) | buffa owned encode/decode (ns) | buffa view encode/decode (ns) |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **26 / 20** | 83 / 130 | 144 / 74 | 67 / 163 | n/a / 115 |
| person | 62 | **35 / 83** | 40 / 198 | 73 / 159 | 42 / 154 | n/a / 81 |
| TAT populated | 87 | **91 / 306** | 161 / 447 | 244 / 398 | 133 / 393 | n/a / 305 |
| packed varint 256 | 388 | **77 / 246** | 537 / 806 | 465 / 893 | 584 / 358 | n/a / 393 |
| map 64 | 500 | **258 / 939** | 663 / 2316 | 989 / 3147 | 411 / 1767 | n/a / 1102 |
| nested depth 8 | 26 | **245 / 130** | 2343 / 1059 | 1131 / 354 | 629 / 1378 | n/a / 1457 |
| strings | 163 | **62 / 153** | 118 / 328 | 191 / 173 | 104 / 311 | n/a / 190 |
| unpacked varint 256 | 896 | **448 / 1141** | 531 / 1592 | 767 / 2513 | 811 / 1601 | n/a / 2440 |
| packed fixed32 256 | 1028 | **53 / 90** | 199 / 715 | 223 / 129 | 176 / 195 | n/a / 151 |
| packed fixed64 256 | 2052 | **65 / 94** | 221 / 752 | 253 / 140 | 182 / 197 | n/a / 155 |
| packed float 256 | 1028 | **53 / 89** | 236 / 715 | 220 / 135 | 192 / 198 | n/a / 147 |
| repeated nested 8 | 38 | **62 / 142** | 120 / 217 | 198 / 287 | 118 / 282 | n/a / 255 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

## Extended

Reported, not gated. Lower is better. Each implementation cell is
`encode / decode` in nanoseconds.

| case | payload (bytes) | pbrs encode/decode (ns) | prost encode/decode (ns) | v4 upb encode/decode (ns) | buffa owned encode/decode (ns) | buffa view encode/decode (ns) |
|---|---:|---:|---:|---:|---:|---:|
| person_generated | 62 | **37 / 198** | 40 / 197 | 76 / 162 | 40 / 160 | n/a / 80 |
| bytes | 315 | **73 / 168** | 134 / 508 | 224 / 204 | 122 / 384 | n/a / 219 |
| scalars (bool/enum/float/packed bool) | 77 | **68 / 167** | 163 / 318 | 182 / 223 | 101 / 228 | n/a / 226 |
| unpacked fixed32 256 | 1536 | **218 / 801** | 289 / 1451 | 622 / 1661 | 594 / 1508 | n/a / 2096 |
| oneof string | 23 | **39 / 87** | 96 / 143 | 149 / 99 | 82 / 169 | n/a / 122 |

`person_generated` uses compiler-generated layout (`Repeated<LazyStr>`,
`Map<LazyStr, i32>`). The recorded 83 ns versus 198 ns decode result is for
these two complete implementations; it does not isolate `InlineVec` as the sole
cause of the difference.

## Losses

No gated encode or owned-decode loss. `tat_populated` versus buffa view
is a coin-flip on this capture (306 vs 305 ns) and is still not
process-gated. Person versus buffa view is the same ~3% band (83 vs 81)
and is not process-gated. Packed-fixed view rows win on this host but
are not process-gated: view does not materialize a `Vec`.

## Large payloads (reported, not gated)

Same TAT schema. Lower is better. Each implementation cell is
`encode / decode` in microseconds, not nanoseconds. One Apple M4 Pro; second
of two runs.

| case | payload (bytes) | pbrs encode/decode (µs) | prost encode/decode (µs) | v4 upb encode/decode (µs) | buffa owned encode/decode (µs) | buffa view encode/decode (µs) |
|---|---:|---:|---:|---:|---:|---:|
| bytes 1 MiB | 1,000,004 | 12.1 / 12.0 | 12.3 / 24.4 | 12.2 / 12.5 | 12.2 / 12.3 | n/a / **0.12** |
| bytes 5 MiB | 5,000,005 | 61.6 / 59.9 | 61.7 / 123.5 | 60.6 / 60.7 | 65.1 / 60.7 | n/a / **0.12** |
| packed fixed32 1 MiB | 1,000,005 | 12.0 / 12.0 | 158 / 446 | **12.1 / 11.7** | 127 / 12.3 | n/a / 12.7 |
| packed fixed32 5 MiB | 5,000,006 | 72.7 / 65.7 | 788 / 2527 | **60.5 / 66.2** | 629 / 66.3 | n/a / 64.8 |

At 1-5 MiB the v4 Arena/foreign function interface (FFI) tax is gone. Owned
encode/decode of a bytes blob is a memcpy of the payload. pbrs, v4, and buffa
owned sit in the same band. prost decode is about 2x. buffa `decode_view` on
bytes does not copy (~0.12 µs).

packed-fixed is memcpy for pbrs and v4, and a recode for prost / buffa
owned encode. At 5 MiB v4 encode is a bit faster (60.5 vs 72.7 µs). Decode
is a few percent either way. packed-fixed view still copies; it is not
the bytes-view shortcut.

## Why v4 encode is large on small sizes

Every v4 `serialize` allocates an Arena, calls FFI `upb_Encode`, and
copies to `Vec`. Codec work on <1 KiB is tens of ns. Setup is hundreds.
See `docs/upb.md`.

## Retained-Memory Footprint

Both codec harnesses report retained **Rust-heap** bytes and allocation calls
per parsed message: one parse per codec from the same wire, measured with a
counting global allocator after the equivalence pre-checks warm any one-time
initialization. `bench` emits `*_retained_bytes` / `*_retained_allocs` per
case; `tonic-bench` prints a retained section per row group.

Scope limits are labeled in the output (`retained_scope: rust-heap`) and
apply to every comparison:

- v4 cells count the Rust side only. The upb Arena lives on the C heap, so
  `g36`/`v4` retained cells undercount by the whole arena.
- Borrowed views (`buffa view`/`lazy`, quick-protobuf in `bench`) retain the
  caller's wire buffer, which sits outside the counted window; their small
  cells do not include that buffer.
- Counts are single-message, same-process snapshots, not RSS peaks, and the
  samples are fixed-order sequential runs, not randomized pairs.

Until arena-aware and RSS evidence is recorded with identical input-buffer
lifetimes, retained-memory comparisons stay diagnostic, not claim-grade.

## Holdout-Schema Coverage

Both harnesses measure holdout validation cells alongside the tuned sets
(benchmark contract §3.5). Holdouts are reported, never gated, and never
tuned against; they exist to catch overfitting the primary rows.

`bench` labels every case `"workload": "primary"` (the twelve process-gated
cases) or `"workload": "holdout"` (the nine reported-only cases:
`person_generated`, `bytes`, `scalars`, `unpacked_fixed_256`, `oneof`, and
the four 1-5 MiB rows), in addition to the existing `"topology"` label.
Gates are unchanged historical smoke until BM-13 approves a replacement.

`tonic-bench` measures four holdout shapes from the existing
`codec_cases.proto` types with populations the tuned survey does not cover:
`nest_d8` (deeper recursion than `nest_d4`), `oneof_err` (the unmeasured
`err` union variant), `rpc_sparse_path` (sparse `Rpc` populated at `path`
instead of `id`), and `headers_1` (single-entry map instead of the wide
`map_8`). All four are byte-stable, so wire equality is checked before
timing, and they print in a separate holdout section with their own
first-encode, construction, retained-memory, and raw-sample rows.

The topology table below names the regression risk each family guards; the
`bench` and `tonic-bench` holdout rows above are the measured instances,
not proposals. Their risks still cannot be counted as performance results
beyond the reported cells:

| schema / topology | characteristics | pbrs behavior | regression risk guarded |
|---|---|---|---|
| **Deep recursive trees** (`nest_d4`, `nested_8`) | Deep submessage nesting, no collections | Eager recursion validation, stack-bounded by `RECURSION_LIMIT` | Stack overflow and pointer chasing |
| **Sparse wide messages** (`rpc_sparse`, `empty`) | Hundreds of optional fields, 1 field populated | Fast tag scanning, cold field bypass via `Option<Box<Cold>>` | Size bloat and zero-initialization overhead |
| **Wide header maps** (`map_8`, `headers`) | Dense string-to-string mapping, duplicate key checks | Inline entry decode with `MapView`, small lookup arrays | Hash collision and map rehash stalls |
| **Unaligned packed varints** (`packed_256`, `unpacked_256`) | Multi-byte little-endian base-128 (LEB128) sequences, variable widths | Single instruction, multiple data (SIMD) varint validation + lazy canonical cache | Varint decoding throughput and recoding allocation |
| **Heterogeneous unions** (`oneof_ok`, `oneof`) | Polymorphic variants, tagged union representation | Rust `enum` variant with direct payload access | Tag mismatch and union memory inflation |

## Re-run

```bash
cd bench && cargo build --release && ./target/release/bench
```

## Codegen and downstream compilation (CG-19 diagnostic)

The current broader harness is documented in the
[codegen guide](../bench/codegen/README.md), with the recorded five-repeat
matrix in [SB-09 evidence](evidence/sb09-codegen-matrix.md). The section below
preserves the earlier CG-19 fixed-order, six-message reference diagnostic.
Its statements about unrun larger reference cases describe that specific
`--reference-protoc` flow, not the later `--generators` matrix.

This diagnostic measures code generation and downstream Rust compilation. It
is **unqualified**: useful for finding losses, but not enough for a release or
performance claim.

### Running the diagnostic

`./scripts/codegen-bench.sh --case small` runs the bounded six-message case.
Omit `--case` to request all three seeded, multi-file proto3 corpora:

| case | corpus size |
|---|---|
| `small` | 6 messages / 2 files |
| `100` | 100 messages / 5 files |
| `1000` | 1,000 messages / 20 files |

The default seed is `190019`; `--seed N` changes it. Results go to a **new**
`target/codegen-bench/<UTC timestamp>-<pid>/` directory, or to a new `--out`
directory beneath `target/codegen-bench`. No existing evidence is removed.

Requirements and resource notes:

- The command needs installed `protoc`, Cargo, and Rust.
- It uses Cargo `--offline` and this checkout's `pbrs` path dependency.
- It does not download or add dependencies.
- Run it only when compiler jobs are not competing for resources.
- `--jobs` defaults to `CARGO_BUILD_JOBS` or 2.

### What the harness measures

The Python harness generates and hashes every `.proto` from a fixed SHA-256
field-selection scheme. It then builds a small out-of-band Rust driver that
uses `pbrs::codegen::Config::compile_protos` with messages-only stubs, normal
reflection, and the recorded `protoc` executable. In opt-in comparison mode,
that `protoc` is pinned.

The measured generation phase **includes `protoc` descriptor compilation and
Rust emission**. It does not include Cargo. A second identical invocation
checks all `.rs` bytes, paths, and nanosecond modification times without
rewriting them.

The downstream consumer includes the generated `mod.rs` and retains every
message through parse/serialize calls. Separate phases measure:

- an initially empty per-corpus-target `cargo check`;
- a second check after a real consumer-source edit, asserting Cargo rechecked
  that consumer;
- an offline `cargo build --release` with opt-level 3, thin LTO, one codegen
  unit, and binary size.

Bootstrap and offline lockfile resolution are recorded but **not counted as
generation or consumer check**. Bootstrap reuses the shared
`target/integration-consumers` Cargo cache with jobs capped at 2, then copies
and hashes the executable under the new run's `bin/`. Each corpus still uses
its own **empty** target for a genuine cold check. Local Cargo registry and
compiler-wrapper caches are **not** cleared.

The cold check includes normal `pbrs` dependency compilation and its build
script; it does not rerun corpus generation. Both the driver and that build
script use the recorded `protoc`; the build script uses it through a local
`PATH` symlink. See the [harness notes](../bench/codegen/README.md) for the
cache and isolation boundary.

### What the JSON records

Each `summary.json` (`schema_version: cg19/1`) records:

- pbrs source-tree, wrapper, lockfile, and copied-binary SHA-256s;
- Git HEAD and tracked dirt;
- compiler and `protoc` paths and versions;
- host, cache, and flag data;
- per-corpus and per-input SHA-256s;
- generated bytes, hash, and modification-time assertion;
- per-phase `elapsed_ns`, `peak_rss_bytes`, and raw
  `logs/<case>/<phase>.{stdout,stderr}.log` paths;
- release binary bytes.

RSS is the maximum of the operating system time command's direct-process peak
and a 100 ms sampled descendant-tree **sum of RSS**. That value is a lower
bound on the descendant sum's high-water mark. It is not physical memory:
shared pages can be counted twice, and it is not a complete allocator-aware
peak.

`status: error` and a nonzero exit preserve partial data and logs on
measurement failure.

After each release build and binary hash, a separate **untimed**
`release_smoke` runs the binary from its own consumer directory with a maximum
15-second timeout. It requires exit 0, stdout exactly `1\n`, and empty stderr.
A passing phase records raw stdout/stderr logs, hashes, current working
directory (cwd), timeout, exit code, and `output_verified`. A failing phase
retains status and log paths, plus exit code if the child exited, and stops
comparison without reporting empty losses as success.

The smoke does not invoke `/usr/bin/time` because its resource report would
make raw stderr nonempty. It contributes no timing or RSS row to the 12 cost
metrics.

### Reference opt-in

**Default behavior is unchanged:** no reference is invoked. Successful
pbrs-only runs retain `reference.status: missing`,
`comparison.status: not_run`, `comparison.losing_cells: null`, and
`qualification.qualified: false`. The previous 6/100/1,000-message cells are
historical pbrs-only measurements, **not** pairs.

An explicit reference opt-in accepts **one** seeded 6-, 100-, or 1,000-message
corpus at a time. `--case all` or an omitted case is rejected before creating
artifacts to avoid unplanned cold builds. For the existing **single local
six-message pair** with the pinned compiler, not an arbitrary `PATH`
`protoc`, use:

```sh
CARGO_BUILD_JOBS=2 ./scripts/codegen-bench.sh --case small \
  --reference-protoc "$PWD/target/pinned-protoc-build/protoc" --jobs 2
```

Substitute `--case 100` or `--case 1000` only when there is an approved
compute window for **two separate cold consumer builds** including upstream
C/upb. No such timed larger reference runs or dedicated-host qualification
are recorded here. A local **generation-only** proof with genuine pinned
`libprotoc 35.1` (verified compiler hash above) produced 6 Rust files for
the 100-message corpus and 21 for the 1,000-message corpus, including
`generated.rs`; all 5/20 per-file module references were present. The
respective generated Rust tree digests were
`1c91b8a07095e3d1a5972933ec6ffda40be5b9c0a067303196c48cba2be19ee4`
and
`2727bb5aa3979c1e6e085d480dbfa1b4e0fc2290c46384d2f03daa43604001d4`.
The opt-in Python harness test reproduces these generated-byte checks without
running `rustc` or comparing performance; it does **not** establish that
100/1,000-message consumers build or run.

### Reference comparator boundary

The opt-in fails closed unless that compiler is genuine `libprotoc 35.1`,
SHA-256 `e2b116ef44d4b7f3246945ceb1938c72f04e16040020e321ac601869135ab940`,
from the clean checked upstream source at
`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`. Both generators receive the
same byte-identical `.proto` inputs:

- proto3 scalars;
- repeated and map fields;
- cross-file imports;
- every concrete message in the selected corpus;
- normal reflection metadata.

Both consumers perform the same `new`/serialize/parse/serialize calls per type,
but upstream's generated `generated.rs` module differs from pbrs's `mod.rs`.
Upstream uses its built-in Rust output
(`experimental-codegen=enabled,kernel=upb`) and **independent** registry
`protobuf`/`protobuf-macros` `4.35.1-release` dependencies, verified by
version and Cargo checksum. The reference lockfile must not contain `pbrs`.

Opt-in requires Python 3.11+ for standard-library lockfile verification. That
runtime compiles C/upb through the recorded C compiler. Pbrs uses its own Rust
runtime and a pinned-protoc descriptor subprocess.

Each side gets an initially empty cold-check target and the same two-job Cargo
profile. Bootstrap is excluded, while registry and compiler-wrapper caches are
shared and **pbrs always runs first**.

These are full consumer/build costs. They are not isolated generator, C-free,
identical application binary interface (ABI), or randomized cold-host
comparisons. The reference repeat generation may rewrite identical Rust files;
both bytes and modification-time outcomes are explicit in the report. Only
default-message values execute, so this is not a nonempty-field semantic test.

### Small paired diagnostic

The **corrected** local small-cell diagnostic at
`target/codegen-bench/20260924T211609Z-63179/summary.json` has **40 retained
raw stdout/stderr logs**, 12 paired numeric cost metrics and **eight losing
metrics** for pbrs (raw units are ns for time and bytes for RSS). All seven
previously observed losses persist; this run also observes a generation RSS
loss, which is subject to the sampling limitation above:

| Metric where pbrs is larger (unit in metric name; lower is better) | pbrs raw value | Pinned upb reference raw value |
|---|---:|---:|
| generation elapsed ns | 294591958 | 178592792 |
| generation peak RSS bytes | 19709952 | 18235392 |
| clean check elapsed ns | 8771809791 | 5682561709 |
| clean check peak RSS bytes | 1036795904 | 441270272 |
| incremental check elapsed ns | 237852459 | 150435834 |
| incremental check peak RSS bytes | 127369216 | 107036672 |
| release build elapsed ns | 37132551417 | 10040398125 |
| release build peak RSS bytes | 1583726592 | 468123648 |

The other four raw metrics (including unchanged-generation time/RSS) are in
`comparison.metrics`; pbrs's generated Rust totals **126195 versus 127468
bytes**, and release executables **527808 versus 658160 bytes**. Both
release binaries passed their retained `release_smoke` proof: exit 0, stdout
`1\n`, empty stderr and a 15-second cap. The raw output logs are
`logs/small/release-smoke.{stdout,stderr}.log` and
`logs/small/reference/release-smoke.{stdout,stderr}.log` beneath that run.
Pbrs preserved all three output mtimes, while the reference rewrote three
byte-identical files. RSS estimates have the sampled lower-bound/shared-page
limitations above. This one macOS pbrs-first pair does **not** establish a
relative performance ranking, uncertainty bound, or claim about 100/1,000
messages. It remains `status: unqualified`,
`qualification.qualified: false`, with every loss present in
`comparison.losing_cells`; `--require-qualified` exits 2 even if requested
with a reference. CG-19 and the [benchmark contract](benchmark-contract.md)
still require the 100/1,000-message reference cells, multiple randomized
paired runs with uncertainty on independent pinned hosts, controlled cache
policy, retained/published raw data, and review of all losing cells before a
qualified claim. No continuous integration (CI) performance gate or release
claim follows from this diagnostic.

The earlier pair at
`target/codegen-bench/20260924T205157Z-28795/summary.json` retains its
historical 36 logs and seven losses; its binaries were checked manually
afterward, not by that harness. A subsequent attempted correction at
`target/codegen-bench/20260924T211311Z-57833/summary.json` stopped before
the reference ran when `/usr/bin/time` contaminated the first smoke's
stderr; it has `status: error` and no comparison, not a paired result.

## tonic Codec survey (Apple M4 Pro)

Same-process encode into `BytesMut` (`Serialize::encode` / prost
`Message::encode`). v4 is `Serialize::serialize` (new Arena + FFI +
copy; no EncodeBuf). Not kernel `./bench`. Release-mode timing is not in CI;
unit correctness is. Two consecutive
`./target/release/tonic-bench` runs; second capture below.

`hello` / `hello_4kib` are the old 1-string rows. The other historical
tables use `proto/codec_cases.proto`: one message per common unary shape,
so gencode is specialized (hello-sized), not TestAllTypes. The new Person
diagnostics below use `proto/person.proto` instead.

For the `hello` rows, pbrs/prost use `hello.proto` but v4 uses the
wire-equivalent `codec_cases.proto` `Name`; the generated schemas are not
identical even though their encoded bytes and observed name match.
The v4 common-shape bindings are byte-checked output from the pinned
protobuf v35.1 Rust generator, matching runtime `4.35.1-release`. CI checks
their source/generated SHA-256 values and compiles that output; a default
local build requires the genuine pinned `protoc 35.1` and rejects a newer
compiler whose gencode version was merely rewritten.

**Historical first-encode caveat:** the `pbrs enc (fresh / cached)` numbers in
the tables below were captured by an older harness that subtracted separate
parse and parse+encode medians and clamped the result to cached encode. Those
fresh numbers are estimates, **not** direct first-encode measurements; the
historical tables have not been rerun.

The current `tonic-bench` directly times the first encode of separately parsed
messages for **pbrs, prost, and v4**, using the same input wire, sample count,
and per-case prepared-message count (up to 10,000 and an estimated 32 MiB of
prepared inputs per sample). Parsing is outside the interval; each message is
encoded once, with a reused `BytesMut` destination for pbrs/prost and a new
v4 Arena/FFI-allocated `Vec`. It consumes full output buffers and verifies
byte equality, or decoded message equality when map iteration order differs,
before timing; parse-and-touch results are also cross-checked. The executable
prints a separate first-encode comparison with the actual prepared-message
count. A case whose estimated prepared message alone exceeds 32 MiB fails
explicitly rather than silently breaching that budget. This is **first encode
after parse**, not the cost of constructing and populating a new object. pbrs
may retain wire-backed lazy fields/canonical
caches, prost materializes owned fields, and v4 uses an upb Arena; first-encode
numbers do not erase those materialization differences. Borrowed views are
reported separately in `bench`, not in this survey. Parse-and-touch
checksums read every populated field and every string/bytes payload byte,
and must match across codecs before timing.

**Person layout diagnostic (not historical, not gated):** `tonic-bench/build.rs`
generates a pbrs `Person` from the repository's `proto/person.proto` alongside
the existing common-shape bindings. Its `person_handwritten` and
`person_generated` layout rows parse **the same 62-byte Person wire**. Each independently
measures repeated encode, first encode after parse, parse-only, and parse-and-touch
for pbrs, prost and the existing v4 upb Person. Full re-encoded buffers must
match the shared input; the touch checks read every populated field and every
string/bytes payload byte (id, name, present email, both tags, the one scores
entry, nested address city, plus the typed extras entry on the extras row),
and match across layouts and codecs before timing. The diagnostic prints
actual iteration/sample counts and raw times without declaring winners or
changing any smoke gate. A third `person_generated_extras` row carries one nonempty typed `extras` map entry
(tag 16) and measures generated pbrs against prost/v4 independently, with
the same sample budget and checked wire/parsed-field/touch equivalence.
There is **no handwritten comparator** for that row. Repeated
prost/v4 encode does not imply a pbrs-style cached-size optimization, and
prost/v4 comparator cells are **timed anew** for each layout, not copied.
Each row also times fresh construction per codec through the same builders.
All these measurements are fixed-order and same-process, not randomized
paired runs on independent pinned hosts.

The handwritten `pbrs::testdata::Person` does **not** expose `extras` (tag 16);
the two-layout fixture leaves it empty for all four representations. The
generated-only row proves that **one** nonempty entry is observable by the
three codecs with a typed field; it does not establish handwritten/generated
full-schema parity for arbitrary `extras` contents.
Generated pbrs retains lazy, wire-backed fields with different repeated/map
storage; prost owns decoded fields, and v4 allocates an upb Arena. Equal wire
bytes for this specimen do not make ownership, retained memory, or full-schema
costs equal.

**Mutation before encode (separate diagnostic, not a gate):**
`tonic-bench` compares `proto/person.proto` with handwritten
**and generated** pbrs Person, a locally prost-derived matching schema, and
the existing v4 upb binding in `rust_out_person/src/person.u.pb.rs` (pinned
to 4.35.1-release). All four rows use the shared input above, with no `extras`.
The diagnostic now has **four** independently measured rows: handwritten
and generated pbrs layouts, each for ID and name mutation. Parse and pre-warm
take place outside the timer. On the same object before **every encode**, the
ID rows alternate 42 and 43 while the name rows alternate `"ada"` and
`"ada lovelace with a longer name"` (different encoded lengths). Timed work
includes the setter/assignment and serialization, not parsing or
construction. pbrs/prost reuse a `BytesMut`; v4 allocates its upb-backed
output. Full buffers are consumed. Before timing, both states for each field
must byte-match the expected wire, reparse with handwritten and generated
pbrs, prost and v4, and agree on populated fields; any mismatch fails the
run. All four rows use the same input and iteration/sample counts within the
existing 10,000/estimated-32-MiB cap.
No raw host qualification or numeric speed claim follows from these
diagnostics; map/bytes/other field-mutation parity remains BM-03 work.

**Construction, retained memory, and raw samples (current executable, not
historical):** every `tonic-bench` row times fresh construction per codec
through the same builders its other columns use, reports retained Rust-heap
bytes/allocation calls per parsed message (v4 excludes the upb Arena), and
contributes its per-sample timings to a trailing `tonic-raw/1` JSON block.
The raw block covers all fifteen survey columns per row plus the four
mutation rows, so medians can be recomputed and uncertainty bounded without
rerunning. Samples are fixed-order sequential runs, not interleaved or
randomized pairs; none of these sections change the historical smoke gates.

Historical table columns report:
- `pbrs enc (fresh / cached)`: older derived fresh estimate alongside cached
  encode (pre-warmed size and pre-encoded packed varints).
- `pbrs dec (parse / touch)`: parse-only decode (dropping message immediately)
  alongside parse-and-touch (accessing selected populated fields).
- `prost enc / dec / touch`: prost encode, decode, and parse-and-touch.
- `v4 enc / dec / touch`: v4 serialize, parse, and parse-and-touch.

Combined win/loss, including the unchanged executable smoke gates, is pbrs
(cached encode + parse decode) vs that column; the first-encode diagnostic
does not change those comparisons.

`tonic-bench` exits non-zero if `name_4kib` or `blob_4kib` combined
encode+decode loses to prost, if `rpc_sparse` decode loses to prost, or
if `tags_32` decode loses to v4.

### Published 1-string

Lower is better for all time columns; timings are nanoseconds.

| case | payload (bytes) | pbrs encode fresh/cached (ns) | pbrs decode parse/touch (ns) | prost encode/decode/touch (ns) | v4 encode/decode/touch (ns) | vs prost | vs v4 |
|---|---:|---:|---:|---:|---:|---|---|
| hello | 5 | 6.1 / 5.5 | 11.4 / 11.7 | 1.5 / 22.2 / 23.3 | 34.2 / 44.3 / 49.9 | win | win |
| hello_4kib | 4099 | 53.8 / 46.1 | 86.8 / 86.7 | 48.4 / 147.3 / 148.8 | 105.6 / 274.2 / 262.0 | win | win |

### Common shapes

Lower is better for all time columns; timings are nanoseconds.

| case | payload (bytes) | pbrs encode fresh/cached (ns) | pbrs decode parse/touch (ns) | prost encode/decode/touch (ns) | v4 encode/decode/touch (ns) | vs prost | vs v4 |
|---|---:|---:|---:|---:|---:|---|---|
| empty | 0 | 2.0 / 1.3 | 0.3 / 0.3 | 0.3 / 0.3 / 0.3 | 27.5 / 32.2 / 32.4 | loss | win |
| id | 2 | 3.7 / 3.3 | 2.3 / 2.0 | 1.0 / 3.6 / 3.6 | 31.9 / 38.3 / 44.8 | loss | win |
| scalars | 23 | 14.2 / 13.8 | 12.9 / 12.9 | 21.1 / 16.1 / 16.5 | 48.2 / 58.3 / 91.9 | win | win |
| name_short | 5 | 5.6 / 4.7 | 9.2 / 9.8 | 1.5 / 24.9 / 23.3 | 35.1 / 45.4 / 51.1 | win | win |
| name_80 | 82 | 6.4 / 6.4 | 21.9 / 20.7 | 4.4 / 23.7 / 22.2 | 32.2 / 43.5 / 48.6 | loss | win |
| name_4kib | 4099 | 55.3 / 45.1 | 92.9 / 89.7 | 48.1 / 144.4 / 148.5 | 106.1 / 265.4 / 263.0 | win | win |
| blob_32 | 34 | 5.6 / 5.0 | 17.9 / 18.1 | 5.7 / 38.6 / 39.8 | 35.4 / 40.9 / 47.8 | win | win |
| blob_4kib | 4099 | 75.0 / 44.2 | 65.4 / 65.2 | 46.4 / 132.0 / 156.5 | 101.5 / 106.4 / 107.3 | win | win |
| blob_64kib | 65540 | 1044.5 / 588.7 | 741.8 / 1310.7 | 578.9 / 1674.2 / 1657.4 | 713.9 / 740.7 / 743.1 | win | win |
| envelope | 30 | 18.2 / 18.2 | 51.4 / 97.4 | 28.0 / 53.5 / 57.0 | 50.4 / 77.5 / 106.8 | win | win |
| nest_d4 | 14 | 20.3 / 20.3 | 50.4 / 144.9 | 40.0 / 56.6 / 59.5 | 52.4 / 78.0 / 122.4 | win | win |
| packed_16 | 18 | 147.0 / 7.0 | 37.6 / 132.6 | 32.3 / 85.4 / 89.3 | 40.2 / 82.1 / 146.1 | win | win |
| packed_256 | 387 | 1233.2 / 9.7 | 149.2 / 832.0 | 925.4 / 736.5 / 740.5 | 365.4 / 820.2 / 1805.3 | win | win |
| tags_4 | 27 | 19.2 / 19.2 | 71.3 / 69.4 | 18.9 / 115.2 / 112.8 | 44.0 / 74.9 / 96.4 | win | win |
| tags_32 | 160 | 145.9 / 113.8 | 304.3 / 336.4 | 158.4 / 865.3 / 859.2 | 118.1 / 388.2 / 526.6 | win | win |
| map_8 | 172 | 95.8 / 88.8 | 287.3 / 439.7 | 119.8 / 724.8 / 729.4 | 126.2 / 452.0 / 470.6 | win | win |
| oneof_ok | 6 | 6.0 / 5.5 | 22.2 / 21.3 | 4.9 / 27.1 / 26.0 | 32.8 / 43.8 / 50.4 | win | win |
| rpc_mixed | 176 | 218.3 / 94.9 | 349.5 / 527.7 | 155.4 / 708.0 / 701.3 | 152.9 / 366.8 / 484.9 | win | win |
| rpc_sparse | 2 | 4.9 / 4.9 | 4.5 / 5.3 | 7.5 / 16.5 / 16.3 | 39.5 / 36.4 / 41.6 | win | win |

In this historical capture v4 loses every cached-encode-plus-parse row.
Typical unary `rpc_mixed` is ~2× prost on that host. Packed encode's
9.7 ns cached cell reflects pre-encoded bytes; its 1233 ns historical
fresh cell is **not** a verified direct cold/cached ratio. Parse-and-touch
exposes deferred materialization (e.g., the historical `packed_256` pbrs
touch cell is 832 ns versus 149 ns parse-only), but the older run did not
perform the new cross-codec touch/output equivalence checks.

### What to chase

Do not spend the next pass on:

- prost empty (zero-sized type, ZST, 0.3 ns)
- short-string encode (5.0 vs 3.8)
- flatten `merge_inner` (#39, made hello worse)

`rpc_sparse` decode is process-gated (pbrs decode must beat prost).
`tags_32` decode is process-gated (pbrs decode must beat v4).

Worth measuring next:

- `name_80` combined (still a loss): 80-byte string is just over the
  small-string optimization (SSO) cutoff. Leftover is `merge_inner` after a
  draft heap-copy try (#57) that is not shipped.

Keep: packed canonical cache, bytes window, `simdutf8` string arm,
same-tag repeated strings, map/repeated vs prost.

Linux x86_64 1-string line of record after dropping the per-message
`Vec` (#31): hello 6.8 / 45.4 vs 3.8 / 22.1 (combined 52.2 vs 25.8).
4 KiB 36.8 / 153.8 vs 32.7 / 133.4. That host is not this one.

```bash
PROTOC="$PWD/target/pinned-protoc-build/protoc" CARGO_BUILD_JOBS=2 \
  CARGO_TARGET_DIR=target cargo build --release --locked \
  --manifest-path tonic-bench/Cargo.toml
./target/release/tonic-bench
```

## pbrs-grpc vs tonic 0.14 (transport)

Excluded crate `rpc-bench/`. Both sides serve the same official
`grpc.testing.TestService`, generated by `protoc-gen-pbrs` from the same
`.proto`, over the same pbrs codec. The only variable is the gRPC
transport, so a delta here is a transport delta and not a serialization
one.

> **SUPERSEDED (SB-01).** The Xeon tables below ran the tonic peer with a
> bare builder: Nagle on (tonic ignores `tcp_nodelay` under
> `serve_with_incoming`), 64 KiB windows against the kernel's 16 MiB, and
> 16 KiB frames against 1 MiB. They measure settings mismatch, not
> transports. The [SB-01 fair rerun](#sb-01-fair-rerun-dev-loop-host)
> replaces them; the old tables stay for archaeology only.

Host: 4-core Intel Xeon, Linux x86_64. Client, kernel server, and tonic
server all share one tokio runtime, so absolute numbers are contended;
the ratio is the number to read. Three consecutive release runs, all
exited 0.

### Unary latency

`empty_unary` is a 0-byte request and reply. `large_unary` is a
271828-byte request and a 314159-byte reply. 2000 and 200 samples after
warmup.

| run | case | kernel p50 (unit in cell; lower better) | tonic p50 (unit in cell; lower better) | kernel p99 (unit in cell; lower better) | tonic p99 (unit in cell; lower better) |
|---|---|---:|---:|---:|---:|
| 1 | empty | **54.6 µs** | 88.7 µs | **186 µs** | 42.6 ms |
| 2 | empty | **54.9 µs** | 86.1 µs | **191 µs** | 41.7 ms |
| 3 | empty | **53.9 µs** | 87.4 µs | **110 µs** | 41.8 ms |
| 1 | large | **616 µs** | 1.48 ms | **1.69 ms** | 44.9 ms |
| 2 | large | **822 µs** | 1.71 ms | **1.60 ms** | 45.3 ms |
| 3 | large | **821 µs** | 1.49 ms | **1.04 ms** | 45.3 ms |

Process-gated in code: the kernel must be strictly faster on both p50
and p99, on both cases. (Status after SB-01: the p50 legs hold; the
empty-unary p99 leg is red under fair settings — kernel ~125 µs vs
tonic ~110 µs, stable across runs. SB-20 owns noise-based thresholds;
until then the gate result is data, not a verdict.)

The 42-45 ms tonic tail in the table above was Nagle plus 64 KiB
windows, not transport architecture: with matched settings it collapses
to ~110 µs (see the fair rerun). The earlier tower-buffering
explanation is retracted — it was never measured, only theorized.

### Sustained unary throughput

Reported, not gated: it moves with core count and scheduler luck. Best of
three 2-second windows per side. `conns` is HTTP/2 connections
(`ChannelConfig::connections` against N tonic channels). Nonzero remote
procedure call (RPC) errors fail the process, so these are also a correctness
check.

| case | concurrency | HTTP/2 connections | kernel QPS (higher better) | tonic QPS (higher better) |
|---|---:|---:|---:|---:|
| empty | 1 | 1 | **73.6k** | 2.5-2.9k |
| empty | 16 | 4 | **83.7-84.0k** | 21-27k |
| large | 1 | 1 | **2.4-3.0k** | 90 |
| large | 16 | 4 | **8.3-8.5k** | 5.8-6.2k |

At `conc=1` the kernel sustains close to its inverse latency
(73.6k ≈ 1/13.6 µs of client-side work per RPC), while tonic sustains far
below its own measured latency. (Superseded: that gap was Nagle — the
fair rerun shows 1.3x at `conc=1`, not 30x.)

### Server-streaming throughput

One server-streaming RPC of 2000 messages × 1 KiB, best of eight rounds.
Six consecutive runs:

| run | kernel msgs/s (higher better) | tonic msgs/s (higher better) | ratio |
|---|---:|---:|---:|
| 1 | **1093k** | 917k | 1.19x |
| 2 | **1028k** | 1025k | 1.00x |
| 3 | **1055k** | 916k | 1.15x |
| 4 | **1393k** | 890k | 1.57x |
| 5 | **849k** | 822k | 1.03x |
| 6 | 825k | 879k | 0.94x |
| median | **1041k** | 903k | **1.15x** |

The kernel is ahead on five of six runs and by 15% at the median, but the
per-run spread is wide enough that a strictly-faster gate would fail on
noise, so the gate is 90% of tonic. That still catches a real regression:
this axis started at **0.24x** (201k against 670k).

Four changes closed it, in the order they mattered:

1. **Batched output.** gRPC messages are length-prefixed, so one HTTP/2
   DATA frame may carry any number of them. Writing one frame per message
   cost a wakeup and often a syscall per message. `OutBatch` accumulates up
   to 32 KiB and writes once. 201k → 419k.
2. **Inline inbound decoding.** `Streaming` reads and decodes on the task
   that calls `message()`, rather than behind a spawned pump task and a
   channel. That removes a task, a queue, and a copy per message on both
   sides. 419k → 843k.
3. **No speculative flow-control reservation.** `h2` buffers up to the
   connection's send budget without waiting, so reserving capacity first was
   a needless round trip through the connection task. Also 12% off large
   unary latency.
4. **Yielding to a saturated producer.** A producer running ahead of the
   network is bounded by its channel depth, so draining it yields only that
   many messages and the write is smaller than it could be. One `yield_now`
   before flushing lets it top the queue up, halving the writes and the
   wakeups. 843k → 1041k median.

Step 4 is conditional on more than one message being queued. Exactly one
means the producer is *not* ahead — a request/response stream, say — and a
scheduling turn there would be pure added latency. That is why unary
latency and the `ping_pong` interop case are unaffected by it.

The remaining structural difference is one task hop: hyper drains a response
body on the connection task, while the kernel drains it on the per-RPC task.
Closing it would mean polling active response streams from the accept loop,
which is a different architecture rather than a tuning change; the batching
above is what makes that hop cheap enough not to decide the result.

### Bidi ping-pong throughput (loopback)

One bidi `FullDuplexCall` of 256 empty request/response pairs, best of eight
rounds after warmup, reported as round-trips/s. Process-gated at 90% of tonic,
the same band as server-streaming: each round-trip waits for a response
before the next request, so scheduler luck shows up as round-trip time (RTT).
This axis is a loopback capture in `rpc-bench`; it is not the 4-core Xeon
unary/QPS/stream tables above.

This host, one release run, kernel and tonic sharing one process:

| kernel round-trips/s (higher better) | tonic round-trips/s (higher better) | ratio |
|---|---:|---:|
| **33919** | 22595 | **1.50x** |

The 90% gate passed. These numbers are not the Xeon tables.

### Client-streaming upload throughput (loopback)

One client-streaming `StreamingInputCall` of 2000 messages × 1 KiB, best of
eight rounds after warmup, reported as messages/s. Process-gated at 90% of
tonic, the same band as server-streaming. This axis is a loopback capture in
`rpc-bench`; it is not the 4-core Xeon unary/QPS/stream tables above.

This host, one release run, kernel and tonic sharing one process:

| kernel msgs/s (higher better) | tonic msgs/s (higher better) | ratio |
|---|---:|---:|
| **1447599** | 555431 | **2.61x** |

The 90% gate passed. These numbers are not the Xeon tables.

### SB-01 fair rerun (dev-loop host)

SB-01 matches the tonic peer to the native peer: TCP_NODELAY set plus
getsockopt-verified on every accepted socket (34/34 below), 16 MiB
stream/connection windows, 1 MiB frames, 256 streams, no adaptive
window, 16 KiB header list. Both sides run the same `TestService` over
the same codec; `rpc-bench` refuses to print comparisons unless every
endpoint role meets that spec. Host: Apple M4 Pro, macOS; release;
two consecutive runs (FAIRNESS record from run 1):

| case | kernel p50 (µs; lower better) | tonic p50 (µs; lower better) | kernel p99 (µs; lower better) | tonic p99 (µs; lower better) |
|---|---:|---:|---:|---:|
| empty (run 1) | **70.1 µs** | 94.5 µs | 123.0 µs | **110.3 µs** |
| empty (run 2) | **69.5 µs** | 94.6 µs | 131.4 µs | **109.4 µs** |
| large (run 1) | **345.6 µs** | 378.8 µs | **384.7 µs** | 463.4 µs |
| large (run 2) | **348.3 µs** | 377.8 µs | **412.2 µs** | 426.3 µs |

| case | concurrency | HTTP/2 connections | kernel QPS (higher better) | tonic QPS (higher better) |
|---|---:|---:|---:|---:|
| empty | 1 | 1 | **13.1k** | 9.8k |
| large | 1 | 1 | **2.5k** | 2.0k |
| empty | 16 | 4 | **30.0k** | 18.3k |
| large | 16 | 4 | **3.0k** | 2.2k |

| axis | kernel (unit in axis; higher better) | tonic (unit in axis; higher better) | ratio |
|---|---:|---:|---:|
| server-stream msgs/s | **636-672k** | 461-472k | **~1.4x** |
| ping-pong round-trips/s | 12.8-13.8k | **13.3-14.5k** | 0.95x |
| upload msgs/s | **683-755k** | 305-333k | **~2.2x** |

Read: kernel leads p50 latency, unary throughput, server-streaming, and
upload in these two local runs. Tonic leads empty-unary p99 and ping-pong.
The p99 loss is a concrete follow-up target; two runs do not establish its
uncertainty or behavior on other hosts. The old 30x QPS and 100x p99 gaps
came from mismatched settings and cannot support a transport claim.

### Re-run

```bash
cd rpc-bench && cargo build --release && ./target/release/rpc-bench
```

`FAIRNESS {...}` lines in the output carry the per-endpoint observed
settings; a `REFUSED` line plus nonzero exit means an endpoint diverged
from the spec and the side-by-side numbers must not be read.

The separate-process RT-07 [mixed-load diagnostic](../rpc-bench/README.md#5-rt-07-mixed-load-diagnostic)
now runs scheduled small unary calls alongside repeating bulk streams in the
`bulk_vs_small_streams` scenario. It reports per-class outcomes and actual
endpoint RSS, but its sampled budget peaks are lower bounds and the transport
exposes no permit gauges or full queue wait. The report is explicitly
unqualified; it does not replace paired dedicated-host fairness evidence.

## pbrs-grpc vs grpc-go (server)

`rpc-bench` puts client and both servers in one process, which makes it
sensitive to scheduler luck. `scripts/grpc-server-bench.sh` narrows the
question instead: one kernel client, the same `grpc.testing.TestService`, the
same payloads, pointed at two servers in separate processes. The only variable
is the server.

Same 4-core Xeon. Three rounds, 2000 `empty_unary` and 200 `large_unary`
samples each, nanoseconds:

| Round | Server | empty p50 (ns; lower better) | empty p99 (ns; lower better) | large p50 (ns; lower better) | large p99 (ns; lower better) |
|---|---|---:|---:|---:|---:|
| 1 | **kernel** | **53232** | **71064** | **596055** | **779373** |
| 1 | grpc-go | 77763 | 109962 | 910288 | 1658270 |
| 2 | **kernel** | **54387** | **66724** | **595006** | **863454** |
| 2 | grpc-go | 75316 | 116663 | 939219 | 1587095 |
| 3 | **kernel** | **54260** | **66371** | **602411** | **849000** |
| 3 | grpc-go | 77643 | 113256 | 900704 | 1497070 |

Roughly 1.4x on `empty_unary` p50, 1.7x on its p99, 1.5x on `large_unary` p50,
and 1.8x on its p99. Round-to-round spread is a few percent, because unlike
`rpc-bench` the two servers are not competing for the same runtime.

Reported, not gated: the script needs a Go toolchain and network access to
fetch `google.golang.org/grpc`, so it is not something CI should depend on.

### Bidi ping-pong and upload vs grpc-go (loopback)

The same kernel client also measures empty bidi ping-pong (256 pairs) and
client-streaming upload (2000 × 1 KiB), best of eight rounds after warmup.
This is a loopback capture in `scripts/grpc-server-bench.sh`; it is not the
4-core Xeon unary tables above.

This host, three rounds, kernel client, two servers in separate processes.
Reported, not gated. Later rounds are noisier on a contended machine.

| Round | Server | ping_pong round-trips/s (higher better) | upload msgs/s (higher better) |
|---|---|---:|---:|
| 1 | **kernel** | **57899** | **695945** |
| 1 | grpc-go | 40998 | 538945 |
| 2 | **kernel** | **58259** | **874125** |
| 2 | grpc-go | 9522 | 464726 |
| 3 | **kernel** | **14103** | **823888** |
| 3 | grpc-go | 7681 | 315210 |

Best of three: ping_pong **58259** vs 40998 (~1.42x), upload **874125** vs
538945 (~1.62x). These numbers are not the Xeon tables.

```bash
./scripts/grpc-server-bench.sh
```
