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
| gRPC | `protobuf-tonic` adapter (not `tonic-prost`) | Not tonic’s default | **tonic-prost** (default tonic) | Separate | Separate |
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

Generated types wrap `DynamicMessage` (one binary/JSON/text codec). They implement `Parse` + `Serialize`. They do **not** implement `prost::Message`.

## tonic

tonic’s default stack is **prost** (`tonic-prost`). These types cannot implement `prost::Message`.

Use the `protobuf-tonic` crate in this repo: `ProtobufCodec<Encode, Decode>` over this kernel. Tonic is only HTTP/2 + framing. See `protobuf-tonic/README.md`. The adapter is currently proven on **tonic 0.12** (unary echo); it is not a crate from the tonic workspace.

```toml
protobuf = { package = "pure-protobuf", git = "https://github.com/mingley/pure-protobuf" }
protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }
tonic = { version = "0.12", default-features = false, features = ["transport", "codegen"] }
```

## Status

Toy / research kernel, but gated:

- proto2 / proto3 / editions **binary**, **JSON**, and **text** via official `conformance_test_runner` (protobuf v35.1): required **5565** binary+JSON + **907** text, 0 unexpected failures
- `RECURSION_LIMIT = 100` (deeper parse returns `Err`, no abort)
- Person-sized encode/decode benches beat prost and typed protobuf v4 upb on this machine (hand-written `testdata::Person`, not the generated `DynamicMessage` wrappers)
- No crates.io publish (the name `protobuf` is not ours)

Recommended-only JSON gaps remain (duplicate keys, base64url, …). Beating buffa is not a goal and was not measured.

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
