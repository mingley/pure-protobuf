# Benchmarks

Same `.proto` on every row: plugin-generated pbrs types vs `prost-build` of
that proto, crates.io `protobuf` **4.35.1-release** (`protoc --rust_out
kernel=upb`), buffa **0.9.1** owned, and buffa `decode_view` where it
exists.

Decode uses pbrs wire bytes. `./bench` (from `bench/`): 40 000 iters, median
of 15 after warmup. Release, thin LTO, one codegen unit.
`size_of::<TestAllTypesProto3>()` = **624**. Default ~20 ns.

`./bench` exits non-zero if a gated case loses encode or owned decode to
prost, v4, or buffa owned. Buffa view is gated except `tat_populated`
(~3% band) and `packed_fixed_256` (view does not build an owned `Vec`; we
still win that row on this host, it is not a process gate).

Not gated: JSON, text, KiB+ payloads, proto2 required, maps larger than 64,
WKT. Those are conformance, not this timer.

## Gated (process fails on a loss)

Encode ns / decode ns. One Apple Silicon host. Two consecutive
`./target/release/bench` runs, second capture below.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **24 / 20** | 80 / 123 | 144 / 79 | 66 / 166 | n/a / 111 |
| person | 62 | **38 / 70** | 38 / 191 | 76 / 156 | 39 / 149 | n/a / 78 |
| TAT populated | 87 | **87 / 311** | 148 / 440 | 247 / 399 | 129 / 397 | n/a / 302 |
| packed varint 256 | 388 | **76 / 266** | 533 / 808 | 465 / 872 | 568 / 383 | n/a / 389 |
| map 64 | 500 | **251 / 996** | 674 / 2329 | 970 / 3162 | 408 / 1794 | n/a / 1078 |
| nested depth 8 | 26 | **225 / 144** | 2373 / 1070 | 1151 / 302 | 624 / 1323 | n/a / 1393 |
| strings | 163 | **58 / 160** | 118 / 334 | 192 / 169 | 107 / 316 | n/a / 193 |
| unpacked varint 256 | 896 | **410 / 1150** | 528 / 1656 | 763 / 2484 | 794 / 1644 | n/a / 2432 |
| packed fixed32 256 | 1028 | **48 / 85** | 214 / 728 | 223 / 127 | 182 / 202 | n/a / 151 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

TAT populated vs buffa view is a tie at this size. Do not quote a win.
Person view has more headroom. packed-fixed owned decode is faster than
buffa view here; that is not gated.

## Extended (reported, not gated)

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| bytes | 315 | **70 / 174** | 132 / 500 | 221 / 193 | 116 / 382 | n/a / 222 |
| scalars (bool/enum/float/packed bool) | 77 | **68 / 167** | 158 / 312 | 178 / 216 | 98 / 240 | n/a / 220 |
| packed fixed64 256 | 2052 | 85 / 171 | 202 / 741 | 258 / **143** | 181 / 202 | n/a / **152** |
| packed float 256 | 1028 | 78 / 162 | 184 / 723 | 217 / **137** | 190 / 201 | n/a / **145** |
| unpacked fixed32 256 | 1536 | **215 / 781** | 291 / 1453 | 622 / 1645 | 585 / 1543 | n/a / 2054 |
| oneof string | 23 | **36 / 86** | 95 / 142 | 152 / 88 | 82 / 176 | n/a / 117 |
| repeated nested 8 | 38 | 91 / 259 | 120 / **223** | 205 / 268 | 124 / 304 | n/a / 247 |

Owned-decode losses vs v4: **packed_fixed64_256**, **packed_float_256**.
Those slots live in TAT `Cold`. Only `packed_fixed32` is on the hot struct.
Encode still wins. View losses on those rows are allowed (view does not
materialize a `Vec`).

Owned-decode loss vs prost: **repeated_nested_8**. Eight small submessages
through Cold. Encode still wins.

Re-run:

```bash
cd bench && cargo build --release && ./target/release/bench
```

## Why v4 encode is large on these sizes

Every v4 `serialize`: Arena, FFI `upb_Encode`, copy to `Vec`. Codec work on
<1 KiB is tens of ns. Setup is hundreds. See `docs/upb.md`.
