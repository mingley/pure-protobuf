# Identity frame staging screen, November 10

A chunk-staging draft is **rejected**. It held more input backing allocations
and slowed whole-frame component cases in this screen. The gain for one
fragmented case does not establish an RPC speedup.

| Framing workload | Candidate / baseline elapsed time: median (range) |
|---|---:|
| 1 KiB whole frame | 1.350 (1.344–1.371) |
| 8 MiB whole frame | 1.355 (1.151–1.401) |
| 8 MiB in 1 MiB chunks | 0.983 (0.976–0.992) |
| 8 MiB in 64 KiB chunks | 1.004 (0.991–1.014) |
| 1 MiB in 16 KiB chunks | 1.003 (0.988–1.028) |

Lower is better. Each row has six alternating-order pairs on one shared
GitHub-hosted Linux runner, pinned to one logical CPU. The CPU model was not
recorded. These are private framing component timings, not tonic comparisons,
endpoint CPU measurements or an overall ranking.

The component test prepared its input before timing. Each iteration cloned
the input chunks, constructed and dropped a reader, decoded one frame, and
checked its length and first payload byte. The timer covers those operations;
it excludes the input preparation. Whole frames used one chunk. Fragmented
frames used the chunk sizes in the table. The 8 MiB / 64 KiB and 1 MiB /
16 KiB cases exceed the prototype's 32-chunk staging cap and fall back to
ordinary carry copying. The same source buffer backed the prepared chunks, so
this timing test does not reproduce HTTP/2 read-buffer reuse. The baseline
and candidate unit-test executables include different candidate-only tests;
their code layout can also affect timings.

For the memory check, two partial readers were fed eight interleaved 16 KiB
slices each. Every slice came from a separate synthetic owner requesting a
4 MiB backing allocation. At the checkpoint, 256 KiB of input was visible:
the baseline retained 2 owners (8 MiB requested) and the draft retained all
16 owners (64 MiB requested). Dropping the readers released every owner in
both cases. A single process high-water RSS sample was 22,704 KiB for
baseline and 71,976 KiB for the draft; that process-wide figure includes the
test harness and is not a production memory limit or an RPC measurement.

The draft staged validated, incomplete large identity frames, coalesced after
all frame bytes arrived, and fell back after at most 32 staged chunks. A chunk
count bounds metadata but does not bound the size of each referenced backing.
The actual HTTP/2 decoder uses shared read allocations; its
[delayed-reserve design](../../pbrs-grpc/src/h2_backend/codec/framed_read.rs)
depends on earlier returned frames being dropped so it can reclaim a shared
remainder. Holding chunks in the reader can defeat that optimization. The
synthetic owner test demonstrates the retention mechanism; it does not measure
the backing sizes or pressure of a real multiplexed connection.

The [screen](https://github.com/mingley/pure-protobuf/actions/runs/42731854600)
used baseline [`8a84230d`](https://github.com/mingley/pure-protobuf/commit/8a84230db1ba1b4e3fc28911149744a50e2381f6)
and unmerged draft [`014330aa`](https://github.com/mingley/pure-protobuf/commit/014330aada94df96f3e31877717c012071fe2090).
Rust 1.99.1 and LLVM 23.1.1 built release unit-test executables; allocation
and copy counters were off. The [raw paired timings and memory observations](identity-frame-staging-20261110.json)
include binary hashes and process time/RSS samples. The temporary branch was
deleted.

A separate [correctness run](https://github.com/mingley/pure-protobuf/actions/runs/42731482152)
passed three draft-specific tests and 22 existing wire tests. Its exact-length
mutation compiled and failed at the intended assertion, but the workflow
itself failed because `rg` was unavailable for its log verifier. This is
not a passing full correctness or integration gate.
