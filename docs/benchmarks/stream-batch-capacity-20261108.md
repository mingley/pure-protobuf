# Server-stream batch capacity diagnostic, November 8

A one-off experiment on a shared GitHub-hosted runner compared a smaller
initial server-stream item buffer with the existing burst-sized allocation.
The candidate is an experimental patch; these measurements do not qualify
current main or establish a performance win.

The [run](https://github.com/mingley/pure-protobuf/actions/runs/42521790701)
and [job log](https://github.com/mingley/pure-protobuf/actions/runs/42521790701/job/128856665714)
record the results. The
[raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42521790701/artifacts/13634605573)
contains the schedule, samples, source patch, toolchain and correctness logs.
Its uploaded ZIP has SHA-256
`e0c5d1802fcc0d6097d4e417fcc97537b44c14c7ea4a5ed41db50cdf4cc354f5`;
GitHub listed its expiration as December 8, 2026. The table below preserves
the job's reported summary if the artifact expires.

## Inputs and controls

The baseline is historical commit
[`0cf60a9fecffec2b761515f740d0f25b7d0cc13c`](https://github.com/mingley/pure-protobuf/commit/0cf60a9fecffec2b761515f740d0f25b7d0cc13c),
not the commit running the workflow. Both sides used detached worktrees of that
baseline. The candidate applied the
[16 KiB initial-capacity patch](https://github.com/mingley/pure-protobuf/blob/36dce54ce097a39655a8ae1b32ca9f7de8686075/.github/experiments/stream-batch-16k.patch)
to `pbrs-grpc/src/server/drain.rs`. It chooses a power-of-two initial item
capacity from the item size and a 16 KiB target, clamped to the existing burst
limit; the vector can grow and one item may exceed the target. The receive
limit and permits buffer remain unchanged. The baseline and candidate drain
Git blob IDs were `1dca292525ed01028682bec341d7fdd124308c63` and
`331be368a605cb431c25a42a438fdc9856caf9a0`; the input patch blob was
`49e44cf3294dc4c63ab488484f1ac6c6b89bbe7f`.

The job built identical harness sources in separate target directories using
Rust 1.99.1 and the pinned protoc. It ran 192 sequential samples: two message
fixtures, two producer modes, 4/64/128 replies, N=100 and 2N=200 RPCs, two
sides, and four repeats. Each sample used 25 warmup RPCs; baseline/candidate
and N/2N order were balanced. After sampling, four diagnostic tests and five
server-streaming tests passed, and the job checked the source and binary pins.
These controls cover the experiment, not every production behavior.

## Paired allocation observations

Each value is the median of four paired **candidate minus baseline** per-RPC
slopes, where a side's slope is `(counter at 200 - counter at 100) / 100`.
Brackets give the minimum and maximum of those four paired differences.
Bytes are requested allocation bytes; reallocations count as allocation calls
and their full requested size contributes to bytes. They are not retained or
resident memory. `live8` uses a live producer with channel capacity 8;
`ready128` preloads replies into a capacity-128 channel before returning the stream.

| Fixture | Producer | Replies | Requested bytes Δ [min, max] | Allocation calls Δ | Reallocations Δ |
|---|---|---:|---:|---:|---:|
| sparse-v0 | live8 | 4 | -144000.76 [-144008.60, -144000.00] | 0.00 | 0.00 |
| sparse-v0 | live8 | 64 | -85810.42 [-86571.40, -85329.84] | 2.22 | 2.00 |
| sparse-v0 | live8 | 128 | -85856.64 [-87119.16, -84177.36] | 2.20 | 2.00 |
| sparse-v0 | ready128 | 4 | -144000.76 [-144004.60, -143999.04] | 0.00 | 0.00 |
| sparse-v0 | ready128 | 64 | 144003.82 [144000.00, 144015.28] | 4.03 | 4.00 |
| sparse-v0 | ready128 | 128 | 144000.00 [144000.00, 144000.00] | 4.00 | 4.00 |
| query-d3-v0 | live8 | 4 | -1.24 [-3.64, 2.48] | 0.00 | 0.00 |
| query-d3-v0 | live8 | 64 | -2971.14 [-24868.92, 1646.40] | -0.70 | 0.00 |
| query-d3-v0 | live8 | 128 | -125.72 [-15883.28, 22204.68] | -0.03 | -0.01 |
| query-d3-v0 | ready128 | 4 | 1.82 [-1.52, 2.48] | 0.01 | 0.00 |
| query-d3-v0 | ready128 | 64 | 0.00 [-6.12, 0.00] | 0.00 | 0.00 |
| query-d3-v0 | ready128 | 128 | 0.00 [0.00, 6.12] | 0.00 | 0.00 |

The sparse four-reply cases requested fewer bytes in this capture, while the
larger ready-producer cases requested more bytes and made four additional
reallocations per RPC. The live-producer sparse cases traded fewer requested
bytes for about two additional reallocations at 64 and 128 replies. Query
live-producer ranges were mixed and wide for the larger bursts.

The client and server run in the same process, so allocator samples combine
both endpoints. Individual counter updates are atomic, but the relaxed
multi-counter snapshots, arming boundary and overflow flag are not an atomic
transaction and can race with allocator callbacks. The shared runner is not an
isolated host; wall time was instrumented and is not a latency or CPU result.
Use a controlled, counters-off performance campaign and separate endpoint
accounting before drawing a shipping conclusion.
