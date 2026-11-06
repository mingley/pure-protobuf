# Rejected borrowed query capability, November 6

**REJECTED; unshipped.** This candidate removes indexed string-query ownership
copies, but repeatedly slows a small LazyStr map control.

The baseline is `6f9798aa2935e5b50603b96a650bf4432ce779bd`. Both sparse checkouts
use the same measurement-only workspace adaptation (`members = ["."]`) and
identical offline-pruned lockfile. Only `src/map.rs` and `src/lazy.rs` differ in
runtime source. Index storage, layout, mutation and iteration are unchanged.

The candidate adds two defaulted query capabilities: borrowed key bytes and a
static optional ordering function. Built-in strings compare borrowed bytes in
the existing sorted index. Custom queries keep their single owned-key fallback
and trait-object support. Small reads retain their reverse equality scan. Raw
string reads borrow query bytes; unsupported mutable queries retain their owned
predicate.

Nine randomized process pairs per case measured warm lookup CPU on one pinned
CPU of a shared Linux host. Map construction, query preparation and lazy index
creation precede the measured loop. Each process executes three million lookups,
checks its checksum and records process CPU time. All **108 processes** passed.
Lower ratios are faster; ranges contain all nine paired ratios.

Baseline and candidate were built from separate source paths. Retained Cargo
records show distinct path package IDs and library artifact hashes
(`a81bc2796a0c6141` / `463f85305ff90e12`). The recorded command is not verbose
enough to recover every rustc disambiguator flag. Package-path identity and
linked code placement can affect these comparisons, including unchanged scans;
the measured loss is not attributed to the pointer reload. A future experiment
could build both variants sequentially in one canonical root and freeze each
output to control this source of variation.

| Query and fixture | Candidate / baseline CPU | Pairs above one |
|---|---:|---:|
| ProtoString, 8 entries, duplicate keys, 8-byte hit | 1.005 (0.940–1.029) | 6/9 |
| LazyStr, 8 entries, duplicate keys, 8-byte hit | **1.044 (1.021–1.091)** | **9/9** |
| Integer, 256 entries, distinct-key hit | 1.001 (0.972–1.178) | 5/9 |
| ProtoString, 256 entries, 96-byte common-prefix hit | 0.905 (0.872–0.919) | 0/9 |
| LazyStr, 256 entries, 96-byte common-prefix hit | 0.829 (0.810–0.842) | 0/9 |
| ProtoString, 16 entries, duplicate keys, 8-byte hit | 0.477 (0.389–0.500) | 0/9 |

The repeated small LazyStr loss is the rejection condition. No timing grid or
attribute retuning followed these observations. Earlier
[borrowed-map candidates](../rejected-map-lookups-20261103/README.md) found the
same kind of small-map loss; merely outlining indexed search had already been
tried there.

A separate final, clock-free allocation binary ran **76 cases / 152 processes**.
For all 24 long-string indexed cases, 4,096 lookups drop from 4,096 allocations
and 393,216 requested bytes to zero. Remaining short-string, numeric and
HashMap controls request zero lookup allocations on both sides. Counters exclude
construction and index creation. They are requested allocations, not retained
memory or RSS. HashMap is a storage reference, not a Prost decoding benchmark.

Before timing, the comparator changed from a query reference to a byte slice.
The three extracted lookup functions have identical assembly across both forms
and the final correctness refinement. The candidate ProtoString small scan
reloads a query pointer from the stack where the baseline uses a register; this
does not establish the cause of the small LazyStr regression.

The final candidate passed 17 map tests and strict Clippy for the library and
test targets with `default-features = false, features = ["reflect"]`. Tests cover
invalid byte keys, index thresholds, last-wins updates, slice-backed scans,
custom queries with distinct equality/owned/raw predicates, exactly one fallback
conversion, trait objects and mutable message lookup. The timing harness uses
the minimal runtime with default features disabled. These finite checks do not
qualify every possible query implementation or shipping profile.

The archive also keeps **152 preliminary clock-free allocation processes**,
preliminary sources/builds/assembly, a workspace-lock setup rejection and a
test-only inferred-`usize` error fixed before final checks. The preliminary
allocation controller was not retained by value; those observations remain
separate from the final controller and results.

From a checkout containing the named baseline, run:

```sh
python3 docs/evidence/rejected-query-capability-20261106/check.py
```

The checker uses its containing Git checkout, falling back to the invocation's
checkout for a standalone copy, and reports the chosen root. It
reconstructs all 142 selected source inputs for each build, checks archive and
build links, verifies semantic receipts, recomputes all paired CPU summaries and
allocation counts, and compares the extracted assembly. It does not execute
archive code, rebuild omitted executables or independently qualify the host.
Recorded compiler strings and binary hashes are provenance records, not compiler
attestation or a complete compiler filesystem-read trace. No overall ranking,
Prost decoder comparison, end-to-end RPC speedup or production qualification
follows from this diagnostic.

[Integrity manifest](manifest.json) · [Sources, controllers and raw observations](raw.tar.gz)
