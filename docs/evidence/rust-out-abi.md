# rust_out kernel ABI inventory (UK-01)

protoc v35: `libprotoc 35.1`; protoc v36: `libprotoc 36.1`. 139 upstream + corpus protos generated per protoc, compiled against pbrs as the `protobuf` crate.

## Item status

| Status | Count |
|---|---|
| implemented | 33 |
| stub | 8 |
| missing | 0 |

## Known stubs (acceptance checklist)

- `build_enum_mini_table`: stub (curated: returns null; enum tables unimplemented (link_mini_table also ignores _subenums); src/runtime/mini_table.rs:155; 196 gencode uses)
- `ExtensionRegistryPtr`: stub (curated: type alias only; no extension registry; src/runtime/extension.rs:10; 0 gencode uses)
- `MiniTableExtensionPtr`: stub (curated: type alias only; no extension table/link support; src/runtime/extension.rs:8; 0 gencode uses)
- `MessageViewInterop::__unstable_wrap_raw_message`: stub (curated: unimplemented! default; src/runtime/reflect.rs:71; 0 gencode uses)
- `message_eq`: stub (curated: constant false; src/runtime/reflect.rs:434; 0 gencode uses)
- `debug_string`: stub (curated: constant "<msg>"; src/runtime/reflect.rs:137; 4368 gencode uses)
- `parse_into`: stub (curated: private kernel parser drops unknown fields via skip_field instead of retaining them (decode.rs); src/runtime/decode.rs:23; 0 gencode uses)

## Compile errors (cargo check per protoc)

- v35: 0 errors
- v36: 0 errors

## Generation failures

None: every proto generated under both protocs.

## Missing items (named by gencode, absent from pbrs)

None.

## Regenerating

```bash
python3 scripts/rust-out-abi.py
python3 scripts/rust-out-abi.py --verify   # fail on drift
```
