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
view is gated except `tat_populated` and the packed-fixed rows (view does
not build an owned `Vec`; we still win those rows on this host, they are
not a process gate). `tat_populated` view used to sit in a ~3% band; it
wins with headroom on this capture and is still not gated.

JSON, text, proto2 required, maps larger than 64, and WKT are not gated.
1 MiB and 5 MiB rows are reported below and are not gated. Iters drop
with payload (120x9 at 1 MiB, 40x7 at 5 MiB) so the timer stays
memcpy-bound rather than a 40k-iter wall clock.

Numbers below are one Apple M4 Pro. Two consecutive
`./target/release/bench` runs after the MiniTable rust_out merges; the
second capture is below. Both runs exited 0 (all twelve gated cases still
win encode and owned decode).

## Gated

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **26 / 22** | 80 / 127 | 148 / 80 | 71 / 163 | n/a / 113 |
| person | 62 | **36 / 72** | 41 / 203 | 81 / 159 | 41 / 159 | n/a / 87 |
| TAT populated | 87 | **94 / 281** | 154 / 445 | 237 / 405 | 131 / 394 | n/a / 309 |
| packed varint 256 | 388 | **79 / 263** | 534 / 802 | 465 / 907 | 575 / 362 | n/a / 404 |
| map 64 | 500 | **262 / 919** | 665 / 2370 | 972 / 3180 | 409 / 1787 | n/a / 1097 |
| nested depth 8 | 26 | **257 / 128** | 2311 / 1075 | 1151 / 321 | 648 / 1430 | n/a / 1457 |
| strings | 163 | **64 / 165** | 122 / 327 | 194 / 174 | 110 / 323 | n/a / 198 |
| unpacked varint 256 | 896 | **458 / 1166** | 541 / 1649 | 765 / 2535 | 824 / 1637 | n/a / 2451 |
| packed fixed32 256 | 1028 | **52 / 86** | 215 / 723 | 225 / 134 | 183 / 202 | n/a / 151 |
| packed fixed64 256 | 2052 | **61 / 97** | 216 / 754 | 260 / 145 | 179 / 204 | n/a / 149 |
| packed float 256 | 1028 | **51 / 88** | 186 / 730 | 223 / 144 | 195 / 200 | n/a / 146 |
| repeated nested 8 | 38 | **63 / 143** | 121 / 219 | 212 / 287 | 122 / 288 | n/a / 251 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

## Extended

Reported, not gated.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes | 315 | **72 / 167** | 133 / 513 | 227 / 201 | 120 / 380 | n/a / 227 |
| scalars (bool/enum/float/packed bool) | 77 | **65 / 167** | 159 / 321 | 187 / 230 | 100 / 232 | n/a / 230 |
| unpacked fixed32 256 | 1536 | **216 / 769** | 291 / 1465 | 640 / 1682 | 550 / 1544 | n/a / 2094 |
| oneof string | 23 | **39 / 84** | 98 / 149 | 151 / 97 | 87 / 179 | n/a / 121 |

## Losses

No gated encode or owned-decode loss. `tat_populated` versus buffa view
now wins on this host (281 vs 309 ns) and is still not process-gated.
Person view has more headroom. Packed-fixed view rows win on this host
but are not process-gated: view does not materialize a `Vec`.

## Large payloads (reported, not gated)

Same TAT schema. Cells are **microseconds** (encode / decode), not
nanoseconds. One Apple M4 Pro; second of two runs.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes 1 MiB | 1,000,004 | 12.3 / 12.1 | 12.5 / 25.5 | 12.1 / 12.3 | 12.1 / 12.4 | n/a / **0.12** |
| bytes 5 MiB | 5,000,005 | 64.9 / 65.4 | 73.0 / 136.3 | 64.7 / 68.8 | 67.5 / 64.7 | n/a / **0.12** |
| packed fixed32 1 MiB | 1,000,005 | 12.3 / 12.3 | 161 / 461 | **12.2 / 12.5** | 130 / 12.4 | n/a / 12.2 |
| packed fixed32 5 MiB | 5,000,006 | 66.4 / 71.5 | 787 / 2668 | **60.0 / 68.3** | 628 / 68.4 | n/a / 68.7 |

At 1-5 MiB the v4 Arena/FFI tax is gone. Owned encode/decode of a bytes
blob is a memcpy of the payload. pbrs, v4, and buffa owned sit in the
same band. prost decode is about 2x. buffa `decode_view` on bytes does
not copy (~0.12 µs).

packed-fixed is memcpy for pbrs and v4, and a recode for prost / buffa
owned encode. At 5 MiB v4 encode is a bit faster (60 vs 66 µs). Decode
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

## tonic Codec (Apple M4 Pro)

Same-process `ProtobufCodec` vs `ProstCodec`, no transport. Not kernel
`./bench`. Not a Google peer. Not in CI. Two consecutive
`./target/release/tonic-bench` runs; second capture below.

| case | ProtobufCodec enc / dec | ProstCodec enc / dec |
|---|---:|---:|
| hello | 5.4 / **10.7** | **3.7** / 18.9 |
| hello_4kib | 49.7 / 154.4 | **43.2 / 142.2** |

Combined encode+decode: hello **16.1 vs 22.6** ns (win). 4 KiB 204.1 vs
185.4 ns (loss). Encode is still behind on both rows. Remaining 4 KiB
gap is decode (`Parse` / `merge_from_bytes`). Encode writes into
`EncodeBuf`; decode uses a contiguous frame. No per-message `Vec`.

Linux x86_64 line of record after dropping the per-message `Vec` (#31):
hello 6.8 / 45.4 vs 3.8 / 22.1 (combined 52.2 vs 25.8). 4 KiB 36.8 /
153.8 vs 32.7 / 133.4 (combined 190.6 vs 166.1). That host is not this
one.

#29 first-run (historical, with the `Vec`): hello 93.6 vs 22.4 ns
combined, 4 KiB ~400 vs 202.

```bash
cd tonic-bench && cargo build --release && ./target/release/tonic-bench
```
