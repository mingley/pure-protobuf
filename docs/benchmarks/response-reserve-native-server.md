# Small-response reserve: native server screen

This [run](https://github.com/mingley/pure-protobuf/actions/runs/42746384142) compares native servers at baseline `713038c7` and reserve prototype `a675c45` with the same baseline native client. It does not measure the later debt-accounting proposal, a published crate release, or tonic.

Each sample used one plaintext loopback connection and 6,000 validated calls after 128 warmups. The server used either its default DATA budget or an explicitly configured 512 KiB budget. Unary requests had 8-byte payloads; client-streaming calls sent four 8-byte messages and checked the aggregate response. A separately instrumented diagnostic observed the zero-slot small-DATA path during client streaming in both server settings; it did not observe that path for unary calls.

On a shared x86-64 AMD EPYC 7763 host with Rust 1.99.1, client and server each used four Tokio workers. Client CPUs 0 and 2 shared physical cores with server CPUs 1 and 3. Each row summarizes five alternating-order baseline/prototype pairs, with baseline/baseline and prototype/prototype controls per pair. The ratio is prototype server process CPU divided by baseline server CPU.

| Server budget | Shape | Concurrency | Median ratio | Min–max | Prototype slower |
| --- | --- | ---: | ---: | ---: | ---: |
| default | unary | 16 | 1.0108 | 0.9162–1.0806 | 3/5 |
| default | unary | 64 | 1.0415 | 0.9802–1.0819 | 3/5 |
| default | client stream | 16 | 0.9771 | 0.9366–1.1642 | 2/5 |
| default | client stream | 64 | 1.0135 | 1.0103–1.0783 | 5/5 |
| explicit | unary | 16 | 0.9907 | 0.9818–1.1165 | 2/5 |
| explicit | unary | 64 | 1.0043 | 0.9591–1.0418 | 3/5 |
| explicit | client stream | 16 | 1.0220 | 0.9280–1.0919 | 4/5 |
| explicit | client stream | 64 | 1.0145 | 0.9500–1.1135 | 3/5 |

For default client streaming at concurrency 64, all five paired ratios exceeded 1; same-binary controls ranged 0.9623–1.0592 for baseline and 0.9702–1.1597 for prototype. Five pairs and the broad controls do not resolve a small regression or establish an improvement.

Separate allocation-instrumented server binaries used 1,000- and 2,000-call samples after 128 warmups at concurrency 16. Across five repetitions per shape, setting and version, marginal process-wide estimates ranged about 14.4–14.9 allocation calls per unary RPC and 19.7–20.0 per client-streaming RPC. Counts include reallocations and unrelated process work; they are not peak memory measurements.

All 40 CPU blocks and 40 allocation samples are in [the saved data](response-reserve-native-server.json). The [scratch fixture](https://github.com/mingley/pure-protobuf/tree/5f2fd2437b12fc421bf841562df9c343dd02c996/.github/experiments/response-reserve) records the workload and instrumentation. [Extraction](https://github.com/mingley/pure-protobuf/actions/runs/42748303231) verified the original artifact SHA-256 `53989caf18cbda62bebc3fd2d800229786fd6a877ff3252f84b438fa09531617`.
