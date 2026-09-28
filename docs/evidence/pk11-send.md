# PK-11: zero-copy send for shared fields (Phase 2) — verification record

Base: `8f98202d` (MX-01). Implements zero-copy plan Phase 2: large bytes
fields are retained as segments and sent scatter-gather instead of being
copied into the frame buffer.

## What changed

- `src/wire.rs`: `WireOut::put_shared` (default copies) and
  `WireOut::supports_shared` (default false); `SHARED_SEND_THRESHOLD`
  (`T_send` = 32 KiB); `encode_len_field_shared` helper.
- `src/lazy.rs`: `LazyBytes::shared_bytes()` — `Some` only when the
  buffer is obtainable without a copy (shared or owned backing; `None`
  for privately-parsed windows, where sharing would copy anyway).
- `src/codegen/encode.rs`: `emit_write` routes singular/option/repeated
  **bytes** fields through the helper; strings unchanged. Old generated
  code (checked-in fixtures) keeps compiling and behaving identically.
- `pbrs-grpc/src/wire/encode.rs`: `SegSink` (segmented `WireOut`) and
  `SegFrame` (first segment inline, so the unsegmented case allocates
  nothing extra); `frame_from_msg`/`append_frame`/`encode_msg` return
  segments; gzip path stays contiguous and single-segment.
- `pbrs-grpc/src/wire/send.rs`: `send_frame` (end-of-stream on the last
  segment only); unary/stream/retry/hedge/watch call sites moved from
  `Bytes` frames to `SegFrame`; `OutBatch` accumulates a sink with
  checkpoint/rollback and total-byte fullness.
- Counters: `shared_segments`/`shared_bytes` zero-copy witness;
  `encode_bytes` now counts only bytes actually copied. Surfaced in
  devloop's `CopyCountsJson` (additive, serde-defaulted).
- Tests: `WireOut` default + threshold/backing routing (`src/wire.rs`);
  emission shape for bytes vs string (`src/codegen.rs`); segment
  concatenation differential, single-segment fast path, and
  copy-removal counters (`pbrs-grpc/src/wire/encode.rs`).

Two regressions caught by the gates during development and fixed:

1. `Bytes::clone` on a uniquely-owned buffer **promotes** (small alloc).
   Calling `shared_bytes()` unconditionally added one ~24 B alloc per
   encode on `serialize()` sinks, where the handle would only be copied
   back out. Fixed with `supports_shared()`: only segmented sinks take
   the shared branch. Blob alloc cells back to exactly 2.0.
2. `SegSink::finish` pushed the head through the segment `Vec` even when
   unsegmented (+1 alloc/encode). Fixed with a fast path returning the
   head directly when no sharing happened.

## Accept 1 — outbound copy removed (SB-13 style)

`shared_send_removes_outbound_copy`: a 1 MiB bytes field framed via
`frame_from_msg` witnesses `shared_bytes >= 1 MiB` while `encode_bytes`
stays under 100 KB (head only). Gzip path untouched (contiguous,
covered by `native-codegen` + compressed devloop/rpc cells); TLS
correctness by the 20-test `tls` suite plus a live TLS A/B below.

## Accept 2 — byte-identical frames

`segments_concat_to_identical_frame`: concatenated segments equal the
5-byte prefix + `serialize()`. End-to-end: all rpc/hostile/message_size
suites pass against fresh generated code, and every A/B run below shows
0 failures across ~15k large-payload RPCs per side.

## Throughput (rpc-bench, loopback, release)

| Shape | Base qps | PK-11 qps | Delta |
|---|---|---|---|
| upload 8x1MiB, plaintext (2+2 runs) | 224.4, 227.0 | 239.3, 237.1 | **+5.5%** |
| download 8x1MiB server_stream (1+1) | 231.5 | 239.1 | **+3.3%** |
| upload 8x1MiB over TLS (1+1) | 182.0 | 200.7 | **+10.3%** |
| unary 8MiB req+resp (6+6 runs, ABBA) | ~115.5 avg | ~115.0 avg | flat (±0.5%, in noise) |

Unary 8M is round-trip/parse dominated at p50 ~34 ms, so the ~1 ms
send-copy saving sits inside run variance; streams batch efficiently
enough for the saving to show. Raw reports in `docs/evidence/pk11-send/`.

## Gates

- `format`, `core-lib` (43), `native-lib` (390), `native-rpc`,
  `native-hostile`, `native-tls` (20), `codegen` (14+26+41),
  `native-codegen` (197), `tonic`, `clippy`, conformance (909): green.
- `devloop-compare` vs pre-change baseline: no regressions (66 cells).
- Checked-in generated files and fixtures intentionally untouched: old
  gencode never calls the new helper (by design); fresh gencode (grpc
  build.rs, devloop, bench) picks it up automatically.
