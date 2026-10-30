# Rejected runtime candidates, 30 October

These development screens use commit `15e738499f0dc937b0e54fd4325271b75ba7e5f4`
(pbrs 0.2.0). They do not measure the later 0.2.1 release candidate. Neither
map candidate nor the repeated-field candidate shipped. There is no overall
performance ranking claim.

## Repeated strings and bytes

The candidate changes only `src/codegen/parse.rs` (SHA-256
`c7e81ddfadeb5ee07a11ea92cbbe02883a4388bfff7915bc00af3fa201fd0c56`).
It scans a repeated run before allocating storage. It reduced speculative
allocation for an unrelated message suffix, but common workloads slowed down.
The candidate was rejected.

The randomized quiet-window screen covers 11 workloads and two operations for
both lazy and owned profiles: **44 diagnostic CPU rows**. Each uses five paired
processes, one sample per process, and freshly interleaved Prost controls.
Allocation binaries are separate from CPU binaries. Of those rows, 13 lazy
and 14 owned were slower in all five pairs; two per profile were faster in
all five pairs. This is a focused screen, not a complete codec benchmark.

| Workload and operation | Owned candidate / baseline CPU | Lazy candidate / baseline CPU |
|---|---:|---:|
| 1,024 strings, parse | 1.131 (1.121–1.142) | 1.127 (1.039–1.251) |
| 64 strings + 64 KiB suffix, roundtrip | 2.025 (1.771–2.168) | 2.053 (1.942–2.332) |

Values are paired median ratios, with the range of all five paired ratios in
parentheses. Lower is faster. For the 1,024-string parse, the freshly measured
Prost control ratio was 1.004 owned and 1.002 lazy.

The suffix fixture's parse requested 359,600 → 68,320 bytes. Requested bytes
are not peak RSS. Mixed-64 parse allocation calls rose from 13 to 19 even as
requested bytes fell from 8,436 to 6,236. Raw controls, outliers and all 44
summaries remain in `raw.tar.gz`, under
`work/round30-map-screen/reserve-{lazy,owned}-results/`.

Separate focused proofs cover malformed input, cursor and partial-message
state, group framing, and UTF-8 modes. Their source and logs are retained.
The archive also retains a distinct scalar-extension correctness experiment
(SHA-256 `897c4c8afb709f933a11fc8cd52104f1bcc35a453be2edb208c93a79548ae3c5`).
That extension was not timed and is not the candidate in the 44 measured rows.
These experiments do not establish full feature parity.

## Map iteration

Both map designs were rejected without timing. Their generated assembly still
compared map cardinality inside a scalar unique-entry loop; the fresh baseline
hoisted that check and vectorized the loop. Assembly is a warning about
compiler behavior, not a measured speed ratio.

The archive includes source overlays, assembly analysis, build records and
216 zero-iteration semantic controls. It also records the oversized-fallback
panic-state difference and the absence of native 32-bit testing. No additional
map variant was selected.

## Verify the evidence

From a checkout containing the pinned commit, run:

```sh
python3 docs/evidence/rejected-runtime-candidates-20261030/check.py
```

The checker reads the archive without extracting or executing it. It verifies
member hashes, **609 Git-blob source entries**, three isolated overlays,
11 successful build records, harness and lockfile hashes, fresh executable
artifact records, and the 44 result rows' binary identities. All 11 retained
executables also matched their recorded hashes before they were omitted from
the capsule. Compiled binaries and Cargo caches are omitted.

Those 609 source entries are **not a full filesystem checkout**. Six unrelated
`rpc-bench/proto` symlinks were frozen as ordinary files containing the Git
link-target text: `benchmark_service.proto`, `control.proto`, `payloads.proto`,
`services.proto`, `stats.proto` and `worker_service.proto`. The checker asserts
that exact exception set. Each Cargo artifact stream identifies only the
frozen core and its isolated harness as local packages. The core source,
core build script and harnesses do not reference `rpc-bench`; the core build
uses its vendored conformance descriptor. These six representation
differences did not enter the screened builds.

This is an input and build-record audit, not a complete filesystem read trace.
Generated `OUT_DIR` Rust and compiler dep-info were not retained after cache
cleanup. The 11 build records do not capture contemporaneous rustc or protoc
executable digests. Registry lockfiles retain dependency checksums; registry
source contents are not included. Reproductions must record their own tools
and generated inputs. These limits keep the screens diagnostic rather than
release qualification. Separate binaries can differ in code layout, and all
measurements share one host.
