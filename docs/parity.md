# Tonic/prost parity and performance proof

The full claim does not pass. Direct comparisons found missing functionality,
behavior differences and a substantial codec performance loss. Two transport
bugs found by the audit are fixed. The [dated evidence](evidence/parity-proof-20261027/README.md)
contains the raw failures, successful controls, source pins and all benchmark rows.

## Compatibility

| Check | Result |
|---|---|
| Protobuf parse and merge against prost 0.14.4 | 652 of 654 proto3 operations agree. The other two use the same wrong-wire-type input. |
| Missing proto2 required field | pbrs rejects it; prost accepts it. |
| Generated consumers | Nine same-schema roundtrips pass, including imports, oneof, presence, maps, bytes, Timestamp and external mappings. |
| Embedded, alloc-only runtime | Prost compiles for `thumbv7em-none-eabi`; pbrs requires `std` and fails. |
| gRPC message presence against tonic 0.14.6 | All 18 raw-wire comparisons pass after the repair. |
| Existing mixed-peer suite | All 41 tests pass after restoring native connection information in the tonic service adapter. |

A wrong wire type for a known field is preserved as unknown by pbrs and
rejected by prost. Missing required-field handling also differs. These are
measured policy differences, not evidence that pbrs should abandon protobuf
conformance to imitate prost.

Full functionality also needs custom executors, an independent idle keepalive
policy, and configurable TCP_NODELAY. Those tonic capabilities have no native
equivalent today. Six `prost-build` configuration probes reproduce API and
representation customization gaps; these alone do not establish wire failures.
HTTP/1.1 gRPC-Web is available through the documented Tower/Hyper recipe.

The [comparison guide](guides/comparison.md) covers the broader migration
surface. Passing a finite corpus establishes its tested behavior; it cannot
establish every possible schema, API or wire input.

## Performance

The current codec comparison includes parsing, reading every populated field
and dropping each result. Both backends receive the same bytes. Five randomized
process pairs per row use separate CPU and allocation builds.

| Work | pbrs CPU | Prost CPU | Median paired pbrs/prost ratio |
|---|---:|---:|---:|
| 8-level message, read every field | 671 ns | 149 ns | 4.47× |
| 64-level message, read every field | 28,914 ns | 1,722 ns | 16.92× |
| 64-level message, parse only | 791 ns | 1,644 ns | 0.48× |

Ratios above one are losses. Parsing alone leaves work deferred. Source review
found that reading each nested child validates its remaining descendants again;
a 64-node chain causes 2,016 validation visits before counting materialization.
That repeated work needs repair. Scalar and packed-field rows do better than
prost; the complete evidence retains all wins and losses.

A separate same-Prost-codec 1 KiB unary comparison gives native client CPU
medians of 21 µs/RPC versus tonic’s 66 µs, and native server medians of
46 µs versus 31 µs. Pipeline results vary widely. These are local endpoint
diagnostics; all repeats and measurement limits are in the dated record.

The compiler audit checks generated behavior, not generator or consumer build
speed. The [earlier codegen matrix](evidence/sb09-codegen-matrix.md) retains
build-cost losses; it does not establish a ranking for the current generator.

These measurements ran on one shared x86_64 VM. They identify an engineering
problem; they do not establish a performance ranking. A scoped leadership claim
needs matched competitors, workloads, resources, successful calls and repeated
measurements on controlled hosts. The [benchmark contract](benchmark-contract.md)
defines those requirements. Comparing against tonic and prost alone cannot
establish leadership over other implementations.

## Reproduce and check claims

The [test index](../tests/proof/README.md) links the pinned consumers and commands.
The default protobuf differential command fails because the strict behavior
claim has counterexamples. Its explicit known-differences mode passes only
when exactly those counterexamples recur and the remaining cases agree.
The [parity audit workflow](../.github/workflows/parity.yml) runs that regression
check; green means the audit reproduced its results, including the gaps.

The [RPC CPU preflight](../tests/proof/ranking/README.md) checks raw comparative
campaigns and recomputes intervals. It rejects omitted required peers,
insufficient samples, changed sources/settings and incomplete calls. Its 13
tests include misleading winner labels, measured losses and synthetic data.
A numerical preflight cannot certify host isolation or honest measurements;
independent campaign review remains necessary.
