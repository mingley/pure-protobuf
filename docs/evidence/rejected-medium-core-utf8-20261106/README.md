# Rejected medium-string core UTF-8 validator

**REJECTED; unshipped.** Replacing one validator call improved the primary Name80 paired median by 6.6022%, but nine pbrs representatives regressed by more than 2%. Four representatives had a paired median more than 1% slower **and every one of their nine pairs was slower**. Five unchanged Prost controls shifted by more than 2%, so timing attribution is also inconclusive under the rule frozen before measurement. All results remain unadjusted. This packet establishes no full-devloop win, production speedup, feature superiority or ranking.

This is a local shared-host diagnostic at Git base `10d94697dc89e3aa89c843720f7150f1186f417a`. Main was unchanged. The candidate changed only `src/lazy.rs`: the existing checked exact-owned string branch of 24 through 256 bytes used `core::str::from_utf8` directly instead of the SIMD wrapper, with the same `invalid utf-8` error mapping and validation before allocation. Lengths 24 through 63 already used core validation through that wrapper. Inline strings, larger strings, unchecked parsing, the global validator and genuine shared-window storage were unchanged. A shared-input Name80 still takes the changed exact-owned branch; it is not an unchanged control.

Run the read-only, Python-standard-library checker from a Git checkout containing the named base:

```sh
python3 -B docs/evidence/rejected-medium-core-utf8-20261106/check.py
```

The checker discovers the checkout from the packet's location, verifies all archived bytes and the original Git inputs, recomputes the fixed schedule and every result, and rejects ten deliberately corrupted accounting controls plus one source-integrity control. The accounting fixtures reseal their derived hashes in memory and must fail at their named semantic assertion, so their rejection does not rely on an earlier hash mismatch. It reads text and Git objects only; it does not build or execute archived code, invoke a benchmark, or recover missing executables. The compressed archive holds source, records, logs and selected assembly by value. No executables, targets or full source trees are included. Absolute paths in historical records describe provenance; they are not replay requirements.

## Every measured row

CPU values are medians in ns per operation. The ratio is the **median of nine paired candidate/baseline CPU ratios**; the parentheses give the entire paired range, and values above 1 are slower. It is not the ratio of the two separate CPU medians. Allocation calls and requested bytes come from separate, clock-free counting binaries, 128 operations per cell; every corresponding baseline/candidate count and layout matched. All 756 CPU captures and 84 allocation captures passed. No row, process or slower control was dropped, and no rerun or tuning followed.

| Case / peer | CPU ns/op baseline → candidate | Paired ratio (all-pair range) | Slower pairs | Allocation calls/op | Requested bytes/op |
| --- | ---: | ---: | ---: | ---: | ---: |
| `name.ascii.23.borrowed / pbrs` | 33.7546 → 33.8588 | 1.01219 (0.94474–1.21587) | 6/9 | 0 → 0 | 0 → 0 |
| `name.ascii.23.borrowed / prost` | 29.5334 → 30.1322 | 1.01677 (0.97139–1.10108) | 7/9 | 1 → 1 | 23 → 23 |
| `name.ascii.24.borrowed / pbrs` | 38.1069 → 38.8309 | 1.01394 (0.91367–1.19654) | 5/9 | 1 → 1 | 24 → 24 |
| `name.ascii.24.borrowed / prost` | 31.1970 → 32.1493 | 1.03824 (0.91054–1.41585) | 7/9 | 1 → 1 | 24 → 24 |
| `name.ascii.63.borrowed / pbrs` | 39.4940 → 39.0661 | 0.98789 (0.60295–1.05005) | 3/9 | 1 → 1 | 63 → 63 |
| `name.ascii.63.borrowed / prost` | 34.6150 → 35.0113 | 1.00795 (0.92337–1.06347) | 6/9 | 1 → 1 | 63 → 63 |
| `name.ascii.64.borrowed / pbrs` | 35.2001 → 40.2942 | 1.14472 (0.88352–1.21998) | 8/9 | 1 → 1 | 64 → 64 |
| `name.ascii.64.borrowed / prost` | 28.9067 → 28.8767 | 1.02168 (0.92715–1.14512) | 5/9 | 1 → 1 | 64 → 64 |
| `name.ascii.65.borrowed / pbrs` | 44.7173 → 40.5091 | 0.91218 (0.84796–1.18738) | 1/9 | 1 → 1 | 65 → 65 |
| `name.ascii.65.borrowed / prost` | 29.7677 → 29.5660 | 0.98757 (0.89626–1.13543) | 2/9 | 1 → 1 | 65 → 65 |
| `name.ascii.80.borrowed / pbrs` | 44.8488 → 41.4183 | 0.93398 (0.88619–1.24303) | 2/9 | 1 → 1 | 80 → 80 |
| `name.ascii.80.borrowed / prost` | 28.5907 → 28.3773 | 0.99323 (0.90977–1.72556) | 4/9 | 1 → 1 | 80 → 80 |
| `name.ascii.128.borrowed / pbrs` | 36.5196 → 42.1911 | 1.15640 (1.00620–1.47423) | 9/9 | 1 → 1 | 128 → 128 |
| `name.ascii.128.borrowed / prost` | 35.0988 → 35.3457 | 1.00211 (0.96904–1.11097) | 5/9 | 1 → 1 | 128 → 128 |
| `name.ascii.255.borrowed / pbrs` | 47.1093 → 47.0132 | 0.98143 (0.91816–1.01201) | 3/9 | 1 → 1 | 255 → 255 |
| `name.ascii.255.borrowed / prost` | 42.2029 → 41.9356 | 0.99261 (0.95145–1.04186) | 3/9 | 1 → 1 | 255 → 255 |
| `name.ascii.256.borrowed / pbrs` | 43.7604 → 48.5875 | 1.12253 (0.92788–1.20017) | 8/9 | 1 → 1 | 256 → 256 |
| `name.ascii.256.borrowed / prost` | 38.0104 → 39.7197 | 1.04014 (0.95451–1.16579) | 6/9 | 1 → 1 | 256 → 256 |
| `name.ascii.257.borrowed / pbrs` | 76.9125 → 75.4477 | 0.98924 (0.95194–1.06456) | 4/9 | 1 → 1 | 280 → 280 |
| `name.ascii.257.borrowed / prost` | 41.3423 → 42.4359 | 1.00031 (0.82160–1.11128) | 5/9 | 1 → 1 | 257 → 257 |
| `name.ascii.4096.borrowed / pbrs` | 139.9362 → 139.9668 | 0.98507 (0.85542–1.05630) | 3/9 | 1 → 1 | 4112 → 4112 |
| `name.ascii.4096.borrowed / prost` | 163.7532 → 160.1174 | 0.98837 (0.83080–1.78727) | 3/9 | 1 → 1 | 4096 → 4096 |
| `name.utf8_2.80.borrowed / pbrs` | 52.2707 → 71.8478 | 1.37483 (1.07605–1.59742) | 9/9 | 1 → 1 | 80 → 80 |
| `name.utf8_2.80.borrowed / prost` | 65.1895 → 64.5569 | 1.00264 (0.91130–1.28619) | 5/9 | 1 → 1 | 80 → 80 |
| `name.utf8_3.80.borrowed / pbrs` | 54.7076 → 74.6175 | 1.36840 (0.90248–1.51538) | 8/9 | 1 → 1 | 80 → 80 |
| `name.utf8_3.80.borrowed / prost` | 64.6088 → 66.4144 | 1.00388 (0.97082–1.38212) | 6/9 | 1 → 1 | 80 → 80 |
| `name.utf8_4.80.borrowed / pbrs` | 52.1262 → 74.4598 | 1.41256 (1.26329–1.76066) | 9/9 | 1 → 1 | 80 → 80 |
| `name.utf8_4.80.borrowed / prost` | 68.1110 → 68.8474 | 1.01067 (0.95030–1.05745) | 7/9 | 1 → 1 | 80 → 80 |
| `name.invalid_early.80.borrowed / pbrs` | 15.8929 → 9.8693 | 0.62220 (0.29082–0.74069) | 0/9 | 0 → 0 | 0 → 0 |
| `name.invalid_early.80.borrowed / prost` | 54.0234 → 52.6130 | 0.99542 (0.89938–1.06468) | 3/9 | 3 → 3 | 280 → 280 |
| `name.invalid_late.80.borrowed / pbrs` | 22.1024 → 18.8611 | 0.88110 (0.69281–1.04157) | 1/9 | 0 → 0 | 0 → 0 |
| `name.invalid_late.80.borrowed / prost` | 59.0105 → 58.8299 | 0.99687 (0.92828–1.05700) | 4/9 | 3 → 3 | 280 → 280 |
| `name.invalid_cross64.80.borrowed / pbrs` | 15.9651 → 17.2666 | 1.08325 (1.00442–1.44726) | 9/9 | 0 → 0 | 0 → 0 |
| `name.invalid_cross64.80.borrowed / prost` | 59.3747 → 58.9529 | 1.00531 (0.62670–1.11877) | 5/9 | 3 → 3 | 280 → 280 |
| `name.partial_4.80.borrowed / pbrs` | 21.8724 → 20.2450 | 0.88291 (0.67186–1.63781) | 2/9 | 0 → 0 | 0 → 0 |
| `name.partial_4.80.borrowed / prost` | 59.6722 → 59.0269 | 0.97589 (0.84699–1.14025) | 3/9 | 3 → 3 | 280 → 280 |
| `name.ascii.80.shared / pbrs` | 60.1735 → 58.0237 | 0.95581 (0.90942–1.01100) | 2/9 | 1 → 1 | 80 → 80 |
| `name.ascii.80.shared / prost` | 33.6518 → 34.2414 | 1.01191 (0.78267–1.08607) | 6/9 | 1 → 1 | 80 → 80 |
| `pad.ascii.80.borrowed / pbrs` | 94.4977 → 95.5402 | 1.03690 (0.82294–1.27557) | 6/9 | 1 → 1 | 232 → 232 |
| `pad.ascii.80.borrowed / prost` | 66.2395 → 65.6986 | 0.99183 (0.94507–1.01567) | 2/9 | 3 → 3 | 336 → 336 |
| `pad.ascii.80.shared / pbrs` | 90.1201 → 89.5572 | 1.02695 (0.89265–1.26744) | 6/9 | 1 → 1 | 128 → 128 |
| `pad.ascii.80.shared / prost` | 71.2740 → 71.0454 | 0.97792 (0.93305–1.12006) | 4/9 | 2 → 2 | 208 → 208 |

The candidate still loses the primary whole-message row to pinned Prost 0.14.4: 41.4183 versus 28.3773 ns/op in the candidate artifact. The primary paired range includes losses. ASCII128, UTF-8 two-byte and four-byte, and invalid-cross64 rows satisfy the repeated-loss gate. In particular ASCII128's minimum ratio is 1.00620 and invalid-cross64's is 1.00442: the rule requires **median > 1.01 and all nine ratios > 1.0**, not every pair > 1.01. The frozen `RESULTS.md` in the archive contains the imprecise sentence “four controls are slower in all nine pairs by more than 1%”; its original bytes are retained as provenance, and this paragraph supplies the correction. The checker uses the exact conjunction.

## Source and operation

Both variants were built sequentially in the same canonical source and harness paths, with equal package IDs, locks, recorded compiler commands, profiles and crate metadata. Of 166 runtime source inputs, only `src/lazy.rs` differs. Runtime features were `std,json,reflect,text`; the standalone harness omitted full-devloop codegen, conformance, copy counters and surrounding dispatch. Release settings were opt-level 3, ThinLTO and one codegen unit, on Rust 1.99.1 / LLVM 23.1.1, with no target-specific RUSTFLAGS. The archive retains the manifests and lock files, including the sparse-workspace adaptation, plus their original files at the Git base. No dependency version change is a claimed optimization.

The pbrs caller uses the actual default-generated `Name` from `proto/codec_cases.proto`, regenerated byte-identically before freeze. It does not substitute the handwritten test type or enable owned-message fields. Its declaration uses `CachedSize`, backed by AtomicU64 in this std configuration. The native artifacts observed Name size 56/alignment 8, LazyStr 40, ProtoString 32, UnknownFields 8 and cache 8; no field offsets or native 32-bit layout are claimed. The peer is an actual Prost 0.14.4 derive of the matching one-string message. Known-padding controls use matching schemas on both sides, not unknown-field padding.

The valid inner operation is parse → getter → byte length, with the black box before the statement temporary drops, as in the registered Name80 benchmark. Fixture construction, a 4096-byte dispatch warmup, 256 cell warmups, full-content checks, CLI work and JSON output occur outside measurement. Shared-input promotion occurs outside measurement; its clone remains inside. The fixed schedule uses seed 20261106, nine shuffled paired blocks and alternating baseline/candidate-first order. CPU and clock-free allocation executables are separate. The rule required a primary paired gain of at least 2%, blocked any pbrs paired median regression over 2%, and also blocked a median regression over 1% with all nine pairs slower. An unchanged-Prost median shift over 2% makes attribution inconclusive. A pass could only advance to a matched full-devloop confirmation, never shipping or a ranking.

## Correctness and limits

Both variants passed 113 minimal native library tests, four focused tests and four actual generated-harness tests. Candidate strict Clippy and the one targeted Miri test on x86_64 plus interpreted i686 passed; i686 is interpreter evidence, not native execution or a platform-wide proof. The direct helper controls cover 891 malformed strings per variant across eleven byte lengths, nine invalid patterns, three frame suffixes and three backing states. Eighteen outer real-parser errors per variant also matched exactly. These checks retain existing error-before-copy allocation behavior, slot identity, parser cursor and prior-field preservation; the unchanged >256 exact-copy error path can allocate before validation. Valid Unicode includes two-, three- and four-byte code points and U+10FFFF. Invalid controls include overlong forms, surrogates, above-maximum scalars, partial endings and a sequence crossing the 64-byte boundary.

Two early successful baseline receipt inventories refer to a three-test integration file that was not compiled by the library-only or separate harness command; the focused baseline receipt compiled the four-test file before formatting. The archived `receipt-sources/` snapshots were **reconstructed later** to match those already-recorded SHA-256 digests, not captured contemporaneously. The checker binds each reconstructed byte sequence to its original receipt. Final candidate/native/Miri and benchmark inputs use the final formatted four-test source. All original successful gate records and logs are retained; the capsule does not replay them.

The independent assembly review retains twelve selected regions per CPU artifact, exact binary/source/build bindings and ELF relocation resolutions. The measured Name80 path keeps validation, malloc(80), memcpy(80) and drop/free on each iteration in both variants; no asymmetric allocation, copy or validation hoisting was observed on that path. Normalized Prost decode and timed loops are equal across variants. This is static selected-code evidence, not a trace of the runtime SIMD target, a causal timing explanation or exhaustive assembly equivalence.

These System-allocator CPU binaries are a minimal context. The registered devloop uses allocation/copy instrumentation and places outer codec dispatch inside its iteration loop; this harness dispatches peer/shape outside that loop. Exclusive hardware access, frequency control and cross-machine reproducibility were not established. Some malformed rows are very short, and all scatter is retained. Pbrs's unit ParseError performs less error-detail work than Prost's owned DecodeError stack, so invalid-input timing does not demonstrate equivalent error features. The unchanged reference shifts remain visible and are not used to adjust pbrs results. Full-devloop confirmation was not run because the candidate failed the screen.
