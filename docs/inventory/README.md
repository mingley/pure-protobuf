# Closed inventory (do not merge the diffs)

Cursor drafts [#27](https://github.com/mingley/pure-protobuf/pull/27),
[#32](https://github.com/mingley/pure-protobuf/pull/32),
[#36](https://github.com/mingley/pure-protobuf/pull/36),
[#39](https://github.com/mingley/pure-protobuf/pull/39), and
[#41](https://github.com/mingley/pure-protobuf/pull/41) were closed so they
would not sit stale. The measurements and discarded experiments live here.
The throwaway harnesses are excluded crates under `parse-leftover/`.

| Source | Finding | Reproduce |
|---|---|---|
| #27 | Official rust_out vs pbrs was 234 rustc errors (`__internal::runtime` missing). Superseded by #42. | `cd rust_out_person && cargo test --offline` |
| #32 | Hello Parse ~23 ns vs prost; leftover is a **fixed per-message cost**, not bytes. Parent `Arc` was on the path then. | `cd parse-leftover/parse-hello-gap && cargo run --release` |
| #34 (landed) | Short strings (`len ≤ 23`) skip `Wire::ensure`. Hello Parse leftover shrank; 4 KiB did not. | — |
| #36 | After #34, leftover is `merge_inner` wrapper (Default 48 B vs 24 B, `CachedSize::dirty`). Do not sum isolated proxies. Do not mix hosts with #31. | `cd parse-leftover/parse-hello-delta && cargo run --release` |
| #39 | Flatten `merge_from_bytes` → `merge_inner` made hello Parse worse (~24.5 → ~32 ns). Do not retry that flatten. | see `flatten-merge-inner.md` |
| #41 | 4 KiB still `Wire::ensure`s the 4099-byte parent frame. Leftover ~21–23 ns vs prost (reconstruct already slower than prost). | `cd parse-leftover/parse-4kib-delta && cargo run --release` |
| name_80 | Combined leftover vs prost. 80-byte name is almost-whole: payload `Arc<[u8]>` + `from_utf8_payload`, not parent ensure, not SSO. String arm already slower than prost full decode. Do not sum proxies. Do not mix hosts with the M4 Pro survey row. | `cd parse-leftover/parse-name80-delta && cargo run --release` |

Verified codec line of record remains **#31: 52.2 vs 25.8 ns** hello combined.
Do not write other VM numbers into `docs/status.md`.

Needs rustc ≥ 1.88 and `protoc`. Same timer as `tonic-bench` (40000 × 15,
median, release thin-LTO). Not in CI. Not `cargo test --workspace`.
