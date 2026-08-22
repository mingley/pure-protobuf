# Inventory: hello Parse gap (~23 ns vs prost)

MEASURE ONLY. Not done. No rewrite. Do not merge as a Parse win.
Do not treat this file, the throwaway harness, or these numbers as codec
parity. Kernel API, `ProtobufCodec`, README, and `docs/status.md` are
untouched.

Cited Head-of-Kernel line (`8db64f8` / #30 #31): hello decode **45.4 vs
22.1 ns** (~23 ns). This VM (Linux x86_64, rustc 1.88, thin LTO):
**44.7 vs 22.1 ns** (delta **22.6 ns**). Same payload, Parse-only, no
codec framing.

## (a) Parse-only ns (verbatim)

```
pbrs hello Parse:  44.7 ns
prost hello decode: 22.1 ns
delta hello:        22.6 ns (pbrs − prost)
pbrs hello_4kib Parse:  153.6 ns
prost hello_4kib decode: 135.5 ns
delta 4kib:              18.1 ns
pbrs empty Parse:   0.8 ns
prost empty decode: 8.0 ns
```

Reproduce (not a workspace member; `cargo test --workspace` does not
build it):

```
cd tmp/parse-hello-gap && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc` (`protobuf-tonic` `build.rs` +
`prost-build`). Same timer as `tonic-bench`: 40000 iters, 15 samples,
median, release thin-LTO. Criterion is not in-tree; this is that
harness, not a redesign.

Host note: #30's 45.4 / 22.1 was a different machine. The gap reproduced
here to ~1 ns. 4 KiB delta is **18.1 ns**, same order as hello's
**22.6 ns**, so the leftover is a **fixed per-message cost**, not
per-byte UTF-8 / memcpy of `name`.

## Hello bytes

`tonic-bench` / `ProtobufCodec` unary hello is `HelloRequest { name:
"ada" }` from `proto/hello.proto` (proto3, field 1 = `string name`).

Wire (5 bytes), pbrs and prost encode identically:

```
0a 03 61 64 61
```

- `0a` = tag 1, wire type 2 (`LEN`)
- `03` = length 3
- `61 64 61` = `ada`

`hello_4kib` is the same tag + 4096 `x` (4099 bytes).

## Call path used by codec + tonic-bench

`ProtobufDecoder::decode` (`protobuf-tonic/src/lib.rs`) on a contiguous
frame (typical unary):

```
Parse::parse(&chunk[..n])
```

Split-buffer fallback is `copy_to_bytes` then the same `Parse::parse`.
No per-message `Vec` on the one-chunk path (#30).

`tonic-bench` `pbrs_codec_decode` is the same body:

```
Parse::parse(src)
```

`ClearAndParse` is **not** called by the codec. `Parse::parse` for
`T: Default + ClearAndParse` does `Default` then
`merge_from_bytes` (no `clear()` after zeroed default).

## (b) Call stack that owns the extra time

pbrs hello (`protobuf_tonic::hello::HelloRequest`):

```
tonic-bench pbrs_codec_decode
  / ProtobufDecoder::decode
    Parse::parse                         src/message.rs
      HelloRequest::default              zeroed_message (48 B)
      ClearAndParse::merge_from_bytes    impl_typed_message
        HelloRequest::merge_bytes        generated
          HelloRequest::merge_inner      generated (the tag loop)
            CachedSize::dirty            atomic store
            decode_tag                   src/wire.rs
            read_len_span                src/wire.rs
            str::from_utf8               proto3 VERIFY (utf8==2)
            Wire::ensure                 Arc<[u8]> of the full 5 B
            LazyStr::from_span           len≤23 → ProtoString inline
                                         (Arc is then dropped)
```

Generated `HelloRequest::merge_inner` string arm (OUT_DIR after
`protobuf-tonic` build; same emitter as `src/codegen.rs` /
`src/generated/wrappers.rs` `StringValue`):

```
let (s, e) = pbrs::rt::read_len_span(data, pos)?;
let b = &data[s..e];
std::str::from_utf8(b).map_err(|_| ParseError::new("invalid utf-8"))?;
self.name = pbrs::rt::LazyStr::from_span(pbrs::rt::Wire::ensure(wire, data), s, e);
```

prost hello (`helloworld::HelloRequest` from `prost-build` of the same
proto; 24 B `{ name: String }`):

```
tonic-bench prost_codec_decode
  / ProstDecoder
    prost::Message::decode               prost-0.14.4 src/message.rs
      HelloRequest::default              empty String
      Message::merge
        decode_key
        merge_field                      prost-derive match tag
          string::merge
            bytes::merge_one_copy        one copy into String
            str::from_utf8
          _ => skip_field                not taken (no unknowns)
```

No `CachedSize`. No `UnknownFields` storage. No `Arc` of the parent
frame. Unknowns are skipped, not captured.

## Layout

| type | bytes |
|---|---:|
| pbrs `HelloRequest` | 48 |
| prost `HelloRequest` | 24 |
| `LazyStr` | 32 |
| `Wire` | 24 |
| `ProtoString` | 32 |
| `UnknownFields` | 8 |
| `CachedSize` | 8 |
| `String` | 24 |

pbrs: `name: LazyStr` + `unknown: UnknownFields` +
`cached_size: CachedSize`. Default is `mem::zeroed`.

## (c) Candidate sinks

Not a clean in-function split. No kernel probes. Isolated public-API
timings overlap and **must not be summed** to 22.6. They still name
the sites.

| sink | on hello? | evidence |
|---|---|---|
| `Wire::ensure` / `Arc<[u8]>` of the **whole** message | **yes (dominant)** | `Wire::from_slice(hello 5B)` **17.7 ns** isolated. Generated arm always calls `Wire::ensure` before `from_span`. For `ada` (3 B ≤ 23) `from_span` then **discards** the `Wire` and copies into inline `ProtoString`. Alloc + memcpy 5 + drop, then a second 3-byte copy. prost copies **once** into `String` (`String::from("ada")` 6.8 ns isolated). |
| Extra copy | **yes** | Same as above. 4 KiB keeps the `Wire` window (`len > 23`), so no second payload copy; hello does both. |
| UTF-8 check | shared, **not the gap** | Both run `str::from_utf8`. Isolated `"ada"` is 6.2 ns (pessimistic; 3 ASCII bytes). 4 KiB UTF-8 is 79.1 ns and **both** pay it; 4 KiB delta stays ~18 ns. |
| Unknown-field skip / capture | **no** (this payload) | Hello has only field 1 / `LEN`. `capture_unknown` is not called. Dead match arms only. prost `skip_field` also not taken. |
| `CachedSize::dirty` | **yes, small** | Atomic store every `merge_inner`. Isolated default+dirty **0.3 ns**. Always on. prost has no size cache. |
| Trait object | **no** | `Parse` / `ClearAndParse` are static generics, not `dyn`. Extra is monomorphized layers (`Parse` → `merge_from_bytes` → `merge_bytes` → `merge_inner`). |
| Trait / required / group glue | **yes, residual** | `Parse − reconstruct` **8.7 ns**: `Default`, `cached_size.dirty`, recursion check, `until` group test, `check_required` (empty), loop. Reconstruct is the string arm only (36.0 ns). |
| Larger `Default` / zeroed 48 B | **yes, small** | pbrs default 1.3 ns vs prost 0.4 ns. |
| Proto3 defaulting | **no** | `name` is present. `EMPTY_PARSE_OK` unused. Empty payload is a pbrs **win** (0.8 vs 8.0) via the empty fast path. |
| Codec framing / extra `Vec` | **no** | This inventory is Parse-only. #30 already dropped the codec `Vec`. |
| Alloc of the string itself | **yes, but different shape** | pbrs: wasted `Arc` + inline `ProtoString` (no heap for 3 B). prost: one heap `String`. Net extra is the `Arc`, not a missing SSO on pbrs. |

### Proxy timings (do not add)

```
pbrs HelloRequest::default:                  1.3 ns
prost HelloRequest::default:                 0.4 ns
ClearAndParse::merge_from_bytes (w/ Default): 39.9 ns
CachedSize::default+dirty:                   0.3 ns
from_utf8("ada"):                            6.2 ns
from_utf8(4KiB):                             79.1 ns
String::from("ada"):                         6.8 ns
ProtoString::from_bytes("ada"):              9.9 ns
LazyStr::from_bytes("ada"):                  5.3 ns
Wire::from_slice(hello 5B):                  17.7 ns
Wire::from_slice(hello_4kib):                42.7 ns
decode_tag + read_len_span:                  2.5 ns
reconstruct string arm hello:                36.0 ns
reconstruct string arm 4kib:                 145.2 ns
Wire::from_slice + from_span:                24.3 ns
```

`reconstruct` is the generated string arm via public `rt` helpers (tag,
UTF-8, `Wire::ensure`, `from_span`). No `Default`, no `cached_size`, no
unknown / required / group.

Rough, overlapping attribution of the **22.6 ns**:

- **~15–18 ns** — `Wire::ensure` `Arc::from` of 5 bytes that
  `from_span` does not keep.
- **~7–9 ns** — `merge_inner` wrapper (`Default` 48 B, `dirty`,
  group/`until`, `check_required`, extra frames).
- UTF-8 / varint / tag walk — paid on both sides; not the leftover.

Cannot split tag-loop vs UTF-8 vs alloc **inside** `merge_inner`
without a kernel probe. The proxies plus the 4 KiB vs hello comparison
are the evidence.

## What this is not

- Not a faster `Parse`.
- Not a shim.
- Not a win.
- Not codec parity.
- Not a kernel change.
