# Unary filter state and allocations, November 10

An early-filter draft captured a boolean in the unary future and increased
that future's measured size by eight bytes. A follow-up reads the existing
cancellation handle instead; its measured future size returned to the
baseline size. These are build-specific measurements, not ABI guarantees.

| Source | Unary future | Cancellation `Inner` |
|---|---:|---:|
| [`1e6174f2`](https://github.com/mingley/pure-protobuf/commit/1e6174f2c6463f29488bcfbd432525e0cd9f94f1) baseline | 5,632 bytes | 72 bytes |
| [`9449f8e8`](https://github.com/mingley/pure-protobuf/commit/9449f8e8cabd05451edf460a0e30a9a73097ef29) unary-log draft | 5,632 bytes | 80 bytes |
| [`a0ba5497`](https://github.com/mingley/pure-protobuf/commit/a0ba54970b38d1157fbefaf98613c1d6ad84eb5b) captured-filter draft | 5,640 bytes | 80 bytes |
| [`ecc5c436`](https://github.com/mingley/pure-protobuf/commit/ecc5c436a9c3c151923cc3ce5b2e81ef27555b43) handle-based draft | 5,632 bytes | 80 bytes |

The cancellation `Inner` is still eight bytes larger in all three drafts on
this x86-64 build. A spare can be reused only after its handles are gone;
concurrent live flags cannot share one spare. The table measures the struct,
not the complete `Arc` allocation, allocator overhead or peak resident memory.

The [check](https://github.com/mingley/pure-protobuf/actions/runs/42734016136)
used Rust 1.99.1 on a shared AMD EPYC 9V45 runner. Layout probes instrumented
the source and were not used for timing. Separate allocation binaries
measured process-wide successful requests from a native client and server
over loopback. Each process warmed 50 RPCs; five alternating N=250 and 2N=500
samples estimate requests per added RPC.

| Logger / workload | Allocations | Reallocations | Baseline, prior and handle draft bytes | Captured draft bytes |
|---|---:|---:|---:|---:|
| No logger or excluded / success | 11.008 | 0 | 15,962.928 | 15,970.928 |
| No logger or excluded / cancellation | 26 | 0 | 18,562 | 18,570 |
| Enabled / success | 30.008 | 21 | 17,117.992–17,119.928 | 17,127.928 |
| Enabled / cancellation | 38 | 15 | 19,222–19,223 | 19,230–19,231.956 |

Enabled logging used a no-op sink; the excluded filter was checked to emit no
records. Cancellation included mpsc entry/drop coordination. The small
variation in enabled requested bytes comes from the observed samples. These
are warmed N/2N deltas, not cold allocation or peak-memory measurements; in
particular they do not measure the first boxed logging-state allocation.
They do not establish a performance win.

A separate [retained-call check](https://github.com/mingley/pure-protobuf/actions/runs/42736645471)
counted construction of N or 2N simultaneously live, unpolled calls after 50
warmups. Across five pairs in every logger mode, each additional call requested
two allocations and zero reallocations: 5,720 bytes at baseline, 5,728 in the
unrefined and handle-based drafts, and 5,736 in the captured draft. The Vec
capacity was allocated before counting, and calls were dropped after the
snapshot. This directly measures an eight-byte cold construction increase in
these drafts, but excludes drop-time logging-state allocation and peak memory.

The [data](unary-filter-layout-20261110.json) retains every allocation sample,
variant, source and binary hash. The original
[artifact](https://github.com/mingley/pure-protobuf/actions/runs/42734016136/artifacts/13727105497)
has SHA-256 `8e969c630cac5fc904dcbd6653a915f473e2c0810fe7499d61a8619c0801085e`
and expires November 24, 2026. The [retained-call artifact](https://github.com/mingley/pure-protobuf/actions/runs/42736645471/artifacts/13727072695)
has SHA-256 `b8559936d03559a70c4d131975df094afc4f3f8a0e76d2841c963d46f43254c1`
and expires November 24, 2026. The earlier
[integrated cancellation record](integrated-unary-cancel-screen-20261109.md)
has the preceding no-logger experiments.
