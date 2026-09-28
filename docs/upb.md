# Relative to upb

This page compares `pbrs` with Google's Rust protobuf 4.x runtime over upb. It is for Rust developers deciding whether this project is a drop-in runtime, a codegen path, or a benchmark subject. The bottom line: `pbrs` matches the application traits but does not implement the upb C kernel.

crates.io `protobuf` 4.x is Google's Rust API over upb (C) (`links=upb`,
`cc`). A C++ kernel also exists in the protobuf repository. That is not what
Cargo downloads.

`pbrs` implements the application traits of that Rust API: `Parse`,
`Serialize`, `Clear`, `proto!`, `ProtoStr`, `RepeatedView`, and
`DynamicMessage`. It does this in pure Rust. It does not implement the upb
kernel.

## Same job, different object

| Area | pbrs | crates.io protobuf 4.x |
|---|---|---|
| Object | Field-wise Rust struct. | `OwnedMessageInner { ptr, arena }`. |
| Parse | Generated `merge_inner`. | FFI `upb_Decode` + minitable. |
| Serialize | `write_to` into `Vec<u8>`. | New Arena, FFI `upb_Encode`, then `slice.to_vec()`. |
| Accessors | Struct fields and lazy slots. | FFI into the arena object. |
| Codegen | `protoc-gen-pbrs`. | `protoc --rust_out kernel=upb`. |
| Link of Google rust_out | MiniTable stand-in (`src/runtime.rs`); `rust_out_person`. | yes (upb C). |

v4 encode looks slow on small messages because every `serialize` allocates an
encode arena, calls into C, and copies the result into a Rust `Vec`. upb C is
not the bottleneck there. See `third_party/protobuf/rust/upb/wire.rs` after
running `./scripts/fetch-protobuf.sh`.

That overhead shrinks on large payloads. At 1-5 MiB, owned encode/decode of a
bytes blob is memcpy-bound. pbrs, v4, and buffa owned sit in the same band.
Packed-fixed 5 MiB encode is a bit faster on v4. See `docs/benchmarks.md`.

## Tests we share with rust_upb

1. Official `conformance_test_runner` v35.1: 5631 binary+JSON + 909 text,
   0 unexpected. `--enforce_recommended` also reports 0 unexpected. rust_upb
   still ships `failure_list_rust_upb.txt` for proto2 UTF-8. We do not skip it.
2. `rust/test/shared` behaviors, ported in `tests/google_shared.rs` as
   38 cargo tests against plugin-generated types.

`upb/test/*.cc` and `rust/test/upb/` test C, minitable, and arena internals.
They are not applicable and are not vendored.

## Gaps vs the upb kernel and vs Google rust_out

| Gap | Instead | Consequence |
|---|---|---|
| Google `protoc --rust_out` | `protoc-gen-pbrs`, or `pbrs::codegen::compile_protos` | Official rust_out 4.35.1-release of `person.proto` links as `protobuf` through `src/runtime.rs` (`rust_out_person`). Plugin gencode is the application path. |
| JSON / text specialized in upb/C++ | Field-wise JSON and text for Person-shaped proto3, extra proto3 scalars (bool / int64 / uint32 / uint64 / sint / fixed / sfixed / float / double / bytes / open enums, including repeated and scalar maps), real oneofs of that set (`OneofHole`), and `google.protobuf.Timestamp` / `Duration` / `Empty` / proto3 wrappers (official proto3 JSON: strings / `{}` / wrapped value). Map-of-enum is skipped because map-entry descriptors lack enum names at codegen. TAT and the other WKT (Struct, Value, ListValue, Any, FieldMask) still serialize, then `DynamicMessage`. | Correct for conformance. Not a JSON/text microbench winner. TAT is not closed. |
| Edition 2024 extensions (`extensions.proto` in rust/test) | Plugin max is 2023; `extensions_test.rs` is an empty stub. | Proto2 extensions on dynamic messages work (`tests/json_text_ext.rs`). |
| C++-only string types | Ordinary strings. | `ctype=STRING_PIECE` / `CORD` and `pb.cpp.string_type=VIEW` are stored as ordinary strings. |
| `protobuf_gtest_matchers` | Skip `gtest_matchers_test.rs`. | Not implemented. |
| `__internal == ()` | `__internal` is a module (`SealedInternal`). | Deliberate: generated code needs `SealedInternal`. `no_internal_access_test.rs` does not apply. |
| `proto!` `#[cfg(bzl)]` `::crate::Type` | Skipped. | Bazel-qualified `proto!` paths are not supported. |
| Map representation | Last-wins `Vec` plus lazy index on `get` / `remove` / unique `len`; upb uses a hash table. | Fine at map_64. Not a codec win. Huge hot lookup no longer scans. |
| Arena lifetime | Drop frees Rust allocations. | You cannot parse into a caller-owned buffer and keep submessages alive by keeping that buffer. |
| cpp kernel, lite runtime, no_std | None. | Not offered. |
| Cargo swap for `protobuf` 4.x | Different package name, gencode, and `__internal`. | Application traits match. Rebuild. |
| Fuzzing | Conformance + cargo tests only. | Not in-tree. |
| 2 GiB cap | `MAX_MESSAGE_BYTES = 2^31 - 1`. | Same order as C++. |
| Nested `field.message` on a raw FileDescriptorSet skeleton | Look up by `type_name` in the pool. | Pointers on the skeleton are empty. |

### Views

Google Rust `MessageView` on upb can borrow the arena object. buffa
`decode_view` walks tags and returns field slices into the input buffer.

Our `FooView` is `&Owned` after copy. `LazyStr` can still point into the parse
`Arc<[u8]>` until `set_*`. That is not a first-class view type.

### tonic and pbrs-grpc

Google Rust gRPC is not Tonic. Existing Tonic 0.14+ services using Prost cannot
`impl prost::Message` on these types.

The plugin emits `FooClient`/`FooServer` over
`protobuf-tonic::ProtobufCodec`. The protobuf kernel stays Tonic-free.
`pbrs-grpc` is a separate HTTP/2 gRPC crate over the same `Parse` /
`Serialize` types; it does not use Tonic. `protoc` is still required at codegen
time.

### Layout specialization

`packed_fixed32`, `packed_fixed64`, `packed_float`, and
`repeated_nested_message` are on the TAT hot struct. Remaining packed and
unpacked scalars stay in `Cold`.

Growing every memcpy-packed slot onto hot (TAT 824) lost `strings` vs v4. A
648-byte layout still wins that row. The split is shaped around TestAllTypes,
not a general overlay kernel.

## Already matching rust_upb

- Recommended conformance passes without rust_upb's skip list.
- Plugin TestAllTypes is driven by the runner.
- Recursion limit is 100, matching upb/prost/C++.
- Unknown fields round-trip.
- Truncated packed is a parse error.
- Delimited messages are groups.
- Proto3 explicit presence holds on oneofs / optional.
- Application-level encode/decode of the same-schema `./bench` suite vs the
  crates.io rust+upb wrapper includes packed-fixed32/64/float and unpacked 256.
