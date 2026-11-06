# Benchmarks

[Run benchmarks](benchmarks/running.md) · [Earlier results](benchmarks/grpc-20261024.md)
· [Qualification](scoreboard.md)

| Read | Scope |
|---|---|
| [Latest full hosted comparison](evidence/hosted-devloop-10d94697-20261106/README.md) | All 1,634 cells at `10d94697`, with selected wins, remaining Prost losses and instrumentation limits, November 6 |
| [Earlier full hosted comparison](evidence/hosted-devloop-20261106/README.md) | All 1,634 cells and repaired fresh lifecycle at `05a3cca5`, November 6 |
| [Balanced authority storage](evidence/balanced-authority-storage-20261106/README.md) | Dev/test layout and constructor allocations for the routing fix; remaining costs included, November 6 |
| [Rejected medium-string validator](evidence/rejected-medium-core-utf8-20261106/README.md) | An 80-byte name gain with Unicode and boundary losses; all 756 CPU processes, November 6 |
| [Rejected packed readiness routing](evidence/rejected-packed-readiness-routing-20261106/README.md) | Fresh gains with a remaining large encode-then-getter loss; all 432 processes, November 6 |
| [Rejected cache-first packed encoding](evidence/rejected-packed-cache-first-20261106/README.md) | A local fresh TAT win against pinned Prost with large getter and owned regressions, November 6 |
| [Rejected fused packed encoding](evidence/rejected-packed-fused-20261106/README.md) | Fresh and noncanonical gains with cached and getter regressions; all 432 processes, November 6 |
| [Rejected packed-wire cache](evidence/rejected-packed-wire-cache-20261106/README.md) | Fresh-encode gains with cached and noncanonical regressions; all 432 processes, November 6 |
| [Fresh-encode warmup repair](evidence/fresh-encode-warmup-20261106/README.md) | Separate warmup objects and retain the previously hidden cold-allocation cost, November 6 |
| [Map removal](#map-removal-november-6) | Correctness repair, indexed update gains and small-map costs, November 6 |
| [Rejected borrowed queries](evidence/rejected-query-capability-20261106/README.md) | Lookup allocation gains with a repeatable small-map loss, November 6 |
| [Numbered name allocation](#numbered-name-allocation-november-5) | Repeated-prefix generation and ordinary controls, November 5 |
| [Safe generated defaults](#safe-generated-defaults-november-5) | Opt-in construction, encoding, parse and build costs, November 5 |
| [Endpoint origin cost](#endpoint-origin-cost-november-5) | Matched in-memory unary CPU, allocation and layout observations, November 5 |
| [Earlier generation cost](#generation-cost-november-5) | Matched generator wall time after naming repairs, November 5 |
| [Map equality](#map-equality-november-5) | Warm and cold map comparison CPU and allocations, November 5 |
| [Owned byte buffers](evidence/byte-ownership-20261105/README.md) | Conversion allocations and capacity retention, November 5 |
| [Recursive validation](#recursive-validation-november-4) | Selected protobuf parse and read CPU, November 3–4 |
| [Send-buffer RPC diagnostic](#send-buffer-rpc-diagnostics-november-3) | Selected endpoint CPU comparisons, November 3 |
| [Latest buffer loop](#buffer-chunk-ownership-november-2) | Isolated ownership operations, November 2 |
| [Codec and generator campaign](evidence/runtime-and-codegen-20261029/README.md) | Combined measurements and remaining losses, October 29 |
| [RPC endpoint results](#correctness-and-rejected-candidates-october-28) | Complete earlier transport comparison, October 28 |
| [Benchmark input checks](#benchmark-input-checks-november-2) | Canonical input identity and qualification |
| [Borrowed-map lookup experiment](evidence/rejected-map-lookups-20261103/README.md) | Rejected lookup candidates, November 3 |
| [Rejected candidates](evidence/rejected-runtime-candidates-20261030/README.md) | Unselected repeated-field and map experiments |
| [Earlier codec and codegen](#codec-and-codegen-comparisons) | Earlier comparisons and their recorded sources |

Choose a result by the operation you need to improve. Each record names its
source, workload, controls and remaining losses; the tables cover different
scopes. These local diagnostics do not establish wins on every benchmark,
complete feature parity or an overall ranking.

## Map removal, November 6

Removing an entry now preserves unrelated duplicate-key winners and repairs
an existing lookup index. The [full record](evidence/map-removal-20261106/README.md)
retains the corruption reproducer, panic and ownership controls, and all seven
performance rows. Removal still exposes an earlier stored duplicate of the
target key; it does not match Prost/HashMap deletion in that case.

| Owned-map operation | New / old CPU | Pair range |
|---|---:|---:|
| Remove/read/reinsert, 8 integer keys | 1.557 | 1.538–1.771 |
| Remove/read/reinsert, 8 short string keys | 1.395 | 1.346–1.434 |
| Remove/read/reinsert, 256 integer keys | 0.152 | 0.148–0.169 |
| Remove/read/reinsert, 64 long string keys | 0.103 | 0.098–0.115 |
| Remove/read/reinsert, 256 long string keys | 0.053 | 0.052–0.054 |
| Single removal, unindexed 8-key integer map | 1.294 | 1.203–1.403 |
| Single removal, unindexed 256-key integer map | 1.232 | 1.190–1.287 |

These are nine pairs per row against an isolated overlay of `6f9798aa` on a
shared host. Small and unindexed removals cost more; large indexed cycles avoid
rebuilding the index and cloning its keys. For 256-key maps over 256 cycles,
separate allocation counts fall from 512 to zero calls for integer keys and
from 66,560 to 512 calls for long string keys. Retained memory, typed/raw mutator CPU and
tonic/Prost performance are unmeasured. This correctness fix does not establish
a general speedup.

## Numbered name allocation, November 5

The allocator remembers suffixes for repeated prefixes and reuses generated
companion names. The [record](evidence/naming-cursors-20261105/README.md)
compares an isolated allocator overlay of `0ae8a43d`; the CLI includes
`protoc`, generation and file handling.

| Input | Operation | Paired new / old wall time (range) |
|---|---|---:|
| 1,024 repeated-prefix groups | Fresh output | 0.543 (0.510–0.647) |
| 1,024 repeated-prefix groups | Unchanged output | 0.575 (0.531–0.720) |
| 1,000 ordinary messages | Fresh output | 0.981 (0.935–1.313) |
| 1,000 ordinary messages | Unchanged output | 1.001 (0.886–1.044) |

All pairs in both repeated-prefix rows favor the new allocator, with identical
generated bytes. Ordinary controls remain mixed. A separate five-pair
combined-core run leaves boxing selectors empty and safe defaults disabled.
All ordinary generation ranges cross one, small unchanged-output medians retain overhead,
and namespace qualification increases output bytes. Neither run establishes
a common-case speedup, absence of a regression or consumer-build performance.

## Safe generated defaults, November 5

The opt-in [safe-default policy](guides/codegen.md#safe-generated-defaults)
supports generated consumers that forbid unsafe code. The
[cost record](evidence/safe-defaults-20261105/README.md) compares ordinary and
safe output from the same generator/runtime in one lazy-storage fixture.

| Release operation | Safe / ordinary elapsed time | Pair range |
|---|---:|---:|
| Construct, expose and drop | 0.875 | 0.870–0.895 |
| Construct and encode an empty message | 1.201 | 1.180–1.203 |
| Parse | 0.998 | 0.914–1.133 |
| Parse and read all fields | 1.017 | 0.959–1.033 |

These are five randomized pairs per row on a shared host. Empty encoding
costs about 20% more in release; all four debug rows also cost more in this
fixture. Three dependency-warm build pairs give dev/release medians of
1.036/0.996, with both ranges crossing one. Generated source shrinks by
863 bytes while the release executable grows by 752 bytes. The policy stays
disabled by default; this is neither a speed claim nor a comparison with
Prost, RPCs or full clean workspace builds. Runtime dependencies retain their
own unsafe implementations.

## Endpoint origin cost, November 5

The [origin record](evidence/endpoint-origin-20261105/README.md) retains local
`dc2261c8` as its capture baseline. Its checker reconstructs 407 selected
baseline inputs from public `0ae8a43d`; equivalence is limited to those inputs.
Ten native overlays are compared using matched in-memory unary calls on one
current-thread runtime, with nine CPU pairs per mode.

| Operation | Median paired CPU change | Pair range |
|---|---:|---:|
| Stock unary, no origin | +0.42% | −3.53% to +6.29% |
| Unary with a 5-second deadline, no origin | +0.38% | −2.03% to +5.43% |
| Equivalent HTTP origin | +0.16% | −2.33% to +12.54% |

All medians retain small costs and the ranges include wins and losses.
The private `ChannelLocal` grows from 176 to 184 bytes; the public `Channel`
handle stays 16 bytes in this build. Separate allocation observations retain
the same call counts and request eight fewer bytes per combined client/server
RPC. Requested bytes do not measure retained memory. This shared-host
diagnostic establishes neither zero overhead nor network, TLS, streaming,
custom-scheme or tonic/Prost performance.

## Map equality, November 5

The runtime walks the sorted unique-key indexes directly when comparing
indexed maps. The [full record](evidence/map-equality-20261105/README.md)
retains all four variants, measurements and allocation controls.

| Equal maps, 256 unique keys | Before CPU ns | After CPU ns | Paired new / old CPU (range) |
|---|---:|---:|---:|
| Integer keys | 4,298 | 276 | 0.0651 (0.0627–0.0701) |
| ProtoString keys | 34,770 | 873 | 0.0251 (0.0235–0.0262) |
| LazyStr keys | 41,619 | 1,154 | 0.0277 (0.0273–0.0299) |

These are nine paired warm comparisons of isolated runtime overlays at
`1c6ec8ca` on one shared host. Construction and index creation are excluded.
All 18 cold equal-map medians also improve, with index creation included;
allocation counts stay unchanged. Some false comparisons regress: LazyStr
size mismatches cost about 0.2–0.3 ns more, and one small ProtoString mismatch
costs about 1.3 ns more. Small string maps still lose to the HashMap storage
reference. This does not measure decoding, RPCs or the combined commit.

## Generation cost, November 5

The naming repairs fix valid schemas that previously failed to compile. A
[separate cost check](evidence/codegen-naming-20261105/README.md) compares
before/after generator CLIs on noncolliding schemas; generated files match
exactly. The focused 100-message run gives these nine-pair wall-time ratios:

| Generation | New / old wall time | All-pair range |
|---|---:|---:|
| Fresh output | 1.001 | 0.936–1.104 |
| Unchanged output | 1.037 | 0.932–1.151 |

The unchanged-output overhead persists in the medians, while all ranges
cross one on the shared host. The earlier five-pair screen also includes
6- and 1,000-message schemas. These results establish neither a codegen
speedup nor absence of a regression. They measure CLI generation, including
startup and file handling, rather than consumer compilation.

## Recursive validation, November 4

Generated validators for a narrow class of self-recursive messages now use
borrowed byte slices for their recursive checks. The [full record](evidence/borrowed-recursive-validation-20261104/README.md)
retains the source overlay, correctness controls and all paired measurements.
The measured snapshot predates the separate attribute-selector changes.

| Nine-pair focused holdout | New / old CPU | New / Prost CPU |
|---|---:|---:|
| Depth-8 parse | 0.798 (range 0.767–0.808) | 0.609 (0.582–0.646) |
| Depth-64 parse | 0.693 (0.674–0.711) | 0.289 (0.277–0.299) |
| Depth-8 parse and full read | 0.924 (0.897–0.951) | 2.374 (2.309–2.444) |
| Depth-64 parse and full read | 0.934 (0.917–0.962) | 2.014 (1.915–2.051) |

Lower is faster. The results are paired median CPU ratios on one shared host;
ranges include all nine pairs. Earlier screens ran before an environment
restart. An earlier seven-pair mixed-read screen had a 1.030 new/old median;
the later nine-pair mixed-read result was 1.004 (0.918–1.034). The controls
do not establish absence of a regression. Full recursive reads remain slower
than Prost in these cases. The record does not establish RPC performance or an
overall ranking.

The [November 3 borrowed-map experiment](evidence/rejected-map-lookups-20261103/README.md)
removed allocations from indexed long-string lookups, but its final candidate
slowed a small duplicate-key lookup by 5.2% in the nine-pair holdout. It was
rejected; map construction and index creation were outside that measurement.

## Send-buffer RPC diagnostics, November 3

Two source-pinned release builds compared the chunk sender before and after
`Bytes::split_to`. Both transports used Prost messages. The initial three-pair
screen covered 64 KiB and 8 MiB unary messages; a five-pair follow-up covered
8 MiB. All 132 captures completed, accounting for 109,800 calls. The
[full record](evidence/send-buffer-rpc-20261103/README.md) keeps every run,
source and build record, control and CPU slope.

| 8 MiB endpoint, five-pair follow-up | New / old native CPU | New native / tonic CPU |
|---|---:|---:|
| Client | 0.777 (range 0.663–1.201) | 0.906 (0.782–1.808) |
| Server | 0.923 (0.763–1.043) | 1.021 (0.905–2.056) |

Ratios are medians of paired N/2N slopes; ranges show all five pairs. Lower
CPU ratios are better. The first 8 MiB screen gave a 1.389 new/old client
median, and a 64 KiB server control had a nonpositive slope, so its ratios were
omitted. Tonic-only controls also varied widely. The runs are inconclusive
about an RPC speedup.

The diagnostic used one shared host, plaintext, identity compression,
zero-filled payloads and one RPC in flight. Payload checks cover length, not
every byte. Client CPU includes process startup; both endpoint windows include
connection work. The binaries were compiled separately, and the compiler
version was not recorded at build time. These results do not establish latency,
throughput, an overall ranking or production qualification.

## Buffer chunk ownership, November 2

The gRPC sender advances a large frame's tail with `Bytes::split_to`, avoiding
an extra shared-owner clone and drop for each chunk. In an isolated optimized
8 MiB / 16 KiB chunk-loop screen, new/old timed wall has a 0.2869 paired median
(range 0.2835–0.2887). All five tested cases favor the new loop; the
[complete record](evidence/readiness-followup-20261102/README.md#shared-buffer-chunk-ownership)
keeps all five pairs per case and the separate whole-process CPU observations.

This times shared-buffer ownership with immediate fragment drops on one shared
host. It excludes fresh-buffer setup, HTTP/2 enqueueing, flow control, sockets
and TLS. The screen does not measure RPC CPU, latency or throughput and has no
tonic or Prost control. The later RPC diagnostic above has its own limits.

## Benchmark input checks, November 2

RPC comparison children now report the fingerprint and length of the common
prepared message. Re-encoding a Prost map can change entry order between
processes; it is unsuitable as an input fingerprint. Full map values,
cardinality and equal encoded lengths remain checked independently. An input
fingerprint does not claim that unordered-map output bytes are identical.

The [follow-up record](evidence/readiness-followup-20261102/README.md#benchmark-inputs-and-qualification)
keeps the detecting CI failure, functional process controls and exact-source
baseline reporting repair. The standalone collector derives selected and
blocked cells from its complete inventory. These accounting repairs add no
speed ranking evidence; the earlier measured results retain their source pins.

## Rejected runtime candidates, October 30

A repeated string/bytes pre-scan reduced speculative allocation but slowed
common workloads. Parsing 1,024 strings cost 1.131 times prior CPU in the
owned profile; a 64-string roundtrip with an unrelated 64 KiB suffix cost
2.025 times prior CPU. The candidate was rejected. Two map designs were also
rejected after assembly inspection; they have no measured speed result.

The [complete screen](evidence/rejected-runtime-candidates-20261030/README.md)
keeps all 44 diagnostic CPU rows, allocation controls, correctness proofs,
source overlays and a verifier. It measures the earlier `15e73849` source,
not the later release candidate. The [October 29 results](evidence/runtime-and-codegen-20261029/README.md)
remain the latest selected codec and codegen measurements.

## Correctness and rejected candidates, October 28

Source `ce0a06e5` repairs overflowing transport byte/token counters and outbound
frame lengths. It keeps the previous map, lazy-message and generator designs.
The [complete October 28 record](evidence/optimizations-20261028/README.md)
retains 160 timing rows with 80 nonwins, 320 resource rows with 271 nonwins,
and nine missing axes. The goal of beating tonic/prost on every benchmark
remains unmet. The
[October 27 record](evidence/decode-and-map-speed-20261027/README.md) retains
124 timing rows with 48 losses and seven ties, 236 resource rows with 55 losses
and 138 ties, and nine missing benchmark axes.

The selected `ce0a06e5` campaign completes all 684 RPC captures. Client and
server both use Prost in the native and tonic stacks; separate counters-off
CPU runs retain fresh controls. The selected results still include substantial losses:

| Selected workload | Comparison | Result |
|---|---|---|
| Twelve large shipping RPC endpoint rows | Native / tonic CPU | Five wins, five losses, two ties |
| Four default unary/pipeline endpoint rows | Native / tonic CPU | Four wins |
| 8 MiB identity, longer client confirmation | Native / tonic CPU | 1.494; paired range 0.699–2.455 |
| 8 MiB identity, longer server confirmation | Native / tonic CPU | 1.654; paired range 0.927–1.878 |
| Forty-two warm-map rows | Typed map / standard HashMap CPU | Five wins, 37 losses |
| Lazy depth-8 parse, read and drop | pbrs / Prost CPU | 2.925; paired range 2.916–2.935 |
| Lazy depth-64 parse, read and drop | pbrs / Prost CPU | 2.382; paired range 2.312–2.449 |
| Owned mixed parse, read and drop | pbrs / Prost CPU | 1.095; paired range 1.066–1.142 |

These counts compare medians; they do not make every pair conclusive. The
longer identity confirmation remains separate from the coarse short-run
shipping cells. Duplicate-heavy maps also retain large losses: reading the
first surviving entry in the 256-entry fixture costs 278.571 times HashMap CPU
(paired range 179.483–290.749). Warm maps exclude parsing and index construction.

Selected release fresh-generation elapsed ratios against Prost are 1.196 for
the small schema, 0.968 for 100 messages and 0.809 for 1,000 messages. The
small-schema pairs span 0.943–1.311; the 1,000-message pairs span 0.770–0.849.
Generated output remains 36,310,287 versus 412,622 bytes at 1,000 messages.
Every release before/after pair range crosses one, and consumer build costs
remain outside this generator comparison.

Core source is unchanged in this selected round. Separately compiled binaries
can have different code layout, and this shared host adds noise. Differences
from the earlier core campaign therefore do not establish source-level speed
changes. The selected transport changes are correctness repairs, not a
demonstrated speed improvement. The all-feature workspace run at `ce0a06e5`
passes 3,992 tests with zero failures and nine existing ignores. The later
`cf97c83b` test-only lint repair passes its affected suite and the remaining
validation, including official conformance, MSRV, interop, lints and
regeneration. The [status](status.md) keeps those source scopes and the strict
Prost differences; successful checks do not establish complete equivalence.

The follow-up compares isolated candidates with the `6d1336e0` runtime
baseline on a shared x86_64 VM. Ordinary map loops, collection and cold
construction accompany the original warm-map tests; both message profiles
and corrected debug/release generator timings remain in scope. Ratios below
are paired candidate/baseline medians. Above one means a regression.

| Rejected candidate | Why it was rejected |
|---|---|
| Lazy validation enum | Depth-8/64 full reads use 1.066/1.075 times prior CPU despite eight fewer requested bytes in the tested lazy allocations. |
| Initial map prefix/fold changes | Distinct 16/64/256 ordinary full loops use 5.473–7.547 times prior CPU. |
| Narrow map changes | The same loops use 5.802–8.226 times prior CPU. |
| Forced-inline map changes | Distinct collection uses 1.306–1.449 times prior CPU; ordinary full loops use 2.568–3.364 times prior CPU. |
| First-survivor-only map changes | Distinct collection uses 1.377–1.448 times prior CPU; ordinary full loops use 2.479–3.371 times prior CPU. |
| Generator move-and-shrink | Small fresh release generation has a 1.171 elapsed ratio, with pairs spanning 0.825–1.266. Lower measured memory did not establish a broad speed win. |
| Broad borrowed-wire helpers | Owned scalar parse/read uses 1.269/1.174 times prior CPU, despite lower lazy nested-read CPU. |
| Narrow borrowed-wire helpers | Owned mixed parse/read uses 1.102/1.101 times prior CPU; small fresh release generation uses 1.102 times prior elapsed time. |

The narrow borrowed-wire candidate improves lazy depth-8/64 full reads to
0.906/0.908 times prior CPU, but they still cost 2.344/2.038 times Prost CPU.
Passing correctness tests and removing clone operations did not establish a
faster full lifecycle. Inlining and binary-layout observations do not override
the measured regressions. Map-only controls are standard `HashMap`, not Prost
parsing.

The selected changes repair correctness. Budget acquisitions no longer
publish a transient wrapped counter. Identity frames validate their wire and
host allocation bounds before reservation; compressed plaintext keeps its
separate length rules. Earlier server replies still precede error trailers.
The combined measurements above retain their costs. No CPU improvement is
claimed here.

## Latest runtime and generator results, October 27

The [complete record](evidence/decode-and-map-speed-20261027/README.md) compares
`71422f00` with `0287879d`. Regenerated singular lazy children reuse validation,
short map entries avoid backing-buffer work, and an index that proves every
map key unique lets iteration walk the borrowed entries directly.

| Parse, read and drop | Before | After | Prost | Median paired after/Prost |
|---|---:|---:|---:|---:|
| Lazy depth 64 | 28,300 ns | 3,968 ns | 1,657 ns | 2.339 |
| Owned depth 64 | 1,507 ns | 1,502 ns | 1,583 ns | 0.946 |
| Lazy mixed message | 864 ns | 802 ns | 895 ns | 0.909 |
| Owned mixed message | 855 ns | 837 ns | 845 ns | 0.991 |

Large unique-map iteration falls from 1,943 to 94 ns at 256 entries, versus
253 ns for the standard HashMap control: a paired after/before ratio of 0.049
and after/reference ratio of 0.368. That is warm iteration; parsing and index
construction are outside this microbenchmark. Only three of 42 map rows beat
the HashMap median. Small and duplicate-heavy maps remain losses.

The owned profile has lower CPU than Prost in 15 of 16 parse/read rows;
depth-64 parsing is near parity at 1.004. All 16 encoding holdout rows favor
pbrs. Lazy depth-64 full reads and the depth-eight partial read still lose.
Five randomized pairs retain every sample, with separate CPU and allocation
builds. These results do not meet the goal of winning every benchmark.

Generator timings use a corrected completion timer. The previous polling
wait introduced millisecond plateaus, so those older wall times cannot support
a speed claim. In the corrected comparison, all six canonical debug generation
rows favor pbrs, while four of six matched release rows do. Small release
fresh/unchanged generation still loses slightly. The 1,000-message release
fresh-generation paired ratio is 0.798; its output is 36.31 MB versus Prost's
0.413 MB. CLI time does not establish downstream build time or output-size
parity. The [record](evidence/decode-and-map-speed-20261027/README.md#correcting-generator-timing)
keeps both profiles, output volume and the timer regression controls.

The compressed-only transport follow-up in `6d1336e0` measures allocator
traffic with the same Prost codec and fresh tonic controls. These are two
paired N/2N repeats; requested MB per RPC means 1,000,000 bytes, not RSS:

| Work / native endpoint | Before | After | Tonic | Paired after/before | Paired after/tonic |
|---|---:|---:|---:|---:|---:|
| Four × 1 MiB gzip noise, bidi client | 30.864 | 22.288 | 22.774 | 0.722 | 0.979 |
| Four × 1 MiB gzip noise, bidi server | 29.805 | 21.229 | 21.720 | 0.712 | 0.977 |
| 1 MiB gzip noise, unary client | 12.763 | 10.666 | 10.971 | 0.836 | 0.972 |
| Four × 1 MiB gzip noise, upload client | 16.260 | 16.072 | 12.151 | 0.988 | 1.323 |
| Four × 1 MiB gzip noise, download server | 23.360 | 21.075 | 18.557 | 0.902 | 1.136 |
| 8 MiB identity, unary client | 66.133 | 66.133 | 44.129 | 1.000 | 1.499 |

Compressed bidi and unary requested bytes fall below tonic locally. Upload
client, download server and large identity still use more. Zero-filled bidi
requested bytes are essentially unchanged. Allocation counts and CPU are
separate metrics; these savings do not establish a CPU win.

A broader reservation experiment cut large-identity bytes by 25.4% but its
longer check showed about 50% higher client CPU and 56% higher server CPU.
That identity reservation was rejected. The final source keeps the earlier
identity growth policy and was built and measured separately; the rejected
captures remain in the complete record.

The original `71422f00` framer adjustment alone did not reduce RPC allocation
traffic. The [resource guide](resource-budgets.md#2-buffer-ownership) explains
the shared retention ledger, unchanged capacity limits and compressed-only
scope of the later input hint.

The caller-worker and small-map-cache prototypes were rejected after mixed
results and regressions. Their full campaigns remain alongside the selected
source. Read older tables below at the revisions they name.

## Typed map iteration, October 27

This earlier comparison describes the `0287879d` delivery. Typed map views
project borrowed entries as they are read instead of building a temporary
vector. Iterator size hints use conservative bounds
instead of rescanning duplicate keys. Small-map lookups compare borrowed keys
without cloning them.

The map-only comparison against `77f361eb` uses five randomized process pairs
per row. These rows time already-built maps; parsing, map construction and
index construction are outside the measured loop. The control is a standard
`HashMap`, the map representation Prost uses. Median CPU falls in all 42
direct iteration rows; collecting a map into a vector uses 11.1–44.2% less CPU. Iterating every distinct entry gives:

| Map entries | Before | After | Standard HashMap | Median paired CPU change |
|---|---:|---:|---:|---:|
| 4 | 27.4 ns | 8.9 ns | 6.2 ns | −67.7% |
| 64 | 525.5 ns | 345.1 ns | 64.5 ns | −34.5% |
| 256 | 2,441.6 ns | 1,919.6 ns | 245.1 ns | −21.7% |

These gains leave substantial losses to the `HashMap` control, especially on
larger maps. Full-message reads also show why a direct iteration win does not establish
an overall codec win:

| Mixed-message parse, read and drop | Before | After | Median paired CPU change |
|---|---:|---:|---:|
| Default lazy fields | 914.5 ns | 870.2 ns | −3.62% |
| Owned message fields | 917.3 ns | 955.6 ns | +5.85% |

Both profiles remove one allocation and 128 requested bytes per mixed-message
read. Lazy full reads are near Prost locally, with a paired CPU ratio of
1.006. Owned full reads cost 1.128 times Prost's CPU. Owned mixed parsing alone
regresses 16.3%, and sparse-message full reads regress about 20% in both
profiles. CPU and allocation measurements use separate binaries.

The [complete record](evidence/parity-and-maps-20261027/README.md) retains all
32 codec rows, all 42 map rows, every process pair and the
intermediate map implementation's collection regressions. These shared-host
results guide the next optimization; they do not establish a ranking.

## Default transport costs, October 27

This earlier transport comparison describes `12cfd09f`. The new controls also have a separate comparison with their defaults
unchanged, using Prost on both endpoints. All 120 captures complete:
1,890,000 successful RPCs across 1 KiB unary and 64 × 1 KiB pipeline calls.
Five before/after pairs include fresh Tonic controls. Every native endpoint's
before/after CPU ratio spans one across the repeats, so this run establishes
neither a CPU improvement nor zero overhead from the added controls. The
[complete record](evidence/parity-and-maps-20261027/README.md) retains all
repeats, including substantial control variation.

Against the fresh tonic control, the candidate unary client uses 19.5 versus
65.5 µs/RPC and the server uses 25.5 versus 31.0 µs/RPC. Median paired ratios
are 0.294 and 0.823; all five unary pairs favor native on both endpoints.
These are scoped endpoint-CPU results under the stated closed-loop settings,
not an overall implementation ranking.

## Server scheduling and nested messages, October 27

The [earlier server and nested-message fixes](evidence/performance-fixes-20261027/README.md)
follow the [direct parity audit](parity.md). Coalescing connection wakes lowers
native-server CPU in five local before/after pairs: unary 1 KiB medians move
from 45 to 23.5 µs/RPC, a median paired reduction of 54.8%. A 64 × 1 KiB
pipeline moves from 270 to 200 µs/RPC, a paired reduction of 28.0%. All
1,890,000 RPCs complete without terminal errors. Fresh tonic controls and
client results, including inconclusive comparisons, remain in the record.

An explicit [owned-message codegen profile](guides/codegen.md#owned-nested-message-fields)
constructs nested messages during parsing and avoids repeated validation when
reading their fields. The default lazy profile preserves its advantages for
partial reads and forwarding unchanged messages. The complete record compares
full reads, partial reads and encoding before recommending either profile.

| Parse, read and drop | Default lazy | Owned | Prost | Paired owned/prost CPU |
|---|---:|---:|---:|---:|
| 8-level message | 660 ns | 130 ns | 144 ns | 0.902 |
| 64-level message | 29,925 ns | 1,523 ns | 1,594 ns | 0.963 |
| Mixed RPC-shaped message | 908 ns | 906 ns | 860 ns | 1.053 |

The deep-64 owned read is about 19.6 times faster than the lazy read locally,
while the mixed-message read loses to Prost at that source pin. Owned deep-64
parsing alone costs more than lazy parsing, because it constructs the tree
immediately.

Encoding favors lazy storage in these fixtures. At depth 64, owned first
encode costs 567 ns versus lazy's 26 ns and Prost's 6,831 ns. Cached encode
costs 329 ns versus 30 ns and 6,750 ns. The
[complete record](evidence/performance-fixes-20261027/README.md#partial-reads-and-encoding)
includes partial reads, first-child mutation, lifecycle costs and allocations.

These are shared-host diagnostics, not a qualified ranking. The
[original audit](evidence/parity-proof-20261027/README.md) retains the earlier
16 codec rows, including the lazy depth-64 read-all loss of 16.92× prost's CPU.
That result still describes the default layout at its source pin. The codegen
audit checks compatibility; it does not establish a generator-speed ranking.

## Integrated changes, October 27

On a shared Linux x86_64 host with Rust 1.99.1, compare `a24c0e35` with the
previous `1d773f97` implementation. Both endpoints use Prost. Allocation rows
are median paired changes across two N/2N runs; they measure requested bytes,
not resident memory. Each stream has 64 messages of 1 KiB, one connection and
one call in flight.

| Identity bidi pipeline | Before, MiB/RPC | After, MiB/RPC | Paired change |
|---|---:|---:|---:|
| Native client | 0.344 | 0.326 | −5.20% |
| Native server | 0.317 | 0.328 | +3.79% |

The change is the median of each paired change, not the ratio of the two
displayed medians. Repeats range from −11.58% to +1.18% on the client and
−2.27% to +9.86% on the server. A separate counters-off run of 1 KiB unary
calls gives native-server CPU medians of 46 and 38 µs/RPC, versus 30 µs for
the fresh tonic/prost control.
The shared host and coarse CPU sampling limit timing conclusions. The
[dated source and raw captures](evidence/grpc-parity-and-costs-20261027/README.md)
include all repeats, controls and smaller isolated measurements.

## Compressed streams, October 26

These results describe `1d773f97` versus `e7595eb7`, on a shared Linux x86_64
host with Rust 1.99.1. Both endpoints use Prost unless stated otherwise. The
[full record](evidence/grpc-encode-reuse-20261026/README.md) contains its own
source and raw captures.

### Allocation bytes

Requested MiB per RPC, lower is better. These are allocator requests, not RSS.
Four-message streams use 1 MiB bodies, gzip level 6, one connection and one RPC
in flight. Values are medians from two repeated N/2N comparisons.

| Native endpoint | Payload | Before | After | Paired change |
|---|---|---:|---:|---:|
| Bidi client | Zero | 19.405 | 14.421 | −25.69% |
| Bidi server | Zero | 18.395 | 13.411 | −27.10% |
| Bidi client | Noise | 38.197 | 29.435 | −22.94% |
| Bidi server | Noise | 37.187 | 28.425 | −23.56% |
| Upload client | Zero | 11.766 | 7.277 | −38.15% |
| Server-streaming server | Zero | 14.773 | 10.285 | −30.38% |

Against fresh tonic/prost controls, zero-filled bidi uses 13.50% fewer client
bytes and 14.39% fewer server bytes. Noise bidi still uses **35.53% more client
bytes and 37.23% more server bytes**. Identity pipelined client bytes for
64 × 1 KiB rise 2.98% versus baseline. All controls and repeated values remain
in the complete record: 448 captures and 83,160 successful RPCs.

### CPU

A separate build with counters and profilers disabled completes 180 captures
and 272,160 successful RPCs. Three repeats show mixed CPU changes. In 1 MiB
noise unary calls, the paired median CPU change is +6.90% on the native client
and +16.00% on the native server; the server's individual repeats span −7.14%
to +22.22%. Allocation reductions do not establish a speed improvement.

For 1 KiB identity unary, native-server CPU slopes range from 30–56 µs/RPC,
versus 24–30 µs for fresh tonic controls. The host and coarse CPU sampling do
not support a universal CPU or latency claim. See the
[complete CPU results](evidence/grpc-encode-reuse-20261026/README.md).

## Codec and codegen comparisons

The [latest component follow-up](evidence/efficiency-20261027/README.md) removes
MiniTable length-calculation allocations and lowers generator allocation
requests by 7.60–13.17% in the measured multi-file cases. Generator timing is
mixed, and some parallel cases use more peak live memory. These component
results do not update the endpoint tables above.

The [codec survey](../tonic-bench/README.md) separates parsing from reading
every field, and owned messages from borrowed views. The
[codegen matrix](evidence/sb09-codegen-matrix.md) records generator time,
generated output size and downstream build costs, including pbrs losses.
Use the [benchmark commands](benchmarks/running.md) to run either harness.

A later [MiniTable compatibility repair](evidence/minitable-groups-20261027/README.md)
preserves groups and unknown fields. Its 100-fragment non-group fixture costs
about 5% more time in a local diagnostic; group-body timing was not measured.

The [earlier gRPC tables](benchmarks/grpc-20261024.md) retain instruction
counts and previous CPU, allocation and bulk results at their original pins.
The [benchmark contract](benchmark-contract.md) defines the stronger evidence
needed for a performance claim.

[Compatibility checks](evidence/grpc-compatibility-fallback-20261026/README.md)
cover trait objects, native MSRV profiles and consumer gates. Their earlier
encode experiment retains its own source and measurement scope.
