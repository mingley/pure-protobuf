# Architecture

pbrs is a protobuf kernel: parse, serialize, reflection, JSON, text, plugin
codegen. No upb, no libprotobuf, no C.

## Crates

| crate | role |
|---|---|
| `pbrs` | kernel, `protoc-gen-pbrs`, conformance child |
| `protobuf-tonic` | tonic 0.14 `Codec` and generated `FooClient` / `FooServer` |

Kernel has no tonic dependency. Stubs do.

Lib and Cargo package are both `pbrs` (`use pbrs::prelude::*`). GitHub repo
is `mingley/pure-protobuf`.

## Parse / encode

```
bytes
  -> Parse::parse
  -> generated merge_inner (tag match, depth <= 100)
  -> field storage (scalars, LazyStr, Packed, LazyMsg, Map, Repeated)
  -> getters materialize lazy slots on first access
```

Encode is the reverse: `CachedSize`, then `write_to` into a `Vec<u8>`. Nested
and packed fields write in place (`encode_len_header` + `write_to`). No
scratch `Vec` per submessage.

JSON and text are not a second codec. Generated `to_json` / `from_json`
serialize to bytes, then `DynamicMessage` transcodes with a `DescriptorPool`.

## Codegen

`protoc-gen-pbrs` is a normal protoc plugin (`--pbrs_out`).
`./scripts/gen.sh` finds or builds it, runs protoc, rustfmts the `.rs` it
wrote.

Generated messages are field-wise Rust structs plus `impl_typed_message!`.
Not `DynamicMessage` wrappers. Not Google `OwnedMessageInner`.

Same-crate `build.rs` cannot invoke the plugin bin. Conformance TestAllTypes
lives in `src/generated/` and is re-exported from `pbrs::gencode`.

## Modules

| module | job |
|---|---|
| `rt` | `CachedSize`, `OptBool`, packed aliases, wire helpers |
| `lazy` | `Wire`, `LazyStr`, `LazyBytes`, `LazyMsg` |
| `packed` | packed scalars; memcpy only for fixed-width |
| `repeated` / `map` | 8-byte empty (`Option<Box<Vec<_>>>`) |
| `dynamic` | `DescriptorPool`, `DynamicMessage` |
| `json` / `text` | WKT + spec codecs on dynamic messages |
| `codegen` | plugin + FileDescriptorSet |
| `gen_support` | `impl_typed_message!`, default instances |

## Conformance process

`src/bin/conformance.rs` speaks the official runner protocol. The runner is
C++ (`conformance_test_runner` at protobuf v35.1). Fetch with
`./scripts/fetch-protobuf.sh`. The protobuf tree is gitignored (~115 MiB).
Pin and rust_upb skip lists live in `vendor/google/` (~304 KiB).
