# pure-protobuf

Pure-Rust Protocol Buffers **kernel** with the Google protobuf **v4 application API**
(`Parse` / `Serialize` / `proto!` / `ProtoStr` / `RepeatedView` / `DynamicMessage`).

It is a stand-in for *application* code that would have used Google’s Rust `protobuf` crate.
It is **not** Google’s crates.io `protobuf` 4.x, and it is **not** prost.

```toml
# lib name stays `protobuf` so `use protobuf::prelude::*` still works.
protobuf = { package = "pure-protobuf", git = "https://github.com/mingley/pure-protobuf" }
```

## How this differs

| | **pure-protobuf** (this repo) | crates.io `protobuf` **4.x** | **prost** | **buffa** (Anthropic) | rust-protobuf **3.x** |
|---|---|---|---|---|---|
| Runtime | Pure Rust | **upb (C)** via `cc` (`links=upb`) | Pure Rust | Pure Rust | Pure Rust |
| Public API | Google v4 *application* traits | Google v4 + upb internals | prost derive / `Message` | Own API, views/lazy | stepancheg v3 |
| `protoc --rust_out` gencode | Will **not** link (`OwnedMessageInner`) | Yes (upb minitables) | N/A (prost-build) | Own codegen | Own codegen |
| Codegen here | `protoc-gen-pure-protobuf` | Google’s rust plugin | `prost-build` / `tonic-prost-build` | buffa plugin | `protoc-gen-rs` |
| Editions | What the pinned conformance runner executes | Official | No | First-class | proto2/3 |
| gRPC | Plugin-generated `FooClient`/`FooServer` + `protobuf-tonic` codec (not `tonic-prost`) | Not tonic’s default | **tonic-prost** (default tonic) | Separate | Separate |
| crates.io name `protobuf` | Do not publish as `protobuf` | Google owns **4.x-release** | `prost` | `buffa` | still Cargo’s *stable* `protobuf` **3.7.2** |

`cargo add protobuf` still resolves **stepancheg 3.7.2**. Google v4 is a prerelease (`4.x.y-release`). Neither is this crate.

Google’s generated Rust (`protoc --rust_out kernel=upb`) calls `::protobuf::__internal` / `OwnedMessageInner`. That gencode is hard-wired to upb. **Regenerate with this plugin** (or use `DynamicMessage`). Do not point `protoc --rust_out` at this crate.

## Generate

```bash
protoc \
  --plugin=protoc-gen-pure-protobuf=target/debug/protoc-gen-pure-protobuf \
  --pure-protobuf_out=. \
  -I proto proto/your.proto
```

Generated types store fields directly (not a `DynamicMessage` wrapper). JSON/text go through a shared descriptor codec. They implement `Parse` + `Serialize`. They do **not** implement `prost::Message`.

## tonic

tonic’s default stack is **prost** (`tonic-prost`). These types cannot implement `prost::Message`.

The plugin emits `FooClient` / `FooServer` (unary + streaming) that use `protobuf-tonic::ProtobufCodec`. Tonic is only HTTP/2 + framing. Kernel stays tonic-free. **tonic 0.14+ only** (adapter MSRV 1.88). See `protobuf-tonic/README.md`.

```toml
protobuf = { package = "pure-protobuf", git = "https://github.com/mingley/pure-protobuf" }
protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen", "router"] }
```

## Status

Toy / research kernel, but gated:

- proto2 / proto3 / editions **binary**, **JSON**, and **text** via official `conformance_test_runner` (protobuf v35.1): required **5631** binary+JSON + **909** text, 0 unexpected failures. `--enforce_recommended` also 0 unexpected failures.
- Plugin gencode is per-field storage (`protoc-gen-pure-protobuf`), including TestAllTypes. Conformance drives those types.
- `RECURSION_LIMIT = 100` (deeper parse returns `Err`, no abort)
- `protobuf-tonic`: plugin-generated `GreeterClient`/`GreeterServer` unary + bidi streaming echo (tonic **0.14+**, not `tonic-prost`)
- No crates.io publish (the name `protobuf` is not ours)

## Benchmarks

Plugin-generated `TestAllTypesProto3` (optional scalars, nested message, `repeated_int32`, `map_int32_int32`, `packed_int32`) vs prost 0.13, typed crates.io `protobuf` **4.35.1-release** (`protoc --rust_out kernel=upb`), and buffa **0.9.1** generated TAT. Same machine, two consecutive `./target/release/bench` runs. Decode uses this crate’s wire bytes. 40 000 iters, median of 9 samples.

| | ours | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|
| payload bytes | 87 | 83 | 87 | 87 | 87 |
| encode ns (run 1) | 89.796 | 81.441 | 240.378 | 133.917 | — |
| encode ns (run 2) | 90.931 | 94.895 | 245.297 | 135.204 | — |
| decode ns (run 1) | 380.381 | 280.790 | 420.131 | 404.357 | 309.423 |
| decode ns (run 2) | 379.753 | 281.739 | 419.951 | 412.169 | 311.275 |

Both runs: ours faster than typed v4 **and** buffa **owned** on encode and decode. prost still wins decode. buffa’s zero-copy `decode_view` is faster than our owned decode (we materialize fields).

## Conformance (optional)

The official runner is not vendored. From a protobuf **v35.1** tree:

```bash
cmake -S third_party/protobuf -B target/conformance-build
cmake --build target/conformance-build --target conformance_test_runner
cargo build --release --bin conformance
./target/conformance-build/conformance_test_runner --maximum_edition 2023 target/release/conformance
```

`build.rs` expects `third_party/protobuf` for the descriptor set used by the conformance binary (clone tag `v35.1` there).

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
