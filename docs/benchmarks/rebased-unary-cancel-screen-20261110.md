# Rebased unary cancellation screens, November 10

Two endpoint screens compared an **unmerged, unrefined** unary binary-log
draft with its 1e baseline. The filtered-cancellation screen showed higher
candidate CPU in nine of ten blocks. All ranges crossed one, and the runners
were shared. These results do not establish a precise regression or a
production ranking. A later filter-selection refinement is not measured here.

| Configuration and workload | Candidate / baseline CPU: median (range) | Slower blocks |
|---|---:|---:|
| No logger, successful unary | 0.9971 (0.9309–1.0143) | 4 / 10 |
| No logger, cancellation | 1.0020 (0.9875–1.0543) | 7 / 10 |
| Logger configured, method excluded: successful unary | 1.0018 (0.9954–1.0042) | 7 / 10 |
| Logger configured, method excluded: cancellation | 1.0067 (0.9980–1.0169) | 9 / 10 |

Lower is better. The [no-logger run](https://github.com/mingley/pure-protobuf/actions/runs/42728961873)
used a shared AMD EPYC 9V74; the
[filtered run](https://github.com/mingley/pure-protobuf/actions/runs/42729369969)
used a shared Intel Xeon 8573C. Do not pool their raw times. The filter
excluded the method, and each process verified that its sink stayed empty.

The baseline is [`1e6174f2`](https://github.com/mingley/pure-protobuf/commit/1e6174f2c6463f29488bcfbd432525e0cd9f94f1)
and the draft is [`9449f8e8`](https://github.com/mingley/pure-protobuf/commit/9449f8e8cabd05451edf460a0e30a9a73097ef29).
Both screens used exact private runtime source, Rust 1.99.1 release builds,
and a native client and server in one current-thread Tokio process over
loopback TCP without TLS. Each process warmed 50 calls. Ten ABBA/BAAB blocks
used 10,000 successful calls or 2,000 cancellations per process, and each
block included baseline A/A and candidate B/B controls. The executable also
kept the configured-logger branch reachable and checked it in smoke tests.

Timed binaries omitted explicit handler-count, allocation-count and copy-count
instrumentation. Each successful reply was checked. Cancellation included
per-call Tokio mpsc entry/drop notifications for coordination and verification.
Runtime internals may use atomics. Same-binary control ranges were:

| Configuration and workload | Baseline A/A range | Candidate B/B range |
|---|---:|---:|
| No logger, successful unary | 0.9325–1.0195 | 0.9856–1.0153 |
| No logger, cancellation | 0.9722–1.0541 | 0.9620–1.0104 |
| Filtered, successful unary | 0.9910–1.0108 | 0.9872–1.0061 |
| Filtered, cancellation | 0.9962–1.0064 | 0.9829–1.0050 |

Separate allocation binaries measured process-wide successful requests for
both endpoints together. All five alternating N=250 and 2N=500 pairs matched
between baseline and draft in both configurations: per additional successful
call, 11.008 allocations, zero reallocations and 15,962.928 requested bytes;
per cancellation, 26 allocations, zero reallocations and 18,562 bytes. These
warmed-spare deltas do not include first-use boxed logging state, cold-call
costs or peak memory.

The [retained data](rebased-unary-cancel-screen-20261110.json) contains every
block, same-binary control, CPU and wall time, allocation pair, source blob
and binary hash. The original
[no-logger artifact](https://github.com/mingley/pure-protobuf/actions/runs/42728961873/artifacts/13723588798)
has SHA-256 `348c3786fb4dc2ebdc3fbb1927e2b24b81b8d932b6fbe636087d72ae0050f79a`.
The [filtered artifact](https://github.com/mingley/pure-protobuf/actions/runs/42729369969/artifacts/13724042623)
has SHA-256 `0720392918880b9ff54fa50d49408e66aa407d299dc3c6bc40ca6c4b4ead28cb`.
Both expire November 24, 2026. Earlier experiments against the older base are
in the [integrated unary cancellation record](integrated-unary-cancel-screen-20261109.md).
Neither record is a tonic comparison.
