# Allocation and generator follow-up, October 27

Source `31e7d69d36d670d37952a57e7063103549af260d` adds an allocation-free
MiniTable length walk, caches imported generator paths within each target,
and avoids server terminal clock reads when no observer uses them. It also
corrects large `uint32` map keys to unsigned varints. The comparison baseline
is `9ad348e6`. These are component diagnostics on a shared host; they do not
establish an overall tonic/prost or world performance ranking.

[Results](results.json), [raw captures](raw.tar.gz), and [manifest](manifest.json)
retain source and binary hashes, commands, fixtures, every measured loss and
rejected experiment. Verify them without extracting files with
`python3 docs/evidence/efficiency-20261027/check.py`. Compiled executables are
omitted. Original paths start with `work/`; the included reproduction notes
explain source reconstruction and dependency locks.

## MiniTable length calculation

Previously `serialized_len()` built the complete serialized message and its
temporary buffers. The new walk counts the wire representation without
creating those buffers. Actual serialization follows its existing path;
computing a length now does no encoding or allocation. Tests cover scalar
boundaries, packed fields, nested messages, groups, maps and unknown data.

| Nested levels | Requested allocations before → after | Requested bytes before → after |
|---|---:|---:|
| 0 | 2 → 0 | 16 → 0 |
| 8 | 17 → 0 | 204 → 0 |
| 64 | 129 → 0 | 5,082 → 0 |

Three alternating process pairs return matching lengths and wire-output
hashes. These are small synthetic fixtures. Their timings were recorded while
other builds ran and are diagnostic only. The table measures length
calculation, not parsing, serialization throughput or RPC latency.

A separate expected-wire control checks that a `uint32` map key of
4,294,967,295 uses five value bytes rather than ten sign-extended bytes.
Length calculation uses the same unsigned representation.

## Generator cache

Imported type names repeatedly required the same package and nested-module
formatting. The cache keeps each resolved imported path for one target,
clears it between targets and requests, and preserves local-name and extern
path precedence. All six measured outputs are byte-identical.

Multi-target allocation requests fall 7.60–13.17%, and requested bytes fall
0.61–1.24%. The single-file local-type control and retained output memory are
unchanged. Peak live requested bytes rise as much as 9.91% in some parallel
cases; this is not RSS.

Warm wall medians improve 3.04–8.11% across the five multi-target cases. Warm
CPU improves in four cases and rises 1.78% in the remaining case. Cold CPU
rises 10.86% in the 20-file default case; cold wall time rises 10.06% in the
100-file default case. Pair ranges are broad. These results support an
allocation reduction; they do not establish a consistent latency gain.
Both sides use the same scratch lockfile, including serde_json 1.0.154
instead of the repository lock's 1.0.151. Downstream compilation was not timed.

## Server terminal timing

The terminal transition now evaluates elapsed time only for the first end
with an observer. Registry accounting and callback ordering remain covered
by controls. The isolated unobserved-terminal probe measures 30.55 → 3.49 ns
per call; this removes one clock read and does not establish endpoint CPU
or latency improvement. The earlier pipeline allocation variance remains
unresolved.

## Rejected parser lookup

A checked dense-field lookup passed 49 kernel tests and strict Clippy but
slowed the 100-fragment fixture by 2.95% across six alternating pairs.
Allocations were unchanged. It was reverted; all 300 measured rows, its
patch and source hashes remain in the archive. The preceding group-repair
parse cost remains open.

## Validation

At the clean committed source above, 202 targeted core/kernel/recursion tests
and 750 all-feature gRPC library tests passed, with one existing library
ignore. Strict core all-target and gRPC library Clippy, the all-feature
workspace build check, and five Rust 1.85 core/native/Tower library profiles
passed. The official protobuf v35.1 runner passed two required runs and a
recommended run, each with 5,631 binary/JSON and 909 text successes, no skips
and no failures; its maximum tested edition is 2023.

All 13 bundled bindings matched regeneration with the pinned compiler. The
separate pinned-descriptor onboarding check also passed. These checks do not
replace a new full workspace integration run, sustained-resource qualification
or a controlled endpoint comparison.

The final documentation link and caveat suite passed 25 tests.
