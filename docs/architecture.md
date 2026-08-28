# Architecture

pbrs is a protobuf kernel: parse, serialize, reflection, JSON, text, and
plugin codegen. There is no upb, no libprotobuf, and no C.

## Crates

| crate | role |
|---|---|
| `pbrs` | protobuf kernel, `protoc-gen-pbrs`, conformance child |
| `protobuf-tonic` | tonic 0.14 `Codec` and generated `FooClient` / `FooServer` |
| `pbrs-grpc` | HTTP/2 gRPC kernel over pbrs (not tonic) |

The protobuf kernel has no tonic, h2, or hyper dependency. `pbrs-grpc` has no tonic dependency. `protobuf-tonic` has no `pbrs-grpc` dependency. A consumer can use pbrs alone, pbrs plus the tonic adapter, or pbrs plus the gRPC kernel.

The Cargo package and the library are both named `pbrs`
(`use pbrs::prelude::*`). The GitHub repo is `mingley/pure-protobuf`.

## Parse / encode

1. Bytes enter through `Parse::parse`.
2. Generated `merge_inner` matches tags, with depth at most 100.
3. Values land in field storage: scalars, `LazyStr`, `Packed`, `LazyMsg`,
   `Map`, `Repeated`.
4. Getters materialize lazy slots on first access.

Encode is the reverse. `CachedSize` is filled first, then `write_to` writes
into a `Vec<u8>`. Nested and packed fields write in place
(`encode_len_header` + `write_to`). There is no scratch `Vec` per
submessage.

Generated proto3 JSON and text are field-wise for messages whose fields
are proto3 scalars (int32, int64, uint32, uint64, sint32, sint64,
fixed32, fixed64, sfixed32, sfixed64, bool, float, double, string,
bytes), open proto3 enums, repeated and scalar maps of those types,
nested messages of that set, real oneofs of that set (`Person`,
`hello`, `ExtraScalars`, `OneofHole`), or `google.protobuf.Timestamp`
/ `Duration` / `Empty` / proto3 wrappers / `FieldMask` (official
proto3 JSON: Timestamp / Duration strings; Empty is `{}`; wrappers
are the wrapped value, not an object; FieldMask is a
comma-separated path string; text is the existing field mapping).
Map-of-enum is skipped (map-entry descriptors lack enum names at
codegen). Other WKT (Struct, Value, ListValue, Any) and TAT still
serialize to bytes and transcode through `DynamicMessage`. TAT is
not closed.

## Codegen

`protoc-gen-pbrs` is a normal protoc plugin (`--pbrs_out`).
`./scripts/gen.sh` finds or builds it, runs protoc, and rustfmts the `.rs`
it wrote.

Generated messages are field-wise Rust structs plus `impl_typed_message!`.
They are not `DynamicMessage` wrappers and not Google `OwnedMessageInner`.

A same-crate `build.rs` cannot invoke the plugin binary. Conformance
TestAllTypes lives in `src/generated/` and is re-exported from
`pbrs::gencode`.

## Modules

| module | job |
|---|---|
| `rt` | `CachedSize`, `OptBool`, packed aliases, wire helpers |
| `lazy` | `Wire`, `LazyStr`, `LazyBytes`, `LazyMsg` |
| `packed` | packed scalars; memcpy only for fixed-width |
| `repeated` / `map` | 8-byte empty (`Option<Box<Vec<_>>>`) |
| `dynamic` | `DescriptorPool`, `DynamicMessage` |
| `json` / `text` | WKT + spec codecs on dynamic messages; field-wise generated helpers for Person-shaped proto3, extra proto3 scalars, real oneofs of that set, Timestamp / Duration, Empty, proto3 wrappers, and FieldMask |
| `codegen` | plugin + FileDescriptorSet |
| `gen_support` | `impl_typed_message!`, default instances |

## Conformance process

`src/bin/conformance.rs` speaks the official runner protocol. The runner
is C++ (`conformance_test_runner` at protobuf v35.1). Fetch it with
`./scripts/fetch-protobuf.sh`. The protobuf tree is gitignored (~115 MiB).
Pin and rust_upb skip lists live in `vendor/google/` (~304 KiB).
