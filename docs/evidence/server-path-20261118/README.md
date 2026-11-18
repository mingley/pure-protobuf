# Server method path ownership

Owned method paths used to be copied so the handler and the outbound response
could each refer to the path. The server now passes ownership to the handler
when it has no response hook or lifecycle observer. It still supplies the
outbound context when a response hook runs.
[Source change](https://github.com/mingley/pure-protobuf/commit/46073280ab260106a3cc8c041a1d59eb711306b7)

| Warmed x86-64 loopback, 1,024 RPCs per sample | Allocation requests per RPC | Requested bytes per RPC |
| --- | ---: | ---: |
| Unary, cached path | 8 → 8 | 16281 → 16281 |
| Unary, owned path | 11 → 10 | 16577 → 16541 |
| Server stream, owned path | 19.002 → 18.002 | 23630.039 → 23595.039 |
| Unary with response hooks | 41 → 41 | 20951 → 20951 |
| Unary with lifecycle observer | 20 → 20 | 16771 → 16771 |

Both endpoints ran in one process. Each case used a baseline, candidate and
baseline-repeat sample. The two baseline samples had identical counts. This
screen measures allocator requests and accumulated bytes. It gives no endpoint
CPU or latency estimate.
[Raw totals](allocation-results.json) ·
[Original allocation archive](https://github.com/mingley/pure-protobuf/actions/runs/44350911965/artifacts/14449535370) ·
[Hook and observer archive](https://github.com/mingley/pure-protobuf/actions/runs/44352431584/artifacts/14448523892)

The original run measured the ordinary cases, then stopped at its hooked
baseline check because the probe appended metadata while expecting replacement.
The replay corrected that test and verified path, compression and limit
overlays, hook ordering, trailer metadata, rejection and observer status.
Compiler-reported record and future sizes and alignments matched on x86-64
and i686. The exact candidate passed 45 native, tonic, gRPC-Web, dispatch,
deadline, terminal and compression tests.
[Focused tests](https://github.com/mingley/pure-protobuf/actions/runs/44350223936/job/134963507453) ·
[i686 layout](https://github.com/mingley/pure-protobuf/actions/runs/44350911965/job/134965953984) ·
[x86-64 layout and control replay](https://github.com/mingley/pure-protobuf/actions/runs/44352431584)

The archived workflows show the source-pinned Cargo example builds and test
commands. The raw archives retain the compiler output and complete results.
[Source pins](source-pins.json) · [Patch](candidate.patch) ·
[Ordinary cases](ordinary-workflow.yml) · [Hook and observer replay](control-workflow.yml)
