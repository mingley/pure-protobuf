# Design

This page summarizes the core protobuf runtime design. It is for readers who want to understand how `pbrs` stores, parses, and encodes messages without first reading generated code. The bottom line: `pbrs` matches Google protobuf v4 application traits, passes official conformance, and avoids C.

## Storage

Empty collections are null pointers, not empty `Vec`s. A TestAllTypes (TAT, the
conformance kitchen-sink message) with no packed or map fields does not
allocate those slots.

Messages with six or more cold fields box those fields in
`Option<Box<MsgCold>>`. Cold fields include packed and unpacked scalars,
repeated messages, and well-known types (WKT).

Some fields stay on the hot struct:

- maps and repeated string/bytes (`map_64` / `strings`)
- `packed_fixed32`
- `packed_fixed64`
- `packed_float`
- `repeated_nested_message`

Those benchmark rows do not pay a Cold allocation.

`Default` is `mem::zeroed` for that layout. Because zeroed `Option<bool>` is
`Some(false)`, explicit bools use `OptBool` where `0` means unset. Optional
string and bytes fields use `Option<Box<LazyStr>>` / `LazyBytes`.

TAT `size_of` is 648 bytes. `TestAllTypesProto3::new` is about 19 ns.

## Parse

Parse is one pass. Truncated packed fields, bad varints, UTF-8 errors
(according to edition), and recursion depth are rejected during parse, not on a
later getter.

Scalar-only parses do not `Arc` the input. The first lazy bytes, nested,
packed-varint, or long string field builds a `Wire` (`Arc<[u8]>` + range).
Short strings (`len <= 23`) copy into `ProtoString` and do not call
`Wire::ensure` for the parent frame.

After validation:

| Field shape | Stored representation |
|---|---|
| Strings <= 23 bytes | Small-string optimization (SSO) copy; no parent-frame `Arc`. |
| Longer strings / bytes | `Wire` window. |
| Packed varints | Validated payload kept; first getter builds a `Vec`. Encode recodes canonical form, so overlong memcpy fails recommended `ValidDataRepeated`. |
| Packed fixed-width | Payload-only `Wire`, not the parent message. Encode copies that payload; first getter builds a `Vec`. |
| Nested messages | `LazyMsg` holds the subslice; first getter builds the nested struct with `OnceLock`. |

Unpacked scalar runs of the same tag reserve and push without re-matching the
whole tag table each time.

`FooView` is `&Owned` after this parse. It is not a wire overlay.

## Encode

`CachedSize` is an `AtomicU64` and is ignored by `PartialEq`. Every setter,
`_mut`, and merge calls `dirty()`. The first `serialized_len` or `serialize`
fills it.

Map encode walks the raw pair slice (`pairs()`). On parse, the last key wins
through `push_entry` with no scan. Lookup on `get` scans.

testdata `Person` inlines up to 4 tags/scores with `MaybeUninit`, so the person
benchmark does not heap-allocate those repeats.

## API shape

Generated accessors follow Google Rust:

- nested getter returns `&T`, using a default instance if unset
- presence is `has_` / `*_opt`
- open enums use a `From<i32>` newtype
- closed enums use `TryFrom`
- `proto!` supports `__{}` inference and `..spread`

`__internal` is a module (`SealedInternal`, `Private`). Google rust_upb tests
treat `__internal` as `()`. Application code should not use it.

## Not copied from upb

Plugin-generated types (`protoc-gen-pbrs`) are ordinary Rust structs. Rust drop
frees them. There is no C.

Official `protoc --rust_out kernel=upb` links `src/runtime.rs`, a pure-Rust
MiniTable/Arena stand-in (`OwnedMessageInner`, `MessagePtr`). That application
binary interface is not upb C and is not used by the plugin-generated code.
