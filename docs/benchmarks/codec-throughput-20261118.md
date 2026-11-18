# Codec throughput on Rust 1.100.0

The November 18 survey measured 28 message shapes on one logical CPU of an
Ubuntu 24.04 GitHub Actions runner. It used Rust 1.100.0, Prost 0.14.4 and
`protobuf` 4.35.1-release, which uses the upb C runtime. The release build
used thin LTO and the standard allocator. All value checks and the survey's
smoke gates passed. [Source](https://github.com/mingley/pure-protobuf/commit/8df5555ff406caf4cf41d628a29ee826d0af9c18)
· [Raw samples and provenance](https://github.com/mingley/pure-protobuf/actions/runs/44410693837/artifacts/14476544984)

## Results

Counts show how many pbrs median timings were faster, tied or slower.

| Operation | Against upb | Against Prost |
| --- | ---: | ---: |
| Owned decode | 27 / 0 / 1 | 18 / 0 / 10 |
| Decode and read every field | 28 / 0 / 0 | 16 / 1 / 11 |
| First encode after parse | 26 / 0 / 2 | 14 / 0 / 14 |
| Repeated encode | 28 / 0 / 0 | 17 / 0 / 11 |
| Fresh construction | 27 / 0 / 1 | 4 / 0 / 24 |

| Case | Operation | pbrs | upb | Prost |
| --- | --- | ---: | ---: | ---: |
| `blob_32` | Owned decode | 80.261 ns | 69.672 ns | 40.988 ns |
| `nest_d4` | Decode and read every field | 307.254 ns | — | 95.207 ns |
| `nest_d8` | Decode and read every field | 660.455 ns | — | 211.732 ns |
| `packed_16` | First encode after parse | 105.191 ns | 89.346 ns | — |
| `packed_16` | Fresh construction | 55.207 ns | — | 10.212 ns |

Decode uses the same wire input. The read-all check reads populated fields
and folds byte contents into its checksum. pbrs and Prost reuse a
`BytesMut` during encode. The upb binding returns owned bytes, so those
encode ratios include different output lifecycles.

Samples ran in fixed codec order on a shared host. Use these results to
choose which paths to profile. Performance qualification uses repeated pairs
on dedicated hosts and compares equivalent work. [Benchmark contract](../benchmark-contract.md)


## Borrowed input with a final bytes field

The decoder now copies only the payload for a final bytes field up to
4 KiB when the message has no wire backing. A private payload keeps the
same sharing behavior. Larger fields and fields with existing backing
follow their existing paths.

Two pairs on one hosted logical CPU gave the following medians on Rust
1.100.0. Each cell shows the two pair values.

| Owned decode | Base pbrs | Candidate pbrs | upb in candidate runs | Prost in candidate runs |
| --- | ---: | ---: | ---: | ---: |
| 32-byte payload | 59.70 / 60.93 ns | 43.36 / 42.76 ns | 47.57 / 47.15 ns | 31.01 / 30.90 ns |
| 4 KiB payload | 96.97 / 98.34 ns | 81.07 / 80.28 ns | 90.51 / 91.46 ns | 104.83 / 105.11 ns |

The 64 KiB owned-decode control moved by 0.1–1.2%, and the mixed read-all
control moved by less than 1% in either direction. One scalar-only
read-all row was 3.7–3.9% slower. Its schema never calls the bytes parser.
The raw capture includes each control and the exact source hashes.
[Matched runs](https://github.com/mingley/pure-protobuf/actions/runs/44414741695/artifacts/14478383804)

## Run the survey

Use Rustup and a C compiler. This run used protoc 3.21.12 with checked upb
bindings.

~~~sh
bench_output=$(mktemp -d)
PURE_PROTOBUF_CHECKED_V4=1 cargo run --locked --release \
  --manifest-path tonic-bench/Cargo.toml > "$bench_output/codec-survey.txt"
~~~

The report includes its raw timing samples and the `allocation_counts` flag.
Use `--features allocation-counts` for a separate retained Rust heap run.
For direct C upb, use matching corpus bytes and account for the arena
lifecycle. [Codec survey](../../tonic-bench/README.md) · [C upb harness](../../bench/xlang/upb/README.md)
