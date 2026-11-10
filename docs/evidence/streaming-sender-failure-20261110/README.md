# Streaming sender-failure benchmark sources

[Benchmark record](../../benchmarks/streaming-sender-failure-20261110.md)

The CPU and first allocation screens used a common harness based on
`63ff4b4072f35631c9ddb11dadec76c9487ef80b`. Their baseline was
`b0e693148fd8adcd242aa247358bb87dcacb99f5`. The
[patch](b0e-to-63ff.patch) changes the five measured runtime files; applying
it to b0e produces the complete 63ff tree. The [verification run](https://github.com/mingley/pure-protobuf/actions/runs/42784588622)
checked that tree and each fixture below.

- [CPU probe](streaming-poll-probe.rs) measures successful client, server and
  bidi streaming calls.
- [Allocation probe](streaming-poll-alloc.rs) counts requests and requested
  bytes for the same workload.
- [Histogram probe](streaming-alloc-hist.rs) records allocation sizes and
  is also used by the exact-source f73 comparison.
- [Item layout witness](item-layout-witness.rs) models cold boxed errors; it
  is a representation test, not an RPC benchmark.
- [Layout instrumentation](instrument-split-layout.py) records call-future
  sizes in a separate diagnostic build.

The original [CPU workflow](cpu-screen.yml),
[histogram workflow](histogram-screen.yml),
[exact-source allocation workflow](exact-f73-alloc.yml) and
[item layout workflow](item-layout-workflow.yml) record the run setup and
sampling commands. They are historical scratch workflows: adapt their source
pins, paths and cleanup steps when reproducing a screen.

To reconstruct the measured 63ff source, check out b0e, apply the patch at the
repository root, then copy the relevant probe into `pbrs-grpc/src/bin/`.
The exact-source allocation screen built separate 63ff and f73 trees. Host
load, compiler, target and allocator can change the results.
