# Split streaming observer experiment, November 10

[Benchmarks](../benchmarks.md) · [Raw measurements](streaming-observer-split-20261110.json) · [Workflow run](https://github.com/mingley/pure-protobuf/actions/runs/42759452786)

The unmerged [observer prototype `7056ff07a93903c7926ceb4dab1e8acd9c58f897`](https://github.com/mingley/pure-protobuf/commit/7056ff07a93903c7926ceb4dab1e8acd9c58f897) is based on runtime `41d94325b8aa48b10b31fb6709c1a41dbc44a72a`. Its unmerged child [`b9533c54a2fda532420ba9381bdfbdcc39a87243`](https://github.com/mingley/pure-protobuf/commit/b9533c54a2fda532420ba9381bdfbdcc39a87243) gives calls without a binary logger a separate future with a zero-sized observer guard. Configured loggers, including filtered methods, retain the existing guard. The split was **rejected for integration** after the screen below. Neither prototype is a release.

The [source patches and benchmark fixture](../evidence/streaming-observer-split-20261110/README.md) preserve the tested source and harness. The original [artifact 13737786866](https://github.com/mingley/pure-protobuf/actions/runs/42759452786/artifacts/13737786866) has SHA-256 `3dbc3f45a26f2496480fefbd9eb9206c4b113fe4e634feac12f1e3a423741d27`; its complete CPU and allocation rows are also retained in the adjacent JSON.

## CPU and allocations

Each CPU ratio is split divided by observer baseline; lower is faster. Each row has ten balanced blocks and same-binary controls. The 4-message case uses 500 calls per sample; the 128-message case uses 20.

| Logger and bidi workload | Median ratio | Block range | Baseline controls | Split controls |
| --- | ---: | ---: | ---: | ---: |
| None, 4 messages | 1.0137 | 0.9911–1.0203 | 0.9643–1.0153 | 0.9793–1.0082 |
| None, 128 messages | 1.0273 | 1.0120–1.0751 | 0.9713–1.0154 | 0.9708–1.0056 |
| Filtered, 4 messages | 1.0156 | 1.0007–1.0308 | 0.9873–1.0075 | 0.9843–1.0097 |
| Enabled, 128 messages | 1.0208 | 1.0116–1.0381 | 0.9699–1.0330 | 0.9568–1.0255 |

All ten no-logger 128-message bidi blocks favored the baseline. Other cells and all control samples are in the JSON. The client and server shared one process and one pinned CPU over loopback TCP on a shared AMD EPYC 7763 host, with 32-byte response payloads, no TLS and a no-op enabled-log sink. Client-streaming requests also carry 32-byte payloads; bidi requests specify the response size. These measurements do not compare tonic or establish a general ranking.

In five process-wide allocation pairs per 4-message shape, the split requested **16 fewer bytes per call** without a logger and did not change allocation counts. Filtered calls had unchanged counts and requested bytes. Requested bytes are not live or peak memory. Separately instrumented builds measured no-logger call futures at 2056→2040 bytes for client streaming, 3432→3416 for server streaming and 2096→2080 for bidi; filtered and enabled sizes were unchanged. See the [layout run](https://github.com/mingley/pure-protobuf/actions/runs/42759746018).

## Cost and correctness scope

The measured fixture executable grew from 10,183,928 to 10,249,144 bytes. The GNU `size` text column grew from 6,955,533 to 7,014,601 bytes. Four alternating clean package rebuilds took 87.38 and 86.52 seconds for the baseline and 87.84 and 87.46 seconds for the split; this small shared-host sample is not a general compile-time comparison.

The [correctness screen](https://github.com/mingley/pure-protobuf/actions/runs/42759217559) passed 23 attribution and closure controls and the existing binary-log, hedge, streaming-terminal, cancellation and Tower suites. Four StreamSender explicit-failure expectations and the proposed Tower input-error attribution remained unresolved in both prototypes. The [first-header sink-panic check](https://github.com/mingley/pure-protobuf/actions/runs/42760826174) covered one panic point across all three shapes. These checks do not establish full feature parity or production readiness.
