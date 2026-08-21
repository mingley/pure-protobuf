# Benchmarks

## Method

Every row uses the same `.proto`. Most cases are
`TestAllTypesProto3` (TAT), Google's kitchen-sink conformance message.
pbrs types are plugin-generated.
Competitors are prost 0.13 (`prost-build` of that proto), crates.io
`protobuf` 4.35.1-release (`protoc --rust_out kernel=upb`), buffa 0.9.1
owned, and buffa `decode_view` where it exists.

Decode uses pbrs wire bytes. `./bench` (from `bench/`) runs 40000
iterations and reports the median of 15 after warmup.

Builds are release, thin LTO, one codegen unit.
`size_of::<TestAllTypesProto3>()` is 648. Default is ~19 ns.

Each cell is encode ns / decode ns. Payload is encoded size in bytes.
Buffa view has no encode, so that side is `n/a`.

`./bench` exits non-zero if a gated case loses encode or owned decode to
prost, v4, or buffa owned. Twelve cases are gated: the original nine plus
`packed_fixed64_256`, `packed_float_256`, and `repeated_nested_8`. Buffa
view is gated except `tat_populated` (~3% band) and the packed-fixed
rows (view does not build an owned `Vec`; we still win those rows on
this host, they are not a process gate).

JSON, text, proto2 required, maps larger than 64, and WKT are not gated.
1 MiB and 5 MiB rows are reported below and are not gated. Iters drop
with payload (120x9 at 1 MiB, 40x7 at 5 MiB) so the timer stays
memcpy-bound rather than a 40k-iter wall clock.

Numbers below are one Apple Silicon host. Two consecutive
`./target/release/bench` runs; the second capture is below.

## Gated

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **27 / 18** | 81 / 128 | 148 / 82 | 72 / 164 | n/a / 110 |
| person | 62 | **38 / 71** | 40 / 201 | 74 / 164 | 42 / 156 | n/a / 82 |
| TAT populated | 87 | **98 / 315** | 181 / 447 | 250 / 419 | 132 / 408 | n/a / 305 |
| packed varint 256 | 388 | **89 / 266** | 547 / 840 | 474 / 900 | 582 / 379 | n/a / 384 |
| map 64 | 500 | **259 / 1060** | 664 / 2376 | 1001 / 3203 | 420 / 1756 | n/a / 1080 |
| nested depth 8 | 26 | **251 / 147** | 2343 / 1113 | 1153 / 323 | 664 / 1370 | n/a / 1450 |
| strings | 163 | **70 / 165** | 123 / 333 | 198 / 172 | 111 / 321 | n/a / 197 |
| unpacked varint 256 | 896 | **436 / 1112** | 540 / 1638 | 786 / 2680 | 830 / 1676 | n/a / 2473 |
| packed fixed32 256 | 1028 | **61 / 83** | 221 / 770 | 231 / 138 | 187 / 205 | n/a / 148 |
| packed fixed64 256 | 2052 | **70 / 94** | 219 / 754 | 257 / 144 | 184 / 207 | n/a / 159 |
| packed float 256 | 1028 | **62 / 84** | 198 / 739 | 234 / 136 | 196 / 201 | n/a / 152 |
| repeated nested 8 | 38 | **65 / 154** | 123 / 224 | 215 / 295 | 122 / 304 | n/a / 252 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

## Extended

Reported, not gated.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes | 315 | **81 / 180** | 136 / 522 | 240 / 204 | 122 / 391 | n/a / 225 |
| scalars (bool/enum/float/packed bool) | 77 | **74 / 168** | 161 / 320 | 182 / 224 | 101 / 226 | n/a / 222 |
| unpacked fixed32 256 | 1536 | **237 / 772** | 293 / 1479 | 651 / 1731 | 533 / 1557 | n/a / 2110 |
| oneof string | 23 | **48 / 89** | 97 / 148 | 155 / 102 | 86 / 178 | n/a / 124 |

## Losses

`tat_populated` versus buffa view sits in a ~3% band on this host (315
vs 305 ns). Person view has more headroom. Packed-fixed view rows win
on this host but are not process-gated: view does not materialize a
`Vec`.

## Large payloads (reported, not gated)

Same TAT schema. Cells are **microseconds** (encode / decode), not
nanoseconds. One Apple Silicon host; second of two runs.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes 1 MiB | 1,000,004 | 12.3 / 12.2 | 13.1 / 25.5 | 12.4 / 12.7 | 12.7 / 13.0 | n/a / **0.12** |
| bytes 5 MiB | 5,000,005 | 63.7 / 66.3 | 69.5 / 128.0 | 65.3 / 67.5 | 69.0 / 66.6 | n/a / **0.12** |
| packed fixed32 1 MiB | 1,000,005 | 12.4 / 12.8 | 161 / 472 | **12.0 / 12.9** | 128 / 13.3 | n/a / 12.7 |
| packed fixed32 5 MiB | 5,000,006 | 69.5 / 71.7 | 804 / 2692 | **65.1 / 67.3** | 640 / 66.1 | n/a / 70.9 |

At 1-5 MiB the v4 Arena/FFI tax is gone. Owned encode/decode of a bytes
blob is a memcpy of the payload. pbrs, v4, and buffa owned sit in the
same band. prost decode is about 2x. buffa `decode_view` on bytes does
not copy (~0.12 µs).

packed-fixed is memcpy for pbrs and v4, and a recode for prost / buffa
owned encode. At 5 MiB v4 encode is a bit faster (65 vs 70 µs). Decode
is a few percent either way. packed-fixed view still copies; it is not
the bytes-view shortcut.

## Why v4 encode is large on small sizes

Every v4 `serialize` allocates an Arena, calls FFI `upb_Encode`, and
copies to `Vec`. Codec work on <1 KiB is tens of ns. Setup is hundreds.
See `docs/upb.md`.

## Re-run

```bash
cd bench && cargo build --release && ./target/release/bench
```
