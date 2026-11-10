# Streaming sender-failure logging: performance screen

[Benchmarks](../benchmarks.md) · November 10, 2026

The streaming sender-failure change landed in
[f73b85d](https://github.com/mingley/pure-protobuf/commit/f73b85d067ea861fe36f23536153e4c5add3137f).
It attributes a native upload failure only after the ordered upload loop
reaches it. Other streaming cancellation-attribution gaps remain; see
[release readiness](../release-readiness.md). This screen does not compare tonic
or establish general gRPC performance.

## Setup

The [CPU and allocation screen](https://github.com/mingley/pure-protobuf/actions/runs/42777653060)
used Rust 1.99.1 on a shared AMD EPYC 9V74 host. A native client and server
shared one loopback process and one pinned CPU, without TLS. Client-streaming
requests carried 32-byte payloads; server-streaming and bidi responses carried
32-byte payloads. Bidi requests specified the response size. The short case
sent four messages over each of 500 calls; the long case sent 128 messages
over each of 20 calls.

The comparison used a common harness based on
[63ff4b4](https://github.com/mingley/pure-protobuf/commit/63ff4b4072f35631c9ddb11dadec76c9487ef80b).
It overlaid five runtime files from either
[b0e6931](https://github.com/mingley/pure-protobuf/commit/b0e693148fd8adcd242aa247358bb87dcacb99f5)
or 63ff4b4. It is not a comparison of two independently checked-out full
trees. CPU figures have ten balanced blocks and same-binary controls per case.
The result is candidate CPU divided by baseline CPU; lower is better. The ranges
below are observed minima and maxima, not confidence intervals.

| Logging | Shape | Length | Ratio median (range) | Baseline control range | Candidate control range |
| --- | --- | --- | --- | --- | --- |
| None | Client stream | Short | 1.0063 (0.9891–1.0204) | 0.9346–1.0262 | 0.9821–1.0418 |
| None | Client stream | Long | 1.0083 (0.9938–1.0300) | 0.9696–1.0237 | 0.9408–1.0405 |
| None | Server stream | Short | 1.0024 (0.9764–1.0298) | 0.9818–1.0393 | 0.9814–1.0239 |
| None | Server stream | Long | 0.9968 (0.9860–1.0015) | 0.9808–1.0612 | 0.9810–1.0323 |
| None | Bidi | Short | 1.0018 (0.9907–1.0109) | 0.9846–1.0169 | 0.9883–1.0154 |
| None | Bidi | Long | 1.0077 (0.9946–1.0207) | 0.9733–1.0249 | 0.9763–1.0187 |
| Excluded | Client stream | Long | 1.0098 (1.0003–1.0268) | 0.9323–1.0297 | 0.9869–1.0228 |

Of all 18 tested shape, length and logging combinations, 17 observed ratio
ranges crossed 1. The excluded client-stream long case above was the
exception. Controls were broad; these samples do not establish a general
speedup or rule out a regression. The full controls, raw samples, logger-enabled
cases and allocation pairs are in the [saved data](streaming-sender-failure-20261110.json).

## Allocation tradeoff

With logging absent or filtered out, five allocation pairs per shape had the
same allocation counts for b0e6931 and 63ff4b4. Client- and server-streaming
requested-byte totals matched in every pair. Bidi requested 4,000 more bytes
over 500 calls, or eight more requested bytes per call. These are process-wide
allocation requests, not peak live memory.

Each queued upload error now carries a boxed origin tag. A separate
[layout witness](https://github.com/mingley/pure-protobuf/actions/runs/42771033487)
modeled a boxed tagged status on x86-64 with Rust 1.85 and 1.99. Its five
samples per configuration requested one additional 24-byte allocation. This
is a representation diagnostic, not a measured failure-RPC latency; layout can
vary by compiler and target.

A [separate histogram and future-layout screen](https://github.com/mingley/pure-protobuf/actions/runs/42777585064)
used a common 63ff-based harness with runtime-file overlays for b0e6931,
[260465c](https://github.com/mingley/pure-protobuf/commit/260465c07898297fc46d035c1b790df8c697a254),
and 63ff4b4. In five no-logger triples, 260465c changed a server-future
allocation from 3,048 to 3,056 bytes per call; 63ff4b4 returned it to
3,048 by ending the boxed error's lifetime before the pending flush. The
separate 840-to-848-byte bidi allocation remained. The diagnostic is
compiler-, target-, and workload-dependent.

A [follow-up diagnostic](https://github.com/mingley/pure-protobuf/actions/runs/42779231075)
measured the live-bidi future as 840 bytes on b0e6931 and 848 on
[268edce](https://github.com/mingley/pure-protobuf/commit/268edce99745469da24fd6c59d3a782d8407683e).
Its two reduced-state mutants failed the dropped-Call or deadline-ordering
controls, so neither is a valid optimization.
A [three-state variant](https://github.com/mingley/pure-protobuf/actions/runs/42781468063)
passed those controls but still measured an 848-byte bidi future and the same
allocation histograms in five pairs per shape. It was set aside.

A separate [exact-source allocation check](https://github.com/mingley/pure-protobuf/actions/runs/42782159653)
built detached 63ff4b4 and [f73b85d](https://github.com/mingley/pure-protobuf/commit/f73b85d067ea861fe36f23536153e4c5add3137f)
trees with the same allocation probe. On Rust 1.99.1 and a shared AMD EPYC
9V45 host, all five pairs for each shape with logging absent or excluded had
matching allocation histograms, counts and requested bytes; none overflowed
the histogram. The probe executable hashes differed. This check measures no
CPU time, so the timing table above remains specific to 63ff4b4.

## Evidence

The [saved data](streaming-sender-failure-20261110.json) retains the measured
samples and source pins. The [source patch and fixtures](../evidence/streaming-sender-failure-20261110/README.md)
preserve the measured prototype and probes. The original
[CPU artifact](https://github.com/mingley/pure-protobuf/actions/runs/42777653060/artifacts/13744788740)
has SHA-256 `ca04317fb62051650346342b69599c5a2b241e7ba0c11688cbedafc9ffc33d56`;
the [histogram artifact](https://github.com/mingley/pure-protobuf/actions/runs/42777585064/artifacts/13745735763)
has SHA-256 `8e041648ceb8da169a0bdcb14b5c88b90f657445658985404416abca09025852`.
The [exact-source allocation artifact](https://github.com/mingley/pure-protobuf/actions/runs/42782159653/artifacts/13747330515)
has SHA-256 `143dbdb1e58d7a703600e73f6188c66783746cb3adf92267831aacf6ce2b49cd`.
The archived artifacts expire on February 8, 2027. The CPU screen measures
successful calls, not the cold failure callback.
