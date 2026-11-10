# Small-response reserve: longer client CPU follow-up

This [follow-up run](https://github.com/mingley/pure-protobuf/actions/runs/42745297055) repeats the default-client comparison from the [revised screen](response-reserve-screen-revised.md) with larger samples. It compares baseline `713038c7` and prototype `208d4d2` against a fixed upstream `h2` peer; it does not measure the native server or tonic.

A [Miri real-frame test](https://github.com/mingley/pure-protobuf/actions/runs/42744927524) found an invalid stream-slot pointer reborrow in prototype `208d4d2`. These timings describe that historical source and do not establish correctness, release readiness or the performance of a repair.

On a shared x86-64 AMD EPYC 7763 host with Rust 1.99.1, each process used four Tokio workers. The client ran on logical CPUs 0 and 2, and the peer on 1 and 3; they were sibling hardware threads on two physical cores. For each concurrency there were 20 alternating baseline/prototype pairs of 16,000 validated calls after 64 warmups, with baseline/baseline and prototype/prototype controls per block. Ratios divide prototype client process CPU by baseline client process CPU.

| Concurrency | Median ratio | Min–max | Prototype slower | Same-binary control ranges |
| ---: | ---: | ---: | ---: | --- |
| 1 | 1.0071 | 0.9732–1.0402 | 11/20 | baseline 0.9712–1.0295; prototype 0.9757–1.0341 |
| 64 | 1.0042 | 0.9462–1.0382 | 11/20 | baseline 0.9722–1.0475; prototype 0.9709–1.0483 |

These samples do not resolve whether the prototype has a small CPU regression. Host and scheduler noise remain visible in both comparison and control ranges.

All 40 paired blocks are in [the saved data](response-reserve-screen-long.json). The [scratch fixture](https://github.com/mingley/pure-protobuf/tree/b5b73880137a5da6d0b50fefe403465654816c63/.github/experiments/response-reserve) pins the commands; [extraction](https://github.com/mingley/pure-protobuf/actions/runs/42747375975) verified the original artifact SHA-256 `4874b5e8867e15ec4971c8b11cb707f33ca55de316689b695cf1505c334827b6`.
