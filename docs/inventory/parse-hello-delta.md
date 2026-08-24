Source: closed [#36](https://github.com/mingley/pure-protobuf/pull/36). Harness: `parse-leftover/parse-hello-delta/`.

# Inventory: leftover hello Parse Δ (merge_inner / CachedSize / Default)

MEASURE ONLY. Not a win. Do not merge as done. No rewrite. No API change.

This is the leftover after #34 (`LazyStr::from_parse_span`: strings
`len ≤ 23` skip `Wire::ensure`). `from_parse_span` is present on this
branch (`src/lazy.rs`, generated hello string arms). Confirmed in
protobuf-tonic `OUT_DIR` `HelloRequest::merge_inner`.

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
- Generated `HelloRequest` (protobuf-tonic `OUT_DIR` after #34):

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
                std::str::from_utf8(b)?;
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

`Wire::ensure` is **off** the hello `"ada"` path. It stays **on** 4 KiB
(`len > 23`). Harness asserts `from_parse_span` on `0a 03 61 64 61`
leaves the parent `wire` slot `None`.

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

Linux x86_64, rustc 1.88.0, Instant median, 40000 × 15, release thin-LTO.
Same style as tonic-bench / #32. Two consecutive
`cd parse-leftover/parse-hello-delta && cargo run --release` runs.

**Run 1 (verbatim)**

```
pbrs hello Parse:  24.0 ns
prost hello decode: 21.9 ns
delta hello:        2.1 ns (pbrs − prost)
pbrs hello_4kib Parse:  154.9 ns
prost hello_4kib decode: 135.2 ns
delta 4kib:              19.7 ns
pbrs empty Parse:   0.8 ns
prost empty decode: 8.0 ns
```

**Run 2**

```
pbrs hello Parse:  24.9 ns
prost hello decode: 21.7 ns
delta hello:        3.2 ns (pbrs − prost)
pbrs hello_4kib Parse:  158.2 ns
prost hello_4kib decode: 137.5 ns
delta 4kib:              20.7 ns
pbrs empty Parse:   1.0 ns
prost empty decode: 8.0 ns
```

This VM leftover hello Δ is **2.1–3.2 ns**. #34's cited **~4.5 ns**
(26.2 vs 21.7) is a **different host**. Same 2–5 ns leftover band. Do
not mix hosts. Do not treat 24.0 / 2.1 as the Verified line.

4 KiB stays a **~20 ns** loss (parent `Wire::ensure` still on). Empty
is still a pbrs win (0.8 vs 8.0).

Reproduce (not a workspace member; `cargo test --workspace` does not
build it):

```
cd parse-leftover/parse-hello-delta && cargo run --release
```

Needs rustc ≥ 1.88 and `protoc`.

## Proxy timings (do not add)

Run 1 / run 2. Isolated public-API timings. They overlap.

```
pbrs HelloRequest::default:              1.0 / 1.0 ns
prost HelloRequest::default:             0.4 / 0.4 ns
ClearAndParse::merge_from_bytes
  (w/ Default):                         20.3 / 20.0 ns
CachedSize::default+dirty:               0.3 / 0.3 ns
CachedSize::dirty (reused):              0.5 / 0.5 ns
from_utf8("ada"):                        6.2 / 6.3 ns
from_utf8(4KiB):                        78.5 / 78.8 ns
String::from("ada"):                     6.7 / 6.8 ns
ProtoString::from_bytes("ada"):          9.7 / 10.7 ns
LazyStr::from_bytes("ada"):              5.3 / 5.3 ns
LazyStr::from_parse_span(ada):           5.2 / 5.3 ns
Wire::from_slice(hello 5B):             17.8 / 17.8 ns   OFF hello path
Wire::from_slice(hello_4kib):           42.8 / 42.7 ns   ON 4 KiB path
decode_tag only:                         0.6 / 0.6 ns
read_len_span only:                      1.8 / 1.8 ns
decode_tag + read_len_span:              2.5 / 2.5 ns
reconstruct from_parse_span hello:      17.2 / 17.3 ns
reconstruct from_parse_span 4kib:      147.8 / 147.9 ns
reconstruct old Wire::ensure hello:     36.0 / 35.8 ns   OFF hello path
Default + dirty + reconstruct:          17.8 / 17.5 ns
Parse − reconstruct hello:               6.8 / 7.6 ns
reconstruct − (walk + utf8) hello:       8.5 / 8.5 ns
```

`reconstruct` is the generated string arm via public `rt` helpers (tag,
UTF-8, `from_parse_span`). No `Default`, no `cached_size`, no unknown /
required / group. Same split as #32.

Old `Wire::ensure` reconstruct (36.0 ns) matches #32's reconstruct on
the other VM. New reconstruct (17.2 ns) is the #34 skip in isolation
(~18 ns off the string arm). That Arc is **not** in the leftover.

## Bucket table

Isolated proxies overlap. Do not add the `ns` column to the leftover Δ.
Prefer **Parse − reconstruct** for the merge_inner glue (same split as
#32).

| sink | ns or unknown | evidence | on hello? |
|---|---|---|---|
| `HelloRequest::default` / Default size (48 B vs prost 24 B) | **1.0 vs 0.4** | `zeroed_message` 48 B vs prost empty `String` 24 B. Isolated `Default`. Small piece of the wrapper. | **yes** |
| `CachedSize::dirty` | **0.3** (default+dirty); 0.5 reused | Atomic store every `merge_inner`. Isolated. Always on. prost has no size cache. | **yes** |
| merge_inner wrapper / Parse → merge_from_bytes → merge_bytes → merge_inner | **6.8–7.6** (`Parse − reconstruct`) | Recursion check, `until` group test, `check_required` (empty), loop, extra frames, `Default`, `dirty`. Reconstruct is the string arm only (17.2). This is the leftover. | **yes** |
| `decode_tag` / `read_len_span` | **0.6 / 1.8**; walk **2.5** | Isolated. prost also decodes the same 1-byte tag + 1-byte len. Shared work. | **yes** (shared) |
| proto3 `from_utf8` | **6.2** isolated `"ada"`; **78.5** 4 KiB | Both sides. Isolated 3-byte timing is pessimistic (in-loop it folds). 4 KiB UTF-8 is large and **both** pay it; 4 KiB leftover stays ~20 ns. | shared, **not the leftover** |
| `from_parse_span` / inline `ProtoString` | **5.2** isolated; reconstruct−(walk+utf8) **8.5** | No parent Arc. prost `String::from("ada")` 6.7. Reconstruct (17.2) is **faster** than prost full decode (21.7–21.9). Materialize is not the leftover Δ. | **yes** (materialize; not the Δ) |
| `Wire::ensure` / parent `Arc<[u8]>` | **0 on hello** | #34 skip. Isolated `Wire::from_slice(5 B)` **17.8** is off this path. Old ensure-reconstruct **36.0**. 4 KiB still pays `from_slice` **42.8**. | **no** (hello); **yes** (4 KiB) |
| unknown-field capture | **0** | Only field 1. `capture_unknown` not called. Dead match arms only. | **no** (this payload) |
| trait object / `dyn` | **0** | `Parse` / `ClearAndParse` are static generics, not `dyn`. Extra is layered functions. | **no** |
| proto3 defaulting / empty fast path | **0** | `name` is present. `EMPTY_PARSE_OK` unused. Empty is a pbrs **win** (0.8 vs 8.0). | **no** |
| codec `Vec` / framing | **0** | Parse-only. #30 already dropped the codec `Vec`. | **no** |

## Where the leftover ~2–5 ns goes

Not a clean in-function split. No kernel probes. Do not sum the
proxies.

After #34 the string arm is no longer the expensive side:

- reconstruct (`from_parse_span`) **17.2 ns**
- prost full decode **21.7–21.9 ns**
- pbrs full Parse **24.0–24.9 ns**

So **Parse − reconstruct ≈ 6.8–7.6 ns** is the merge_inner wrapper
(`Default` 48 B, `CachedSize::dirty`, group/`until`, `check_required`,
`Parse` → `merge_from_bytes` → `merge_bytes` → `merge_inner` frames).
prost pays its own `Default` (24 B) + `merge` + one `String` alloc.
Net leftover on this VM is **2.1–3.2 ns** (wrapper minus prost's own
overhead). #34's **~4.5 ns** is the same leftover on another host.

`Default` (1.0 vs 0.4) and `dirty` (0.3) are **on** the path and
**small**. They sit inside the 6.8–7.6 ns wrapper; they are not a
second 1.3 ns you can add on top.

`decode_tag` / `read_len_span` / `from_utf8` are paid on both sides.
`Wire::ensure` is gone on hello. 4 KiB still pays it and stays a loss.

Cannot split tag-loop vs UTF-8 vs alloc **inside** `merge_inner`
without a kernel probe. The reconstruct split plus the two-run
proxies are the evidence.

## What this is not

- Not a faster `Parse`.
- Not a rewrite.
- Not an API change.
- Not a win.
- Not codec parity.
- Not a kernel change.
- Not a replacement of the #31 Verified numbers.
- Not a claim that 2.1 ns is the line of record.
