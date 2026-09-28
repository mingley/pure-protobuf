# Compression backend, deflate, and zstd (RX-06)

Decisions for the `pbrs-grpc` message-compression registry
(`src/compression/`): which DEFLATE backend, what `deflate` means on the
wire, and whether to ship zstd.

## Backend: miniz_oxide stays (flate2 1.1.10 + `runtime_detection`)

Evaluated flate2's two pure-Rust backends with a throwaway harness
(`/tmp/zbench`, since removed) driving `GzEncoder`/`GzDecoder` over
protobuf-like payloads (tags, varints, short strings, zero runs) at
1 KiB / 32 KiB / 300 KiB and levels 1 (the kernel default) and 6, release
mode, Apple Silicon, interleaved A/B runs with backend verified via linked
symbols (`zlib_rs` present/absent). Medians of 3 runs, ms/iter:

| size  | level | miniz enc | zlib-rs enc | miniz dec | zlib-rs dec |
|-------|-------|-----------|-------------|-----------|-------------|
| 32K   | 1     | 0.044     | 0.051       | 0.037     | 0.035       |
| 32K   | 6     | 0.260     | 0.264       | 0.033     | 0.036       |
| 300K  | 1     | 0.552     | 0.613       | 0.310     | 0.347       |
| 300K  | 6     | 4.134     | 2.508       | 0.296     | 0.310       |

(`runtime_detection` on for both: without it flate2 leaves zlib-rs scalar
and the comparison is meaningless. Ratios within 0.01 either way.)

miniz_oxide wins encode and decode at the default level 1 (~8-10%);
zlib-rs wins only level-6 encode (~40%). Both are reviewed codebases, so
the measured default-level winner stays: no new dependency, no MSRV
change (zlib-rs 0.6 needs rust 1.75, so it would have fit, but there is
no reason to switch on this evidence).

Two changes did ship:

- flate2 1.1.9 -> 1.1.10 with `runtime_detection`: free SIMD dispatch
  (crc32fast `std`, miniz_oxide SIMD paths). ~3-8% on the same harness.
- The `zlib-rs` crate name now appears as an (inactive) edge in
  `Cargo.lock` because flate2 1.1.10 declares the optional backend. It is
  not compiled (`cargo tree` shows no path to it) and the pure-Rust audit
  reads the activated graph, so the gate is unaffected. An auditor seeing
  it in the lock should check `cargo tree`, not the lock edge list.

Re-measure trigger: x86-64 Linux server numbers. zlib-rs's published
wins are largest on x86 SIMD; if a server-side shootout flips the table,
switching is a one-line feature change plus this doc.

## Framing: compress straight into the frame buffer

`encode_msg`/`append_frame` used to serialize, compress into an
intermediate `Vec`, then copy that into the framed buffer: 3 allocations
and 2 full copies per compressed message. The encoder now writes through
`Codec::encode_into` directly into the frame `BytesMut` (the 5-byte
header goes out first with a zero length, patched once the stream ends),
saving 1 allocation and 1 bulk copy per message on every shape in both
directions. Byte-identical output is pinned by
`compression::tests::encode_into_matches_encode`.

Dev-loop evidence (new `rpc.pbrs.unary_compressed` /
`rpc.pbrs.server_stream_compressed` cells, 32 KiB mixed payload,
macOS, medians of 5x1000 RPCs; instructions/syscalls are Linux-only so
this host reports allocs + wall):

| cell | allocs/RPC before | after | delta |
|------|-------------------|-------|-------|
| unary_compressed | 87.3 | 85.3 | -2.28% |
| server_stream_compressed | 949.4 | 880.3 | -7.27% |

Both clear the 2% improvement bar; uncompressed cells are untouched
(allocs parity +0.00%). Wall time moved favorably too (-5~-6%) but this
host's run-to-run wall drift (±6-11% on repeats of the same binary)
drowns single-digit effects, so wall is reported, not claimed. The
Linux CI perf lane (SB-04) runs these cells on every PR and will render
the instructions verdict.

## `deflate` means zlib (RFC 1950), not a raw stream

gRPC's `deflate` coding is the zlib wrapper (2-byte header + adler32),
matching HTTP's `deflate` content-coding. C-core is authoritative:
`GRPC_COMPRESS_DEFLATE` uses `deflateInit2(..., 15, ...)` /
`inflateInit2(..., 15)` (windowBits 15 = zlib framing;
`third_party/grpc/src/core/lib/compression/message_compress.cc`).

The first RX-06 cut implemented raw RFC 1951 streams and C++ interop
caught it immediately: once we advertised `identity,gzip,deflate`, the
C++ server answered `server_compressed_*` with `grpc-encoding: deflate`
and our raw inflate failed with `corrupt deflate stream`. The codec now
uses flate2's `ZlibEncoder`/`ZlibDecoder`, pins CPython-zlib vectors at
levels 1 and 6, and explicitly rejects raw streams
(`compression::deflate::tests::raw_deflate_stream_is_rejected`).

Negotiation notes:

- We advertise `identity,gzip,deflate` whenever inbound compression is
  on, and decode per the RPC's `grpc-encoding` token.
- Outbound defaults to gzip (`ChannelConfig`/`ServerConfig`::
  `compression_codec`, default `Codec::Gzip`). A deflate-configured
  server falls back to gzip for a gzip-only peer rather than sending
  identity (`preferred_codec`), and unknown codings are refused as
  `UNIMPLEMENTED` instead of failing inflate as `Internal`.
- The official interop compression procedures accept any non-identity
  coding when compression was requested (the C++ client only rejects
  `GRPC_COMPRESS_NONE`); our `interop_cases` server-compressed
  assertions were relaxed from hard-coded `gzip` to match.

## zstd: deferred

Not shipped. Three independent reasons, any one of which would do it:

1. No peer speaks it. C-core's registry is none/deflate/gzip only
   (`GRPC_COMPRESS_ALGORITHMS_COUNT` = 3,
   `include/grpc/impl/compression_types.h`); grpc-go's `encoding/`
   ships gzip only. There is nothing to interop against.
2. No in-MSRV pure-Rust codec. The only pure-Rust implementation,
   `ruzstd`, declares rust-version 1.87, above the pbrs-grpc MSRV of
   1.85, and its encoder is immature relative to its decoder.
3. No official procedure. The interop suite has no zstd case, so a zstd
   sender would be unverifiable wire behavior.

Revisit when: a peer ships `grpc-encoding: zstd`, or an in-MSRV
pure-Rust codec pair with a stable encoder exists. The `Codec` enum is
the extension point; adding a variant threads through the same
negotiation both directions.
