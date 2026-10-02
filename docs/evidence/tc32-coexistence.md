# TC-32: prost and pbrs coexistence

Date: 2026-09-30

## Shipped API

`protobuf-tonic` exposes schema-agnostic `prost_to_pbrs` and
`pbrs_to_prost` conversion functions. Both encode the source representation to
its canonical protobuf wire image and let the target decoder consume ownership
of that buffer. The prost-to-pbrs path turns the encoded `Vec<u8>` into
`bytes::Bytes` without copying; lazy pbrs fields retain that shared storage.
The reverse path also moves the encoded buffer into prost's `Buf` decoder.

The bridge source performs one encode and one target decode, using an owned
wire buffer with no explicit intermediate wire-buffer copy. Target messages
and their shared-storage owners can make their ordinary allocations. Total
allocations and bytes require actual API measurements; the source structure
is not a numeric cost result.

This explicit conversion is preferable to generated `From` implementations:
it works across independently generated schema crates, covers nested/imported
messages and unknown fields, and does not create an orphan-rule or feature
coupling between those crates.

## Mixed build and test coverage

`protobuf-tonic/tests/coexist.rs` defines an independently generated prost
representation next to the pbrs representation, converts both ways, requires
identical wire bytes, and checks round trips at the SB-26 public size/count
boundaries: 100, 256, 512 and 800-byte request fields plus 10, 100 and 1000
records represented by 384-byte payloads. `protobuf-tonic/tests/codec_path.rs`
separately runs unary and streaming tonic services over pbrs messages.
`pbrs-grpc/tests/prost_fixtures.rs` covers prost-generated native services.
Together these fixtures pin that one Cargo build can contain both runtimes and
that a migration boundary can cross between their representations.

Focused check:

```text
cargo test -p protobuf-tonic --test coexist
2 passed; 0 failed
```

## Generator selection

The codec is fixed for one generated tonic service module, because tonic's
`Codec` has one encode and one decode type per RPC. Mixed applications generate
pbrs-backed packages with `codec_path("::protobuf_tonic::ProtobufCodec")` and
prost-backed packages in a separate `tonic_prost_build` configuration, then
mount both generated services on the same server. Native applications likewise
combine the normal pbrs native generator with `codegen::prost_stubs`; codec
selection is represented in each generated service's concrete request/response
types and has no runtime branch.

## Qualification limit

The original compatibility test exercises SB26's public boundaries. SB26's
recursive-query, Any-record, sparse-field, map and option corpora have since
landed. [TC32a's pinned actual-API inventory](tc32a-bridge-qualification-20261002.md)
qualifies all 336 direction/mode cells across 84 specimens for semantics;
324 cells pass actual wire guards, and 12 map cells remain blocked by the
existing default-value and order policy. The benchmark-only bridge feature is
optional and leaves the default dependency graph unchanged.

Numeric allocation/byte/instruction costs remain `not_run`.
[TC32b's opt-in parent registration and raw-preserving collectors](tc32-bridge-cost.md)
now retain source/binary/fingerprint guards and reconstruct absolute operation
costs. Actual source-pinned captures and unchanged-binary replay, with all
original controls and failures preserved, are still required before the parent
TC32 acceptance can close. The semantic inventory does not establish
performance or headroom.
