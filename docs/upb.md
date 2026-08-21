# Relative to upb

crates.io `protobuf` 4.x is Google's rust API over **upb (C)** (`links=upb`,
`cc`). A C++ kernel exists in the protobuf repo. It is not what Cargo
downloads.

pbrs implements the **application** traits of that rust API
(`Parse` / `Serialize` / `Clear` / `proto!` / `ProtoStr` / `RepeatedView` /
`DynamicMessage`) in pure Rust. It does not implement the upb kernel.

## Same job, different object

| | pbrs | crates.io protobuf 4.x |
|---|---|---|
| Object | field-wise Rust struct | `OwnedMessageInner { ptr, arena }` |
| Parse | generated `merge_inner` | FFI `upb_Decode` + minitable |
| Serialize | `write_to` into `Vec<u8>` | new Arena, FFI `upb_Encode`, `slice.to_vec()` |
| Accessors | struct fields / lazy slots | FFI into the arena object |
| Codegen | `protoc-gen-pbrs` | `protoc --rust_out kernel=upb` |
| Link of Google rust_out | no (`OwnedMessageInner`) | yes |

v4 encode looks slow on small messages because every `serialize` allocates
an encode arena, calls into C, then copies the result into a Rust `Vec`.
upb C is not the bottleneck there. See
`third_party/protobuf/rust/upb/wire.rs` after `./scripts/fetch-protobuf.sh`.

On large payloads that tax shrinks. We have not claimed a win at tens of
KiB.

## Tests we share with rust_upb

1. Official `conformance_test_runner` v35.1: **5631** binary+JSON + **909**
   text, 0 unexpected. `--enforce_recommended` also 0 unexpected. rust_upb
   still ships `failure_list_rust_upb.txt` (proto2 UTF-8). We do not skip it.
2. `rust/test/shared` behaviors, ported in `tests/google_shared.rs` (38
   cargo tests) against plugin-generated types.

`upb/test/*.cc` and `rust/test/upb/` are C/minitable/arena internals. Not
applicable. Not vendored.

## Gaps vs the upb kernel and vs Google rust_out

These are real. Do not sell them as done.

**Will not link Google rust_out.** Gencode calls `::protobuf::__internal` and
`OwnedMessageInner`. Regenerate with `protoc-gen-pbrs`.

**Views.** Google rust `MessageView` on upb can borrow the arena object.
buffa `decode_view` walks tags and returns field slices into the input
buffer. Our `FooView` is `&Owned` after copy. `LazyStr` can still point into
the parse `Arc<[u8]>` until `set_*`. That is not a first-class view type.

**JSON / text.** Specialized in upb/C++. Here: serialize, then
`DynamicMessage`. Correct (conformance). Not a JSON microbench winner.

**Edition 2024 extensions.** `extensions.proto` in rust/test is edition
2024. Plugin max is 2023. `extensions_test.rs` is an empty stub. Proto2
extensions on dynamic messages work (`tests/json_text_ext.rs`).

**C++-only string types.** `ctype=STRING_PIECE` / `CORD` and
`pb.cpp.string_type=VIEW` are stored as ordinary strings.

**`protobuf_gtest_matchers`.** Not implemented. Skip
`gtest_matchers_test.rs`.

**`__internal`.** Google test `no_internal_access_test.rs` asserts
`__internal == ()`. Ours is a module. Deliberate: generated code needs
`SealedInternal`.

**`proto!` Bazel paths.** `#[cfg(bzl)]` `::crate::Type` form is skipped.

**Map representation.** `Vec<(K,V)>`, last-wins on parse, scan on `get`.
upb uses a hash table. Fine at map_64. Wrong if maps are huge and hot on
lookup.

**No arena lifetime.** You cannot parse into a caller-owned buffer and keep
submessages alive by keeping that buffer. Drop frees Rust allocations.

**No cpp kernel, no lite runtime, no no_std.**

**Not a Cargo swap for `protobuf` 4.x.** Different package name, different
gencode, different `__internal`. Application traits match. Rebuild.

**tonic.** Google rust gRPC is not tonic. Existing tonic 0.14+ services
using prost cannot `impl prost::Message` on these types. Plugin emits
`FooClient`/`FooServer` over `protobuf-tonic::ProtobufCodec`. Kernel stays
tonic-free. `protoc` still required at codegen time.

**Fuzzing.** Not in-tree. Conformance + cargo tests only.

**2 GiB cap.** `MAX_MESSAGE_BYTES = 2^31 - 1`, same order as C++.

**FileDescriptorSet.** Nested `field.message` pointers on a raw FDS skeleton
are empty. Look up by `type_name` in the pool.

**Layout specialization.** `packed_fixed32` is on the hot struct. Other
memcpy-packed `packed_*` fields stay in `Cold`, so packed-fixed64 / packed
float decode still pays a Cold malloc and loses to v4 on those benches.
This is a TAT-shaped choice, not a general overlay kernel.

## What is not a gap

Passing recommended conformance without rust_upb's skip list. Plugin
TestAllTypes driven by the runner. Recursion limit 100 (upb/prost/C++).
Unknown field round-trip. Packed truncated = parse error. Delimited
messages as groups. Proto3 explicit presence on oneofs / optional.
Application-level encode/decode of the same-schema `./bench` suite vs the
crates.io rust+upb wrapper, including packed-fixed and unpacked 256.
