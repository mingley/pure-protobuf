# Query field placement experiment, November 9

Keeping the four repeated fields of the adoption `Query` inline reduced
allocation requests for populated fixtures. It also grew an empty Query from
32 to 56 bytes, increased requested bytes during the larger decode, and
showed CPU losses in two tested stages. **The Query-specific overlay was not
merged.** It does not establish a general codegen policy or RPC performance.

The [successful run](https://github.com/mingley/pure-protobuf/actions/runs/42714223573)
compared baseline [`001d0b8b`](https://github.com/mingley/pure-protobuf/commit/001d0b8bc9e89a098047695c3ba73401ba330ea3)
with the [scratch overlay](https://github.com/mingley/pure-protobuf/commit/62ab28999c080d1dc8e6b54bae367e9ed5c11c2c).
The [retained comparison records](query-field-placement-20261109.json)
contain all 33 allocation rows and all 30 CPU cells from the job log. The
[raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42714223573/artifacts/13719615240)
has SHA-256 `d59415e18955d8fbfe7c3a4676ac5b54d0e5230a9fb7b7c119ac3e22ef4281ab`
and expires February 7, 2027. The scratch branch was deleted.

The overlay disables cold storage only for `adoption.Query`; it is not a
proposed heuristic. The generated Query size is 32 bytes at baseline, 56 with
the overlay, and 104 for Prost in this build. These sizes exclude separate
heap allocations and are not ABI guarantees.

## Allocation requests

The following cells show **requests / requested bytes** per operation from
`(2N − N) / N`, with N = 200. Each arm produced two independent reports,
each with repeated N and 2N samples; the validator required agreement.
Reallocations count as requests at their full new size, and all displayed
and retained rows had zero reallocations. The counting allocator and pbrs
`copy-counts` instrumentation were enabled; requested bytes are not live
or retained memory.

| Fixture | Operation | Baseline pbrs | Inline pbrs | Prost |
|---|---|---:|---:|---:|
| one_predicate | fully read request clone (prepared) | 5 / 296 | 4 / 264 | 3 / 78 |
| one_predicate | request decode (shared) | 5 / 512 | 4 / 480 | 3 / 272 |
| one_predicate | request decode (unique) | 6 / 536 | 5 / 504 | 3 / 272 |
| one_predicate | request decode read (shared) | 6 / 552 | 5 / 520 | 3 / 272 |
| one_predicate | request decode read (unique) | 7 / 576 | 6 / 544 | 3 / 272 |
| one_predicate | request first read (shared) | 1 / 40 | 1 / 40 | 0 / 0 |
| one_predicate | response encode (shared) | 1 / 31 | 1 / 31 | 1 / 31 |
| query.d4.v6 | fully read request clone (prepared) | 39 / 2340 | 33 / 2268 | 21 / 1182 |
| query.d4.v6 | request decode (shared) | 39 / 4168 | 34 / 4416 | 21 / 3935 |
| query.d4.v6 | request decode (unique) | 40 / 4192 | 35 / 4440 | 22 / 3959 |
| query.d4.v6 | request decode read (shared) | 47 / 4868 | 41 / 5084 | 21 / 3935 |
| query.d4.v6 | request decode read (unique) | 48 / 4892 | 42 / 5108 | 22 / 3959 |
| query.d4.v6 | request first read (shared) | 8 / 700 | 7 / 668 | 0 / 0 |
| query.d4.v6 | response encode (shared) | 1 / 298 | 1 / 298 | 1 / 298 |

The first-read and encode results match for unique and shared inputs; response
decode+read matches request decode+read for each input mode. Empty inputs use
0 / 0 for clone, decode and read, and 1 / 5 for response encode, with no
change between arms. The response encode scope includes a five-byte framing
stand-in. These are separate synchronous component scopes and cannot be
added to estimate an RPC.

For the 293-byte d4v6 fixture, shared decode used five fewer requests but
requested 248 more bytes; decode+read used six fewer requests but requested
216 more bytes. The inline representation still made more requests than
Prost in these fixture stages.

## Counter-free CPU

Each cell is inline / baseline process CPU: median and observed minimum–maximum
of five paired N-to-2N slopes. ↓ means all five ratios favor inline, ↑ means
all favor baseline, and ~ means the range crosses 1. A cell is below the
floor if any raw N-to-2N process-CPU difference is less than 5 ms; no ratio
is reported for it. The d3v0
encode slopes were negative. These are descriptive ranges, not confidence
intervals; smaller is faster.

| Query shape | Prepared clone | Shared decode | First read | Shared decode + read | First encode |
|---|---:|---:|---:|---:|---:|
| empty | below floor | below floor | below floor | below floor | below floor |
| sparse | below floor | below floor | below floor | below floor | below floor |
| one_predicate | below floor | below floor | below floor | below floor | below floor |
| query_d3_v0 | 0.956 (0.882–1.035) ~ | 0.935 (0.928–0.954) ↓ | 1.127 (1.044–1.446) ↑ | 0.896 (0.893–0.951) ↓ | negative |
| query_d4_v6 | 0.916 (0.800–1.047) ~ | 0.888 (0.873–0.899) ↓ | 1.009 (0.926–1.016) ~ | 0.910 (0.887–1.014) ~ | 1.002 (0.927–1.081) ~ |
| query_d8_v6 | 0.917 (0.905–1.128) ~ | 0.916 (0.901–1.001) ~ | 1.010 (0.998–1.057) ~ | 0.897 (0.868–0.924) ↓ | 1.016 (1.008–1.037) ↑ |

The d3v0 first-read and d8v6 first-encode samples favor baseline in every
pair. The d3v0 and d4v6 shared-decode samples and d3v0 and d8v6 decode+read
samples favor inline in every pair. Other usable ranges cross 1, and the
small fixtures fall below the measurement floor. These results do not show
that either layout is best for all schemas or workloads.

The CPU binary did not use a counting allocator or the `copy-counts`
feature. Each stage includes the relevant message or output drop. Inputs
and full-wire witnesses were checked before measurement. Fresh N and 2N
processes prepare different input volumes, which can change cache and
allocator state; their difference is a two-size lifecycle slope, not a
guaranteed per-operation latency. The screen used one shared Ubuntu 24.04
x86-64 runner with four vCPUs on an AMD EPYC 7763, Rust 1.99.1, thin LTO,
and one codegen unit. CPU affinity does not control host load or frequency.

## Checks

Both arms passed five Query contract tests with stable Rust and Rust 1.88,
including full-value, unknown-field, duplicate-merge, clone and mutation
controls. Both passed eleven codegen compatibility tests on Rust 1.85.
Root and scratch formatting passed. Source, lockfiles, compiler and pinned
`protoc` were checked. The CPU and allocation builds generated identical
files within each arm; across arms, only the delimited Query block changed
in `pbrs/adoption.rs` and `pbrs/sparse.rs`. All bytes outside those
blocks, including every descriptor, matched. Negative controls rejected unexpected
outside changes, a changed Query copy, an extra file and a changed descriptor.

A separate untimed build confirmed that enabling Prost's build-only
`cleanup-markdown` feature changed exactly two known documentation blocks
and did not enable `copy-counts` in the CPU build. Its negative controls
also rejected unexpected attributes, documentation and pbrs source changes.
The earlier [unaligned run](https://github.com/mingley/pure-protobuf/actions/runs/42705012774)
and [first aligned run](https://github.com/mingley/pure-protobuf/actions/runs/42709337735)
failed their generated-output checks before measuring; the successful run
above is the source for every value on this page.
