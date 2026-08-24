# Discarded: flatten Parse onto `merge_inner` ([#39](https://github.com/mingley/pure-protobuf/pull/39))

Do not merge. Do not retry this flatten as a hello Parse fix.

Non-empty `ClearAndParse::merge_from_bytes` was changed from
`self.merge_bytes(data, 0)` to:

```rust
let mut pos = 0;
let mut wire = None;
self.merge_inner(data, &mut wire, &mut pos, 0, true, None)
```

Empty payloads still ran `check_required` (proto2 required).
`#[inline(always)]` on `merge_bytes` and `merge_loop::<false>` (skip
empty `until` / `check_required`) were also tried on that VM; both
widened the loss.

On that host, saved `main` hello Parse was ~24.5 ns. The flatten binary
was ~31–33 ns. Calling `merge_inner` from the trait impl was slower than
going through generated `merge_bytes`. The extra frame was not the
leftover.

Leftover after #34 is still `merge_inner` glue (Default / dirty / tag
loop). See `parse-hello-delta.md`.
