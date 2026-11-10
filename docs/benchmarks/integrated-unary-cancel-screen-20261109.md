# Integrated unary cancellation logging, November 9

An **unmerged** unary binary-log draft showed slower successful calls in all
ten pairs of one no-logger endpoint screen. A longer balanced repeat on a
second runner did not reproduce that result. The measurements do not establish
whether the draft regresses this workload, or why the screens differ.

| Workload | Candidate / baseline CPU: median (range) | Slower pairs or blocks |
|---|---:|---:|
| Successful unary call | 1.0316 (1.0025–1.1700) | 10 / 10 |
| Cancel after handler entry | 1.0098 (0.9576–1.0389) | 6 / 10 |
| Successful unary call, balanced repeat | 0.9918 (0.9855–1.0065) | 3 / 10 blocks |
| Same-host repeat, 2,000 calls | 1.0056 (0.9978–1.0147) | 7 / 10 blocks |
| Same-host repeat, 20,000 calls | 0.9984 (0.9940–1.0063) | 3 / 10 blocks |

Lower is better. The first two rows are ten alternating pairs on a shared AMD
EPYC 7763 runner. The first balanced repeat is ten ABBA/BAAB blocks on a
shared EPYC 9V74; the last two rows are interleaved ABBA/BAAB blocks on one
shared Intel Xeon 8573C. These results compare an integrated draft with its own
baseline; they are not a comparison with tonic and do not establish a
production ranking.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42723556353)
compared baseline [`001d0b8b`](https://github.com/mingley/pure-protobuf/commit/001d0b8bc9e89a098047695c3ba73401ba330ea3)
with unmerged candidate [`ec05162c`](https://github.com/mingley/pure-protobuf/commit/ec05162cefb3d1a4c549f1dbc213c8a5dfb8b8fe).
The scratch workflow and probe are in
[`5058ed9f`](https://github.com/mingley/pure-protobuf/commit/5058ed9fe525f9946fe8a5db48fe741929efbd02).
The candidate runtime blobs are `0b3f87a5e7b79468cfd4f58cf07f5473d8cf1205`
for `rpc_cancel.rs` and `cc1714e8ee8f4493842ebb1db1839d44949dd41c`
for `client/unary.rs`; the probe blob is
`14748b03c9a0aa9a9e58174663200919162e0575`. Both timed endpoint binaries
used private runtime source; neither widened module visibility.

The test used Rust 1.99.1 release builds on a shared AMD EPYC 7763 runner,
pinned to one logical CPU. A native client and server shared one current-thread
Tokio process and communicated over loopback TCP without TLS. Each process
warmed 50 RPCs; each case also had one warmup process per binary. Ten pairs
alternated execution order, with 2,000 successful calls or 500 cancellations
per process. Each reply was checked; cancellation waited for handler entry
and handler drop. The timed binaries omitted explicit handler-count,
allocation-count and copy-count instrumentation. The cancellation case
includes per-RPC entry and drop notifications on Tokio mpsc channels for coordination and verification;
their cost is included. No logger was configured; calls excluded by a
configured filter were not measured.

The [balanced repeat](https://github.com/mingley/pure-protobuf/actions/runs/42724812190)
reversed build order and used 20,000 successful RPCs per process. Each block
ran two baseline and two candidate processes in alternating ABBA/BAAB order;
the ratio compares the two candidate CPU times with the two baseline times.
It interleaved same-binary controls: baseline A/A ratios had median 0.9971
(range 0.9708–1.0117), and candidate B/B ratios had median 0.9971
(0.9902–1.0105). The repeat used the exact same endpoint binary hashes as
the first screen, but ran on a different processor with ten times as many
RPCs per process. Shared-host noise and processor or workload differences
could explain the differing results. The matching binary hashes rule out a
different rebuild as the cause; they do not isolate code-layout effects from
algorithmic effects within a screen.

A [same-host repeat](https://github.com/mingley/pure-protobuf/actions/runs/42726089592)
interleaved ten blocks at each batch length, alternating which length ran
first. Each block included A/A and B/B controls. For 2,000 calls the baseline
A/A median was 1.0048 (range 0.9831–1.0601) and candidate B/B median 0.9929
(0.9693–1.0092). For 20,000 calls the respective medians were 1.0034
(0.9971–1.0070) and 0.9941 (0.9883–1.0049). The shorter controls varied more.
These descriptive samples do not determine the cause of either screen's
result, or exclude differences on other machines and workloads.

Separate allocation-instrumented binaries measured process-wide requests for
both endpoints together. Across five alternating N=250 and 2N=500 pairs,
baseline and candidate each requested 11.008 allocations, zero reallocations
and 15,962.928 bytes per additional successful call; cancellation requested
26 allocations, zero reallocations and 18,562 bytes. These warmed-spare
differences are not cold-call or peak-memory measurements. A separate layout
probe measured the unary future at 5,632 bytes in both builds and the
cancellation `Inner` at 72 bytes for baseline and 80 for the draft.

An earlier [private-source screen](https://github.com/mingley/pure-protobuf/actions/runs/42720654773)
used one relaxed atomic handler counter per RPC in the timed binaries. Its
success and cancellation median ratios were 1.0055 and 1.0088, respectively.
It ran on a different, shared Intel Xeon 8573C runner; do not pool its times
with the later run. An even earlier
[diagnostic](https://github.com/mingley/pure-protobuf/actions/runs/42719416334)
also widened private runtime visibility before compiling the endpoint binaries,
so its timing is not a private-source comparison.

The [retained data](integrated-unary-cancel-screen-20261109.json) contains
all 20 endpoint CPU pairs from the no-handler-counter screen, the balanced
blocks and same-binary controls, the interleaved short/long blocks and
controls, wall times and order, allocation rows, binary hashes, and the earlier
screen's data. The corrected [original artifact](https://github.com/mingley/pure-protobuf/actions/runs/42723556353/artifacts/13721194034)
has SHA-256 `7f4830c7040bf24a4d6fbd4342365f56a58646b32583bcb30d39aa80560e975b`.
The earlier [artifact](https://github.com/mingley/pure-protobuf/actions/runs/42720654773/artifacts/13720172572)
has SHA-256 `7d21cb0431cc3bc0341bcceb8b3d0c84c16dd470e4bc2ea1e255195a71da122f`.
Both expire November 23, 2026. The [repeat artifact](https://github.com/mingley/pure-protobuf/actions/runs/42724812190/artifacts/13722640859)
has SHA-256 `0e02fa5185ce227e2687a51903b35b2ba1b1aec8bce9f99cf499af7ff85e8503`.
The [same-host artifact](https://github.com/mingley/pure-protobuf/actions/runs/42726089592/artifacts/13721784988)
has SHA-256 `a24212fe3f6dcf6538d4a262704565eeb7144e7f7b584c32d77212f345dc628f`.
Both expire November 24, 2026.
