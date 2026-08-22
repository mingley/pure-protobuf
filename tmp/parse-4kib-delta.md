# Inventory: leftover 4 KiB Parse Δ vs prost (after #34)

MEASURE ONLY. Still a loss. Do not merge as done. No rewrite. No API change.

This is the leftover **4 KiB hello** Parse gap after #34
(`LazyStr::from_parse_span`: strings `len ≤ 23` skip `Wire::ensure`;
`len > 23` still `Wire::ensure`s the parent frame). Based on current
`main` (`84e20d8`, includes #34 / #37 / #38). Not based on draft #39
(flatten made hello worse; parked). Does not merge #32 / #36 / #39.

**Verified codec line of record stays #31.** Hello combined **52.2 vs
25.8**; 4 KiB **190.6 vs 166.1**; decode **45.4 vs 22.1**. Do **not**
write 31.8 vs 25.0 or other VM numbers into `docs/status.md` or
tonic-bench crate docs. Those Verified numbers are untouched.

Cited prior same-host Parse-only 4 KiB deltas (other VMs, not this
one): **153.6 vs 135.5** (Δ **18.1**, #32); **159.7 vs 134.8** (Δ
**24.9**, #34 after; #34 before was 159.2 vs 135.6, Δ 23.6 — unchanged
by the inline skip). #36 this-style run: **154.9 vs 135.2** (Δ
**19.7**). Band **~18–25 ns**. Still a loss. tonic-bench 4 KiB stays
a loss.

Scratch only. Kernel API, README, `docs/status.md`, tonic-bench crate
docs, and `protobuf-tonic` codec are untouched.

Isolated proxies overlap; **do not sum**.

## Confirm: 4 KiB payload and the long parse path

tonic-bench `hello_4kib` is `HelloRequest { name: "x".repeat(4096) }`.
Same here.

Wire (pbrs and prost encode the same bytes):

- tag `0x0a` (field 1, `WIRE_LEN`)
- length varint `0x80 0x20` (4096)
- 4096 × `0x78` (`'x'`)
- **4099 bytes** total. Name starts at offset 3.

`from_parse_span` is on the generated string arm
(`src/codegen.rs` singular / optional / repeated). Generated
`HelloRequest::merge_inner` (protobuf-tonic `OUT_DIR` after #34):

```
pub struct HelloRequest {
    name: pbrs::rt::LazyStr,          // 32 B
    unknown: UnknownFields,           // 8 B
    cached_size: pbrs::rt::CachedSize // 8 B
}                                     // 48 B total

fn merge_bytes(...) {
    let mut pos = 0;
    let mut wire = None;
    self.merge_inner(data, &mut wire, &mut pos, depth, true, None)
}

fn merge_inner(...) {
    if depth > RECURSION_LIMIT { ... }
    self.cached_size.dirty();
    while *pos < data.len() {
        let (n, w) = decode_tag(data, pos)?;
        if let Some(g) = until { ... }   // group end; unused on hello
        match n {
        1 => match w {
            WIRE_LEN => {
                let (s, e) = read_len_span(data, pos)?;
                let b = &data[s..e];
                std::str::from_utf8(b)?;           // proto3 VERIFY
                self.name = LazyStr::from_parse_span(wire, data, s, e);
            }
            _ => capture_unknown(...)    // not taken
        }
            _ => capture_unknown(...)    // not taken
        }
    }
    if until.is_some() { ... }           // not taken
    if enforce { self.check_required()?; } // empty
}
```

`LazyStr::from_parse_span` (`src/lazy.rs`):

```
let s = &data[rel_start..rel_end];
if s.len() <= 23 {
    Self::from_bytes(s)                  // hello "ada"; no Wire::ensure
} else {
    Self::from_span(Wire::ensure(slot, data), rel_start, rel_end)
}
```

`from_span` for `len > 23` is `LazyStr::Wire(wire.window(...))` —
a range on the parent `Arc<[u8]>`, **not** a second `String` /
`ProtoString` heap copy.

Harness asserts:

- hello `"ada"` (`0a 03 61 64 61`): parent `wire` slot stays `None`.
- 4 KiB (`len == 4096 > 23`): parent slot is `Some`,
  `as_slice()` is the **whole 4099-byte message**, name is
  `LazyStr::Wire` of 4096 bytes.

So #34 did not change 4 KiB. Long path still Arcs the parent frame.

## Call stack that owns the leftover (4 KiB)

```
ProtobufDecoder::decode / tonic-bench pbrs_codec_decode
  Parse::parse                         src/message.rs
    HelloRequest::default              zeroed_message (48 B)
    ClearAndParse::merge_from_bytes    impl_typed_message
      HelloRequest::merge_bytes        generated
        HelloRequest::merge_inner      generated tag loop
          CachedSize::dirty            atomic store every merge
          decode_tag                   1-byte tag
          read_len_span                2-byte length varint + span
          str::from_utf8               proto3 VERIFY (4096 B)
          LazyStr::from_parse_span     len>23 → Wire::ensure
            Wire::from_slice           Arc<[u8]> of the 4099 B frame
            Wire::window               Arc clone + range (name)
```

Layers: `Parse` → `merge_from_bytes` → `merge_bytes` → `merge_inner`.
Static generics, not `dyn`.

prost: `Message::decode` → `Default` (24 B `{ name: String }`) →
`merge` → `string::merge` → `bytes::merge_one_copy` (one copy of
the **4096-byte name** into `String`) + `from_utf8`. No `CachedSize`.
No `UnknownFields` store. No parent-frame `Arc`.

pbrs extra copy vs prost: pbrs copies the **whole 4099-byte message**
into `Arc<[u8]>` and keeps a window. prost copies **4096 name bytes**
into one `String`. No second pbrs name copy on Parse (stays
`LazyStr::Wire`). Extra vs prost is Arc-of-parent vs one String,
plus 3 prefix bytes (tag+len) in the allocation.

## Layout (compile-time; not a timing)

| type | bytes |
|---|---:|
| pbrs `HelloRequest` | 48 |
| prost `HelloRequest` | 24 |
| `LazyStr` | 32 |
| `UnknownFields` | 8 |
| `CachedSize` | 8 |
| `String` | 24 |
| `Arc<[u8]>` | 16 |

pbrs: `name: LazyStr` + `unknown: UnknownFields` +
`cached_size: CachedSize`. Default is `mem::zeroed`.

## Parse-only ns (this VM)

Linux x86_64, rustc 1.88.0, Instant median, 40000 × 15, release thin-LTO.
Same style as tonic-bench / #32 / #36. Two consecutive
`cd tmp/parse-4kib-delta && cargo run --release` runs.

**Numbers filled after the first release run. Do not treat them as
the #31 Verified line.**

<!-- NUMBERS -->

Reproduce (not a workspace member; `cargo test --workspace` does not
build it):

```
cd tmp/parse-4kib-delta && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc`.

## Proxy timings (do not add)

<!-- PROXIES -->

`reconstruct` is the generated string arm via public `rt` helpers (tag,
UTF-8, `from_parse_span`). No `Default`, no `cached_size`, no unknown /
required / group. Same split as #32 / #36.

On 4 KiB, `from_parse_span` **is** `Wire::ensure` + `from_span`. The
old ensure reconstruct and the current reconstruct are the same body.

## Bucket table

Isolated proxies overlap. Do not add the `ns` column to the leftover Δ.
Prefer **Parse-only 4 KiB pbrs vs prost** plus **reconstruct vs full
Parse**.

<!-- BUCKETS -->

## What this is not

- Not a faster `Parse`.
- Not a rewrite.
- Not an API change.
- Not a win.
- Not codec parity.
- Not a kernel change.
- Not a replacement of the #31 Verified numbers (52.2 vs 25.8).
- Not a merge of #32 / #36 / #39.
- Not a claim that 4 KiB is closed.
