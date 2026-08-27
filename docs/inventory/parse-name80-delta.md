Source: this draft. Harness: `parse-leftover/parse-name80-delta/`.

# Inventory: leftover `name_80` combined vs prost

MEASURE ONLY. Still a loss. Do not merge as done. No rewrite. No API change.

This is the leftover **`name_80` combined** (encode+decode) gap named in
`docs/status.md` Remaining / `docs/benchmarks.md` What to chase. 80-byte
string, just over the SSO cutoff (`len ≤ 23`). Inventory captured on
`0383ed0`. Branch rebased onto `cb5f92b` (#56 harness/docs only; QPS
reported, not gated). Does not merge #32 / #36 / #39 / #41.

**Verified codec line of record stays #31.** Hello combined **52.2 vs
25.8**. The Apple M4 Pro survey table in `docs/benchmarks.md` is
untouched (`name_80` **6.4 / 25.3** vs prost **4.6 / 23.7**). Do **not**
write this VM's numbers into `docs/status.md` Verified or that table.
Do not mix hosts.

**Read order.** Everything from here through "Where the leftover ns
goes" is the **pre-cut** path: payload `Arc<[u8]>` /
`Wire::from_utf8_payload`, `LazyStr::Wire`. That is **not** the current
arm. The current arm is the later section **After the heap-copy cut**:
almost-whole `24..=256` → heap `ProtoString` (`LazyStr::Owned`).
`name_4kib` stays on `from_utf8_payload`. Combined leftover shrank and
did not widen. Still a loss. Isolated proxies overlap; **do not sum**.

## Pre-cut path (payload Arc / `Wire::from_utf8_payload`)

Not the current arm. This is `from_parse_span` as measured on `0383ed0`
before the heap-copy.

## Confirm: generated `Name` path (call site; still current)

tonic-bench `name_80` is `cases.Name { name: "x".repeat(80) }` from
`proto/codec_cases.proto` (proto3, field 1 = `string name`). Same
1-string shape as hello. Wire (pbrs and prost encode identically):

```
0a 50 78 78 … 78
```

- `0a` = tag 1, wire type 2 (`LEN`)
- `50` = length 80
- 80 × `0x78` (`'x'`)
- **82 bytes** total. Name starts at offset 2.

Generated `Name` (tonic-bench / harness `OUT_DIR` `pbrs/codec_cases.rs`):

```
pub struct Name {
    name: pbrs::rt::LazyStr,          // 32 B
    unknown: UnknownFields,           // 8 B
    cached_size: pbrs::rt::CachedSize // 8 B
}                                     // 48 B total

fn merge_inner(...) {
    if depth > RECURSION_LIMIT { ... }
    self.cached_size.dirty();
    while *pos < data.len() {
        let (n, w) = decode_tag(data, pos)?;
        if let Some(g) = until { ... }   // unused on Name
        match n {
        1 => match w {
            WIRE_LEN => {
                let (s, e) = read_len_span(data, pos)?;
                self.name = LazyStr::from_parse_span(wire, data, s, e)?;
            }
            _ => capture_unknown(...)    // not taken
        }
            _ => capture_unknown(...)    // not taken
        }
    }
    if until.is_some() { ... }           // not taken
    if enforce { self.check_required()?; } // empty
}

fn write_to(...) {
    if !self.name.is_empty() {
        encode_len_field(out, 1, self.name.as_bytes());
    }
    self.unknown.encode(out);
}
```

**Pre-cut** `LazyStr::from_parse_span` (`src/lazy.rs`) on this payload
(not the current arm):

```
let s = &data[rel_start..rel_end];
if s.len() <= 23 {
    from_bytes(s)                        // name_short "ada"; OFF
} else if s.len() + 8 >= data.len() {
    Wire::from_utf8_payload(s)           // PRE-CUT name_80 and name_4kib
} else {
    from_span(Wire::ensure(slot, data), …)  // medium-in-larger; OFF
}
```

`80 + 8 >= 82`, so pre-cut name_80 is the **payload-only** arm:
`Arc<[u8]>` of the **80-byte name**, UTF-8 via `simdutf8` on that copy,
`LazyStr::Wire`. It does **not** `Wire::ensure` the 82-byte parent.
`name_4kib` is the same arm (4096 + 8 ≥ 4099). name_short stays inline
`Owned`. After the cut, name_80 is heap `Owned`; `name_4kib` stays
`Wire` (see After).

Pre-cut harness asserts:

- name_short `"ada"`: parent slot `None`, `LazyStr::Owned`
- name_80: parent slot `None`, `LazyStr::Wire` of 80 bytes (**pre-cut**)
- name_4kib: parent slot `None`, `LazyStr::Wire` of 4096 bytes
- 80 bytes inside a 163-byte frame: parent slot `Some` (shared frame)

## Call stack that owns the leftover (pre-cut)

```
tonic-bench pbrs_codec_decode / Parse::parse
  Name::default                         zeroed_message (48 B)
  ClearAndParse::merge_from_bytes
    Name::merge_bytes
      Name::merge_inner
        CachedSize::dirty
        decode_tag                      1-byte tag
        read_len_span                   1-byte length + span
        LazyStr::from_parse_span        80+8>=82
          Wire::from_utf8_payload       PRE-CUT: Arc<[u8]> of 80 + require_utf8
```

Encode (tonic-bench `Serialize::encode` into `BytesMut`) is a
**set_name** message (`LazyStr::Owned(ProtoString::Heap)`), not a
parsed `Wire`:

```
Serialize::encode
  compute_size                          CachedSize hit after warmup
  write_to
    encode_len_field(1, name.as_bytes())
    unknown.encode                      empty
```

prost: `Message::decode` → `Default` (24 B `{ name: String }`) →
`string::merge` → one `String` copy of 80 + `from_utf8`. Encode is
`Message::encode` of that `String`. No `CachedSize`. No `UnknownFields`
store. No `Arc`.

## Layout (compile-time; not a timing)

| type | bytes |
|---|---:|
| pbrs `Name` | 48 |
| prost `Name` | 24 |
| `LazyStr` | 32 |
| `Wire` | 24 |
| `ProtoString` | 32 |
| `UnknownFields` | 8 |
| `CachedSize` | 8 |
| `String` | 24 |

## Parse + encode ns (this VM, pre-cut)

Linux x86_64, rustc 1.98.0, Instant median, 40000 × 15, release thin-LTO.
Same style as tonic-bench / #32 / #36 / #41. Two consecutive
`cd parse-leftover/parse-name80-delta && cargo run --release` runs.

**Run 1 (verbatim)**

```
pbrs name_80 Parse:  53.5 ns
prost name_80 decode: 24.8 ns
delta name_80 decode: 28.7 ns (pbrs − prost)
pbrs name_80 encode:  5.8 ns
prost name_80 encode: 4.8 ns
delta name_80 encode: 1.1 ns
pbrs name_80 combined:  59.3 ns
prost name_80 combined: 29.5 ns
delta name_80 combined: 29.8 ns
pbrs name_4kib Parse:  88.4 ns
prost name_4kib decode: 130.2 ns
```

**Run 2**

```
pbrs name_80 Parse:  62.0 ns
prost name_80 decode: 24.7 ns
delta name_80 decode: 37.3 ns (pbrs − prost)
pbrs name_80 encode:  6.0 ns
prost name_80 encode: 4.0 ns
delta name_80 encode: 2.0 ns
pbrs name_80 combined:  68.0 ns
prost name_80 combined: 28.7 ns
delta name_80 combined: 39.3 ns
pbrs name_4kib Parse:  88.2 ns
prost name_4kib decode: 130.4 ns
```

This VM leftover combined Δ is **29.8–39.3 ns**, decode-dominated
(**28.7–37.3**). Encode is **1.1–2.0 ns**. Do not mix with the M4 Pro
survey row (combined ~3 ns). Do not treat 59.3 / 29.8 as the Verified
line. name_4kib decode stays a pbrs win on this host (88 vs 130). Empty
is still a pbrs win (0.8 vs 8.0).

Reproduce (not a workspace member; `cargo test --workspace` does not
build it):

```
cd parse-leftover/parse-name80-delta && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc`.

## Proxy timings (do not add; pre-cut)

Run 1 / run 2. Isolated public-API timings. They overlap.

```
pbrs Name::default:                         1.3 / 1.3 ns
prost Name::default:                        0.5 / 0.4 ns
ClearAndParse::merge_from_bytes name_80:   43.8 / 43.6 ns
CachedSize::default+dirty:                  0.3 / 0.3 ns
from_utf8(80):                              6.7 / 6.5 ns
require_utf8(80):                           7.5 / 7.5 ns
from_utf8(4KiB):                           78.4 / 78.5 ns
require_utf8(4KiB):                        33.8 / 33.0 ns
String::from(80):                           6.8 / 7.0 ns
ProtoString::from_bytes(80):               15.5 / 16.5 ns
LazyStr::from_bytes(80):                   15.3 / 19.1 ns
LazyStr::from_parse_span(80):              36.9 / 37.2 ns   ON
Wire::from_slice(80 payload):              23.2 / 23.2 ns   ON
Wire::from_slice(82 msg):                  23.2 / 23.2 ns   OFF
Wire::from_utf8_payload(80):               31.0 / 31.1 ns   ON
Arc<[u8]>::from(80):                       17.8 / 17.8 ns
Vec::from(80):                              6.8 / 7.0 ns
decode_tag + read_len_span:                 2.5 / 2.8 ns
reconstruct from_parse_span name_80:       40.5 / 40.6 ns
reconstruct parent ensure name_80:         40.2 / 40.2 ns   OFF
reconstruct String copy name_80:           23.5 / 24.0 ns
Parse − reconstruct name_80:               13.0 / 21.4 ns
reconstruct − prost decode name_80:        15.7 / 15.9 ns
leftover name_80 decode Δ:                 28.7 / 37.3 ns
leftover name_80 encode Δ:                  1.1 / 2.0 ns
leftover name_80 combined Δ:               29.8 / 39.3 ns
```

`reconstruct` is the generated string arm via public `rt` helpers (tag,
`from_parse_span`). No `Default`, no `cached_size`, no unknown /
required / group. Same split as #32 / #36 / #41.

Parent-ensure reconstruct (40.2) matches **pre-cut** reconstruct (40.5).
The leftover is **not** a wasted parent-frame Arc. It is the payload
`Arc<[u8]>` + `from_utf8_payload` vs prost one `String`. That arm is
not the current one.

`simdutf8` at 80 bytes is **not** cheaper than `from_utf8` (7.5 vs
6.5–6.7). At 4 KiB it is (33 vs 78); that is why `name_4kib` already
wins and `name_80` does not.

## Bucket table (pre-cut)

Isolated proxies overlap. Do not add the `ns` column to the leftover Δ.
Prefer **Parse-only name_80 pbrs vs prost** plus **reconstruct vs full
Parse**. Encode is a separate small bucket.

| sink | ns or unknown | evidence | on name_80? |
|---|---|---|---|
| `Wire::from_utf8_payload` / payload `Arc<[u8]>` of 80 | **31.0–31.1** isolated; `from_slice(80)` **23.2**; `Arc::from(80)` **17.8** | Almost-whole arm. Harness: parent slot `None`, `LazyStr::Wire` of 80. | **yes** |
| extra copy vs prost one `String` | isolated Arc vs `String` **~11**; reconstruct−prost **15.7–15.9** | pbrs Arcs the **80-byte name**. prost `merge_one_copy` copies 80 into one `String` (**6.8–7.0** isolated; String-copy reconstruct **23.5–24.0** ≈ prost full **24.7–24.8**). | **yes** |
| proto3 UTF-8 | `from_utf8(80)` **6.5–6.7**; `require_utf8(80)` **7.5** | Both sides. simdutf8 is **not** the 4 KiB win at 80. | shared, **not the leftover** |
| merge_inner wrapper | **13.0–21.4** (`Parse − reconstruct`; run 2 noisier) | `Default` 48 B, `dirty`, group/`until`, `check_required`, extra frames. Do not flatten (#39). | **yes** |
| `HelloRequest`/`Name` Default 48 B vs 24 B | **1.3 vs 0.4–0.5** | Inside the wrapper. Do not add on top. | **yes** (small) |
| `CachedSize::dirty` | **0.3** | Inside the wrapper. | **yes** (small) |
| encode `set_name` / `BytesMut` | **1.1–2.0** | `encode_len_field` + `as_bytes` + empty unknown vs prost `String`. Not the decode leftover. Do not chase short-string encode. | **yes** (small) |
| parent `Wire::ensure` of 82 B | **0** | `80+8>=82` skips it. Isolated `from_slice(82)` **23.2** is off this path. | **no** |
| unknown-field capture | **0** | Only field 1. | **no** |
| trait object / `dyn` | **0** | Static generics. | **no** |
| codec `Vec` / framing | **0** | Parse-only + `Encode` into `BytesMut`. | **no** |

`Default` and `dirty` sit **inside** the wrapper row. Do not add them
on top.

## Where the leftover ns goes (pre-cut)

Not a clean in-function split. No kernel probes. Do not sum the
isolated Arc / UTF-8 / memcpy rows.

Prefer this split of **Parse − prost** (reconstruct is a subset of
Parse; prost is independent). Encode is listed separately and is
**not** added into the decode leftover.

| piece | this VM | what it is |
|---|---|---|
| reconstruct − prost | **15.7–15.9 ns** | string arm: payload `Arc<[u8]>` + `from_utf8_payload` vs prost one `String` copy. UTF-8 / tag sit on both sides of this difference. |
| Parse − reconstruct | **13.0–21.4 ns** | merge_inner wrapper. Run 2 is noisier; do not treat 21.4 as a second 16. |
| Parse − prost | **28.7–37.3 ns** | leftover decode. Still a loss. |
| encode Δ | **1.1–2.0 ns** | `set_name` `Owned` vs prost `String`. Small. Combined leftover is decode-dominated **on this host**. |

On hello after #34 the string arm is faster than prost and the leftover
is the wrapper only. On name_80 the string arm is **already slower**
than prost full decode (40.5 vs 24.8). That is the same class as #41's
4 KiB-before-`from_utf8_payload` finding, at 80 bytes: `Arc<[u8]>` of
the payload vs one `String`. `name_4kib` keeps `from_utf8_payload`
because simdutf8 wins at 4 KiB.

Cannot split Arc-header vs memcpy vs enum wrap **inside**
`from_utf8_payload` without a kernel probe. The reconstruct split plus
the two-run proxies are the evidence.

## After the heap-copy cut (this VM, same host)

**This is the current arm.** `from_parse_span` for almost-whole
`24 <= len <= 256` now `require_utf8` + `from_bytes` (heap
`ProtoString`). `name_4kib` stays on `from_utf8_payload`. Two
consecutive harness runs after the cut:

`from_parse_span_unchecked` (proto2 / editions `utf8_validation =
NONE` only) now uses the same almost-whole copy strategy, still
without a UTF-8 check, so `\x80` Parses. That match is not a win.

**Run 1**

```
pbrs name_80 Parse:  40.8 ns
prost name_80 decode: 25.0 ns
delta name_80 decode: 15.8 ns
pbrs name_80 encode:  5.8 ns
prost name_80 encode: 4.0 ns
pbrs name_80 combined:  46.6 ns
prost name_80 combined: 29.0 ns
delta name_80 combined: 17.6 ns
reconstruct from_parse_span name_80: 26.8 ns
name_80 classification: LazyStr::Owned, parent slot None
pbrs name_4kib Parse:  88.5 ns
prost name_4kib decode: 129.8 ns
```

**Run 2**

```
pbrs name_80 Parse:  40.7 ns
prost name_80 decode: 24.6 ns
delta name_80 decode: 16.0 ns
pbrs name_80 encode:  5.7 ns
prost name_80 encode: 4.0 ns
pbrs name_80 combined:  46.4 ns
prost name_80 combined: 28.6 ns
delta name_80 combined: 17.8 ns
reconstruct from_parse_span name_80: 26.7 ns
```

| piece | before (run 1 / 2) | after (run 1 / 2) |
|---|---|---|
| reconstruct − prost | 15.7 / 15.9 | **1.8 / 2.1** |
| Parse − reconstruct | 13.0 / 21.4 | **14.0 / 14.0** |
| Parse − prost | 28.7 / 37.3 | **15.8 / 16.0** |
| combined Δ | 29.8 / 39.3 | **17.6 / 17.8** |

String arm is no longer the expensive side. Leftover is the
merge_inner wrapper plus a small encode Δ. Combined is still a loss.
`name_4kib` decode stayed a pbrs win (88 vs 130). Do not write these
into the M4 Pro survey table or the #31 Verified line.

## What this is not

- Not a faster `Parse`.
- Not a rewrite.
- Not an API change.
- Not a win.
- Not codec parity.
- Not a kernel rewrite (one `from_parse_span` arm). The heap-copy cut is
  the current VERIFY arm. proto2 NONE matches that copy strategy only;
  not a win.
- Not a replacement of the #31 Verified numbers (52.2 vs 25.8).
- Not a replacement of the M4 Pro `name_80` survey row.
- Not a merge of #32 / #36 / #39 / #41.
- Not a claim that 29.8 ns is the line of record.
