# Selected byte storage

Use `Config::bytes(paths)` (added in `pbrs` `0.2.1`) to store selected protobuf
`bytes` fields as `ProtoBytes`, backed by immutable `bytes::Bytes`.

For `proto/blob.proto`:

```proto
syntax = "proto3";
package example;
message Blob { bytes payload = 1; }
```

Select the field in `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    pbrs::codegen::Config::new()
        .bytes([".example.Blob.payload"])
        .compile_protos(&["proto/blob.proto"], &["proto"])?;
    Ok(())
}
```

The singular getter `payload()` still returns `&[u8]`, borrowing the message.
`payload_shared()` returns an owned `ProtoBytes`; `as_shared()` returns an
owned `bytes::Bytes`. Both shared handles can outlive the message. Add
`bytes = "1"` to runtime dependencies for this example:

```rust
use pbrs::Parse;
include!(concat!(env!("OUT_DIR"), "/mod.rs"));

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wire = bytes::Bytes::from_static(b"\x0a\x03abc");
    let message = example::Blob::parse_bytes(wire)?;
    let payload: pbrs::ProtoBytes = message.payload_shared();
    let shared: bytes::Bytes = payload.as_shared();
    drop(message);
    assert_eq!(shared.as_ref(), b"abc");
    Ok(())
}
```

## Selectors and getters

`.` selects all bytes fields. Leading-dot selectors match qualified prefixes;
relative selectors such as `Blob.payload` match suffixes. Matching uses whole
name components; `*` is literal. Each `bytes()` call replaces the previous
selection; an empty iterator clears it.

| Field | Selector |
|---|---|
| Ordinary, optional or repeated `payload` | `.example.Blob.payload` |
| `payload` in a real oneof | `.example.Blob.payload`, without the oneof name |
| A map's bytes values | `.example.Blob.value`, using the synthetic `value` name |

The plugin equivalent is `--pbrs_opt=bytes=.example.Blob.payload`.
Presence fields add `payload_shared_opt() -> Option<ProtoBytes>` to distinguish
absence from an empty value. Repeated and map shared getters take an index or
key and return `Option<ProtoBytes>`. Occupied helper names gain numbered
suffixes. Selected repeated/map storage uses `ProtoBytes` elements.

`new()` and `Default` initialize selected storage correctly without a separate
option. Choose `safe_message_defaults(true)` separately when generated
bindings must forbid unsafe code.

## Input lifetime and costs

Unselected fields retain their existing lazy storage and small-field copy policy.

`parse_bytes(Bytes)` retains selected payload windows; `parse(&[u8])` copies
them. Setters take ownership of moved `Vec<u8>` and `Bytes` buffers; borrowed
slices copy. Shared getters clone the buffer owner, not its payload.

A small selected slice can retain a whole input frame. Use `ProtoBytes::into_vec()` for an
independent copy. Generate parents with matching selected-byte configuration:
older or unconfigured eager repeated/map parents still copy child payloads.
Regenerating only the child does not change that.

Without reflection, complex nested/imported schemas outside the typed-format
subset omit JSON/text methods. Map removal still reveals older parsed
duplicates; selected storage leaves that behavior unchanged. The
[consumer controls](../../tests/proof/codegen_parity/bytes.md) record these
limits and ownership checks.
