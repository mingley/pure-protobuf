# Stable map removal, November 6

Removing one key could change an unrelated key's value. For stored entries
`[(2,20), (1,10), (2,21), (2,22)]`, removing key `1` moved `(2,22)` before
`(2,21)`, changing key `2` from `22` to `21`. Stable removal now preserves that
winner in owned maps, typed mutators and arena-backed raw mutators.

Removal still pops only the target key's latest stored occurrence. An earlier
parsed duplicate can become visible. This preserves historical behavior; it
does not establish Prost/HashMap target-deletion parity.

An existing large-map index is repaired without rebuilding or cloning its keys.
Mutation never creates an index, and maps below 16 entries drop it. Taking the
index out before mutation prevents stale cached positions after a key comparison
or destructor panics. The removed key is explicitly dropped before returning
its value, allowing value cleanup if that destructor panics. Double panics can
abort normally; rollback is not promised. Storage layout and public method
signatures are unchanged.

The baseline is `6f9798aa2935e5b50603b96a650bf4432ce779bd`. The correctness
reproducer fails in all three backends at 4/15/16/17/64 entries, with and without
prior reads: 30 changed-winner observations. The final implementation passes
100 minimal-runtime library tests, strict library/test Clippy and three
strict-provenance Miri tests. A stable multimap/pop-target model checks 3,072
interleaved mutation steps per backend. Controls cover target history, index
thresholds, capacity, key clones, panic cleanup and raw bytes ownership.

Performance is a tradeoff. Nine randomized process pairs per row ran on one
pinned CPU of a shared Linux host. All **126 processes** passed logical checks.
Ratios below one are faster; ranges contain every pair. These rows use owned
maps with unique keys. Duplicate corruption is correctness-only evidence.

| Owned-map workload | Candidate / baseline CPU | Pairs slower |
|---|---:|---:|
| Remove/get/reinsert, 8 integer keys | **1.557 (1.538–1.771)** | 9/9 |
| Remove/get/reinsert, 8 inline 8-byte string keys | **1.395 (1.346–1.434)** | 9/9 |
| Remove/get/reinsert, 256 integer keys | 0.152 (0.148–0.169) | 0/9 |
| Remove/get/reinsert, 64 96-byte string keys | 0.103 (0.098–0.115) | 0/9 |
| Remove/get/reinsert, 256 96-byte string keys | 0.053 (0.052–0.054) | 0/9 |
| Single removal, unindexed 8-key integer map | **1.294 (1.203–1.403)** | 9/9 |
| Single removal, unindexed 256-key integer map | **1.232 (1.190–1.287)** | 9/9 |

The mutation cycle rotates target keys and includes every reinsertion cost.
Initial preparation is excluded; index rebuilds caused by removal are included.
Single-removal controls delete the middle key once per freshly cloned map.
CPU is summed across batches of 1,024 maps, excluding preparation, verification
and destruction. “Unindexed” describes index state, not cache-cold memory.
For context, single-removal medians are 5.45→7.12 ns at 8 keys and
61.73→77.09 ns at 256. Paired ratio medians differ from ratios of separate medians.

Separate binaries with no measurement clock calls completed **14 allocation
processes**. The following totals cover 256 mutation cycles and include
reinsertion. Preparation, verification and destruction are excluded.

| Keys | Allocation calls, before→after | Requested bytes, before→after |
|---|---:|---:|
| 256 integers | 512→0 | 3,133,440→0 |
| 64 96-byte strings | 17,152→512 | 3,532,800→49,152 |
| 256 96-byte strings | 66,560→512 | 16,760,832→49,152 |

Both small cycles and both single-removal controls request zero allocations on
both sides. These are requested bytes, not RSS or retained memory. Keeping an
index can retain its previous capacity while the map remains large. Typed/raw
CPU, serialization order, other key types, retained memory and broader workloads
are unmeasured. No overall ranking or tonic/Prost speed comparison follows.

Baseline and candidate were built sequentially in one canonical source and
harness path, with identical lockfiles, profile, package IDs, artifact names
and verbose rustc commands, including crate metadata. Release settings are
optimization level 3, thin LTO, one codegen unit and debug info disabled. The
runtime has default features disabled. Only `src/map.rs` differs, alongside an
identical measurement-only workspace reduction and offline-pruned lockfile.
The measured candidate includes eight method-documentation lines added after
the recorded correctness checks; the checker verifies that exact difference.
This controls the separate-package-path confound in the rejected query record,
but does not remove shared-host noise or source-induced code-placement effects.

The archive retains baseline failures, the draft key-destructor value-cleanup
failure and its fix, test-only compile/lint corrections, exact source overlays,
controllers, build commands, binary hashes and every capture. Executables,
toolchains, caches and full source trees are omitted. Run:

```sh
python3 docs/evidence/map-removal-20261106/check.py
```

The checker uses its containing Git checkout, with invocation-checkout fallback
for standalone copies. It reconstructs 142 selected inputs, checks receipts and
recomputes all CPU ratios, terminal statuses and allocation totals. It does not
execute archived code, rebuild binaries or independently attest compiler identity,
all compiler filesystem reads or production qualification.

[Manifest](manifest.json) · [Sources and raw observations](raw.tar.gz)
