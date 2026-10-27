# Transport controls, codegen options and map iteration, October 27

Implementation `12cfd09f` adds native TCP_NODELAY settings, client idle
keepalive policy, connection-driver executors, generated type names and URLs,
retained enum prefixes and typed well-known `Any` conversions. It also removes
the temporary projection vector from typed map iteration. This record compares
the implementation with `77f361eb` and preserves the cases that slow down.

[Results](results.json) · [Raw data](raw.tar.gz) · [Manifest](manifest.json)
· [Checksum checker](check.py). See the [benchmark guide](../../benchmarks.md),
[transport guide](../../guides/transport-controls.md),
[codegen guide](../../guides/codegen.md) and [parity summary](../../parity.md).

## Map iteration

Typed views borrow their entries and project each key/value when yielded.
Small-map lookup compares borrowed keys before considering an owned indexed
lookup. Typed iterator hints use cheap conservative bounds; exact unique-key
counting would duplicate the iterator's duplicate scan. Raw arena snapshots
retain their existing projection and lifetime handling. No new unsafe code is
needed for the typed path.

All 42 warm integer-map iteration rows improve in median paired CPU against
the baseline. Collection uses 11.1–44.2% less CPU. These probes construct maps
and warm their indexes before timing; they do not measure parsing or cold
index construction. The control labelled `prost` in the raw map rows is the
standard `HashMap` representation, not a Prost decode operation.

| Iterate every distinct entry | Before | After | Standard HashMap | Paired after/before |
|---|---:|---:|---:|---:|
| 4 entries | 27.4 ns | 8.9 ns | 6.2 ns | 0.323 |
| 64 entries | 525.5 ns | 345.1 ns | 64.5 ns | 0.655 |
| 256 entries | 2,441.6 ns | 1,919.6 ns | 245.1 ns | 0.783 |

The remaining large-map loss is substantial. Some fast first-entry samples
are shorter than one millisecond. They establish a local before/after
diagnostic, not a ranking or a cold-map result.

## Full codec lifecycle

The separate codec matrix parses, optionally reads fields, and drops each
message. Both implementations receive identical wire bytes and verify decoded
checksums before timing. These rows use real Prost 0.14.4 controls.

| Mixed message, parse/read/drop | Before | After | Paired after/before | Paired after/Prost |
|---|---:|---:|---:|---:|
| Default lazy profile | 914.5 ns | 870.2 ns | 0.964 | 1.006 |
| Owned nested-message profile | 917.3 ns | 955.6 ns | 1.059 | 1.128 |

Each profile removes one allocation and 128 requested bytes in this read.
Lazy allocation traffic falls from 1,920 to 1,792 bytes; owned traffic falls
from 1,872 to 1,744. Prost requests 976 bytes. Requested bytes measure allocator
traffic, not RSS or retained memory.

The owned full read regresses 5.85%, and owned mixed parsing alone regresses
16.3%. Sparse full reads regress about 20% in both profiles. Direct map gains
therefore do not establish an overall codec gain. The cause of the profile
timing difference is unproven; only the map source changes in these frozen
codec comparisons.

Five randomized process pairs per row contain three samples per process.
CPU and allocation captures use separate binaries. The final matrices retain
all 32 codec rows and 42 map rows, not just those shown above. The superseded
V1 implementation also remains in the archive: it removed the allocation but
counted exact iterator hints, slowing several small-map collection cases.
The final V2 implementation removes that extra scan. Both versions' full
campaigns, including losses, are retained.

## Default transport regression comparison

All 120 captures complete 1,890,000 RPCs without terminal errors. Each side
uses the same Prost codec and matched handlers, one connection and one call
in flight, with two server runtime workers. Unary RPCs carry 1 KiB; pipeline
RPCs carry 64 messages of 1 KiB. CPU uses N/2N differences and five randomized
before/after pairs with fresh tonic controls. These settings come from the
harness configuration; per-capture FAIRNESS and socket attestation are absent.

| Native endpoint | Before | After | Paired after/before | Pair range |
|---|---:|---:|---:|---:|
| Unary client | 20.0 µs/RPC | 19.5 µs/RPC | 1.000 | 0.647–1.441 |
| Unary server | 27.5 µs/RPC | 25.5 µs/RPC | 0.905 | 0.690–1.208 |
| Pipeline client | 240 µs/RPC | 220 µs/RPC | 0.917 | 0.414–2.182 |
| Pipeline server | 210 µs/RPC | 220 µs/RPC | 1.045 | 0.750–1.286 |

Every before/after range spans one. Fresh tonic pipeline controls also drift
substantially. This is inconclusive timing evidence; it establishes neither
an incremental transport CPU gain nor a regression-free claim. All controls
and individual captures remain in the archive. Separate medians need not
produce the median of paired ratios.

The candidate also uses less endpoint CPU than its fresh tonic control in
these local unary pairs:

| Unary endpoint | Native | Tonic | Paired native/tonic | Pair range |
|---|---:|---:|---:|---:|
| Client | 19.5 µs/RPC | 65.5 µs/RPC | 0.294 | 0.201–0.426 |
| Server | 25.5 µs/RPC | 31.0 µs/RPC | 0.823 | 0.588–0.951 |

All five unary pairs favor native on both endpoints. Pipeline native/tonic
paired medians are 0.727 for the client (range 0.450–1.091) and 0.484 for the
server (0.423–0.529). The client loses one pipeline pair, and the control drift
limits broader interpretation. These results describe the stated same-codec,
closed-loop settings on this host; they do not establish latency under offered
load, overall gRPC superiority or a world ranking.

## Functional coverage

The native transport tests cover TCP sockets with and without TLS, idle
keepalive with fixed and adaptive windows, stream leases, reactivation and
connection cleanup. An outstanding PING retains its original acknowledgement
deadline even when the interval is shorter than the timeout or an RPC ends.
The default connection path avoids the optional idle-activity waker.

Custom executors run connection drivers and their inline handlers through all
four RPC shapes, lazy/eager connection, redial and custom I/O. Tests exercise
executor rejection, panic and ownership when a response outlives its channel.
Tokio still supplies I/O, timers and helper tasks. Static balance helpers
reject endpoints with custom executors; this is a remaining migration gap.

The generated consumer checks nine original roundtrips and 27 additional
controls against pinned Prost: names, nested packages, domain selection,
default URLs, `Any` conversions, malformed URLs/payloads, enum aliases and
proto2 defaults. The three remaining configuration probes are `btree_map`,
`bytes` and `boxed`. The embedded alloc-only runtime remains unsupported.
Wrong-wire-type and missing-required-field policy differences remain explicit
counterexamples to a strict Prost behavior-equivalence claim.

All 23 final validation steps pass on unchanged production and test inputs:

| Check | Result |
|---|---|
| All-feature workspace | 3,963 passed, zero failed, nine existing ignores |
| Core and transport Clippy | Warnings denied, passed |
| Owned-message consumer | Run and strict Clippy passed |
| Tonic message presence / mixed peers | 18 comparisons / 41 tests passed |
| Prost wire differential, both profiles | 652 of 654 operations agree; exact known differences reproduced |
| Codegen consumers | Nine original roundtrips and 27 new configured controls passed |
| Rust 1.85 | Native, Tower and owned-message consumer passed |
| Pinned descriptors / default regeneration | Passed; all 13 default bindings unchanged |
| Official conformance, lazy and owned | Each profile passed three runs of 5,631 binary/JSON and 909 text cases; zero skips or failures; maximum Edition 2023 |
| Documentation / performance claim checker | 25 documentation tests and 13 claim-checker tests passed |

The standalone strict Prost comparison intentionally returns failure for the
known counterexamples. Its known-differences mode passes only when exactly
those differences recur; the final validation checks both outcomes. This is
not a strict behavior-equivalence pass.

Earlier focused logs identify collection-time file hashes. Final validation
binds production and test inputs to `12cfd09f`. Default conformance reports a
dirty checkout because documentation was being completed; the input manifest
confirms unchanged production/test files. The isolated owned conformance
checkout is dirty only from its 13 explicitly regenerated bindings; before/after
manifests and a reconstruction patch retain those changes.

## Reproduce

The archive contains frozen source/build manifests, exact variant overlays,
lockfiles, commands, allocation captures, CPU samples, failed attempts and
final validation logs. Its reconstruction guide describes which baseline and
overlays produce each measured variant. Full duplicate source trees, tool
installations, executables and build caches are omitted.

The RPC comparison uses one immutable randomized plan resumed in bounded
chunks. It aggregates only after all captures finish. The candidate transport
differs from the final source by reviewed formatting; successful same-Prost
RPC cells do not exercise the changed pbrs map runtime. The exact differences
and normalized-source checks are retained rather than claiming binary equality.

These measurements ran on one shared x86_64 VM. They do not establish full
tonic/prost parity or a world performance ranking. Build elapsed times are
provenance and do not constitute a codegen-speed comparison.
