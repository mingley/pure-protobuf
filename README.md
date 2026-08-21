# pbrs

Pure-Rust Protocol Buffers **kernel** with the Google protobuf **v4 application API**
(`Parse` / `Serialize` / `proto!` / `ProtoStr` / `RepeatedView` / `DynamicMessage`).

Repo: [`mingley/pure-protobuf`](https://github.com/mingley/pure-protobuf). Cargo package and lib
are `pbrs` (`use pbrs::prelude::*`). Not Google’s crates.io `protobuf` 4.x, not prost, not
[`pb-rs`](https://crates.io/crates/pb-rs) (quick-protobuf).

```toml
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
```

## How this differs

| | **pbrs** (this repo) | crates.io `protobuf` **4.x** | **prost** | **buffa** (Anthropic) | rust-protobuf **3.x** |
|---|---|---|---|---|---|
| Runtime | Pure Rust | **upb (C)** via `cc` (`links=upb`) | Pure Rust | Pure Rust | Pure Rust |
| Public API | Google v4 *application* traits | Google v4 + upb internals | prost derive / `Message` | Own API, views/lazy | stepancheg v3 |
| `protoc --rust_out` gencode | Will **not** link (`OwnedMessageInner`) | Yes (upb minitables) | N/A (prost-build) | Own codegen | Own codegen |
| Codegen here | `protoc-gen-pbrs` | Google’s rust plugin | `prost-build` / `tonic-prost-build` | buffa plugin | `protoc-gen-rs` |
| Editions | What the pinned conformance runner executes | Official | No | First-class | proto2/3 |
| gRPC | Plugin-generated `FooClient`/`FooServer` + `protobuf-tonic` codec (not `tonic-prost`) | Not tonic’s default | **tonic-prost** (default tonic) | Separate | Separate |
| crates.io | `pbrs` (unpublished) | Google owns **4.x-release** as `protobuf` | `prost` | `buffa` | still Cargo’s *stable* `protobuf` **3.7.2** |

`cargo add protobuf` still resolves **stepancheg 3.7.2**. Google v4 is a prerelease (`4.x.y-release`). Neither is this crate.

Google’s generated Rust (`protoc --rust_out kernel=upb`) calls `::protobuf::__internal` / `OwnedMessageInner`. That gencode is hard-wired to upb. **Regenerate with this plugin** (or use `DynamicMessage`). Do not point `protoc --rust_out` at this crate.

## Generate

`protoc` must be on `PATH`. From this repo:

```bash
./scripts/gen.sh -I proto -o gen proto/your.proto
```

Same thing by hand (`PBRS_PLUGIN` or `PATH` can point at `protoc-gen-pbrs`):

```bash
protoc \
  --plugin=protoc-gen-pbrs=target/debug/protoc-gen-pbrs \
  --pbrs_out=gen \
  -I proto proto/your.proto
```

Generated types store fields directly (not a `DynamicMessage` wrapper). JSON/text go through a shared descriptor codec. They implement `Parse` + `Serialize`. They do **not** implement `prost::Message`.

## tonic

tonic’s default stack is **prost** (`tonic-prost`). These types cannot implement `prost::Message`.

The plugin emits `FooClient` / `FooServer` (unary + streaming) that use `protobuf-tonic::ProtobufCodec`. Tonic is only HTTP/2 + framing. Kernel stays tonic-free. **tonic 0.14+ only** (adapter MSRV 1.88). See `protobuf-tonic/README.md`.

```toml
pbrs = { git = "https://github.com/mingley/pure-protobuf" }
protobuf-tonic = { git = "https://github.com/mingley/pure-protobuf" }
tonic = { version = "0.14", default-features = false, features = ["transport", "codegen", "router"] }
```

## Status

Toy / research kernel, but gated:

- proto2 / proto3 / editions **binary**, **JSON**, and **text** via official `conformance_test_runner` (protobuf v35.1): required **5631** binary+JSON + **909** text, 0 unexpected failures. `--enforce_recommended` also 0 unexpected failures.
- Plugin gencode is per-field storage (`protoc-gen-pbrs`), including TestAllTypes. Conformance drives those types.
- `RECURSION_LIMIT = 100` (deeper parse returns `Err`, no abort)
- `protobuf-tonic`: plugin-generated `GreeterClient`/`GreeterServer` unary + bidi streaming echo (tonic **0.14+**, not `tonic-prost`)
- No crates.io publish yet (`pbrs` is free; do not publish as `protobuf`)

## Benchmarks

Same schema on every row: plugin-generated types vs `prost-build` of the same `.proto`, typed crates.io `protobuf` **4.35.1-release** (`protoc --rust_out kernel=upb`), and buffa **0.9.1** (owned + `decode_view`). Decode uses this crate’s wire bytes. `./bench` 40 000 iters, median of 15 samples after warmup. `size_of::<TestAllTypesProto3>()` = **616** (`Default` ~16 ns). `./bench` exits non-zero if this kernel loses encode or owned decode on any case, including vs buffa view.

Encode ns / decode ns (from a real `./target/release/bench` JSON capture):

| case | ours | prost | v4 upb | buffa owned | buffa view |
|---|---:|---:|---:|---:|---:|
| empty TAT 0 B | **23 / 17** | 83 / 138 | 155 / 83 | 73 / 174 | — / 115 |
| person 62 B | **36 / 72** | 41 / 202 | 80 / 170 | 41 / 163 | — / 82 |
| TAT populated 87 B | **82 / 306** | 151 / 454 | 244 / 411 | 135 / 408 | — / 308 |
| packed_256 388 B | **67 / 256** | 541 / 815 | 476 / 896 | 583 / 378 | — / 376 |
| map_64 500 B | **255 / 969** | 675 / 2435 | 1068 / 3206 | 422 / 1753 | — / 1073 |
| nested_8 26 B | **222 / 143** | 2339 / 1117 | 1174 / 324 | 664 / 1353 | — / 1442 |
| strings 163 B | **63 / 171** | 123 / 353 | 194 / 176 | 109 / 332 | — / 198 |

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
