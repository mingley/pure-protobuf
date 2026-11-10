# Receive-accounting RPC screen, November 10

Two unmerged receive-accounting candidates were compared with their own
baseline. The paired CPU ranges overlap one and the same-binary controls vary,
so this shared-runner screen does not establish a reliable winner.

**Correctness follow-up:** Source review found that the measured read-progress
prototype credits discarded-frame allowance in
[`poll_data_released`](https://github.com/mingley/pure-protobuf/blob/e8a0895b3274f6e766d8f2c551019a9d7ff09f71/pbrs-grpc/src/h2_backend/proto/streams/streams.rs#L2501)
before flow-control release can fail and prevent the data from being returned.
A [later two-allowance variant](https://github.com/mingley/pure-protobuf/blob/43852fa2cd6a39346bd9524dd420f802c91ab4b1/pbrs-grpc/src/h2_backend/proto/streams/streams.rs#L2522)
has the same ordering. Both are held from integration; the later variant was
not measured here. A [real-wire differential](https://github.com/mingley/pure-protobuf/actions/runs/42748301962)
reproduced the unexpected credit on 43852; moving the credit after successful
flow-control release passed the paired control. That witness did not run on the
measured e8a prototype. This timing screen did not exercise the failed-release
path or establish correctness.

| Workload and concurrency | Cumulative / baseline CPU: median (range) | Read-progress / baseline CPU: median (range) |
|---|---:|---:|
| Unhedged, 64 | 1.0113 (0.9922–1.0498) | 0.9858 (0.9706–1.0117) |
| Unhedged, 256 | 0.9939 (0.9527–1.0031) | 1.0133 (0.9505–1.0310) |
| Paired hedges, 64 | 1.0090 (0.9796–1.0372) | 1.0172 (0.9906–1.0288) |
| Paired hedges, 256 | 0.9932 (0.9613–1.0058) | 0.9964 (0.9689–1.0319) |

Lower is better. The [run](https://github.com/mingley/pure-protobuf/actions/runs/42740210925)
compared baseline [`713038c7`](https://github.com/mingley/pure-protobuf/commit/713038c7f2d60da41824531a7b81920183c7510b)
with the unmerged [cumulative](https://github.com/mingley/pure-protobuf/commit/e6427b224a1797befd898f3f55665c2dd731d4d5)
and [read-progress](https://github.com/mingley/pure-protobuf/commit/e8a0895b3274f6e766d8f2c551019a9d7ff09f71)
candidates. Each release binary overlaid the three pinned HTTP/2 source files
onto the baseline; candidate documentation and test-only module declarations
were not overlaid.

The native client and an HTTP/2 peer used separate loopback processes, one
connection and no TLS. Both sides used four Tokio workers; the client and peer
were pinned to different logical CPUs that shared two physical cores. The
shared host used an AMD EPYC 7763 and Rust 1.99.1. Both builds used a
non-default 524,288-unit frame budget to avoid the baseline's 128-call burst
failure. Each process warmed 64 calls and timed 8,000 calls. Six balanced blocks
ran each variant twice in alternating order, with separate same-binary
controls. Peer stream, response and loser-reset counts were checked.

The paired workload cancels losing hedges but does not deliberately deliver
DATA after those cancellations. It measures the common RPC path rather than the
discarded-frame path. Latency samples and peer CPU are retained alongside the
client CPU ratios. This is not a tonic comparison or a production ranking.

A separate [layout check](https://github.com/mingley/pure-protobuf/actions/runs/42741975525)
measured `Counts` at 176→184 bytes and its enclosing connection `Inner` at
712→720 bytes; `Stream` stayed 328 bytes. These are x86_64 Rust 1.99.1
debug-build type sizes, not total heap or peak-memory measurements.

The [retained data](recv-budget-rpc-screen-20261110.json) reproduces all 24
timing blocks and controls from the run log. The original ZIP had a literal
backslash-n after its JSON. A separate
[verification run](https://github.com/mingley/pure-protobuf/actions/runs/42741218147)
checked the artifact and source pins, removed only that suffix, and retained
[normalized JSON](https://github.com/mingley/pure-protobuf/actions/runs/42741218147/artifacts/13728314589).
Its ZIP SHA-256 is
`5f198c2c5efc111fc77d02e0bfb5c5fa008502230b6fbceaec924492d440b6fb`
and it expires February 8, 2027.
