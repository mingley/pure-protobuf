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

## tonic Codec survey (Apple M4 Pro)

Same-process encode into `BytesMut` (`Serialize::encode` / prost
`Message::encode`). v4 is `Serialize::serialize` (new Arena + FFI +
copy; no EncodeBuf). Not kernel `./bench`. Not in CI. Two consecutive
`./target/release/tonic-bench` runs; second capture below.

`hello` / `hello_4kib` are the old 1-string rows. Everything else is
`proto/codec_cases.proto`: one message per common unary shape, so
gencode is specialized (hello-sized), not TestAllTypes.

Cells are encode ns / decode ns. Combined win/loss is pbrs vs that
column.

### Published 1-string

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| hello | 5 | 5.6 / 10.3 | **3.8** / 22.1 | 33.1 / 43.3 | win | win |
| hello_4kib | 4099 | 44.3 / 159.0 | **46.6 / 144.3** | 98.8 / 245.0 | loss | win |

### Common shapes

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| empty | 0 | 1.7 / 1.5 | **0.2 / 0.2** | 26.3 / 33.5 | loss | win |
| id | 2 | **3.1 / 2.6** | 4.7 / 3.1 | 31.4 / 38.7 | win | win |
| scalars | 23 | **13.7 / 13.8** | 20.3 / 16.6 | 47.9 / 61.2 | win | win |
| name_short | 5 | 5.0 / **10.7** | **3.8** / 22.4 | 32.6 / 43.9 | win | win |
| name_80 | 82 | 5.9 / 24.0 | **4.0** / 26.6 | 32.9 / 41.8 | win | win |
| name_4kib | 4099 | 49.6 / 147.8 | **47.6 / 142.8** | 99.6 / 248.1 | loss | win |
| blob_32 | 34 | **5.2 / 20.5** | 5.4 / 38.5 | 32.8 / 40.1 | win | win |
| blob_4kib | 4099 | **45.4 / 63.5** | 46.4 / 124.7 | 97.9 / 103.5 | win | win |
| blob_64kib | 65540 | **563 / 735** | 5408 / 1661 | 705 / 746 | win | win |
| envelope | 30 | **17.4 / 55.2** | 24.4 / 56.1 | 49.2 / 71.0 | win | win |
| nest_d4 | 14 | **20.3 / 45.4** | 39.6 / 55.6 | 52.8 / 77.0 | win | win |
| packed_16 | 18 | **5.7 / 34.9** | 31.3 / 84.4 | 38.8 / 78.5 | win | win |
| packed_256 | 387 | **9.8 / 140** | 905 / 723 | 353 / 798 | win | win |
| tags_4 | 27 | 18.7 / **63.7** | **17.5** / 112.1 | 42.7 / 73.4 | win | win |
| tags_32 | 160 | **108 / 410** | 157 / 828 | 112 / **382** | win | loss |
| map_8 | 172 | **85 / 258** | 124 / 709 | 132 / 473 | win | win |
| oneof_ok | 6 | **5.2 / 21.0** | 6.4 / 21.8 | 36.1 / 43.7 | win | win |
| rpc_mixed | 176 | **94 / 304** | 160 / 674 | 152 / 349 | win | win |
| rpc_sparse | 2 | 6.7 / 19.5 | **6.9 / 14.1** | 37.7 / 36.7 | loss | win |

v4 loses every row except `tags_32` decode (410 vs 382). Typical unary
`rpc_mixed` is already ~2× prost. Packed encode is the cached-bytes
path (10 vs 905 vs 353). `blob_64kib` prost encode jumped 0.56 µs →
5.4 µs across runs; treat that cell as noisy. Decode 0.74 vs 1.66 µs
is stable.

### What to chase

The 1-string 4 KiB **loss vs prost** does not show up on bytes of the
same size (`blob_4kib` decode **63 vs 125**). Same `Wire::ensure` of
the parent frame. Extra on `name_4kib` is the string arm (`from_utf8`
+ `LazyStr`), not “long field” in general.

Do not spend the next pass on:

- prost empty (ZST, 0.2 ns)
- short-string encode (5.0 vs 3.8)
- flatten `merge_inner` (#39, made hello worse)

Worth measuring next, in order:

1. String-only 4 KiB vs bytes 4 KiB (why +85 ns on the string arm).
2. `rpc_sparse` decode (19.5 vs 14.1): fat generated match on a
   9-field message for a 2-byte payload.
3. `tags_32` vs v4 decode (410 vs 382): many short SSO strings vs
   one arena.

Keep: packed canonical cache, bytes window, map/repeated vs prost.

Linux x86_64 1-string line of record after dropping the per-message
`Vec` (#31): hello 6.8 / 45.4 vs 3.8 / 22.1 (combined 52.2 vs 25.8).
4 KiB 36.8 / 153.8 vs 32.7 / 133.4. That host is not this one.

```bash
cd tonic-bench && cargo build --release && ./target/release/tonic-bench
```
