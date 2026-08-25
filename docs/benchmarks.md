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

`tonic-bench` exits non-zero if `name_4kib` or `blob_4kib` combined
encode+decode loses to prost.

### Published 1-string

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| hello | 5 | 5.7 / **11.2** | **3.9** / 23.6 | 33.8 / 46.0 | win | win |
| hello_4kib | 4099 | **48.1 / 92.7** | 50.1 / 148.9 | 109.2 / 268.5 | win | win |

### Common shapes

| case | payload | pbrs | prost | v4 upb | vs prost | vs v4 |
|---|---:|---:|---:|---:|---|---|
| empty | 0 | 1.8 / 1.3 | **0.3 / 0.3** | 26.7 / 31.5 | loss | win |
| id | 2 | **3.3 / 2.7** | 4.8 / 3.4 | 32.5 / 40.0 | win | win |
| scalars | 23 | **14.4 / 15.2** | 19.5 / 16.4 | 48.5 / 60.2 | win | win |
| name_short | 5 | 5.2 / **11.9** | **3.9** / 22.3 | 34.6 / 48.7 | win | win |
| name_80 | 82 | 6.0 / 28.0 | **4.5 / 26.4** | 33.6 / 45.2 | loss | win |
| name_4kib | 4099 | 57.4 / **94.6** | **48.7** / 149.8 | 102.6 / 256.9 | win | win |
| blob_32 | 34 | **5.2 / 24.0** | 5.7 / 41.1 | 36.7 / 41.2 | win | win |
| blob_4kib | 4099 | **47.2 / 69.3** | 53.7 / 128.3 | 100.8 / 105.8 | win | win |
| blob_64kib | 65540 | **577 / 750** | 574 / 1681 | 715 / 784 | win | win |
| envelope | 30 | **17.8 / 55.7** | 26.0 / 58.4 | 52.5 / 77.3 | win | win |
| nest_d4 | 14 | **20.7 / 48.2** | 40.2 / 57.4 | 54.9 / 78.7 | win | win |
| packed_16 | 18 | **6.8 / 37.1** | 31.8 / 87.2 | 40.8 / 80.8 | win | win |
| packed_256 | 387 | **11.3 / 134** | 923 / 734 | 362 / 809 | win | win |
| tags_4 | 27 | 19.1 / **64.7** | **18.1** / 117.2 | 44.0 / 73.7 | win | win |
| tags_32 | 160 | **113 / 403** | 167 / 891 | 117 / **392** | win | loss |
| map_8 | 172 | **88 / 335** | 122 / 751 | 130 / 489 | win | win |
| oneof_ok | 6 | **5.4 / 20.9** | 5.9 / 22.8 | 34.0 / 45.4 | win | win |
| rpc_mixed | 176 | **90 / 327** | 164 / 710 | 156 / 372 | win | win |
| rpc_sparse | 2 | 7.0 / 20.0 | **7.4 / 14.2** | 38.8 / 37.9 | loss | win |

v4 loses every row except `tags_32` decode (403 vs 392). Typical unary
`rpc_mixed` is already ~2× prost. Packed encode is the cached-bytes
path (11 vs 923 vs 362).

`name_4kib` combined is **152 vs 199** ns vs prost (decode 95 vs 150).
The old gap was `str::from_utf8` plus a parent-frame Arc; long strings
now copy the payload once and UTF-8-check with `simdutf8`. `blob_4kib`
is unchanged in kind (decode 69 vs 128).

### What to chase

Do not spend the next pass on:

- prost empty (ZST, 0.3 ns)
- short-string encode (5.2 vs 3.9)
- flatten `merge_inner` (#39, made hello worse)

`rpc_sparse` decode is process-gated (pbrs decode must beat prost).
Messages with a handful of scalar fields plus heavy string/map/message
fields parse scalars without entering the heavy tag match.

Worth measuring next:

1. `tags_32` vs v4 decode (403 vs 392): many short SSO strings vs
   one arena.
2. `name_80` combined (coin-flip / small loss): 80-byte string is
   just over the SSO cutoff.

Keep: packed canonical cache, bytes window, `simdutf8` string arm,
map/repeated vs prost.

Linux x86_64 1-string line of record after dropping the per-message
`Vec` (#31): hello 6.8 / 45.4 vs 3.8 / 22.1 (combined 52.2 vs 25.8).
4 KiB 36.8 / 153.8 vs 32.7 / 133.4. That host is not this one.

```bash
cd tonic-bench && cargo build --release && ./target/release/tonic-bench
```
