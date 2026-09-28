# Better tonic: product shape and names (TC-01)

This decision is for contributors working on tonic migration and parity. It
answers which tonic adoption paths ship, under what names, and in which order.
Decision: ship all four paths, keep every published crate name, and add new
behavior as opt-in features so default dependency graphs do not grow.

Decided 2026-09-28 by the maintainer's direction to reach full tonic parity for
code generation, client and server.

## Adoption paths

| Path | What users get | Where it lives | Default? |
|---|---|---|---|
| (a) tonic-shaped API mode | Generated client/server signatures that mirror tonic (`Request`, `Response`, `Status`, `Streaming`, async-trait style), so handlers port by changing imports and builder setup. | Codegen option in `pbrs` (`src/codegen/compat_stubs.rs`) plus runtime shims in `pbrs-grpc` (`src/compat/`). | No. Native stubs stay the default. |
| (b) tower integration | A `Router` usable as `tower::Service<http::Request<B>>` for axum/hyper co-hosting, and tower layers around a `Channel`. | `pbrs-grpc` optional `tower` feature (`src/tower_server.rs`, `src/tower_client.rs`). | No. Off by default; no buffer in the default client path. |
| (c) prost messages over the native transport | A codec trait in `pbrs-grpc` with pbrs messages as the fast default, plus an optional prost codec and stub generator. | `pbrs-grpc` codec trait (`src/codec.rs`), optional `prost` feature. | Codec trait yes (pbrs default); prost support opt-in. |
| (d) `protobuf-tonic` adapter | tonic 0.14+ with pbrs messages. | Existing crate. | Unchanged. |

## Names

- Keep `pbrs`, `pbrs-grpc` and `protobuf-tonic`. Renaming a published crate
  (for example to `pbrs-tonic`) forces a consumer migration and a second
  package for little gain before 1.0. Revisit only at a 1.0 cut.
- No new crates for (a)-(c). They are modules and features of existing crates,
  which keeps the three-crate release shape.

## Semver and release impact

- All additions are additive: new optional features, new codegen options, and a
  codec trait whose default keeps existing generated code compiling.
- `pbrs-grpc` stays on the `0.1.0-alpha.N` line; `pbrs` gains options in a
  minor release.
- Optional features must pass the pure-Rust dependency audit (`dep-audit`).
  `tower`, `http`, `http-body` and `prost` are pure Rust.

## Migration story

| Starting point | Recommended path |
|---|---|
| tonic + prost | (c): keep prost messages and switch the transport; or (a) after regenerating with pbrs messages. |
| tonic + pbrs | (d) today; (a) to drop tonic. |
| grpc-rust (Google `grpc` preview) | (a) or native stubs; message types regenerate with pbrs. |

## Consequences

- Unblocks TC-02 (codec trait), TC-03 (prost), TC-04/TC-05 (tower) and TC-06
  (tonic-shaped mode).
- gRPC-Web and Connect scope is a separate decision (TC-09).
