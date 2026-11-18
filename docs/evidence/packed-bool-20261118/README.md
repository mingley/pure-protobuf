# Packed boolean encoding

The generated Edition 2023 message with 4,096 overlong booleans made **18 → 9 reallocation requests** during fresh parse and serialization. Allocation requests stayed at five. Median paired thread CPU was 0.281× baseline across the six measured blocks. [Original run](https://github.com/mingley/pure-protobuf/actions/runs/44347181608) · [Raw report](original-results.json.gz)

The candidate changes `Bools::encode` in `src/packed.rs`. A sized iterator converts each decoded bool to its canonical byte and appends the run to the existing output Vec. The path still validates the wire on append and materializes typed values lazily. Parsed zero values re-encode as `0` and all nonzero values re-encode as `1`, including values from valid overlong varints. The focused test checks normalization, an existing output prefix, and an empty append. [Candidate commit](https://github.com/mingley/pure-protobuf/commit/04b4aa04b4e5630e0033428520db9cdd2b49edbc) · [Source patch](source/candidate.patch.gz)

| Cold workload | Candidate/baseline CPU median (range) | Reallocation requests A → B | Requested bytes A → B |
| --- | ---: | ---: | ---: |
| Generated Edition 2023, 4,096 overlong | 0.2809 (0.2736–0.2818) | 18 → 9 | 25,016 → 20,928 |
| 4,096 canonical wire values | 0.1355 (0.1317–0.1389) | 17 → 8 | 16,464 → 12,376 |
| 4,096 overlong wire values | 0.2241 (0.2225–0.2302) | 18 → 9 | 16,472 → 12,384 |
| Eight canonical wire values | 0.9483 (0.9233–0.9646) | 0 → 0 | 120 → 120 |
| Eight overlong wire values | 0.9281 (0.8456–0.9433) | 0 → 0 | 120 → 120 |

Cold wire samples include field construction, validation, first encode, and destruction. The generated cases use the checked-in `TestAllTypesEdition2023` parser and `write_to` with only `repeated_bool` populated and a reused output buffer. Each raw report keeps every process, including the calibration and three differential allocation pairs. Requested bytes sum allocation and reallocation request sizes. [Probe](source/probe.rs) · [Original report](original-results.json.gz)

## Getter controls

The initial 4,096-value canonical cold getter was 1.0512× baseline and 1.0412× the independent baseline build. All six candidate/independent-baseline blocks were slower, spanning 1.0239–1.0987×. The initial overlong cold getter moved alongside the independent baseline, with a candidate/independent-baseline median of 0.9946×. Several warm encode/copy and unchanged fixed32 and uint32 controls moved in the same direction in the two linked variants. [Original report](original-results.json.gz)

A second shared host replayed the exact archived binaries with the same measurement method on eight cases. The canonical cold getter ratio against the independent baseline was 1.0000× (0.9807–1.0148), with three of six blocks slower. The overlong cold getter was 1.0004× and both large warm getters were within 0.04% of the independent baseline at the median. The large canonical first encode retained its gain at 0.2325× baseline. [Replay run](https://github.com/mingley/pure-protobuf/actions/runs/44349349623) · [Raw replay](replay-results.json.gz)

The bool decoder and observed-value accessor have the same symbol sizes across the three builds. Decoder addresses and the normalized disassembly hashes vary across all three, including the two baseline builds. The observed-value accessor's normalized instructions match. The cause of the initial getter shift remains unresolved. [Layout audit](layout.json)

## Method and validation

Both screens used shared x86_64 Linux runners and release binaries built with Rust 1.85.0, thin LTO and one codegen unit. A is baseline, B is the candidate and C independently rebuilds the baseline. Each case ran six rotated, reversed blocks on one allowed logical CPU. Each paired ratio divides the sum of two thread-CPU samples by the corresponding sum in the same block. CPU binaries omit allocation instrumentation and clock-free allocation binaries measure three N-to-2N request differences. [Original driver](source/compare.py) · [Replay driver](source/compare-selected.py)

The baseline calibration targeted 180 ms per timing process. The fastest candidate cells used shorter samples. Minimum process CPU was 5.7 ms for cold owned 4,096-value encoding, 23.1 ms for canonical wire, 38.2 ms for overlong wire and 47.4 ms for generated overlong wire. Treat the magnitude of these wins as specific to these component measurements. [Original report](original-results.json.gz)

The exact candidate passed 18 packed tests on stable and Rust 1.85 in both default and runtime-only profiles, formatting, strict library Clippy, and the alloc-only Cortex compile. The focused bool test also passed strict-provenance Miri on x86_64 and big-endian s390x. The two temporary workflow refs were removed. [Gate job](https://github.com/mingley/pure-protobuf/actions/runs/44347181608/job/134952621815) · [Source and artifact pins](source-pins.json) · [Reproduction](REPRODUCE.md)
