# Server scheduling and nested-message storage, October 27

Implementation commit `ec13f154` includes the server and codegen changes.
The server uses less CPU after coalescing connection wakes. Applications that
read nested protobuf trees can select owned storage during code generation.
The default lazy layout remains useful for partial reads and forwarding
unchanged messages. This record preserves both the gains and their costs.

[Results](results.json) · [Raw data](raw.tar.gz) · [Manifest](manifest.json)
· [Checksum checker](check.py)
· [Main benchmark guide](../../benchmarks.md) · [Parity audit](../../parity.md)

## Server CPU

The isolated server change is commit `48c208e9`. Each connection caches a proxy
waker. Wakes during a poll can trigger bounded local progress instead of another
scheduler round trip. Tokio's cooperative budget and a local poll limit bound
that work. Wakes outside polling still schedule the parent; completion and
drop release it. The implementation adds one allocation per connection and
uses no unsafe code.

Both endpoints use the same Prost codec, matched handlers, one connection and
one call in flight. The server uses two runtime workers. Five randomized
before/after pairs each include a fresh tonic/prost control. CPU uses paired
N/2N process measurements: 20,000/40,000 unary calls or 1,000/2,000 pipeline
calls. A pipeline RPC carries 64 messages of 1 KiB.

| Server work | Native before | Native after | Fresh tonic after | Paired native after/before |
|---|---:|---:|---:|---:|
| 1 KiB unary | 45 µs/RPC | 23.5 µs/RPC | 32.5 µs/RPC | 0.452 |
| 64 × 1 KiB pipeline | 270 µs/RPC | 200 µs/RPC | 310 µs/RPC | 0.720 |

These are separate medians; the paired ratio is the median of the five ratios,
not a division of the displayed medians. All five native-server pairs improve.
For unary, the after native/tonic paired median is 0.723, with a range of
0.576–1.094. The pipeline native/tonic range is 0.487–1.818. Neither is an
all-repeat claim against tonic. The 120 captures complete 1,890,000 RPCs with
zero terminal errors.

Unary client changes are inconclusive: the after/before paired median is 1.10,
with a range of 0.804–1.793. The pipeline client's paired median is 0.750,
with one pair above one. This record makes no client improvement claim.

Separate diagnostic profiles support the scheduling explanation. Native
instructions per unary RPC fall about 5.5%; scheduling-related futex and
eventfd activity falls sharply while socket writes stay near two per RPC.
Receive calls increase. These instrumented profiles overlapped compilation,
so their timing is not shipping-performance evidence. The separate counters-off
CPU campaign above supplies that comparison.

The post-hoc latency review keeps each capture's quantiles intact; it does not
subtract N/2N quantiles. Unary p99 changes vary between pairs. Pipeline p99
improves in the measured samples, but this closed-loop shared-host run does
not establish latency under offered load.

## Choosing protobuf storage

Generate read-heavy nested messages with:

```rust
pbrs::codegen::Config::new()
    .owned_message_fields(true)
    .compile_protos(&["proto/service.proto"], &["proto"])?;
```

The plugin option is `--pbrs_opt=owned_message_fields=true`. Imported or
extern-mapped child types keep their own generated layout; apply the profile
to the relevant schemas when the whole tree should be constructed at parse
time. The [codegen guide](../../guides/codegen.md#owned-nested-message-fields)
explains the storage and serialization tradeoffs.

Owned singular children use `Option<Box<T>>` and ordinary checked parsing.
They avoid repeatedly validating descendants when getters walk the tree.
Nested `parse_bytes` windows preserve shared byte storage for singular,
repeated and map-value messages. A four-byte size cache reduces the measured
compact node from 32 to 24 bytes. Sizes at least `u32::MAX` are left uncached;
size computation still returns their full `u64` value.

The default lazy profile is unchanged. It can validate and retain unread
children without constructing their tree, and can reuse child wire while
encoding. Owned children encode from fields; parsing, sparse reads, untouched
forwarding and cached encoding therefore need separate comparisons.

## Codec results

The selected implementation is the stack-parsing owned variant, recorded as
`owned-v3-stack`. Before means the unchanged default lazy profile from
`934bba31`; after means the explicit owned profile. Both codecs receive
identical wire bytes and must agree on decoded values and byte-exhaustive
checksums. Each timed operation parses, optionally reads fields, and drops the
result. Five randomized pairs per row contain three samples per process.
Allocation measurements use a separate instrumented build.

| Work | Lazy before | Owned after | Fresh Prost after | Paired owned/lazy | Paired owned/Prost |
|---|---:|---:|---:|---:|---:|
| Depth 8, read all | 659.7 ns | 129.5 ns | 143.6 ns | 0.197 | 0.902 |
| Depth 64, read all | 29,925.3 ns | 1,523.2 ns | 1,594.0 ns | 0.051 | 0.963 |
| Depth 64, parse only | 809.2 ns | 1,440.8 ns | 1,527.5 ns | 1.808 | 0.965 |
| Mixed RPC, read all | 907.9 ns | 905.6 ns | 860.5 ns | 1.004 | 1.053 |
| Sparse RPC, parse only | 8.7 ns | 10.2 ns | 40.9 ns | 1.168 | 0.244 |
| Packed 256, read all | 255.9 ns | 269.8 ns | 642.3 ns | 1.051 | 0.416 |

Ratios below one use less CPU. The depth-64 read-all paired owned/Prost ratios
range from 0.906 to 0.985; the small margin on this host does not establish a
broad lead over Prost. The same owned operation requests 63 allocations and
1,512 bytes, versus lazy's 64 and 5,344, and Prost's 63 and 1,008. Parsing
alone moves those child allocations up front: lazy parsing requests only two
allocations and 384 bytes. These requested bytes are allocator traffic, not RSS.

Mixed-RPC parsing also costs 13.1% more CPU than the lazy baseline, and sparse
RPC full reading costs 8.9% more. The table includes representative gains and
regressions. All 16 parse/full-read
rows, paired controls and allocation rows remain in
`work/fix-performance-round/benchmark/owned-v3-stack-comparison/summary.json`.
This matrix contains 320 timed processes and 64 allocation captures.

A heap-first variant and an alternative heavy-field inlining policy were also
measured. Heap-first storage reduced copied initialization work but did not
improve the broader result. The last inlining variant improved the mixed RPC
read to 832.6 ns while regressing deep-64 to 1,801.6 ns and depth-8 to 159.6 ns.
The stack variant remains selected; the alternatives are retained as tradeoffs,
not omitted failed runs.

### Partial reads and encoding

The partial-read holdout parses the same complete tree and reads only the root
and first child. At depth 64, owned CPU is 1,462 ns versus lazy's 1,595 ns and
Prost's 1,498 ns; the paired owned/lazy ratio is 0.903. At depth 8 it is
139 ns versus 246 ns and 140 ns, with a paired ratio of 0.564. The owned
profile still constructs all children: depth-64 requests 63 allocations and
1,512 bytes versus lazy's three and 464. A partial CPU win does not imply a
partial-allocation win, and unread parse-only storage still favors lazy.

Encoding shows why the owned profile is optional:

| Depth-64 work | Lazy | Owned | Prost | Paired owned/lazy | Paired owned/Prost |
|---|---:|---:|---:|---:|---:|
| Parse and first encode | 854 ns | 1,979 ns | 8,316 ns | 2.328 | 0.238 |
| First encode, parsing excluded | 26.4 ns | 566.7 ns | 6,831 ns | 21.479 | 0.084 |
| Unchanged cached encode | 30.5 ns | 329.3 ns | 6,750 ns | 10.800 | 0.049 |
| First-child mutation and encode | 41.5 ns | 332.2 ns | 6,807 ns | 8.145 | 0.049 |

The first-encode-only row prepares bounded batches of parsed inputs outside
the timed region. Cached and mutation rows warm the message before timing.
Every row includes fresh output allocation and dropping that output; the
parse-and-first-encode row includes the whole parse/encode/drop lifecycle.
Mutation checks compare boundary values against Prost. These scope differences
matter: unchanged lazy children can reuse their original wire, while owned
children serialize from fields and cached sizes.

Owned encoding uses less CPU than Prost here but substantially more than lazy
encoding. Depth-8 rows retain the same tradeoff: owned first encode costs
3.36 times lazy, cached encode 1.84 times and mutation/encode 1.60 times.
The selected profile therefore targets applications that need constructed
nested fields; it does not replace the default for forwarding workloads.

Partial and encode summaries are under
`work/fix-performance-round/benchmark/owned-v3-stack-partial-comparison/` and
`owned-v3-stack-encode-comparison/`. They retain all rows, five pairs,
controls and allocation captures. The encode matrix contains 160 timed
processes and 32 allocation captures; the partial matrix contains 40 and eight.

## Rejected experiments

An eager-on-first-get decoder made deep full reads faster but increased
partial-read CPU and allocations sharply. It was reverted. Its hook, source,
fixtures, failed first test and corrected semantic checks remain in the raw
archive. It is distinct from the shipped owned profile, which is explicit
at code-generation time.

An output-buffer codegen experiment lowered peak RSS by 3.1–8.4% but increased
warm CPU by 5.4–39.3% in all six measured cases. It was also reverted. Both
measurement attempts, the corrected scheduling-overlap assessment and the
source-identical output hashes remain available. This work establishes no
generator-speed improvement.

## Validation and limits

At `ec13f154`, the all-features workspace run passes 3,932 tests with zero
failures and nine existing ignored tests. Core and transport Clippy checks
pass with warnings denied. The standalone owned consumer executes its runtime
assertions separately from the workspace tests.
Documentation was edited while these checks ran. The validation report hashes
all production and test inputs and verifies they remain unchanged; the default
conformance report's dirty-source flag also sees documentation changes. Owned
conformance intentionally replaces only the 13 generated bindings in its
isolated source. Both changes are identified in the retained provenance.

The final checks also pass all 18 raw-wire message-presence comparisons and
41 mixed-peer tests. Rust 1.85 checks cover native gRPC, Tower and the owned
consumer. Both storage profiles reproduce the same known differences across
654 differential operations; strict parity mode deliberately fails on those
counterexamples. Default-profile official conformance passes all three runs,
and regeneration checks all 13 default bindings without changing their bytes.

The owned consumer checks recursive parsing, required fields, split merges,
oneof switching, maps, groups, unknown fields, UTF-8 policy, depth limits,
truncation, failure state, shared-input ownership, cloning and mutation.
The public builder, the tracked codec benchmark's `owned-messages` feature and
the frozen selected variant produce byte-identical codec Rust output. The
artifact SHA-256 is
`e84c382bc412bf7fb2ef97c5fc476985444acbe8b5867941413ed6974970aa02`;
`work/fix-performance-round/linear/final/selected-equivalence.json` records
its Cargo-artifact provenance.
The default and owned differential runs retain the known Prost policy
differences. Neither mode treats those differences as full parity.

The official protobuf 35.1 suite also passes against all 13 bundled bindings
regenerated with the owned profile in an isolated checkout of `ec13f154`.
Two required passes and one recommended pass each complete 5,631 binary/JSON
and 909 text cases, with zero skips or failures, through maximum Edition 2023.
The unchanged handwritten module registry, generated patch, executable hashes
and commands are recorded in `work/fix-performance-round/owned-conformance/`.

The host is a shared x86_64 KVM VM with an AMD EPYC 9V74 CPU, five allowed CPUs,
a four-CPU cgroup quota and 16 GiB memory. Builds use Rust 1.99.1 and protoc
35.1. CPU comparisons run separately from allocation instrumentation and
profiling. Randomized paired controls reduce some drift; they do not supply
dedicated-host isolation. Codec comparisons use Prost 0.14.4; RPC controls use
tonic 0.14.6. TLS, compression, broad concurrency and other implementations
are outside the server CPU comparison above.

Full tonic/prost parity still has the [documented gaps](../../parity.md),
including the alloc-only embedded runtime and configuration/customization
surface. No world performance ranking is established.

## Inspect and reproduce

Verify the capsule before extracting it:

```sh
python3 docs/evidence/performance-fixes-20261027/check.py
mkdir -p work/performance-fixes-evidence
tar -xzf docs/evidence/performance-fixes-20261027/raw.tar.gz \
  -C work/performance-fixes-evidence
```

The archive keeps original `work/` paths. Start with these records:

| Path within the archive | Contents |
|---|---|
| `work/fix-performance-round/benchmark/rpc-comparison/` | Raw endpoint captures, randomized plan, source/build manifests and CPU summary |
| `work/server_cpu_fix/` | Scheduler source patch, profiles, failed premises and diagnostic review |
| `work/fix-performance-round/review/` | Independent wake, lifecycle and profile reviews |
| `work/fix-performance-round/benchmark/owned-v3-stack-comparison/` | Selected profile: every parse/full-read row and separate allocation controls |
| `work/fix-performance-round/benchmark/owned-v3-stack-partial-comparison/` | Partial-read CPU and allocation holdout |
| `work/fix-performance-round/benchmark/owned-v3-stack-encode-comparison/` | First, cached, mutation and parse/encode lifecycle holdouts |
| `work/fix-performance-round/benchmark/variant-files/` | Exact changed files for frozen codec variants |
| `work/fix-performance-round/linear/` | Rejected eager decoder and later storage decisions |
| `work/codegen_output/` | Rejected generator experiment, measurements and output equivalence |
| `work/fix-performance-round/validation/` | Exact-source validation commands, logs and results |
| `work/fix-performance-round/owned-conformance/` | Isolated owned regeneration, official-suite logs, source patch and hashes |
| `work/fix-performance-round/final-checks/` | Pinned descriptor parity and final documentation-link checks |

To reconstruct a frozen source variant, follow the explicit parent chain in
`work/fix-performance-round/benchmark/variant-files/reconstruction.json`. The
selected chain is baseline → V2 → V3A; V1 is an independent baseline variant.
The record also identifies the baseline Git archive paths and symlinks.

Binary executables, build caches and duplicate full source checkouts are
excluded. Source manifests, patches, harnesses, lockfiles and raw measurements
remain. The tracked [codec benchmark](../../../tests/proof/codec_benchmark/README.md)
can compare either profile with `--features owned-messages` selecting owned
storage. The [proof index](../../../tests/proof/README.md) links correctness
and policy-difference commands. Reproducing a performance campaign also needs
its recorded source, build flags, payloads, process settings and quiet host.
