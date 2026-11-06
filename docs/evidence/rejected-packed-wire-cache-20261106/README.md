# Rejected packed-wire encoding prototype

**Rejected and unshipped.** Fresh canonical encoding improved, but cached,
small owned-value, and late-noncanonical controls regressed. This candidate
does not meet the performance goal. Its separate cache-recovery fix is
independently useful.

This is a local shared-host diagnostic, not a verified-hardware result or
a performance ranking. Full TAT fresh encoding still loses to the actual
Prost `encode_to_vec` reference in the candidate binary: 381.17 versus
340.21 ns/op. The growing-buffer Prost API takes 451.06 ns/op there;
both reference APIs remain visible below.

## All measured rows

CPU columns are medians of nine samples per variant. Ratios are the median
of nine paired candidate/baseline ratios; a ratio above one is slower.
Ranges include every pair. Consequently, a ratio need not equal the ratio
of the two separately rounded CPU medians. All 432 CPU processes passed;
no row or slow sample was dropped or rerun.

The coordinator paused the active selected-bytes Miri process and held
other local builders and container-readiness work for this short run.
Those readiness statements are attributed in `quiet-slot.json`; this
does not establish exclusive access to the underlying shared hardware.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh` | 501.56 → 381.17 | 0.7682 (0.6358–0.8385) | 7 → 1 | 0/9 |
| `tat.cached` | 111.84 → 146.63 | 1.3135 (1.1812–1.4813) | 1 → 1 | 9/9 |
| `tat.encode_get` | 513.48 → 498.55 | 0.9661 (0.8875–1.0351) | 7 → 5 | 4/9 |
| `i32.canonical.8.fresh` | 87.08 → 36.30 | 0.4239 (0.3849–0.4347) | 4 → 1 | 0/9 |
| `i32.canonical.8.cached` | 17.59 → 22.25 | 1.2436 (1.1252–1.7927) | 1 → 1 | 9/9 |
| `i32.canonical.8.encode_get` | 87.72 → 84.17 | 0.9509 (0.9026–0.9977) | 4 → 3 | 0/9 |
| `i32.canonical.8.get_encode` | 92.10 → 82.88 | 0.8895 (0.8601–0.9976) | 4 → 3 | 0/9 |
| `i32.canonical.8.owned` | 43.73 → 46.72 | 1.0719 (1.0219–1.2212) | 2 → 2 | 9/9 |
| `i32.canonical.4096.fresh` | 8910.11 → 4053.06 | 0.4549 (0.2084–0.7969) | 20 → 1 | 0/9 |
| `i32.canonical.4096.cached` | 74.43 → 78.58 | 1.0639 (0.9733–1.0967) | 1 → 1 | 7/9 |
| `i32.canonical.4096.encode_get` | 9656.78 → 8210.16 | 0.8448 (0.7955–0.9293) | 20 → 10 | 0/9 |
| `i32.canonical.4096.get_encode` | 9304.57 → 8113.92 | 0.8725 (0.8144–0.9261) | 20 → 10 | 0/9 |
| `i32.canonical.4096.owned` | 5094.04 → 5150.08 | 0.9963 (0.8990–1.0644) | 11 → 11 | 4/9 |
| `i32.early.8.fresh` | 86.52 → 92.00 | 1.0654 (0.9783–1.0886) | 4 → 4 | 8/9 |
| `i32.late.4096.fresh` | 8533.94 → 12290.91 | 1.4338 (1.3033–1.5271) | 20 → 20 | 9/9 |
| `bool.normalize.8.fresh` | 66.93 → 70.80 | 1.0591 (0.9054–1.1193) | 3 → 3 | 7/9 |
| `fixed32.canonical.4096.fresh` | 3508.15 → 3522.70 | 1.0195 (0.9736–1.0500) | 1 → 1 | 5/9 |
| `fixed32.canonical.4096.cached` | 3074.25 → 3093.49 | 1.0063 (0.9398–1.0758) | 1 → 1 | 6/9 |

The unchanged Prost implementations are also retained in both linked
binaries. Their median paired ratios shift by 4.7–12.0%. Host variation
and changed compiler/linker output remain confounds; these records do not
isolate the cause of every timing change. No reference-based adjustment
was applied to the pbrs results.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh / prost-grow` | 425.71 → 451.07 | 1.0632 (0.7592–1.1234) | 5 → 5 | 7/9 |
| `tat.fresh / prost-reserve` | 308.41 → 340.21 | 1.0992 (0.9694–1.3214) | 1 → 1 | 7/9 |
| `tat.cached / prost-grow` | 426.18 → 467.94 | 1.1120 (0.9601–1.1632) | 5 → 5 | 8/9 |
| `tat.cached / prost-reserve` | 281.74 → 310.47 | 1.1204 (0.9950–1.1503) | 1 → 1 | 8/9 |
| `tat.encode_get / prost-grow` | 428.84 → 450.56 | 1.0469 (0.7871–1.1126) | 5 → 5 | 7/9 |
| `tat.encode_get / prost-reserve` | 317.47 → 348.94 | 1.0874 (0.9973–1.5660) | 1 → 1 | 8/9 |

## What was compared

The baseline is Git `e72629e4` plus the independent `take_vec` recovery
guard. The candidate adds a default-false `PackedCodec::is_canonical_wire`
hook and a private wire/recoded cache. Built-in codecs reuse retained wire
only when it exactly matches the old decode/encode result. Noncanonical
input takes the old normalization path after the predicate scan. Custom
codecs retain the conservative fallback unless they opt in. Validation,
unknown fields, and lazy child-message bytes remain covered by controls.

Full TAT uses the bundled pbrs message and the full generated Prost
0.13.5 message, retained by value with its provenance. The fixture emits
87 bytes in pbrs and 83 in Prost: pbrs includes default map-entry scalar
tags that Prost omits. Every output is decoded and compared outside the
measured window. Both Prost `encode` into an initially empty Vec and
`encode_to_vec` are measured; the latter is the faster reference here.
Current Prost 0.14, RPC transport, and codegen are outside this screen.

Other rows encode one packed field with exact output reservation. Fresh
objects are parsed/validated before measurement in batches of 32; their
first encode initializes no cache beforehand. Sixteen independent warmup
objects are discarded before measurement. Cached rows reuse a separately
warmed object. Getter-order rows count field access inside the window;
owned rows prepare their values outside it. Parsing, construction,
output checks, and destruction are excluded. Bounded pools may stay in
processor caches. This is not an end-to-end message lifecycle comparison.

Allocation binaries contain no measurement clocks; CPU binaries contain
no allocation counters. All 48 final allocation processes passed. Fresh
TAT falls from seven allocation calls to one, matching the best Prost
API's one call; encode-then-get falls from seven to five and still exceeds
Prost's one. Owned, noncanonical, and fixed-width allocation counts stay
unchanged. Requested-byte totals remain in the raw records.

An earlier 48-process allocation diagnostic incorrectly reported the
87-byte TAT input length as the Prost output length. All records and its
source are retained. The final harness reports the actual 83-byte Prost
output and asserts every output length. Counters, requested bytes,
checksums, and semantic flags match the earlier diagnostic exactly;
only 12 output-size metadata fields changed. No CPU run used the older
harness. An initial harness iterator compile error is also retained.

## Source and correctness evidence

Both variants were built sequentially in one canonical source root and
one harness root, with identical package IDs, crate metadata, manifests,
lockfiles, compiler, and release flags (thin LTO, one codegen unit).
CPU and allocation executables were separately frozen before the run.
Only `src/packed.rs` differs between the measured variants. The sparse
runtime workspace was reduced to `["."]` and lockfiles resolved offline;
these exact adaptations are archived. Package-path differences are
controlled. Changed source/code size and linked placement are not.

Final candidate source is `a7d0f416…8eaf313f`; the guard-only baseline is
`3995c4a2…996d308`. The candidate passes 114 minimal-runtime tests and
strict Clippy. Final targeted strict-provenance Miri runs execute seven
tests on each of 64-bit and interpreted i686: recovery, concurrent access,
layout, hook panic, and malformed windows. The independent guard adds
four Miri recovery executions across those targets. Interpreted i686 is
not native 32-bit execution. Private/public sizes remain 104/8 bytes on
64-bit and 52/4 on interpreted 32-bit; no new unsafe code was introduced.

The recovery bug predates the optimization: an Err or panic during custom
decode could leave stale encoded bytes that survived a later append.
Two baseline red tests and guard-only green tests are retained separately.
The measured baseline already includes the fix, so its correctness gain
is not counted as an optimization gain. Earlier source-bound test stages
remain archived without being added to the final-source test count.

Run `python3 docs/evidence/rejected-packed-wire-cache-20261106/check.py`
from a checkout containing the named Git base. The checker discovers the
actual checkout, checks archived hashes, reconstructs unchanged Git inputs,
binds build/measurement records, recalculates all ratios and counts, and
rejects six deliberate accounting errors. It does not execute archived
source, replay tests/timings, or qualify the host. The compressed archive
contains text sources and records; no executables or full worktrees.
