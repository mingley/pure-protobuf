Source: closed [#41](https://github.com/mingley/pure-protobuf/pull/41). Harness: `parse-leftover/parse-4kib-delta/`.

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

Harness asserts `hello4 == prost4`, `len == 4099`, prefix `0a 80 20`.

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

- hello `"ada"` (`0a 03 61 64 61`): parent `wire` slot stays `None`,
  `LazyStr::Owned`.
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

prost (`prost 0.14.4` `encoding.rs`): `Message::decode` → `Default`
(24 B `{ name: String }`) → `merge` → `string::merge` →
`bytes::merge_one_copy` (one copy of the **4096-byte name** into
`String` via `replace_with(buf.take(len))`) + `from_utf8`. No
`CachedSize`. No `UnknownFields` store. No parent-frame `Arc`.

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
| `Wire` | 24 |
| `UnknownFields` | 8 |
| `CachedSize` | 8 |
| `String` | 24 |
| `Arc<[u8]>` | 16 |

pbrs: `name: LazyStr` + `unknown: UnknownFields` +
`cached_size: CachedSize`. Default is `mem::zeroed`.

## Parse-only ns (this VM)

Linux x86_64, rustc 1.88.0, Instant median, 40000 × 15, release thin-LTO.
Same style as tonic-bench / #32 / #36. Two consecutive
`cd parse-leftover/parse-4kib-delta && cargo run --release` runs.

**Run 1 (verbatim)**

```
pbrs hello Parse:  26.0 ns
prost hello decode: 23.1 ns
delta hello:        2.9 ns (pbrs − prost)
pbrs hello_4kib Parse:  154.8 ns
prost hello_4kib decode: 133.6 ns
delta 4kib:              21.2 ns
pbrs empty Parse:   0.8 ns
prost empty decode: 8.2 ns
```

**Run 2**

```
pbrs hello Parse:  32.9 ns
prost hello decode: 35.4 ns
delta hello:        -2.5 ns (pbrs − prost)
pbrs hello_4kib Parse:  155.9 ns
prost hello_4kib decode: 133.4 ns
delta 4kib:              22.5 ns
pbrs empty Parse:   0.8 ns
prost empty decode: 8.2 ns
```

This VM leftover 4 KiB Δ is **21.2–22.5 ns**. Same **~18–25 ns**
band as #32 / #34 / #36. Do not mix hosts. Do not treat 154.8 / 21.2
as the Verified line (that stays 190.6 vs 166.1 combined / 45.4 vs
22.1 hello decode).

Hello run 1 (26.0 vs 23.1, Δ 2.9) matches the #36 leftover band.
Hello run 2 is noisy after the 4 KiB loop (32.9 vs 35.4). **Not a
hello win.** 4 KiB is the measurement. Empty is still a pbrs win
(0.8 vs 8.2).

Reproduce (not a workspace member; `cargo test --workspace` does not
build it):

```
cd parse-leftover/parse-4kib-delta && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc`.

## Proxy timings (do not add)

Run 1 / run 2. Isolated public-API timings. They overlap.

```
pbrs HelloRequest::default:              0.8 / 0.8 ns
prost HelloRequest::default:             0.4 / 0.4 ns
ClearAndParse::merge_from_bytes 4kib
  (w/ Default):                        146.4 / 147.3 ns
CachedSize::default+dirty:               0.3 / 0.3 ns
CachedSize::dirty (reused):              0.3 / 0.3 ns
from_utf8("ada"):                        6.7 / 6.7 ns
from_utf8(4KiB):                        85.2 / 85.2 ns
String::from("ada"):                     6.4 / 6.6 ns
String::from(4KiB name):                40.1 / 40.1 ns   prost materialize
Vec::from(4KiB name):                   40.1 / 40.2 ns
Arc<[u8]>::from(4KiB name):             50.6 / 50.5 ns
Arc<[u8]>::from(4099 B msg):            43.8 / 44.3 ns
alloc+memcpy 4096:                      60.6 / 60.5 ns
alloc+memcpy 4099:                      68.9 / 68.4 ns
prealloc memcpy 4096:                    0.3 / 0.3 ns   L1 / elision artifact
prealloc memcpy 4099:                   27.6 / 27.7 ns
LazyStr::from_parse_span(ada):           5.4 / 5.4 ns   OFF 4 KiB path
LazyStr::from_parse_span(4kib):         60.6 / 61.2 ns   ON; ensure+window
Wire::from_slice(hello 5B):             23.1 / 23.1 ns   OFF 4 KiB path
Wire::from_slice(hello_4kib):           47.7 / 47.9 ns   ON; parent Arc
Wire::from_slice(4KiB name):            50.7 / 50.8 ns
Wire::ensure(hello_4kib):               48.7 / 49.7 ns
Wire::window(prebuilt 4kib):            11.0 / 11.1 ns
Wire::ensure+window 4kib:               61.0 / 61.1 ns
decode_tag only (4kib):                  0.6 / 0.6 ns
read_len_span only (4kib):               2.6 / 2.3 ns
decode_tag + read_len_span hello:        2.6 / 2.6 ns
decode_tag + read_len_span 4kib:         3.1 / 3.1 ns
reconstruct from_parse_span hello:      17.7 / 17.8 ns
reconstruct from_parse_span 4kib:      144.7 / 145.1 ns
reconstruct ensure+from_span 4kib:     143.6 / 144.5 ns   same body
reconstruct + String::from 4kib:       221.2 / 223.1 ns   not the Parse path
Default + dirty + reconstruct 4kib:    145.4 / 145.2 ns
Parse − reconstruct hello:               8.3 / 15.1 ns   run 2 hello noisy
Parse − reconstruct 4kib:               10.1 / 10.7 ns
reconstruct − prost decode 4kib:        11.2 / 11.8 ns
Wire::from_slice(msg) − String::from:    7.5 / 7.8 ns
from_parse_span 4kib − String::from:    20.5 / 21.1 ns
leftover 4kib Δ (Parse − prost):        21.2 / 22.5 ns
```

`reconstruct` is the generated string arm via public `rt` helpers (tag,
UTF-8, `from_parse_span`). No `Default`, no `cached_size`, no unknown /
required / group. Same split as #32 / #36.

On 4 KiB, `from_parse_span` **is** `Wire::ensure` + `from_span`. The
ensure reconstruct (143.6–144.5) matches current reconstruct
(144.7–145.1).

Do not use prealloc memcpy 4096 = 0.3 as evidence that copy is free.
That is a hot-buffer / size-class artifact. `String::from(4KiB)`
**40.1** is the prost-style materialize proxy.

## Bucket table

Isolated proxies overlap. Do not add the `ns` column to the leftover Δ.
Prefer **Parse-only 4 KiB pbrs vs prost** plus **reconstruct vs full
Parse**.

| sink | ns or unknown | evidence | on 4 KiB? |
|---|---|---|---|
| `Wire::ensure` / parent `Arc<[u8]>` of the whole message | **47.7–47.9** isolated `from_slice(4099)`; **48.7–49.7** `ensure` | #34 skip is **off**. Harness: parent slot `Some`, `as_slice()` is the 4099 B frame. Long `from_parse_span` calls `Wire::ensure`. | **yes** |
| extra copy vs prost one `String` copy | isolated Arc vs `String` **7.5–7.8**; `from_parse_span − String` **20.5–21.1** (overlaps ensure+window) | pbrs copies **4099** into `Arc<[u8]>` and windows. prost `merge_one_copy` copies **4096** name bytes into one `String`. No second pbrs name copy (`LazyStr::Wire`). A forced second `String` reconstruct is **221–223** — that is **not** Parse. Extra vs prost is Arc-of-parent vs one String (+ 3 prefix bytes). | **yes** |
| proto3 `from_utf8` | **85.2** isolated 4 KiB | Both sides (`string::merge` also `from_utf8`). 4 KiB leftover stays **21–23**. | shared, **not the leftover** |
| `HelloRequest::default` / Default size (48 B vs prost 24 B) | **0.8 vs 0.4** | `zeroed_message` 48 B vs empty `String` 24 B. Isolated `Default`. Small piece of the wrapper. | **yes** (small) |
| `CachedSize::dirty` | **0.3** | Atomic every `merge_inner`. Isolated. Always on. prost has no size cache. | **yes** (small) |
| merge_inner wrapper / Parse → merge_from_bytes → merge_bytes → merge_inner | **10.1–10.7** (`Parse − reconstruct` 4 KiB) | Recursion check, `until` group test, `check_required` (empty), loop, extra frames, `Default`, `dirty`. `merge_from_bytes` 146.4–147.3 vs Parse 154.8–155.9. Same class as the hello leftover after #34. | **yes** |
| `decode_tag` / `read_len_span` | **0.6 / 2.3–2.6**; walk **3.1** | 2-byte length varint (hello walk is 2.6). Isolated. prost also decodes the same tag+len. | **yes** (shared) |
| `from_parse_span` / `Wire::window` | **60.6–61.2** isolated; window **11.0–11.1**; ensure+window **61.0–61.1** | Long body of `from_parse_span`. Overlaps `Wire::ensure`. Isolated window is an `Arc` clone; pessimistic vs in-loop. | **yes** |
| reconstruct string arm vs prost full decode | reconstruct **144.7–145.1**; prost **133.4–133.6**; arm − prost **11.2–11.8** | Unlike hello (reconstruct **17.7** < prost **23.1**), the 4 KiB string arm is already slower than prost full decode. This is the leftover #34 did not touch. | **yes** |
| unknown-field capture | **0** | Only field 1. `capture_unknown` not called. | **no** (this payload) |
| trait object / `dyn` | **0** | `Parse` / `ClearAndParse` are static generics, not `dyn`. Extra is layered functions. | **no** |
| proto3 defaulting / empty fast path | **0** | `name` is present. `EMPTY_PARSE_OK` unused. Empty is a pbrs **win** (0.8 vs 8.2). | **no** |
| second `String` / `ProtoString` heap copy | **0** | Stays `LazyStr::Wire`. `from_bytes` / inline is off (`len > 23`). | **no** |
| codec `Vec` / framing | **0** | Parse-only. #30 already dropped the codec `Vec`. | **no** |

`Default` (0.8 vs 0.4) and `dirty` (0.3) sit **inside** the 10.1–10.7 ns
wrapper. Do not add 0.8 + 0.3 on top of it.

## Where the leftover ~21–23 ns goes

Not a clean in-function split. No kernel probes. Do not sum the
isolated Arc / window / UTF-8 / memcpy rows.

Prefer this two-way split of **Parse − prost** (reconstruct is a
subset of Parse; prost is independent):

| piece | this VM | what it is |
|---|---|---|
| reconstruct − prost | **11.2–11.8 ns** | string arm: parent `Wire::ensure` + window vs prost one `String` copy. UTF-8 / tag sit on both sides of this difference. |
| Parse − reconstruct | **10.1–10.7 ns** | merge_inner wrapper (`Default` 48 B, `dirty`, group/`until`, `check_required`, `Parse` → `merge_from_bytes` → `merge_bytes` → `merge_inner`). |
| Parse − prost | **21.2–22.5 ns** | leftover. Still a loss. |

On hello after #34 the string arm is **faster** than prost (reconstruct
17.7 < 23.1) and the leftover is the wrapper only. On 4 KiB the string
arm is **slower** than prost full decode. That is why #34 left 4 KiB
unchanged: the long path still Arcs the parent frame.

Isolated `Wire::from_slice(4099)` **47.7–47.9** is pessimistic vs
in-loop (prost also copies 4096). Incremental extra of parent Arc vs
`String::from(name)` is **7.5–7.8** isolated. In-loop string-arm extra
vs prost full is the **11.2–11.8** row above.

`from_utf8(4KiB)` **85.2** is large and **both** pay it. It is not the
gap.

Cannot split Arc-header vs the extra 3 prefix bytes vs window clone
**inside** `from_parse_span` without a kernel probe. The reconstruct
split plus the two-run proxies are the evidence.

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
- Not a hello win (run 2 hello noise).
