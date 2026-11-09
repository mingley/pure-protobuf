# Packed setter allocation experiment

The proposed `Packed::from_vec` generator change was **not merged**. This
experiment found an allocation reduction in an isolated setter lifecycle, but
no allocation change in the entity workloads and no demonstrated speedup.

[Scratch run 42684433117](https://github.com/mingley/pure-protobuf/actions/runs/42684433117)
passed its targeted correctness checks and deleted its scratch branch. Its
[raw artifact](https://github.com/mingley/pure-protobuf/actions/runs/42684433117/artifacts/13704536261)
has SHA-256
`cbf84305ad2ebd37cd04858d77b84318b6be5d50ed00cba76fc387e6ee4d601e`.
GitHub artifacts expire; retain a copy for longer-term reproduction.

The baseline was commit `6024ac884e6ccda3b14c77ca115732629e2217d7`;
the scratch commit was `cb5e97c70e497a5727c9df225be5e885fbf65215`.
The three builds used the same scratch checkout, replacing the two production
source files and using separate target directories:

| Build | `src/packed.rs` Git blob | `src/codegen/messages.rs` Git blob |
|---|---|---|
| Baseline | `311968f6fd59fe432ff037f9aec6f5464571e294` | `42844854d7c3008f1100619b2b896347b241ce04` |
| Legacy generator, new runtime | `2d52a85b23765b0c04ee3be87c65302890892908` | `42844854d7c3008f1100619b2b896347b241ce04` |
| New generator and runtime | `2d52a85b23765b0c04ee3be87c65302890892908` | `fe0d4fdb575b94df69a8045dc3163e99d1254fb5` |

The allocation probe was Git blob
`34a740bb65ea32a70acfe31f6ba830521c4b4aae`; the separate timing probe was
`4ef6bc226ddcaf558a7a7b640df0f8f1cb200c49`. The source-pinned workflow
was `3950dc2cac26e314fa4d26abcc7c135222c89e5f`. Each build verified
the freshly generated setter. The runner was x86-64 with four reported
vCPUs on an AMD EPYC 9V45, using Rust 1.99.1, LLVM 23.1.1, and
`libprotoc 35.1`. Release builds used thin LTO and one codegen unit.

The instrumented allocation samples were identical across repeats, and the
unchanged Prost controls matched across builds. Counts below include input
construction, message construction, setter execution and drop. Requested bytes
sum successful allocation requests and full new sizes for reallocations; they
are not retained memory.

| Lifecycle | Baseline | New generator | Change |
|---|---:|---:|---:|
| Nonempty three-element array setter | 4 requests, 240 bytes | 3 requests, 216 bytes | −1 request, −24 bytes |
| Nonempty three-element Vec setter | 4 requests, 240 bytes | 3 requests, 216 bytes | −1 request, −24 bytes |
| Empty array setter | 1 request, 88 bytes | 1 request, 88 bytes | none |
| Batch of 32 payloads, one populated | 35 requests, 2,968 bytes | 34 requests, 2,944 bytes | −1 request, −24 bytes per batch |

In the entity-list construction, warm outer encode, and fresh encode cases at
10, 100, and 1,000 entities, allocation counts and requested bytes were
unchanged in all three builds. For example, the fresh-encode case at 100
entities used 3,802 requests for pbrs and 2,735 for Prost, about 39% more
for pbrs. The pbrs Query d3 fresh-encode control used 43 requests versus 20
for Prost; sparse v0 used 7 versus 3. These are existing component-level
differences, not regressions introduced by the candidate. They are not RPC
latency or throughput measurements.

The separate timing binary had no counting global allocator. For six
2,000,000-call samples of the array setter lifecycle, median elapsed times were
78.485 ms for baseline, 78.325 ms for the legacy generator with the new
runtime, and 78.318 ms for the new generator and runtime. Candidate samples
ranged from 76.813 to 85.084 ms. This small, noisy shared-host microbenchmark
does not establish a speedup or regression, and it contains no Prost timing
comparison. The instrumented allocator can itself affect optimization, so its
request counts do not prove the same allocation change in the ordinary build.

The run checked the packed constructors on Rust 1.85 and stable, frozen
old-generated setters with the new runtime, a fresh generated setter's panic
recovery, formatting and library Clippy. It also compiled no_std Cortex
consumers and ran the semantic proof on a std-backed host; it did not execute
on Cortex hardware. The experiment is narrow and does not establish production
readiness, general gRPC performance, or a ranking against Prost or tonic.
