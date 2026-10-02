# GN-14: independent enum alias and deprecation options

Base: `03e2201d`. This is a correctness change, with no performance claim.

The pinned protobuf `descriptor.proto` defines `EnumOptions.allow_alias` as
field 2 and `EnumOptions.deprecated` as field 3. The descriptor reader treated
field 2 as deprecation. Pinned `TestAllTypesProto3.AliasedEnum` therefore acquired
a false public deprecation annotation, while a genuinely deprecated enum
without aliases lost its annotation.

The reader now consumes field 3 for deprecation. The unrelated alias option
still follows the existing option-preservation path. Custom enum options stay
intact. The existing source-info fixture now encodes deprecation on field 3.

The descriptor regression covers 18 combinations: proto2/proto3 and absent,
false or true values for both options. It checks descriptor and generated
annotations, enum openness, and retained custom options. A live plugin fixture
independently checks actual deprecation without aliases and ordinary aliases
without deprecation.

Pins used on Linux x86_64:

- Rust `1.99.0`, commit `b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`.
- Cargo `1.99.0`, commit `5f94df478`.
- `libprotoc 35.1`, protobuf `v35.1` at
  `35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`.
- Repository `Cargo.lock`, with no new dependencies.

Before the reader change, these commands failed as expected:

```sh
cargo test --locked -p pbrs --test dynamic enum_deprecation_is_independent_of_alias_options -- --nocapture
cargo test --locked -p pbrs --test plugin protoc_plugin_enum_deprecation_is_independent_of_allow_alias -- --nocapture
```

The descriptor test failed at `proto2: allow_alias=None, deprecated=Some(true)`:
actual `false`, expected `true`. The plugin test failed because the generated
`UnaliasedDeprecatedEnum` lacked `#[deprecated]`; the same output falsely
annotated `AliasedActiveEnum`.

After the reader change:

```sh
cargo test --locked -p pbrs --test dynamic --test plugin enum_deprecation -- --nocapture
cargo test --locked -p pbrs --test dynamic --test documentation
```

Both targeted tests passed. All 36 dynamic tests and all 24 documentation tests
passed. An earlier documentation run before protoc was available failed two
syntax-validator tests with `failed to execute protoc`; the complete rerun
above passed after provisioning the pinned compiler.

`cargo test --locked -p pbrs --lib` also passed all 96 core library tests.

Full codegen and regenerated pinned conformance gates are coordinated
with GN-13 and GN-12. This evidence alone does not close those integration gates.
