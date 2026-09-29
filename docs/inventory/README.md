# Archived experiments

These records preserve closed experiments and their original measurements.
They are useful for avoiding repeated failed experiments, but they do not
describe today's implementation or performance. `#34` landed, `#27` was
superseded by `#42`, and the remaining draft diffs were closed. Current work
lives in the [execution plan](../plan/world-class/README.md).

Later work can adopt a related technique with new evidence. In particular,
current `src/lazy.rs` does copy qualifying medium strings into owned storage;
that does not turn the discarded `#57` result into a measured win.

Cursor drafts [#27](https://github.com/mingley/pure-protobuf/pull/27),
[#32](https://github.com/mingley/pure-protobuf/pull/32),
[#36](https://github.com/mingley/pure-protobuf/pull/36),
[#39](https://github.com/mingley/pure-protobuf/pull/39),
[#41](https://github.com/mingley/pure-protobuf/pull/41), and
[#57](https://github.com/mingley/pure-protobuf/pull/57) were closed so they
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
| #57 | Draft heap-copy try (almost-whole `24..=256` into `ProtoString`). Closed without a demonstrated win; same-host combined encode/parse still lost. Later related implementations need their own evidence. | see `name80-heap-copy.md` |

The historical comparison used **#31: 52.2 vs 25.8 ns** for hello combined.
Keep measurements from different hosts and revisions separate; this archive
does not set the current scoreboard baseline.

Needs rustc ≥ 1.88 and `protoc`. Same timer as `tonic-bench` (40000 × 15,
median, release thin-LTO). Not in CI. Not `cargo test --workspace`.
