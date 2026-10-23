# Compression backend, deflate, and zstd (RX-06)

`pbrs-grpc` uses `miniz_oxide` through flate2 for gzip and zlib-framed
`deflate` (RFC 1950). Both support the base Rust 1.85 profile. Optional Zstd
uses `ruzstd`, requires Rust 1.87, and has separate peer checks.

## Historical backend choice

The original comparison used two flate2 backends with a temporary
harness (`/tmp/zbench`, since removed). Its source and raw runs are not
available, so these results record the decision's rationale rather than
reproducible evidence for the current implementation. The harness drove
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

The record says `runtime_detection` was enabled for both backends. Results
were mixed: miniz encoded these level-1 payloads faster, while zlib-rs decoded
the 32 KiB level-1 payload faster and encoded 300 KiB at level 6 faster.
These observations do not establish a universal backend winner.

The project retained miniz and enabled flate2 1.1.10's `runtime_detection`.
The old harness also reported gains from that change; they are not current
performance proof. The optional `zlib-rs` lockfile edge is inactive in this
profile; check the activated dependency graph with `cargo tree` when auditing.

A backend change needs fresh, source-pinned x86_64 and arm64 comparisons at
matched settings. The current gRPC candidate campaign uses **gzip level 6 on
both stacks**, rather than the kernel's level-1 default.

## Historical framing change

`encode_msg`/`append_frame` used to serialize, compress into an
intermediate `Vec`, then copy into the framed buffer. That cost 3 allocations
and 2 full copies per compressed message.

`CompressionAlgorithm::encode_into` lets the encoder write into the frame
`BytesMut`: reserve the five-byte header, then patch its length after encoding.
This removed an intermediate allocation and copy in that path. Streaming paths
also use reusable scratch; their current costs require separate measurement.
Byte-identical output is checked by
`compression::tests::encode_into_matches_encode`.

Historical development-loop observations (`rpc.pbrs.unary_compressed` /
`rpc.pbrs.server_stream_compressed` cells, 32 KiB mixed payload,
macOS, medians of 5x1000 RPCs; instructions/syscalls are Linux-only so
this host reports allocs + wall):

| cell | allocs/RPC before | after | delta |
|------|-------------------|-------|-------|
| unary_compressed | 87.3 | 85.3 | -2.28% |
| server_stream_compressed | 949.4 | 880.3 | -7.27% |

These older allocation observations do not describe the current candidate.
Wall time varied too much to support a speed claim. Use the source-pinned
[benchmark results](../benchmarks.md) for later comparisons.

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

## zstd: optional feature (TC-20)

Shipped as the opt-in `pbrs-grpc/zstd` feature. The default dependency graph
is unchanged and still builds with Rust 1.85.

TC-20 originally used `zstd-rs` 0.1.0, but coordinator review rejected it:
the crate was first published on 2026-09-28, had no adoption, and had a single
owner. That is too much supply-chain risk for a production library dependency,
even optional. New dependencies now follow this provenance rule: do not add a
crate younger than roughly 90 days, or with negligible downloads/adoption, or
with a single unknown owner, unless the maintainer explicitly approves it.

The shipped dependency is `ruzstd` 0.9.0 from KillingSpark/zstd-rs.
It declares `rust-version = 1.87`,
so the `zstd` feature has a documented higher MSRV (1.87) than the default
pbrs-grpc 1.85 profile. `ruzstd` 0.9 does provide an encoder:
`ruzstd::encoding::compress_to_vec(source, CompressionLevel::Fastest)`.
Only `Fastest` is implemented; `Default`, `Better`, and `Best` are explicitly
unimplemented. Therefore every pbrs-grpc zstd compression-level request maps
to Fastest, roughly zstd level 1. The feature adds `CompressionAlgorithm::Zstd`, advertises
`identity,gzip,deflate,zstd`, parses `grpc-encoding: zstd`, and negotiates
zstd through the same registry as gzip and deflate. A zstd-configured server
still falls back to gzip or deflate when the peer does not advertise zstd.

Zstd decoding has separate limits for decoded output and decoder history.
`MessageLimits::with_max_decoding` caps the wire payload and decoded message.
`MessageLimits::with_max_zstd_window` caps the advertised history window at
8 MiB by default; larger windows fail with `RESOURCE_EXHAUSTED` before history
allocation. `MessageLimits::unlimited()` uses the backend's 100 MiB window cap.
Lifting only the decoded message cap preserves the configured window limit.

A declared content size above the message cap is rejected before decoding
blocks. For an unknown content size, the backend may buffer a history window
and block data before returning the first decoded byte. The output cap therefore
does not bound all decoder work or total memory. Account for decoder state,
block buffers, compressed input and allocator overhead separately. Decoded
lengths and content checksums are validated for each standard Zstd frame.
All frames share one decoded-output cap. The hostile-peer test
`a_zstd_bomb_cannot_outgrow_the_cap` checks the configured output limit.

The C `zstd` crate is not in `pbrs-grpc`'s dev-dependencies. C-backed zstd
interop proof lives in the standalone `tests/interop/tonic` workspace, which
already allows tonic/C-backed peer tooling. Its test verifies `CompressionAlgorithm::Zstd`
frames decode with the C zstd crate and C zstd frames decode with pbrs-grpc.
`pbrs-grpc/tests/compression.rs` checks that a real unary RPC negotiates zstd
in both directions.

Remaining limits: there is still no official gRPC interop procedure for zstd,
so this is a tonic-compatible proof rather than an upstream interop case.

## Complete compressed input and work limits

Candidate `90201458` implements the complete-input policy described in the
[execution decision](grpc-execution.md). The older
[streaming investigation](../evidence/grpc-streaming-20261022/README.md)
records the original partial-data and trailing-input gaps; its untested proposal
is historical material. The implemented candidate has separate source and
qualification records in the [execution follow-up](../evidence/grpc-architecture-20261022/README.md).

| Coding | Accepted content within one gRPC compressed payload | Rejected content |
|---|---|---|
| gzip | Concatenated RFC 1952 members form one protobuf message; each CRC/ISIZE is checked. | Malformed or truncated later members and non-member tails, including zero padding. |
| deflate | One RFC 1950 zlib stream. | Bytes after the stream, including a second stream or zero padding. |
| Zstd | RFC 8878 standard and skippable frames; standard-frame output is concatenated. | Invalid/truncated later frames, arbitrary tails and reserved descriptor bit 3. |

The aggregate decoded-message cap applies across all members/frames. Every
standard Zstd frame receives independent window admission, declared-size and
checksum checks. Skippable metadata is consumed without allocating its contents;
the unused descriptor bit is ignored as required by the format.

`MessageLimits::with_max_compression_frames` separately caps gzip members and
Zstd standard/skippable frames per gRPC message. The default is **1,024**;
empty members and skippable frames count too. Deflate consumes one admission.
Exceeding the cap returns `RESOURCE_EXHAUSTED` before another decoder reset.
`with_unlimited_decoding` preserves this count limit; trusted callers can lift
it explicitly with `with_unlimited_compression_frames`. `unlimited()` lifts both
byte and count limits, leaving Zstd at the backend's 100 MiB window cap.

Counting frames bounds setup/reset work independently of output bytes. It does
not preempt one admitted frame, bound all decoder memory or limit the number
of gRPC messages in a stream. Zstd decoder scratch is reused within a message
and released afterward. Decoder capacity can exceed its logical history size;
the window cap is not a total allocation or RSS cap.

The opaque tonic adapter now validates and retains the complete decoded gzip
message, releases the compressed views, and emits an uncompressed-flag-zero
message for generated tonic. This avoids first-member-only replay and a second
inflate. Identity input keeps its original byte views. Input/output polling,
aggregate output and member-count limits remain bounded; encoded input and
normalized output can coexist during validation and require separate memory
accounting.

These source changes repair known format gaps. Passing focused tests does not
establish complete specification or production qualification; the final
candidate's full checks and resource campaign remain separate requirements.

## Preserve the published compression selector (AD-03c)

The published `Codec` enum has two exhaustive variants, `Gzip` and `Deflate`.
Adding `Zstd` to it broke downstream exhaustive matches when the optional
feature was enabled. An explicit minor-policy semver check exposed this;
unchanged alpha versions had previously caused the checker to skip its lints.

Keep `Codec`, its discriminants and its public methods unchanged. Optional
codings use the new non-exhaustive `CompressionAlgorithm` enum. Native and
gRPC-Web framing, negotiation and decoding use that registry; the compression
backends, limits, wire tokens and defaults stay as before. `From<Codec>` and
`CompressionAlgorithm::legacy_codec()` connect the two surfaces.

Both config types retain `compression_codec(Codec)` and `send_codec() -> Codec`.
The additive `compression_algorithm(CompressionAlgorithm)` selects the wire
preference, and `send_algorithm()` reports it even for Zstd. Selecting gzip or
deflate updates both getters. Selecting an optional coding keeps the previous
legacy selection for `send_codec()`, documented as such. Calling the legacy
setter again replaces the wire preference. A configured server preference
may fall back after peer negotiation; `Response::encoding()` reports that
actual coding.

This separates an existing exhaustive contract from future registry additions
without suppressing the semver check or changing crate versions.
