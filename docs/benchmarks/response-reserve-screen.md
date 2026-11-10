# Small-response reserve: loopback screen

This is a screen of an isolated prototype, not a current-main benchmark or a tonic comparison. The [baseline](https://github.com/mingley/pure-protobuf/commit/713038c7f2d60da41824531a7b81920183c7510b) and [prototype](https://github.com/mingley/pure-protobuf/commit/68618cdcf744a9c9874c58f0ecc1a87bb05a8d5c) were measured with a fixed upstream `h2` peer. The [run](https://github.com/mingley/pure-protobuf/actions/runs/42740489589) and its [scratch-only fixture](https://github.com/mingley/pure-protobuf/tree/c09b2d0c2278be207ab6cfa78242b86f40bca139/.github/experiments/response-reserve) describe the exact commands. The later prototype 208d4d2 was not measured by this run.

A later, different [208d prototype](https://github.com/mingley/pure-protobuf/commit/208d4d2d40a1891e14c6807d61c4720b319dc5ed)
failed a [real-frame Miri test](https://github.com/mingley/pure-protobuf/actions/runs/42744927524)
on a stream-slot pointer reborrow. A later [a675 repair](https://github.com/mingley/pure-protobuf/commit/a675c45d65a50c7e22fc332b71a16405ed6bf33b)
passed that test and two narrow controls in [Miri](https://github.com/mingley/pure-protobuf/actions/runs/42745861047);
padded/error cases and the full gate remain open. Neither result validates this
older 686 prototype. The [revised screen](response-reserve-screen-revised.md)
measured 208d separately, not the repair.

## Method

On a shared x86-64 AMD EPYC 7763 host with Rust 1.99.1, each process used four Tokio worker threads. The client and peer were separate processes sharing one plaintext loopback connection. The client was pinned to logical CPUs 0 and 2, and the upstream-h2 peer to 1 and 3; these are sibling hardware threads on two physical cores. Results include host and scheduler noise.

For each setting and concurrency, ten alternating-order baseline/prototype pairs each made 8,000 validated calls after 64 warmup calls. Extra baseline/baseline and prototype/prototype controls were taken per block. `explicit` sets the client budget to 512 KiB on both versions; `default` leaves it unset. The ratio below is prototype client process CPU time divided by baseline client process CPU time; lower is better.

| Client setting | Concurrency | Median ratio | Min–max | Prototype slower |
| --- | ---: | ---: | ---: | ---: |
| default | 1 | 1.0022 | 0.9829–1.0164 | 6/10 |
| default | 16 | 1.0028 | 0.9766–1.0446 | 6/10 |
| default | 64 | 0.9837 | 0.9677–1.0397 | 4/10 |
| explicit | 1 | 1.0082 | 0.9745–1.0360 | 7/10 |
| explicit | 16 | 1.0017 | 0.9873–1.0218 | 6/10 |
| explicit | 64 | 0.9873 | 0.9463–1.0218 | 4/10 |

Every min–max interval crosses 1; the same-binary controls also vary substantially. These samples establish neither a CPU improvement nor the absence of a small regression.

A separate, instrumented client binary estimated process-wide allocation changes from 512 versus 1,024 calls, at concurrency 16 after 64 warmup calls. Across five pairs per setting and version, the incremental counts ranged roughly 20.3–20.7 allocations and 8.1–8.4 KiB requested per call. These counts include all process allocations and are sensitive to scheduling. They are not peak memory measurements.

On the same build, x86-64 `Stream`, stream-inner, counts, event, channel config, request-ready future and response future sizes were unchanged; the embedded h2 client builder grew from 152 to 160 bytes. Exact sizes and all 60 paired CPU blocks, same-binary controls, peer counts and 20 allocation samples are in the [saved data](response-reserve-screen.json). The data was extracted from the run artifact by [this run](https://github.com/mingley/pure-protobuf/actions/runs/42742394408); the original artifact SHA-256 is `62694eff1411f39fe557c587f344e56b548cf4f69b3ef2fb7df68c3c2e7311c0`.

The prototype also completed three 2,048-call samples at each of 128, 256, 512 and 1,024 concurrency against the fixed peer with response/count validation. Those are functional screens, not baseline comparisons. This run does not measure the native server, production load, other machines, or tonic.
