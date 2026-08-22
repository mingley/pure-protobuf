# Inventory: leftover hello Parse Δ (merge_inner / CachedSize / Default)

MEASURE ONLY. Not a win. Do not merge as done. No rewrite. No API change.

This is the leftover after #34 (`LazyStr::from_parse_span`: strings
`len ≤ 23` skip `Wire::ensure`). `from_parse_span` is present on this
branch (`src/lazy.rs`, generated hello string arms).

**Verified codec line of record stays #31.** Hello combined **52.2 vs
25.8**; 4 KiB **190.6 vs 166.1**; decode **45.4 vs 22.1**. Do **not**
write 31.8 vs 25.0 into docs. Those Verified numbers are untouched.

Cited #34 same-host Parse-only (that VM, not this one): hello **26.2 vs
21.7 ns** (Δ **~4.5 ns**). 4 KiB unchanged (still a loss). This file
attributes that leftover. Isolated proxies overlap; **do not sum** them
as if they add.

Scratch only. Kernel API, README, `docs/status.md`, tonic-bench crate
docs, and `protobuf-tonic` codec are untouched.

## Confirm: `from_parse_span` is on the hello path

- `src/lazy.rs`: `LazyStr::from_parse_span` copies `len ≤ 23` into
  `ProtoString` and does not `Wire::ensure` the parent frame.
- `src/codegen.rs` singular / optional / repeated string arms emit
  `LazyStr::from_parse_span(wire, data, s, e)`.
- Generated `HelloRequest` (protobuf-tonic `OUT_DIR` after
  `from_parse_span` landed) string arm:

```
let (s, e) = pbrs::rt::read_len_span(data, pos)?;
let b = &data[s..e];
std::str::from_utf8(b).map_err(|_| ParseError::new("invalid utf-8"))?;
self.name = pbrs::rt::LazyStr::from_parse_span(wire, data, s, e);
```

`Wire::ensure` is **off** the hello `"ada"` path. It stays **on** 4 KiB
(`len > 23`).

## Call stack that still owns the leftover

```
ProtobufDecoder::decode / tonic-bench pbrs_codec_decode
  Parse::parse                         src/message.rs
    HelloRequest::default              zeroed_message (48 B)
    ClearAndParse::merge_from_bytes    impl_typed_message
      HelloRequest::merge_bytes        generated
        HelloRequest::merge_inner      generated tag loop
          CachedSize::dirty            atomic store every merge
          decode_tag                   src/wire.rs
          read_len_span                src/wire.rs
          str::from_utf8               proto3 VERIFY
          LazyStr::from_parse_span     inline ProtoString, no Arc
```

Layers: `Parse` → `merge_from_bytes` → `merge_bytes` → `merge_inner`.
Static generics, not `dyn`.

prost: `Message::decode` → `Default` (24 B `{ name: String }`) →
`merge` → `string::merge` → `bytes::merge_one_copy` + `from_utf8`.
No `CachedSize`. No `UnknownFields` store. No parent-frame `Arc`.

## Layout (compile-time; not a timing)

| type | bytes |
|---|---:|
| pbrs `HelloRequest` | 48 |
| prost `HelloRequest` | 24 |
| `LazyStr` | 32 |
| `UnknownFields` | 8 |
| `CachedSize` | 8 |
| `String` | 24 |

pbrs: `name: LazyStr` + `unknown: UnknownFields` +
`cached_size: CachedSize`. Default is `mem::zeroed`.

Hello bytes (`HelloRequest { name: "ada" }`): `0a 03 61 64 61`
(5 bytes). Same on pbrs and prost.

## Parse-only ns (this VM)

Numbers pending the throwaway harness (`tmp/parse-hello-delta`, not a
workspace member). Same Instant median as tonic-bench: 40000 iters, 15
samples, release thin-LTO.

```
(pending first release run)
```

Reproduce:

```
cd tmp/parse-hello-delta && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc`. Host note: #34's 26.2 / 21.7 was a
different machine. Do not mix hosts. Do not treat this VM as the
Verified line.

## Bucket table

Isolated proxies overlap. Do not add the `ns` column to the leftover Δ.
Prefer **Parse − reconstruct** for the merge_inner glue (same split as
#32). Reconstruct is the generated string arm only (`decode_tag`,
`read_len_span`, `from_utf8`, `from_parse_span`).

| sink | ns or unknown | evidence | on hello? |
|---|---|---|---|
| `HelloRequest::default` / Default size (48 B vs prost 24 B) | pending | `zeroed_message` 48 B vs prost empty `String` 24 B. Isolated `Default` proxy. | **yes** |
| `CachedSize::dirty` | pending | Atomic store every `merge_inner`. Isolated `default+dirty` / reused-dirty. prost has no size cache. | **yes** |
| merge_inner wrapper / Parse → merge_from_bytes → merge_bytes → merge_inner | pending | `Parse − reconstruct` (#32 split). Recursion check, `until` group test, `check_required` (empty), loop, extra frames. | **yes** |
| `decode_tag` / `read_len_span` | pending | Isolated tag / len / walk. prost also decodes the same 1-byte tag + 1-byte len. Shared work, not a unique leftover. | **yes** (shared) |
| proto3 `from_utf8` | pending | Both sides. Isolated `"ada"` / 4 KiB. 4 KiB UTF-8 is large and **both** pay it. | shared, **not the leftover** |
| `from_parse_span` / inline `ProtoString` | pending | Reconstruct − (walk + utf8). No parent Arc. prost copies once into `String`. | **yes** (materialize) |
| `Wire::ensure` / parent `Arc<[u8]>` | 0 on hello | #34 skip. Isolated `Wire::from_slice(5 B)` is off this path. 4 KiB still pays it. | **no** (hello); **yes** (4 KiB) |
| unknown-field capture | 0 | Only field 1. `capture_unknown` not called. | **no** (this payload) |
| trait object / `dyn` | 0 | Static generics. Extra is layered functions, not vtable. | **no** |
| proto3 defaulting / empty fast path | 0 | `name` is present. Empty is a pbrs win. | **no** |
| codec `Vec` / framing | 0 | Parse-only. #30 dropped the codec `Vec`. | **no** |

Fill `pending` from the harness output. Do not treat a filled table as
a win.

## What this is not

- Not a faster `Parse`.
- Not a rewrite.
- Not an API change.
- Not a win.
- Not codec parity.
- Not a kernel change.
- Not a replacement of the #31 Verified numbers.
