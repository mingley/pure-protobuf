# GN-04: pbrs codegen is the fastest generator

## Baseline (base SHA `4f081be4`)

SB-09 evidence medians, `generation.elapsed_ns` (debug drivers,
protoc + emission): pbrs trails prost on every large corpus
(1000: 1.5 s vs 267 ms; envoy-core: 1.2 s vs 147 ms; googleapis:
366 ms vs 146 ms; envoy-discovery: 423 ms vs 262 ms).
Direct driver timing on the 1000-message corpus: ~530 ms/iter
(protoc ~86 ms, pbrs ~440 ms).

## Profile

`sample` on the debug driver over the 1000 corpus:

1. `reflection::emit_fds` ~54%: per-byte
   `write!(src, "0x{b:02x},")` through `core::fmt`, no reserve,
   re-executed for every target file (20x on the 1000 corpus).
2. Cold-placement predicates (`stored_hot`/`stored_cold` over
   full field scans) called per field: quadratic.
3. `file_matches`: per-file `format!`s rebuilt per wanted entry,
   repeated path normalization.

## Changes (all byte-identical)

- `reflection.rs`: `fds_hex_block` renders the
  `FILE_DESCRIPTOR_SET` block once per request (table-driven hex,
  single `String::from_utf8`); `descriptors.rs` shares it across
  target files.
- `naming.rs`/`messages.rs`/`parse.rs`: `ColdPlacement`
  precomputes per-message placement once; emitters query per
  field (`emit_merge_arm`, `emit_oneof_clear`, `emit_accessors`,
  `emit_codec` take it by value).
- `descriptors.rs`: `normalize_proto_path_str` fast path, hoisted
  per-file formats, pre-sized per-file buffers.
- `messages.rs`/`codegen.rs`: shared `HEX_DIGITS` /
  `HEX_BYTE_CHUNK` tables; `rust_byte_lit` without `core::fmt`.

Direct driver: 530 ms -> ~210 ms (2.5x) on the 1000 corpus.

## Byte-identical proof

- 1000-corpus outputs `cmp`-clean across all 21 files.
- `regen-generated.sh` diff hash identical before/after
  (`6021fed7`); the drift itself pre-exists (committed generated
  files lag the header format; `regen-check` was already red).

## Verification (SB-09 harness, pbrs vs prost)

Run: `codegen-bench.sh --case all --generators pbrs,prost
--stub-generators pbrs-native --repeats 5`, 80 cells.

PENDING: medians for `generation.elapsed_ns` /
`generation.peak_rss_bytes` per corpus below on completion.

## Gates

`codegen-harness` 62 OK, `docs-contract` 18 OK, `clippy
--all-targets --all-features -D warnings` clean, `fmt` clean.
`regen-check` red pre-existing (proven identical drift
before/after; out of scope).
