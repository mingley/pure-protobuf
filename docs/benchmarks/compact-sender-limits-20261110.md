# Compact streaming sender limits, November 10

[Benchmarks](../benchmarks.md) · [Raw measurements](compact-sender-limits-20261110.json) · [Workflow run](https://github.com/mingley/pure-protobuf/actions/runs/42763942554)

The unmerged candidate [`c134820046c26bcbbce9e23c96967bc0f6493631`](https://github.com/mingley/pure-protobuf/commit/c134820046c26bcbbce9e23c96967bc0f6493631) packs the non-send message limits held by `StreamSender`, retaining the outbound `Option<usize>` and reconstructing the full value for Debug. Its baseline is `c8dd4bd7738074a29dee8727814832a4e754198f`. It was **held** because the smaller sender did not establish a runtime or allocation improvement and added representation complexity. A later borrowed/delegating draft `d53ba29ca256bb2030464ea72a1f449f7c9bd92f` was not measured by this screen. Its [separate check](https://github.com/mingley/pure-protobuf/actions/runs/42765126442) passed selected tests but failed strict Clippy; its negative mutants did not run.

## CPU and allocations

Each ratio is candidate CPU divided by baseline CPU; lower is faster. Each row has ten balanced blocks and same-binary controls. The 4-message case uses 500 calls per sample; the 128-message case uses 20. Bounded mode sets the client outbound message-size cap to 1,024 bytes.

| Workload | Median ratio | Block range | Baseline controls | Candidate controls |
| --- | ---: | ---: | ---: | ---: |
| No logger, client stream, 4 messages | 1.0034 | 0.9939–1.0367 | 0.9774–1.0468 | 0.9452–1.0211 |
| No logger, bidi, 128 messages | 1.0018 | 0.9882–1.0105 | 0.9888–1.0795 | 0.9862–1.0201 |
| Bounded, client stream, 4 messages | 0.9968 | 0.9712–1.0123 | 0.9589–1.0191 | 0.9750–1.0170 |
| Bounded, bidi, 128 messages | 0.9963 | 0.9893–1.0226 | 0.9164–1.0477 | 0.9578–1.0861 |

All 18 measured CPU block ranges crossed 1; the controls overlap the observed effects. The remaining cells and all raw samples are in the JSON. Client and server shared one process and one pinned CPU over loopback TCP on a shared AMD EPYC 9V74 host, with no TLS. Client-streaming request payloads and server/bidi response payloads were 32 bytes; bidi requests specified the response size. Enabled-log cells used a no-op sink. This is not a tonic or Prost comparison.

In five process-wide allocation pairs per 4-message shape, allocation counts and requested bytes were unchanged for both no-logger and bounded modes. Enabled bidi allocation samples were variable. Requested bytes are not live or peak memory.

## Layout and correctness

Separately instrumented x86-64 builds measured `StreamSender` at 64→56 bytes. The tested send futures remained 392 bytes for the input/request types and 552 bytes for the response type; client, server and bidi `Call` futures were unchanged. The fixture executable grew from 10,178,128 to 10,178,592 bytes, while the GNU `size` text column fell from 6,951,789 to 6,951,549 bytes. These sizes are specific to the measured fixture and build.

The [correctness run](https://github.com/mingley/pure-protobuf/actions/runs/42763549568) passed Rust 1.85 and Zstd checks for Debug, cloning, zero, unlimited and explicit maximum values, length-walk and error behavior; it also passed the selected streaming, codec, compression, limit and Tower suites, strict Tower+Zstd Clippy and two negative mutants. The [independent verifier](https://github.com/mingley/pure-protobuf/actions/runs/42765055102) recomputed 180 CPU blocks, 360 controls and 45 allocation pairs and checked source, layout and binary witnesses.

The [source patch and fixtures](../evidence/compact-sender-limits-20261110/README.md) are retained. The original [artifact 13739516331](https://github.com/mingley/pure-protobuf/actions/runs/42763942554/artifacts/13739516331) has SHA-256 `e61d89995e2756ebf704748c6d0d871d3d89a121f56917f1ced0e8281066ef13`. These checks do not establish a production release or general performance ranking.
