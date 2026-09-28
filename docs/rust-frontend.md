# Rust `.proto` frontend review (CG-16)

**Review date:** 2026-09-23. **Repository source:** `d34334e1`. **Decision
status:** blocked, not an approved dependency or implemented build path.
`Config::compile_descriptor_set` already generates from checked descriptors
without `protoc`; compiling new `.proto` source does not. The codegen plugin
still advertises maximum Edition 2023 (`1000`). Edition 2024 (`1001`) descriptor
resolution does not enable Edition 2024 generated consumers.

## Contract for a full-profile frontend

The frontend must produce a deterministic, import-complete
`FileDescriptorSet` with source locations and extension/custom options intact.
It must handle proto2, proto3, Editions 2023 and 2024, public and option-only
imports, include-relative file identity, same-stem files, visibility,
naming and feature inheritance, and the positive and rejected
[Edition 2024 fixtures](../tests/fixtures/edition2024/README.md). The resulting
bytes and generated files must be compared with pinned `protoc` for
[multi-file layout](codegen-layout.md) and
[generator parity](../tests/pbrs_build.rs). A core build dependency must meet
`pbrs`'s Rust 1.85 minimum, license policy, and the shipping Rust-only/no-FFI
boundary across its *transitive* graph.

## Reviewed source, not an integration proof

| Candidate | Verified useful surface | Full-profile gap |
|---|---|---|
| [`protox` 0.9.1](https://github.com/andrewhickman/protox/tree/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0), 2025-12-02 | Its [`Compiler`](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox/src/compile/mod.rs#L93-L109) offers imports and source locations. Raw [`encode_file_descriptor_set`](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox/src/compile/mod.rs#L186-L240) preserves extension options in an [upstream test](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox/tests/compiler.rs#L278-L335). The direct [manifest](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox/Cargo.toml#L1-L43) declares MIT OR Apache-2.0 and MSRV 1.74. | The released [grammar accepts proto2/proto3 only](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox-parse/src/parse/mod.rs#L62-L76); its [imports](https://github.com/andrewhickman/protox/blob/8a9e5a9b80e76a406c67c37b5ba08f321f851bd0/protox-parse/src/parse/mod.rs#L205-L240) lack `option`. A [2026-07 main revision](https://github.com/andrewhickman/protox/blob/3e33003f90ba0708e996d9d95a9efaeeca2ca320/protox-parse/src/ast/mod.rs#L11-L16) still models only those syntaxes. Import-closure and same-stem fixture parity were not run here. |
| [`protobuf-parse` 3.7.2](https://github.com/stepancheg/rust-protobuf/tree/4cb84f305c05f0376ff51b555a2740c5251c1280), 2025-03-10 | [`Parser::pure`](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/parser.rs#L26-L68) avoids `protoc`; conversion records [public imports and extensions](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/pure/convert/mod.rs#L608-L656). Its direct [manifest](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/Cargo.toml#L1-L26) declares MIT, but no MSRV. | Its [grammar is proto2/proto3-only](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/pure/parser.rs#L479-L519); the bundled [descriptor lacks editions](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/proto/google/protobuf/descriptor.proto#L61-L91). The convenient [FDS API omits imports](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/parser.rs#L100-L125), the [lower-level result is unstable](https://github.com/stepancheg/rust-protobuf/blob/4cb84f305c05f0376ff51b555a2740c5251c1280/protobuf-parse/src/lib.rs#L1-L12), and the pure conversion does not populate source locations. |
| [`protofish` 0.5.3](https://github.com/Rantanen/protofish/tree/858e54fe68571bf02d85ebeb7117b9e23a196d49), version-bump commit (not a release tag), 2025-12-06 | Its [direct manifest](https://github.com/Rantanen/protofish/blob/858e54fe68571bf02d85ebeb7117b9e23a196d49/Cargo.toml#L1-L20) declares MIT OR Apache-2.0, without an MSRV. | A decoding-context library, not an FDS compiler: its [grammar requires proto3](https://github.com/Rantanen/protofish/blob/858e54fe68571bf02d85ebeb7117b9e23a196d49/src/proto.pest#L50-L97), while [`Context::parse` ignores imports and file options](https://github.com/Rantanen/protofish/blob/858e54fe68571bf02d85ebeb7117b9e23a196d49/src/context/parse.rs#L14-L63). |

These are direct-source observations, **not** a dependency approval or a
transitive license/MSRV/FFI audit. No candidate was installed or compiled
against this repository.

## Bounded route if a reduced profile is approved

For an explicitly proto2/proto3-only experiment, `protox` has the smallest
candidate API. An opt-in, separately named `Config::compile_protos_rust(...)`
could request import-complete raw descriptor bytes with source information,
then pass them through the same configured output and error path as
`compile_descriptor_set`. It must keep include-relative names, atomic writes,
transitive Cargo rebuild directives and both stub flavours. Editions and any
unresolved option must fail with a diagnostic, not silently acquire proto3 or
Edition 2023 semantics. Existing `compile_protos` stays an explicit `protoc`
route; there is **no** automatic fallback between compilers.

This is an interface proposal, **not** a selected dependency or a claim that
`protox` covers the reduced profile. Before even that route is approved,
review its complete locked dependency graph for licensing, MSRV and C/FFI
requirements, then run differential descriptor/output fixtures for nested and
public imports, same-stem files, custom/extension options, source comments,
and malformed inputs. A passing `cargo check` alone is not a semantic proof.

**Next decision:** the maintainer chooses whether an independently labeled
proto2/proto3-only profile is useful. Full-profile `CG-17` remains blocked
until a reviewed frontend implements Edition 2023/2024 descriptors and grammar,
including `export`/`local`, `import option`, naming and feature defaults, or
those gaps are split into bounded upstream tasks. Do not add a dependency or
build a new protobuf compiler as an unbounded workaround.
