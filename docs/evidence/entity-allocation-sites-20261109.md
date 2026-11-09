# Entity construction allocations, November 9

This diagnostic isolates allocation requests during construction of the same
entity, edge, and payload fixtures used by the adoption benchmarks. It found
higher counts for pbrs in several component operations. The measurements are
instrumented synchronous scopes, not RPC, CPU, or latency results. They
include successful allocations and reallocations, with each reallocation
counting its full new requested size.

The [scratch diagnostic source](https://github.com/mingley/pure-protobuf/blob/451fda3906adee88fabcea12ef3716aa95141ebc/bench/devloop/adoption/src/bin/entity-site-diagnostic.rs)
was based on source `4a2d8e2`. Its original
[preflight](https://github.com/mingley/pure-protobuf/actions/runs/42690801702)
passed its capture, strict Clippy, byte/ownership controls, and detecting
negative controls but **failed its formatting gate**. A
[rerun with the formatted source](https://github.com/mingley/pure-protobuf/actions/runs/42699205724)
passed all its checks. The values below come from the original capture, whose
artifact was verified and read by a separate successful
[reader](https://github.com/mingley/pure-protobuf/actions/runs/42701123723/job/129445795661).
The [retained reader data](2026-11-09-entity-allocation-sites-reader.json)
is reconstructed from its log; it is not the original ZIP or every raw repeat.
The [original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42690801702/artifacts/13707877217)
has SHA-256 `63bc0cf5b5af76e4b6d1386d7b63fd9cf00cb8f1146b11e6d10a3bdb8e41e9a7`
and expires February 7, 2027.

Each row reports allocation requests / requested bytes per operation from
`(2N - N) / N`, with N = 16. Two independent reports and both repeats
agreed. Inputs for the assignment rows were prepared before counting;
messages are dropped in those windows. The rows measure different scopes and
must not be added to estimate the cost of an RPC.

| Operation | pbrs | Prost |
|---|---:|---:|
| Assign a prebuilt vector of two edges | 1 / 24 | 0 / 0 |
| Assign two prepared edges using each factory's array/vector shape | 2 / 168 | 1 / 144 |
| Push two prepared target strings | 2 / 184 | 1 / 96 |
| Set a prebuilt present-empty or nonempty description | 1 / 40 | 0 / 0 |
| Build Entity with absent description | 37 / 2,020 | 27 / 1,148 |
| Build Entity with present-empty description | 38 / 2,186 | 27 / 1,274 |
| Build Entity with present-nonempty description | 39 / 2,328 | 28 / 1,416 |

The description cases use sequence values 0, 1, and 2 respectively, so their
payload inputs also differ. The matched assignment controls show the cost of
setting the two present states, empty and nonempty, with already-created inputs;
they do not time input creation.

For pbrs, the packed-field length check after replacing three measurements
used 1 request / 8 bytes; recalculating with the packed cache warm used
0 / 0. Replacing them with extreme `i64` values then calculating length
used 3 requests / 70 bytes, including two reallocations. Field replacement
was outside each counted window. A Prost length comparison was not measured
for these cache-specific rows.

The recorded object layouts were 32 bytes for pbrs Entity versus 128 for
Prost Entity, and 8 bytes for `Repeated<Edge>` versus 24 for
`Vec<Edge>`. These sizes are specific to this build, exclude separate heap
allocations, and are not an ABI guarantee. They show why changing empty
collection representation requires both sparse-layout and populated-message
measurements. An inline packed-cache experiment could target the first-length
request, but it also needs warm-cache, large-value, mutation, layout, and
counters-off controls. Similarly, removing the optional-description box must
preserve both absent and present-empty values. Current generated defaults use
a zeroed representation; an `Option<LazyStr>` niche must not be assumed to
give the same valid zero representation or presence semantics on every target.

The diagnostic used a wrapped System allocator and a standalone adoption
dependency graph without the devloop copy-count feature. Allocator
instrumentation can affect optimization; requested bytes are neither live
memory nor peak RSS. Each native witness was checked against Prost for full
decoded value equality, and empty, clone, cache and mutation controls were
checked separately. These results do not establish a general performance
ranking or identify individual allocation call sites.
