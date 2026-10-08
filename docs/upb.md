# Relative to upb

`pbrs` implements the Google protobuf application API with
`protoc-gen-pbrs` and Rust message structs. The
separate path for Google's unmodified `--rust_out` output is experimental
and incomplete; neither path is a drop-in implementation of the upb C ABI.

The comparison below uses the repository's pinned protobuf 4.x/upb baseline.
See [compatibility status](codegen-compatibility.md) for tested shared
consumers and [the UK plan](decisions/upb-kernel.md) for the proposed
kernel replacement.

crates.io `protobuf` 4.x is Google's Rust API over upb (C) (`links=upb`,
`cc`). A C++ kernel also exists in the protobuf repository. That is not what
Cargo downloads.

`pbrs` implements the application traits of that Rust API: `Parse`,
`Serialize`, `Clear`, `proto!`, `ProtoStr`, `RepeatedView`, and
`DynamicMessage`. Its storage and generated code differ from upb.

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

Historical contiguous-buffer benchmarks found that this overhead shrank on
large payloads, with a loss for pbrs on 5 MiB packed-fixed encoding. Those
results do not describe the newer shared-buffer native transport path. See
[benchmark evidence](benchmarks.md) and [large-payload copies](zero-copy.md)
for the different workloads and measurement limits.

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
| Edition 2024 | Plugin max is 2024 for a qualified subset; repeated/map closed enums are rejected and typed extension accessors remain CG-14b. | Descriptor support, plugin generation, and official rust_out runtime support are separate qualifications; see the [edition contract](edition-2024.md). |
| C++-only string types | Ordinary strings. | `ctype=STRING_PIECE` / `CORD` and `pb.cpp.string_type=VIEW` are stored as ordinary strings. |
| `protobuf_gtest_matchers` | Skip `gtest_matchers_test.rs`. | Not implemented. |
| `__internal == ()` | `__internal` is a module (`SealedInternal`). | Deliberate: generated code needs `SealedInternal`. `no_internal_access_test.rs` does not apply. |
| `proto!` `#[cfg(bzl)]` `::crate::Type` | Skipped. | Bazel-qualified `proto!` paths are not supported. |
| Map representation | Last-wins pairs; maps with at least 16 entries can build a sorted index for reads. upb uses a hash table. | Measure parse, first access, and repeated lookup separately. Official rust_out raw maps use a different representation. |
| Arena lifetime | Plugin messages own their Rust allocations; `parse_bytes` can retain shared `Bytes`. | Sharing a buffer is supported, but caller-managed arena lifetimes and independent borrowed message roots are not the plugin API. |
| cpp kernel, lite runtime, no_std | None. | Not offered. |
| Cargo swap for `protobuf` 4.x | Different package name, gencode, and `__internal`. | Application traits match. Rebuild. |
| Fuzzing | In-tree `fuzz/` targets exercise wire data, descriptors, formats, varints, and gRPC frames. | A checked corpus and smoke test are not a sustained campaign; see the [recorded fuzz run](evidence/fuzz-2026-09-28.md). |
| 2 GiB cap | `MAX_MESSAGE_BYTES = 2^31 - 1`. | Same order as C++. |
| Nested `field.message` on a raw FileDescriptorSet skeleton | Look up by `type_name` in the pool. | Pointers on the skeleton are empty. |

### Views

Google Rust `MessageView` on upb can borrow the arena object. buffa
`decode_view` walks tags and returns field slices into the input buffer.

Our `FooView` borrows a parsed owned message. Lazy fields can share wire
storage, including a caller-supplied `Bytes` buffer, but there is no generated
wire-view root API yet. That design is tracked in the
[borrowed-view proposal](decisions/borrowed-views.md).

### tonic and pbrs-grpc

Google Rust gRPC is not Tonic. Existing Tonic 0.14+ services using Prost cannot
`impl prost::Message` on these types.

The plugin emits `FooClient`/`FooServer` over
`protobuf-tonic::ProtobufCodec`. The protobuf kernel stays Tonic-free.
`pbrs-grpc` is a separate HTTP/2 gRPC crate over the same `Parse` /
`Serialize` types; it does not use Tonic. `compile_protos` requires `protoc`
for source compilation; `compile_descriptor_set` generates from checked
descriptors without it.

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
