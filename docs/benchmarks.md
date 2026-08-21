# Benchmarks

Same `.proto` on every row: plugin-generated pbrs types vs `prost-build` of
that proto, crates.io `protobuf` **4.35.1-release** (`protoc --rust_out
kernel=upb`), buffa **0.9.1** owned, and buffa `decode_view` where it
exists.

Decode uses pbrs wire bytes. `./bench` (from `bench/`): 40 000 iters, median
of 15 after warmup. Release, thin LTO, one codegen unit.
`size_of::<TestAllTypesProto3>()` = **616**.

`./bench` exits non-zero if a gated case loses encode or owned decode to
prost, v4, or buffa owned. Buffa view is gated except `tat_populated`, which
sits in a ~3% band (win and loss across consecutive runs).

Not gated: JSON, text, KiB+ payloads, proto2 required, maps larger than 64,
WKT, oneofs. Those are conformance, not this timer.

## Gated (process fails on a loss)

Encode ns / decode ns. One Apple Silicon host. Two consecutive
`./target/release/bench` runs, second capture below.

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| empty TAT | 0 | **22 / 20** | 80 / 123 | 143 / 78 | 70 / 168 | n/a / 113 |
| person | 62 | **35 / 71** | 39 / 194 | 77 / 165 | 40 / 152 | n/a / 80 |
| TAT populated | 87 | **78 / 300** | 202 / 446 | 236 / 401 | 129 / 408 | n/a / 307 |
| packed varint 256 | 388 | **65 / 263** | 545 / 825 | 465 / 892 | 583 / 366 | n/a / 374 |
| map 64 | 500 | **238 / 984** | 657 / 2340 | 986 / 3108 | 414 / 1730 | n/a / 1084 |
| nested depth 8 | 26 | **222 / 143** | 2348 / 1057 | 1124 / 324 | 633 / 1369 | n/a / 1430 |
| strings | 163 | **58 / 160** | 123 / 323 | 192 / 169 | 110 / 321 | n/a / 191 |

person uses handwritten `pbrs::testdata::Person` (inline small repeats).
Everything else is generated TestAllTypesProto3.

TAT populated vs buffa view is a tie at this size (about 290 vs 300 ns).
Do not quote a win. Person view has more headroom.

## Extended (reported, not gated)

| case | payload | pbrs | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|---:|
| unpacked varint 256 | 896 | 491 / **1842** | 538 / **1654** | 786 / 2549 | 821 / **1662** | n/a / 2462 |
| packed fixed32 256 | 1028 | 148 / 157 | 201 / 726 | 224 / **127** | 183 / 200 | n/a / **151** |
| bytes | 315 | **69 / 168** | 131 / 499 | 226 / 204 | 118 / 380 | n/a / 220 |
| scalars (bool/enum/float/packed bool) | 77 | **54 / 169** | 159 / 310 | 178 / 226 | 101 / 239 | n/a / 225 |

Losses, owned decode:

- **unpacked_256**: prost and buffa owned. 256 separate tags. Those slots
  live in TAT `Cold`, so parse boxes then grows a `Vec`.
- **packed_fixed_256**: v4 and buffa view. upb memcpy-packs into the arena.
  view does not build an owned `Vec`. We still win encode.

Re-run:

```bash
cd bench && cargo build --release && ./target/release/bench
```

## Why v4 encode is large on these sizes

Every v4 `serialize`: Arena, FFI `upb_Encode`, copy to `Vec`. Codec work on
<1 KiB is tens of ns. Setup is hundreds. See `docs/upb.md`.
