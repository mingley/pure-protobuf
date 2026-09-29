# Discarded: `name_80` heap-copy try ([#57](https://github.com/mingley/pure-protobuf/pull/57))

This is the historical record of a closed draft, not a description of
current `main`. The statements below about the unshipped cut refer to that
draft's baseline. Later `src/lazy.rs` work does use owned copies for qualifying
medium strings; assess that implementation using its own evidence.

The original experiment did not establish a win and its draft diff should
not be revived on the strength of these measurements.

Integrity passed at `dbdbd0d` / follow-up `56e31aa` as inventory only.
Still a loss vs prost. Rebased onto `cb5f92b` (#56: QPS reported, not
gated). Those claims are untouched.

**Verified codec line of record stays #31.** Hello combined **52.2 vs
25.8**. The Apple M4 Pro survey table in `docs/benchmarks.md` is
untouched (`name_80` **6.4 / 25.3** vs prost **4.6 / 23.7**). Do **not**
write leftover VM ns into `docs/status.md` Verified or that table.

## Leftover conclusion (draft #57, same host; cut not live)

A draft try copied almost-whole strings `24..=256` into a heap
`ProtoString` instead of a payload `Arc<[u8]>`. `name_4kib` stayed on
`from_utf8_payload`. Same-host leftover shrank. The string arm was no
longer the expensive side.

Leftover after that try is the `merge_inner` wrapper plus a small
encode Δ. Combined is still a loss. Flatten `merge_inner` (#39) stays
discarded.

| piece | before (run 1 / 2) | after (run 1 / 2) |
|---|---|---|
| reconstruct − prost | 15.7 / 15.9 ns | 1.8 / 2.1 ns |
| Parse − prost | 28.7 / 37.3 ns | 15.8 / 16.0 ns |
| combined Δ | 29.8 / 39.3 ns | 17.6 / 17.8 ns |

Draft-only. Isolated proxies do not sum. Not a replacement of the #31
Verified line or the M4 Pro `name_80` row. The cut is not shipped.

## What this is not

- Not a faster `Parse` on main.
- Not a kernel change (`src/lazy.rs` is untouched).
- Not a win.
- Not codec parity.
- Not a merge of #57.
- Not a rewrite of #56 QPS (reported, not gated).
