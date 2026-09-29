# Compression backend, deflate, and zstd (RX-06)

This decision is for Rust developers working on the `pbrs-grpc`
message-compression registry (`src/compression/`). Decision: keep
`miniz_oxide` through flate2, define gRPC `deflate` as zlib framing
(RFC 1950), and defer zstd until peers and an in-MSRV Rust codec make it
testable.

## Backend: miniz_oxide stays (flate2 1.1.10 + `runtime_detection`)

The backend comparison used flate2's two pure-Rust backends with a throwaway
harness (`/tmp/zbench`, since removed). The harness drove
`GzEncoder`/`GzDecoder` over protobuf-like payloads: tags, varints, short
strings, and zero runs. It tested 1 KiB / 32 KiB / 300 KiB payloads at levels
1 (the kernel default) and 6, in release mode on Apple Silicon. Runs were
interleaved A/B, and the linked symbols verified the backend (`zlib_rs`
present or absent). Medians of 3 runs, ms/iter:

| size  | level | miniz enc | zlib-rs enc | miniz dec | zlib-rs dec |
|-------|-------|-----------|-------------|-----------|-------------|
| 32K   | 1     | 0.044     | 0.051       | 0.037     | 0.035       |
| 32K   | 6     | 0.260     | 0.264       | 0.033     | 0.036       |
| 300K  | 1     | 0.552     | 0.613       | 0.310     | 0.347       |
| 300K  | 6     | 4.134     | 2.508       | 0.296     | 0.310       |

`runtime_detection` was on for both. Without it, flate2 leaves zlib-rs scalar
and the comparison is meaningless. Ratios within 0.01 go either way.

`miniz_oxide` wins encode and decode at the default level 1 (~8-10%).
`zlib-rs` wins only level-6 encode (~40%). Both are reviewed codebases, so the
measured default-level winner stays. This means no new dependency and no
minimum supported Rust version (MSRV) change. `zlib-rs` 0.6 needs Rust 1.75, so
it would have fit, but this evidence gives no reason to switch.

Two changes did ship:

- flate2 1.1.9 -> 1.1.10 with `runtime_detection`: free single instruction,
  multiple data (SIMD) dispatch (crc32fast `std`, miniz_oxide SIMD paths).
  ~3-8% on the same harness.
- The `zlib-rs` crate name now appears as an (inactive) edge in
  `Cargo.lock` because flate2 1.1.10 declares the optional backend. It is
  not compiled (`cargo tree` shows no path to it) and the pure-Rust audit
  reads the activated graph, so the gate is unaffected. An auditor seeing
  it in the lock should check `cargo tree`, not the lock edge list.

Re-measure trigger: x86-64 Linux server numbers. zlib-rs's published wins are
largest on x86 SIMD. If a server-side shootout flips the table, switching is a
one-line feature change plus this doc.

## Framing: compress straight into the frame buffer

`encode_msg`/`append_frame` used to serialize, compress into an
intermediate `Vec`, then copy into the framed buffer. That cost 3 allocations
and 2 full copies per compressed message.

The encoder now writes through `Codec::encode_into` directly into the frame
`BytesMut`. The 5-byte header goes out first with a zero length and is patched
once the stream ends. This saves 1 allocation and 1 bulk copy per message on
every call shape in both directions. Byte-identical output is pinned by
`compression::tests::encode_into_matches_encode`.

Dev-loop evidence (new `rpc.pbrs.unary_compressed` /
`rpc.pbrs.server_stream_compressed` cells, 32 KiB mixed payload,
macOS, medians of 5x1000 RPCs; instructions/syscalls are Linux-only so
this host reports allocs + wall):

| cell | allocs/RPC before | after | delta |
|------|-------------------|-------|-------|
| unary_compressed | 87.3 | 85.3 | -2.28% |
| server_stream_compressed | 949.4 | 880.3 | -7.27% |

Both clear the 2% improvement bar. Uncompressed cells are untouched (allocs
parity +0.00%). Wall time also moved favorably (-5~-6%), but this host's
run-to-run wall drift (±6-11% on repeats of the same binary) drowns
single-digit effects. Wall is reported, not claimed. The Linux CI perf lane
(SB-04) runs these cells on every PR and will render the instructions verdict.

## `deflate` means zlib (RFC 1950), not a raw stream

gRPC's `deflate` coding is the zlib wrapper: 2-byte header plus adler32. It is
not a raw deflate stream. This matches HTTP's `deflate` content-coding.

C-core is authoritative: `GRPC_COMPRESS_DEFLATE` uses
`deflateInit2(..., 15, ...)` / `inflateInit2(..., 15)`. `windowBits = 15` means
zlib framing (`third_party/grpc/src/core/lib/compression/message_compress.cc`).

The first RX-06 cut implemented raw RFC 1951 streams. C++ interop caught it
immediately. Once we advertised `identity,gzip,deflate`, the C++ server
answered `server_compressed_*` with `grpc-encoding: deflate`, and our raw
inflate failed with `corrupt deflate stream`.

The codec now uses flate2's `ZlibEncoder`/`ZlibDecoder`, pins CPython-zlib
vectors at levels 1 and 6, and explicitly rejects raw streams
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

## zstd: optional pure-Rust feature (TC-20)

Shipped as the opt-in `pbrs-grpc/zstd` feature. The default dependency graph
is unchanged and still builds with Rust 1.85.

TC-20 originally used `zstd-rs` 0.1.0, but coordinator review rejected it:
the crate was first published on 2026-09-28, had no adoption, and had a single
owner. That is too much supply-chain risk for a production library dependency,
even optional. New dependencies now follow this provenance rule: do not add a
crate younger than roughly 90 days, or with negligible downloads/adoption, or
with a single unknown owner, unless the maintainer explicitly approves it.

The shipped dependency is `ruzstd` 0.9.0 from KillingSpark/zstd-rs. It is pure
Rust, established, and materially adopted. It declares `rust-version = 1.87`,
so the `zstd` feature has a documented higher MSRV (1.87) than the default
pbrs-grpc 1.85 profile. `ruzstd` 0.9 does provide an encoder:
`ruzstd::encoding::compress_to_vec(source, CompressionLevel::Fastest)`.
Only `Fastest` is implemented; `Default`, `Better`, and `Best` are explicitly
unimplemented. Therefore every pbrs-grpc zstd compression-level request maps
to Fastest, roughly zstd level 1. The feature adds `Codec::Zstd`, advertises
`identity,gzip,deflate,zstd`, parses `grpc-encoding: zstd`, and negotiates
zstd through the same registry as gzip and deflate. A zstd-configured server
still falls back to gzip or deflate when the peer does not advertise zstd.

Decompression stays bounded by the existing message limit: the implementation
uses `ruzstd::decoding::StreamingDecoder` and stops reading one byte past
`MessageLimits::inflate_budget()`. Crossing the cap maps to
`RESOURCE_EXHAUSTED`; it does not first inflate the whole message and then
check its length. The hostile-peer test `a_zstd_bomb_cannot_outgrow_the_cap`
sends a compressed 64 MiB zero payload whose frame length is below the 256 KiB
frame cap; the server rejects it at the uncompressed message limit.

The C `zstd` crate is not in `pbrs-grpc`'s dev-dependencies. C-backed zstd
interop proof lives in the standalone `tests/interop/tonic` workspace, which
already allows tonic/C-backed peer tooling. Its test verifies `Codec::Zstd`
frames decode with the C zstd crate and C zstd frames decode with pbrs-grpc.
`pbrs-grpc/tests/compression.rs` keeps the pure-Rust in-crate proof that a
real unary RPC negotiates zstd in both directions.

Remaining limits: there is still no official gRPC interop procedure for zstd,
so this is a tonic-compatible proof rather than an upstream interop case.
