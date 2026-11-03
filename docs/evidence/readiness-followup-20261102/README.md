# Readiness follow-up, November 2

This record starts from `a35701582fb9e6964c855386b0d9c991c50345ba`.
The [October 30 record](../release-readiness-20261030/README.md) keeps the earlier
release preparation. Core 0.2.1 and adapters 0.1.0-alpha.3 remain unpublished;
the adapters remain previews. This follow-up repairs detecting failures and
benchmark accounting, and removes redundant shared-buffer ownership work. It
adds no CPU ranking or production qualification claim.

All 11 required hosted CI jobs passed at the baseline in
[run 41115687791](https://github.com/mingley/pure-protobuf/actions/runs/41115687791).
The wider scheduled compatibility run then found the descriptor and i686
failures below. A green baseline does not qualify the later repaired source.
The public registry observation at 9:47 p.m. Chicago time on November 2 found all three
candidate versions absent. It records that observation, not future registry state.

## Generated empty defaults

The scheduled native i686 job failed an offset panic-message check and then
crashed in a generated-message group test. Local QEMU i386 reproduced both.
Strict Miri identified the independent validity defect: zeroing `LazyBytes`
selected its `Wire` variant with a null private `Arc` pointer.

`LazyBytes` and `LazyStr` now use `repr(u8)` with `Empty = 0`. The private
`ProtoString` representation explicitly assigns zero to its empty inline tag.
The offset regression keeps both target-specific rejection checks. Tests cover
fresh default/lazy/owned generated messages, empty and populated strings/bytes,
binary and JSON roundtrips, cloning, clearing, presence and oneofs.

| Wrapper | x86_64 before → after | i686 before → after |
|---|---:|---:|
| `LazyBytes` | 40 → 40 bytes | 20 → 24 bytes |
| `LazyStr` | 40 → 40 bytes | 28 → 32 bytes |

These are stable 1.99.1 size observations. Each affected i686 wrapper costs four
additional bytes. CPU costs were not measured. The retained field-family audit
does not create a `ZeroInit` trait or permit zeroing arbitrary Rust types.

The managed host rejects native ELF32 programs. Local i686 execution therefore
uses QEMU user emulation and a local Debian sysroot. It does not replace hosted
native i686 checks. Miri can interpret i686; the earlier documentation claiming
a 64-bit-only restriction was incorrect. Focused checks do not qualify every
compiler, architecture or possible message type.

## Serialized descriptor names

The scheduled descriptor campaign retained a 511-byte input with SHA-256
`52f06f01d32b69d137bb6333e51508492c0b086c7efa66fef3b406284c2d4b28`.
It created invalid symbol keys that lookup could not resolve. The parser now
validates package components and declared identifiers before indexing them.
Empty packages and valid ASCII identifiers still work, including absolute-name
lookup. The crasher is a deterministic API regression and a future fuzz seed.

The initial test helper used non-public wire helpers and failed to compile.
Its corrected baseline had three detecting failures and one positive pass;
the repaired focused checks pass. Those attempts remain separate in the archive.

## Shared-buffer chunk ownership

The selected gRPC change uses `Bytes::split_to` when sending a frame in chunks.
The tail advances in place, removing a redundant shared-owner clone and drop
per chunk. Backpressure, terminal flags and byte limits keep their existing
behavior.

An optimized mechanism screen compares the two chunk loops in one executable,
using the recorded release `bytes` library. Each case has five fresh paired
processes in randomized order, pinned to one CPU on a shared host.

| Frame | Chunk | New / old timed wall | All-pair range |
|---|---|---:|---:|
| 1 MiB | 16 KiB | 0.2917 | 0.2838–0.2941 |
| 8 MiB | 16 KiB | 0.2869 | 0.2835–0.2887 |
| 8 MiB | 64 KiB | 0.2879 | 0.2859–0.2927 |
| 512 bytes | 16 KiB | 0.8533 | 0.8503–0.8814 |
| 64 KiB | 1 KiB | 0.2898 | 0.2840–0.2926 |

Lower is faster. The metric is elapsed time inside the isolated buffer loop.
The loop retains one original owner, clones it per iteration and drops fragments
immediately. Fresh-buffer allocation, promotion and setup are outside the timed
window. It does not enqueue HTTP/2 DATA or measure flow control, sockets or TLS.
The 512-byte row is a mechanism control; it does not measure the sender's
ordinary small-frame fast path.
Whole-process CPU, which also includes setup, is retained separately. The
archive keeps every pair, the driver, source, compiler and library digests.
This screen has no tonic or Prost control and does not establish RPC CPU,
latency, throughput or an overall ranking. The final check table records the
separate network and lifecycle validation.

## Benchmark inputs and qualification

The perf job compared fingerprints from separate RPC children. The transport
harness had re-encoded a Prost map to obtain that fingerprint, allowing random
map iteration order to change it. New children identify the common prepared
wire and record its length. Native RPC reporting carries the same metadata.
Full map values, cardinality, decoded equality and equal wire lengths remain
independent guards. Canonical input identity does not claim that unordered-map
output bytes are identical.

The retained debug proof covers 48 map cells: native and tonic profiles, three
map sizes, unary and server streaming. N/2N children run twice in fresh processes,
for 192 successful children. Their identity and length agree across profiles.
These are functional checks; their allocation counters and short timings do not
establish a performance result. The exact-source baseline overlay repairs only
benchmark reporting and guards, with reviewed before/after hashes.
A separate CLI aggregation control checks six output rows against that same
input identity. Its debug timings, allocation and syscall values are retained
as diagnostics and excluded from performance evidence.

The standalone collector derives selected and blocked cells from its complete
inventory. The current inventory has 512 qualified states; the historical
488-qualified/24-blocked record keeps its own source. Legacy records remain
readable. A new capture missing the required identity/length cannot qualify.

The scheduler control also had stale fixture expectations. Its one-runtime,
one-connection case must account for the fixture's cached cancellation
dispatcher: two connection tasks before initialization, three after, one after
connection drain and zero after dropping the runtime. A closed prequeued bidi
upload needs no upload pump; the live bidi path needs one. Process-wide counters
are checked in a separate child process. The two-second quiescence bound and
balanced RPC/producer checks remain. The repaired 22-test harness suite passes,
including all 512 transport preflights; these are functional controls, not a
performance comparison or a general claim about dispatcher counts in other
runtime topologies.

## Final local checks

The named final reports and selected functional RPC proof bind the reconstructed
final selected inputs. Earlier records have different source states and remain
historical controls. Counts can overlap between checks and must not be added as
distinct coverage.

Workspace coverage uses Cargo partitions with all features enabled. The first
attempt stopped when its cache guard observed a concurrent documentation
consumer. Its outer wrapper exited with status 1; completed test assertions in
that pass had not failed. The guard stayed in place. A separate resume helper
checks source hashes, completed command logs and executable digests before
crediting completed groups from that same pass and running unfinished groups.
It credits no executions from the older baseline. The final union counts every
current test target and doctest library once, with both helpers and the failed
attempt retained.

A separate receipt records cleanup of one completed private cold-consumer build
cache after its matching test reported success. It names only that cache and
the supported Cargo cleanup command. Clean retired worktrees were also removed
through Git while preserving their commits and measurement records. These
housekeeping records add no test executions or qualification credit.

| Check | Result | Report |
|---|---|---|
| Workspace tests | 3855 native tests across 153 targets; 175 doctests; 9 ignored | `native-partitions-resume-final` |
| Strict workspace Clippy | Passed | `lint-retry-final` |
| Formatting | Passed | `fmt-retry-final` |
| Workspace binary builds | Passed | `bins-final` |
| Warning-free rustdoc | Passed | `rustdoc-final` |
| Transport strict Miri | 68 Rust tests passed | `grpc-miri-final` |
| Documentation contracts before capture | 27 Rust tests passed | `docs-pre-capsule` |
| RPC harness and preflights | 22 Rust tests passed | `devloop-final-tests` |
| Adoption controls | 19 Rust tests passed | `adoption-final-tests` |
| Benchmark Python contracts | 33 Python checks passed | `rpc-python-final` |
| Core zero defaults under i686 Miri | 16 Rust tests passed | `ci-bounded-miri-i686-complete` |
| Core zero defaults under x86_64 Miri | 16 Rust tests passed | `miri-x86_64-integrated-summary` |

## Verify the record

Run `python3 check.py` from this directory in a checkout containing the base
commit. The checker reads the archive without extracting or executing its
contents. It verifies hashes, selected final report/log bindings, the source
snapshot reconstructed from Git plus ordinary-file overlays, the complete
192-child input comparison and every partitioned workspace target/result. It
keeps failed and interrupted attempts distinct.

The archive omits executables, package caches, sysroots, QEMU, compiler binaries
and core dumps. It is not a complete filesystem-read trace, dependency mirror,
tool attestation or independent signature. Earlier source-bound checks cannot
qualify changed final inputs automatically.
Recorded binary digests are checked for consistency; omitted executables are
not rehashed or replayed by this checker.

The final pushed commit still needs hosted checks. Publication also requires
the [release policy](../../RELEASE.md): hosted provenance, verified signing,
configured Trusted Publishers and applicable qualification. Sustained resource
recovery, the adopter packet and its aggregate fuzz CPU evidence, and approved
support ownership remain open. No tag or registry publication was attempted.
