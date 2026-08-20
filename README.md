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

Same schema on every row: plugin-generated types vs `prost-build` of the same `.proto`, typed crates.io `protobuf` **4.35.1-release** (`protoc --rust_out kernel=upb`), and buffa **0.9.1** (owned + `decode_view`). Decode uses this crate’s wire bytes. `./bench` 40 000 iters, median of 15 samples after warmup. `size_of::<TestAllTypesProto3>()` = **616** (`Default` ~16 ns). `./bench` exits non-zero if this kernel loses encode or owned decode on any case, including vs buffa view.

Encode ns / decode ns (from a real `./target/release/bench` JSON capture):

| case | ours | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|
| empty TAT 0 B | **24 / 18** | 81 / 129 | 148 / 81 | 71 / 168 | — / 117 |
| person 62 B | **37 / 101** | 39 / 193 | 73 / 158 | 39 / 153 | — |
| TAT populated 87 B | **75 / 294** | 186 / 441 | 235 / 404 | 131 / 414 | — / 306 |
| packed_256 388 B | **64 / 255** | 533 / 819 | 469 / 874 | 579 / 379 | — / 393 |
| map_64 500 B | **251 / 929** | 669 / 2337 | 977 / 3133 | 407 / 1900 | — / 1102 |
| nested_8 26 B | **214 / 143** | 2313 / 1037 | 1110 / 317 | 622 / 1338 | — / 1416 |
| strings 163 B | **61 / 150** | 120 / 327 | 186 / 167 | 102 / 320 | — / 190 |

v4 encode is slow on small messages because every `serialize` allocates a fresh upb `Arena`, FFI `upb_Encode`, then copies the arena buffer into a Rust `Vec`. See `third_party/protobuf/rust/upb/wire.rs`.

## Conformance (optional)

The suite rust_upb runs for wire/JSON/text **behavior** is official `conformance_test_runner` (v35.1). Test cases are generated in C++ inside that binary, not data files. Pin, rust_upb failure lists, and Google rust `shared/` tests live in `vendor/google/` (~304 KiB). The full protobuf tree needed to *build* the runner is gitignored (~115 MiB):

```bash
./scripts/fetch-protobuf.sh   # clones protocolbuffers/protobuf @ v35.1
./scripts/conformance.sh      # builds runner, runs required twice
```

`vendor/google/conformance/failure_list_rust_upb.txt` is rust_upb’s recommended proto2 UTF-8 skip list. This crate currently passes `--enforce_recommended` without it.

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
