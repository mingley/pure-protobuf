# xlang upb C peer (SB-06)

Schema-generic timing harness over the pinned upb runtime (upb ships inside
`protocolbuffers/protobuf` and is built as `libupb` from the same pinned
sources), driven by `scripts/xlang-codec.sh`. One peer, `upb`:

| Op | Timed loop |
|---|---|
| `decode` | fresh `upb_Arena_New` + `upb_Message_New` + `upb_Decode` + `upb_Arena_Free` per op |
| `encode` | message decoded once; fresh arena + `upb_Encode` + free per op |

upb is arena-based throughout, so every op includes arena setup/teardown
(request-scoped arena use), matching the `cpp_arena` peer's accounting.

## Never shipped

TEST TOOL ONLY. This directory is built by `scripts/xlang-codec.sh` into
`target/xlang-codec/bin` (gitignored) and linked against the pinned upb
sources in `third_party/protobuf/upb`. No Cargo manifest, build script, or
shipped crate references it; it cannot enter a shipping dependency graph.

## How it works

1. The driver compiles each SB-05 corpus's manifest roots with the pinned
   `protoc` into `target/xlang-codec/descs/<corpus>.desc` (same descriptor
   sets the C++ peer uses).
2. `harness-upb --verify-only` parses the set with upb's bootstrap
   `descriptor.upb.h` routines, adds each file to a `upb_DefPool` in
   dependency order, and proves the round-trip, recorded per cell as
   `xlang.verify`:
   - `wire_equal`: re-encoded bytes are identical to the payload;
   - `semantic_equal`: the re-encoded bytes decode to a message that compares
     equal under the order-insensitive `upb_Message_IsEqual`.
   
   upb encodes fields in field-number order with per-call map ordering, so
   map-bearing payloads normally land on `semantic_equal`; the recorded
   method may vary run to run for those cells while the pass/fail verdict is
   stable.
3. Only verified cells are timed (`--iters N`), with warmup outside the timed
   window. Output is one JSON object on stdout; exit 2 means verification
   failed and the driver refuses to time that cell.

Reflection (`upb_DefPool` + minitables) is used so one binary covers all 79
corpus schemas without per-schema generated code. Absolute numbers are a
reflection baseline, not peak generated-upb speed.

## Build

Normally built via the driver (`scripts/xlang-codec.sh --build-only`), which
sets up the pinned sources and Release libraries first. Direct rebuild:

```sh
make -C bench/xlang/upb   # needs target/xlang-peer-build (see driver)
```

Link inputs (all pinned): `libupb.a`, `libutf8_range.a`,
`libutf8_validity.a`. No abseil dependency.

## Cell IDs

`xlang.upb.<corpus>.<short>.<tier>.<encode|decode>`, `kind`
`xlang_encode`/`xlang_decode`, reported in devloop/1 JSON with per-op wall
medians. Instructions are measured on Linux with `perf` (N/2N differential);
allocations are `not_run` (upb arenas are invisible to any Rust-side counter
and are not instrumented here either).
