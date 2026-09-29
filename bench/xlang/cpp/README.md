# xlang C++ protobuf peer (SB-06)

Schema-generic timing harness over the pinned C++ protobuf runtime, driven by
`scripts/xlang-codec.sh`. It covers two scoreboard peers from one binary:

| Peer | Harness mode | Decode op | Encode op |
|---|---|---|---|
| `cpp` | `heap` | fresh `new` + parse + `delete` per op | one message, serialize per op |
| `cpp_arena` | `arena` | fresh `Arena::CreateMessage` + parse + `Arena::Reset` per op | arena-owned message, serialize per op |

The arena cell models request-scoped arena use: the arena is reset after every
decode op, so per-op teardown is included.

## Never shipped

TEST TOOL ONLY. This directory is built by `scripts/xlang-codec.sh` into
`target/xlang-codec/bin` (gitignored) and linked against the pinned
`protocolbuffers/protobuf` sources in `third_party/`. No Cargo manifest, build
script, or shipped crate references it; it cannot enter a shipping dependency
graph.

## How it works

1. The driver compiles each SB-05 corpus's manifest roots with the pinned
   `protoc` (`--descriptor_set_out --include_imports`) into
   `target/xlang-codec/descs/<corpus>.desc`.
2. `harness-cpp --verify-only` loads the descriptor set into a
   `DescriptorPool`, parses the payload with a `DynamicMessage`, and proves
   the round-trip one of three ways, recorded per cell as `xlang.verify`:
   - `wire_equal`: plain re-serialization is byte-identical;
   - `wire_equal_deterministic`: deterministic re-serialization (sorted map
     entries) is byte-identical;
   - `semantic_equal`: re-parse plus a deterministic-serialization fixpoint
     and identical `DebugString`.
3. Only verified cells are timed (`--iters N`), with warmup outside the timed
   window. Output is one JSON object on stdout; exit 2 means verification
   failed and the driver refuses to time that cell.

Dynamic (reflection-based) messages are used so one binary covers all 79
corpus schemas without generating and compiling per-schema C++. This adds
reflection overhead versus generated-code peers; it is the same for every
payload shape, so cross-corpus comparisons stay valid, and absolute numbers
should be read as a dynamic-message baseline, not as peak generated-C++ speed.

## Build

Normally built via the driver (`scripts/xlang-codec.sh --build-only`), which
sets up the pinned sources and Release libraries first. Direct rebuild:

```sh
make -C bench/xlang/cpp   # needs target/xlang-peer-build (see driver)
```

Link inputs (all pinned): `libprotobuf.a`, the FetchContent abseil archives
(`absl_pin` recorded in the report provenance), `libutf8_range.a`,
`libutf8_validity.a`.

## Cell IDs

`xlang.<cpp|cpp_arena>.<corpus>.<short>.<tier>.<encode|decode>`,
`kind` `xlang_encode`/`xlang_decode`, reported in devloop/1 JSON with per-op
wall medians. Instructions are measured on Linux with `perf` (N/2N
differential); allocations are `not_run` (the C++ heap is not instrumented).
