# Rejected packed readiness-routing prototype

**Rejected and unshipped.** Sending materialized values through the ordinary encoder leaves a material encode-then-getter loss in this screen: the large i32 case is slower in all nine pairs. Fresh encoding improves, but this candidate does not meet the all-control performance goal. No source or annotation was tuned after freeze, and no timing was rerun.

Fresh full-message TAT encoding is 294.45 ns/op versus the candidate binary's best actual Prost API, `encode_to_vec`, at 349.76 ns/op. TAT encode-then-getter still loses to that reference: 464.55 versus 363.90 ns/op. The growing-buffer API and both full reference implementations in both linked binaries remain visible. This is a shared-host diagnostic, not a verified-hardware or world ranking.

## Every measured row

CPU columns are per-variant medians. Ratios are medians of nine paired candidate/baseline ratios; above one means slower. Ranges include every pair and are observed ranges, not confidence intervals. A paired median need not equal the ratio of the rounded CPU medians. Every 432 CPU and 48 clock-free allocation process passed. No row, slow sample, reference, failure status, or control was dropped or adjusted.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Requested bytes/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| `tat.fresh` | 528.29 → 294.45 | 0.5671 (0.5286–0.6140) | 7 → 3 | 199 → 103 | 0/9 |
| `tat.cached` | 115.25 → 112.11 | 0.9686 (0.8218–1.0918) | 1 → 1 | 87 → 87 | 4/9 |
| `tat.encode_get` | 513.50 → 464.55 | 0.8918 (0.7282–0.9927) | 7 → 7 | 199 → 199 | 0/9 |
| `i32.canonical.8.fresh` | 87.54 → 43.45 | 0.5070 (0.4830–0.6265) | 4 → 2 | 66 → 18 | 0/9 |
| `i32.canonical.8.cached` | 18.12 → 17.57 | 0.9921 (0.9016–1.0778) | 1 → 1 | 10 → 10 | 2/9 |
| `i32.canonical.8.encode_get` | 87.89 → 95.13 | 1.0621 (0.9991–1.1360) | 4 → 4 | 66 → 66 | 8/9 |
| `i32.canonical.8.get_encode` | 92.38 → 93.24 | 1.0195 (0.9142–1.1305) | 4 → 4 | 66 → 66 | 5/9 |
| `i32.canonical.8.owned` | 43.94 → 45.11 | 1.0112 (0.9038–1.1152) | 2 → 2 | 18 → 18 | 7/9 |
| `i32.canonical.4096.fresh` | 8565.96 → 6982.75 | 0.8140 (0.6800–0.8425) | 20 → 2 | 44987 → 8195 | 0/9 |
| `i32.canonical.4096.cached` | 73.97 → 73.54 | 0.9941 (0.8543–1.1028) | 1 → 1 | 4099 → 4099 | 4/9 |
| `i32.canonical.4096.encode_get` | 9410.35 → 11163.69 | 1.2034 (1.1195–1.2330) | 20 → 11 | 44987 → 40899 | 9/9 |
| `i32.canonical.4096.get_encode` | 9303.17 → 9283.77 | 1.0036 (0.9152–1.0691) | 20 → 20 | 44987 → 44987 | 5/9 |
| `i32.canonical.4096.owned` | 5064.16 → 5087.53 | 1.0025 (0.9072–1.0934) | 11 → 11 | 12283 → 12283 | 6/9 |
| `i32.early.8.fresh` | 86.14 → 42.05 | 0.4865 (0.4584–0.5321) | 4 → 2 | 66 → 19 | 0/9 |
| `i32.late.4096.fresh` | 8772.80 → 5867.27 | 0.6736 (0.6265–0.7875) | 20 → 2 | 44987 → 8196 | 0/9 |
| `bool.normalize.8.fresh` | 67.06 → 42.61 | 0.6190 (0.5804–0.6489) | 3 → 2 | 26 → 18 | 0/9 |
| `fixed32.canonical.4096.fresh` | 3491.31 → 3623.64 | 1.0221 (0.9429–1.0676) | 1 → 1 | 16388 → 16388 | 6/9 |
| `fixed32.canonical.4096.cached` | 3174.30 → 3168.53 | 1.0038 (0.9392–1.0918) | 1 → 1 | 16388 → 16388 | 5/9 |

The six unchanged Prost controls move 4.7–14.5% slower in their median paired ratios across the two linked binaries. No reference adjustment was applied to pbrs. Code/link placement, heap/cache placement and shared-host variation remain confounds; the observed 20.3% large encode-then-getter loss is not attributed to a single instruction, helper or cache choice. Near-baseline owned and getter-first ratios do not establish equal CPU cost or a causal repair of the earlier experiments.

| Case | CPU ns/op, baseline → candidate | Paired CPU ratio (range) | Allocation calls/op | Requested bytes/op | Slower pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| `tat.fresh / prost-grow` | 427.89 → 460.89 | 1.0647 (0.9215–1.1516) | 5 → 5 | 248 → 248 | 7/9 |
| `tat.fresh / prost-reserve` | 318.15 → 349.76 | 1.0846 (0.9330–1.1557) | 1 → 1 | 83 → 83 | 8/9 |
| `tat.cached / prost-grow` | 443.63 → 486.50 | 1.0472 (0.8979–1.1844) | 5 → 5 | 248 → 248 | 7/9 |
| `tat.cached / prost-reserve` | 282.08 → 325.30 | 1.1450 (1.0295–1.2837) | 1 → 1 | 83 → 83 | 9/9 |
| `tat.encode_get / prost-grow` | 439.00 → 477.87 | 1.0819 (1.0265–1.1519) | 5 → 5 | 248 → 248 | 9/9 |
| `tat.encode_get / prost-reserve` | 320.09 → 363.90 | 1.1222 (1.0818–1.1614) | 1 → 1 | 83 → 83 | 9/9 |

## Design and correctness

Baseline: Git `e72629e4` plus the independent `take_vec` panic-recovery guard. Candidate: the same guard, original `OnceLock<Vec<u8>>` cache and layout, and the existing opt-in fused varint normalizer. Encoded hits return first. A miss with a ready decoded Vec uses that already-read Vec and the ordinary typed encoder. Only unmaterialized wire enters a fixed cold, non-inlined normalization helper. No capacity, threshold, fixture or attribute sweep was added.

Default custom and fixed-width codecs retain the literal guarded-baseline path. The optional capability defaults to false, preserving the custom codec that decodes no values but emits nonempty bytes on its first call and returns None on the next. Opt-in laws require successful deterministic decoding, nonempty values for nonempty validated wire, and byte equivalence between normalization and decoding then encoding. Opt-in nonempty values may encode to an empty Vec and still return Some(empty). Existing custom implementations inherit no new law.

Ten shared baseline goldens, 122 candidate runtime tests, strict Clippy and independent read-only source review pass. Thirty targeted strict-provenance Miri executions cover the actual normalizer, malformed/truncated windows, signed expansion, lifetime/layout/concurrency, recovery, custom callback order, fixed-width bypass, and cache routing: eight fused, four cache-first, one shared adversarial and two recovery tests on each 64-bit and interpreted i686 target. Interpreted i686 is not native 32-bit execution. Public/private layouts remain 8/104 and 4/52 bytes. No new unsafe code is introduced. The recovery guard is in both variants and is not counted as an optimization gain.

The new route control counts wire normalization, typed decoding and typed encoding for lazy wire, getter-ready wire, owned data, cache reuse and clone. Getter-ready clones now use typed encoding. The original custom/fixed callbacks and state/mutation/clone bodies are checked by literal comparison.

Fresh TAT allocations fall 7→3 while best Prost `encode_to_vec` remains 1. Large i32 fresh allocations fall 20→2; encode-then-getter falls 20→11. Getter-before-encode is 20→20: routing back through the ordinary encoder relinquishes the prior rejected candidate's 11-call result. Owned stays 11→11. Cached and fixed-width allocations are unchanged. Requested bytes are retained above; these counts are not peak live memory.

## Actual full-harness assembly

The actual full-harness i32 field function is 980 baseline / 867 candidate bytes. Decoded-ready misses call the ordinary encoded initializer at `0x50e7f`. Only the wire route calls the 130-byte `encode_wire_cache` at `0xf5350`, via GOT slot `0x17b780`. Those are ordinary calls, not tail jumps.

The typed callback is 149 bytes in both variants with equal normalized instructions. The scalar encoder 332, scalar decoder 297, bulk decoder 1072 and full 1047-byte measured i32 caller also have equal normalized streams; getter callback and SSE2 reduction sequences match. Raw disassembly, selected relocations, normalization rules, symbol addresses and loop alignment are retained. Addresses/alignment differ. This establishes the routing observation, not equal runtime cost or full TAT encoder equivalence. The separately measured prior candidates remain rejected; no old result was replaced.

## Workload and build controls

The full bundled pbrs TAT and full generated Prost/prost-types 0.13.5 TAT are used, with reference source and provenance archived by value. pbrs emits 87 bytes and Prost 83 because pbrs includes default map-entry scalar tags. Actual sizes are correct from the outset. Every full-message output is decoded and compared outside measurement; a separate probe preserves unknown fields and lazy child wire. Current Prost 0.14, transport, codegen and full lifecycle cost are unmeasured.

Other rows encode one packed field with exact output reservation. Fresh values are parsed/validated before measurement in batches of 32. Sixteen independent warmup objects are discarded before the measured pool. Cached rows intentionally use a separately warmed object. Getter-order rows include field access inside the window; owned values are prepared before it. Parsing, construction, output validation and destruction are excluded. Small pools may fit CPU caches.

CPU binaries have no allocation counters; allocation binaries have no measurement clocks and no imported clock_gettime/gettimeofday/timespec_get symbol. All 48 allocation preflight cells passed before CPU timing. Both CPU artifacts were first built, never run, for full-context static inspection; they were later reused after exact source/profile/schema/path/metadata checks. The original static receipts remain unchanged, and derived reuse receipts link them. Allocation variants were then built sequentially at the same canonical source/harness roots. All artifacts use identical within-mode compiler, package IDs, crate metadata, profile, manifests and lockfiles. Only packed.rs differs in runtime source inventories.

The sparse workspace is reduced to ["."] with cached offline dependencies; exact adaptations and original manifests are archived. Thin LTO, one codegen unit, zero debug info and disabled incremental compilation are recorded. Package-path differences are controlled within this experiment. Source size and linked placement change. Comparing absolute times across the four experiment roots is not a matched experiment.

The coordinator reported idle builders, compilers and Miri before the sole authorized slot and held other local CPU work until completion. This is attributed in quiet-slot.json; exclusive hardware access is not established. CPU affinity is not pinned. READINESS.md and benchmark-ready.json in the raw archive are frozen earlier-stage snapshots; decision.json and this page describe the completed experiment.

## Verify the record

Candidate packed.rs SHA starts `0906d8f9`; guarded baseline starts `3995c4a2`. Run `python3 docs/evidence/rejected-packed-readiness-routing-20261106/check.py` from any directory with this packet at that destination in a checkout containing Git base `e72629e4`. The checker locates the actual checkout, validates archive hashes and unchanged Git inputs, binds reused CPU/build/semantic receipts, recalculates every CPU/wall ratio and allocation count, checks the limited full-harness assembly record, and rejects six accounting corruptions. It does not execute archived code, replay tests/timings, or qualify the host. The compressed archive contains text sources and records; no executable or full tree is included.
