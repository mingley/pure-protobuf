# Historical: official rust_out vs pbrs ([#27](https://github.com/mingley/pure-protobuf/pull/27))

Superseded by [#42](https://github.com/mingley/pure-protobuf/pull/42).
Do not re-run the 234-error inventory as if it were current.

On 2026-08-22, `protoc --rust_out` (v35.1, `kernel=upb`,
`4.35.1-release`) of `proto/person.proto` compiled against pbrs as crate
`protobuf` failed with 234 rustc errors:

| n | error |
|---|---|
| 147 | `E0433` no `runtime` in `__internal` (`OwnedMessageInner`, `MiniTable*`, Arena ABI) |
| 6 | `E0433` no `entity_tag` |
| 6 | `E0405` no `EntityType` |
| 4 | `E0050` `into_proxied` arity `(self, Private)` vs `(self)` |
| 70 | `E0277` rust_out types miss pbrs `Message` supertraits |
| 1 | `E0425` no `assert_compatible_gencode_version` |

`pbrs::__internal` then exported only `Private` and `SealedInternal`.

Current check: `cd rust_out_person && cargo test --offline`
(`official_rust_out_person_parse_serialize_roundtrip`). Kernel:
`src/runtime.rs`.
