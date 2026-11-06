# Rejected cache-first packed prototype

**Rejected and unshipped.** Cache-first static dispatch improves fresh
encoding in this screen, but large getter-order and owned controls regress
in all nine pairs. It does not meet the all-control performance goal.

Full TAT fresh encoding measures 295.02 ns/op versus the candidate binary's
best actual Prost API, `encode_to_vec`, at 310.67 ns/op. TAT encode-then-get
still loses: 461.33 versus 312.33 ns/op. The growing-buffer Prost API and
both reference implementations in both linked binaries remain visible.
This is a shared-host diagnostic, not a verified-hardware or world ranking.

## Every measured row

CPU columns are per-variant medians. Ratios are medians of nine paired
candidate/baseline ratios; above one means slower. Ranges include every
pair, so a ratio need not equal the ratio of the rounded CPU medians.
All 432 CPU and 48 clock-free allocation processes passed. No row, slow
sample, reference API, or status was dropped, adjusted, or rerun. Source
and annotations stayed frozen throughout measurement.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh` | 496.28 → 295.02 | 0.5882 (0.5734–0.6106) | 7 → 3 | 0/9 |
| `tat.cached` | 115.95 → 112.43 | 0.9695 (0.8518–1.7534) | 1 → 1 | 2/9 |
| `tat.encode_get` | 519.87 → 461.33 | 0.9069 (0.8154–0.9332) | 7 → 7 | 0/9 |
| `i32.canonical.8.fresh` | 86.21 → 42.89 | 0.5003 (0.4840–0.5196) | 4 → 2 | 0/9 |
| `i32.canonical.8.cached` | 17.71 → 17.57 | 0.9937 (0.9149–1.1550) | 1 → 1 | 4/9 |
| `i32.canonical.8.encode_get` | 89.31 → 90.04 | 1.0075 (0.8843–1.1845) | 4 → 4 | 5/9 |
| `i32.canonical.8.get_encode` | 89.59 → 90.73 | 0.9851 (0.9238–1.0435) | 4 → 4 | 4/9 |
| `i32.canonical.8.owned` | 44.22 → 44.49 | 0.9969 (0.8682–1.1141) | 2 → 2 | 4/9 |
| `i32.canonical.4096.fresh` | 8612.37 → 5858.12 | 0.6850 (0.5040–0.7232) | 20 → 2 | 0/9 |
| `i32.canonical.4096.cached` | 73.58 → 72.03 | 0.9719 (0.8507–1.0121) | 1 → 1 | 4/9 |
| `i32.canonical.4096.encode_get` | 9222.88 → 9917.79 | 1.0680 (1.0243–1.1140) | 20 → 11 | 9/9 |
| `i32.canonical.4096.get_encode` | 9018.48 → 10084.15 | 1.1105 (1.0315–1.1582) | 20 → 11 | 9/9 |
| `i32.canonical.4096.owned` | 5097.04 → 6090.50 | 1.1964 (1.0438–1.2545) | 11 → 11 | 9/9 |
| `i32.early.8.fresh` | 86.55 → 43.02 | 0.4933 (0.4230–0.5335) | 4 → 2 | 0/9 |
| `i32.late.4096.fresh` | 8637.41 → 5867.63 | 0.6805 (0.6604–0.7015) | 20 → 2 | 0/9 |
| `bool.normalize.8.fresh` | 68.12 → 41.98 | 0.6031 (0.5515–0.6633) | 3 → 2 | 0/9 |
| `fixed32.canonical.4096.fresh` | 3535.74 → 3534.58 | 0.9983 (0.9309–1.0480) | 1 → 1 | 4/9 |
| `fixed32.canonical.4096.cached` | 3096.53 → 3098.25 | 1.0050 (0.9714–1.0300) | 1 → 1 | 6/9 |

Unchanged Prost implementations improve by 5.4–10.0% in their median
paired ratios across the linked binaries. No reference adjustment was
applied to pbrs. Code/link placement, heap/cache placement, and shared-host
variation remain confounds. A local fresh win cannot establish a broad
ranking, and the cached TAT range includes a 1.7534 slow pair.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| `tat.fresh / prost-grow` | 462.32 → 412.39 | 0.9004 (0.8712–1.1244) | 5 → 5 | 1/9 |
| `tat.fresh / prost-reserve` | 333.57 → 310.67 | 0.9457 (0.9241–0.9704) | 1 → 1 | 0/9 |
| `tat.cached / prost-grow` | 481.57 → 441.10 | 0.9115 (0.8605–0.9660) | 5 → 5 | 0/9 |
| `tat.cached / prost-reserve` | 301.07 → 280.65 | 0.9260 (0.7323–0.9678) | 1 → 1 | 0/9 |
| `tat.encode_get / prost-grow` | 465.91 → 425.43 | 0.9143 (0.8616–0.9666) | 5 → 5 | 0/9 |
| `tat.encode_get / prost-reserve` | 335.85 → 312.33 | 0.9354 (0.8873–0.9874) | 1 → 1 | 0/9 |

## Design and limits

The baseline is Git `e72629e4` plus the reviewed `take_vec` recovery guard.
The candidate retains the original `OnceLock<Vec<u8>>` and all layouts.
Built-in scalar codecs opt into a static capability and normalization
method. A ready encoded Vec returns before `is_empty`, decoded state,
Wire bounds, or codec dispatch. A fixed cold/never-inline helper handles
misses. It uses the earlier fused normalizer: copy a canonical prefix once,
then normalize remaining varints directly; entirely canonical wire is
copied after scanning. No typed Vec is needed for that first wire encode.

Default custom and fixed-width codecs keep the original complete path.
This restriction preserves a custom codec that decodes no values but
emits nonempty bytes: its first packed-byte call returns bytes and its
second returns None. The opt-in law requires successful deterministic
decoding, nonempty values for nonempty validated wire, and normalization
equivalent to decoding then encoding. Existing custom implementations
inherit no additional requirement. An opt-in codec with nonempty values
and empty encoded bytes still returns Some(empty).

Fresh TAT allocation calls fall from seven to three, requesting
199 → 103 bytes. Cached TAT stays at one call; encode-then-get stays at
seven calls and 199 requested bytes. Late noncanonical 4096-value encoding
falls from twenty calls to two. Owned/fixed-width counts stay unchanged.
Every requested-byte total is retained. Best Prost TAT `encode_to_vec`
needs one allocation, so allocation parity is not established here.

An optimized, separately linked, never-executed inspection consumer shows
an i32 wrapper shrinking from 173 to 35 bytes. Its cache-hit path has no
stack setup, decoded/Wire reads, or encoder-address preparation. Fixed and
default-custom wrappers keep matching normalized instruction streams
(264 and 173 bytes). The separate helper is 206 bytes. The compiler emits
an ordinary miss call with an eight-byte stack-alignment adjustment;
the proposal's tail-jump expectation did not materialize. No annotation
was changed in response. This tiny consumer does not qualify full TAT
assembly or prove the cause of a timing change.

Owned misses still pay the cache probe and helper call. Encode-then-get
still scans wire and later decodes typed values. Those costs and the large
losing rows remain visible. The earlier predicate/marker and fused-Vec
rejections remain frozen and are not replaced by this experiment.

## Workload and source controls

The full bundled pbrs TAT and full generated Prost 0.13.5 TAT are used,
with reference source/provenance archived by value. pbrs emits 87 bytes
and Prost 83: pbrs includes default map-entry scalar tags that Prost omits.
Actual output sizes are correct from the outset. Every full-message output
is decoded and compared outside the measured window. Separate probes
preserve unknown fields and lazy child-message wire. Current Prost 0.14,
RPC transport, and codegen are not measured by this capsule.

Other rows encode one packed field with exact output reservation. Fresh
objects are parsed and validated before measurement in batches of 32.
Sixteen independent warmup objects are discarded before the measured
fresh pool. Cached rows use a separately warmed object. Getter-order rows
include field access inside the window; owned values are prepared before
it. Parsing, construction, output validation, and destruction are excluded.
Small pools may fit CPU caches; this is not a full lifecycle comparison.

CPU binaries have no allocation counters; allocation binaries have no
measurement clocks. Both variants were built sequentially at one new
canonical source/harness path, with identical package IDs, crate metadata,
compiler, profile, manifests, and lockfiles. Only `src/packed.rs` differs
in the measured source inventories. The sparse workspace is reduced to
["."] and dependencies resolved offline; exact adaptations are archived.
Thin LTO, one codegen unit, and zero debug info are recorded. Package-path
differences are controlled within this experiment. Source/code size and
linked placement change. Do not compare raw times across the three
experiment roots as matched measurements.

The coordinator reported completed local build/interpreter/proof work
and idle or read-only agents before the short authorized slot. That
statement is attributed in `quiet-slot.json`; exclusive access to the
underlying shared hardware is not established. CPU affinity is not pinned.

## Correctness and verification

Candidate source is `d9b43c9b…79091`; guarded baseline is
`3995c4a2…996d308`. Ten shared golden tests pass on the baseline. The
candidate passes 121 minimal-runtime tests, strict Clippy, and independent
source review before freeze. Twenty-eight targeted strict-provenance
Miri executions cover the actual normalizer, errors/windows, signed
expansion, lifetime/layout/concurrency, cache mutation, opt-in empty output,
fixed-width bypass, custom empty decoding, and recovery: eight fused,
three cache-first, one shared adversarial, and two recovery tests on each
of 64-bit and interpreted i686. Interpreted i686 is not native 32-bit
execution. Private/public sizes stay 104/8 and 52/4 bytes respectively.
No new unsafe code is introduced. The correctness guard is already in
the baseline and is not counted as an optimization gain.

Run `python3 docs/evidence/rejected-packed-cache-first-20261106/check.py`
in a checkout containing the named Git base. The checker finds its actual
checkout, checks archived hashes and unchanged Git inputs, binds build and
measurement records, recalculates every ratio/allocation count, checks the
limited assembly record, and rejects six accounting corruptions. It does
not execute archived source, replay tests/timings, or qualify the host.
The archive contains text sources and records, no executables/full trees.
