# Design

pbrs matches the application traits of Google protobuf v4. It passes
official conformance (binary + JSON + text). There is no C.

## Storage

Empty collections are null pointers, not empty `Vec`s. A TestAllTypes
(TAT, the conformance kitchen-sink message) with no packed or map fields
does not allocate those slots.

Messages with six or more cold fields (packed/unpacked scalars, repeated
messages, WKT) box them in `Option<Box<MsgCold>>`. Maps and repeated
string/bytes stay on the hot struct (map_64 / strings). `packed_fixed32`,
`packed_fixed64`, `packed_float`, and `repeated_nested_message` stay hot
so those benches do not pay a Cold malloc.

`Default` is `mem::zeroed` of that layout. `Option<bool>` zeroed is
`Some(false)`, so explicit bools use `OptBool` (0 = unset). Optional
string/bytes are `Option<Box<LazyStr>>` / `LazyBytes`.

TAT `size_of` is 648 bytes. `TestAllTypesProto3::new` is ~19 ns.

## Parse

Parse is one pass. Truncated packed, bad varints, UTF-8 (per edition), and
depth are rejected here, not on getter.

Scalar-only parses do not `Arc` the input. The first lazy string, bytes,
nested, or packed-varint field builds a `Wire` (`Arc<[u8]>` + range).

After validation:

- strings <= 23 bytes: SSO copy
- longer strings / bytes: `Wire` window
- packed varints: validated payload kept; `Vec` on first get. Encode recodes
  canonical (overlong memcpy fails recommended `ValidDataRepeated`)
- packed fixed-width: payload-only `Wire` (not the parent message). Encode
  memcpy that payload. `Vec` on first get
- nested messages: `LazyMsg` holds the subslice; nested struct on first
  getter (`OnceLock`)

Unpacked scalar runs of the same tag reserve then push without re-matching
the whole tag table each time.

`FooView` is `&Owned` after this parse. It is not a wire overlay.

## Encode

`CachedSize` is an `AtomicU64` ignored by `PartialEq`. Every setter, `_mut`,
and merge calls `dirty()`. The first `serialized_len` / `serialize` fills
it.

Map encode walks the raw pair slice (`pairs()`). Last key wins on parse
(`push_entry`, no scan). Lookup on `get` scans.

testdata `Person` inlines up to 4 tags/scores (`MaybeUninit`) so the person
bench does not heap-allocate those repeats.

## API shape

Generated accessors follow Google rust:

- nested getter returns `&T` (default instance if unset)
- presence is `has_` / `*_opt`
- open enums: `From<i32>` newtype; closed: `TryFrom`
- `proto!` with `__{}` inference and `..spread`

`__internal` is a module (`SealedInternal`, `Private`). Google rust_upb
tests treat `__internal` as `()`. Application code should not use it.

## Not copied from upb

There is no arena, minitable, FFI, or `MessagePtr`. Generated types are
ordinary Rust structs. Drop is Rust drop.
