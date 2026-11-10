# Small-response reserve: revised loopback screen

This screen compares the [baseline](https://github.com/mingley/pure-protobuf/commit/713038c7f2d60da41824531a7b81920183c7510b) with the [revised reserve prototype](https://github.com/mingley/pure-protobuf/commit/208d4d2d40a1891e14c6807d61c4720b319dc5ed). The [run](https://github.com/mingley/pure-protobuf/actions/runs/42743496342) used a fixed upstream `h2` peer. It does not measure current main, the native server, tonic, or either discarded-frame accounting proposal. A [previous screen](response-reserve-screen.md) measured a different prototype; keep the results separate.

A later [Miri real-frame test](https://github.com/mingley/pure-protobuf/actions/runs/42744927524) found an invalid stream-slot pointer reborrow in this prototype. A later [a675 repair](https://github.com/mingley/pure-protobuf/commit/a675c45d65a50c7e22fc332b71a16405ed6bf33b) passed that test and two narrow controls in [Miri](https://github.com/mingley/pure-protobuf/actions/runs/42745861047). Additional [padded/error Miri checks](https://github.com/mingley/pure-protobuf/actions/runs/42746638162) and [lifecycle and compatibility checks](https://github.com/mingley/pure-protobuf/actions/runs/42746357968) passed on a675. These timings describe the historical 208d source, not the repair, and do not establish correctness or release readiness. The repair has a separate [native-server screen](response-reserve-native-server.md).

## Method

On a shared x86-64 AMD EPYC 7763 host with Rust 1.99.1, each process used four Tokio worker threads. The client and peer were separate processes sharing one plaintext loopback connection. The client was pinned to logical CPUs 0 and 2, and the peer to 1 and 3; these are sibling hardware threads on two physical cores. Results include host and scheduler noise. The [scratch fixture](https://github.com/mingley/pure-protobuf/tree/f6ee53d6ff15e850de56f0e07162acbddf8eb978/.github/experiments/response-reserve) and run record the commands and source pins.

For each setting and concurrency, ten alternating-order baseline/prototype pairs each made 8,000 validated calls after 64 warmup calls. Each block also ran baseline/baseline and prototype/prototype controls. `explicit` sets the client budget to 512 KiB on both versions; `default` leaves it unset. Ratios are prototype client process CPU time divided by baseline client process CPU time; lower is better.

| Client setting | Concurrency | Median ratio | Min–max | Prototype slower |
| --- | ---: | ---: | ---: | ---: |
| default | 1 | 1.0147 | 0.9345–1.0473 | 7/10 |
| default | 16 | 1.0153 | 0.9474–1.0723 | 6/10 |
| default | 64 | 1.0157 | 0.9456–1.0704 | 5/10 |
| explicit | 1 | 0.9853 | 0.9522–1.0440 | 4/10 |
| explicit | 16 | 1.0053 | 0.9440–1.0438 | 6/10 |
| explicit | 64 | 1.0062 | 0.9722–1.0462 | 6/10 |

The three default medians are about 1.5% higher for the prototype. Every paired range crosses 1 and same-binary control ratios span roughly 0.94–1.15 across the six cases. This screen cannot resolve a small regression or establish a speedup.

A separate instrumented client binary estimated process-wide allocation changes from 512 versus 1,024 calls, at concurrency 16 after 64 warmup calls. Across five pairs per setting and version, incremental estimates ranged roughly 19.8–20.9 allocation calls and 8.0–8.6 KiB requested per RPC. Counts include all process allocations and are sensitive to scheduling. They are not peak memory measurements.

In this x86-64 build, `Stream` (328 bytes), stream-inner (712), counts (176), data event (40), channel config (368), request-ready future (248), and response future (24) were unchanged. The embedded h2 client builder grew from 152 to 160 bytes.

All 60 CPU blocks, same-binary controls and 20 allocation samples are in [the saved data](response-reserve-screen-revised.json). They were extracted by [this run](https://github.com/mingley/pure-protobuf/actions/runs/42745167112); the original artifact SHA-256 is `ed40998bc7c32fc368495389a6b81578ef9a74ff3a457443f02da83cc41f9277`.

The prototype also completed three 2,048-call samples at each of 128, 256, 512 and 1,024 concurrency against the fixed peer with response/count validation. Those are functional screens, not baseline comparisons.
