# Release readiness, October 30

The source prepares `pbrs` **0.2.1**, `pbrs-grpc` **0.1.0-alpha.3** and
`protobuf-tonic` **0.1.0-alpha.3**. These versions are **unpublished**. The
adapters remain previews, and sustained production qualification is open.

## Candidate checks

Commit `26389ccf77bf9342fe6e605f1d797853acd6c39a` repairs three qualification
failures:

- The public cancellation handle again implements `UnwindSafe` and
  `RefUnwindSafe`. Its regression catches a panicking wake callback, observes
  cancellation and successfully retries the remaining wake. Internal waiter
  registration is not asserted unwind-safe.
- The MiniTable test fixture retains `Rc` owners while raw table pointers are
  in use. Strict-provenance Miri caught an invalid pointer after the earlier
  `Box` owners were moved.
- RPC map fixtures now have equal encoded lengths across both codecs, while
  retaining full value checks and the wire-length guard. The original codec
  corpus and historical benchmark results remain intact.

The [candidate package record](candidate-26389/README.md) contains a successful
local publisher dry run, an archive/source audit and isolated consumers of
all three crates. Both package-consumer tests pass. The core archive is
byte-identical across those checks; adapter archives differ only in the
recorded `Cargo.lock` provenance from Cargo's local registry overlay.

That package record applies to `26389ccf`, before the later transport cleanup
repair. It is not final-source release evidence or a hosted provenance
attestation. All 11 required hosted jobs were still queued in its retained
observation. Later source changes need their own checks.

A [later CI observation](hosted-ci-26389-1816.json), at 18:16 UTC, records ten
successful jobs at `26389ccf`, including public API semver, MSRV, both language
interop lanes and conformance. The workspace test job was still running.
This observation also predates the transport follow-up below.

At 18:49 UTC, [all 11 required jobs passed](hosted-ci-26389-green.json) in
[run 41100856391](https://github.com/mingley/pure-protobuf/actions/runs/41100856391).
That green result belongs to `26389ccf`; it does not qualify the later
transport source automatically.

The prepared versions align both adapters' runtime and build dependencies
with core 0.2.1. All 21 maintained lockfiles pass locked offline metadata
checks; their diffs change only local package versions. Historical evidence
locks retain their original versions.

## Transport ownership follow-up

Reset, EOF and error cleanup previously destroyed queued outbound payloads
while holding the engine and send-buffer locks. Early DATA rejection could
do the same. A caller-owned `Bytes` backing store can run its own destructor;
if that destructor reenters the connection, it can deadlock.

Cleanup now detaches queued frames into an outer owner before releasing the
locks. DATA validation borrows an outer frame until queue acceptance, so
rejection and validation panics also release the guards before destroying
the payload. A full try-send still returns the original buffer to its caller.
Tests check actual lock availability, reset/header ordering, selective GOAWAY,
window errors and destructor unwinding.

Expanded strict Miri coverage also exposed invalid cached stream pointers.
The repair limits mutable slab projection to the selected slot and refreshes
pointers after callbacks or helpers that reborrow that slot. The debug check
validates the key before deriving the returned pointer. An unchanged address
alone does not preserve Rust pointer provenance.

Accepted sends add no payload clones or heap-allocation operations in source.
Cold cleanup needs temporary metadata proportional to its detached queue.
The changed stack and call layout has not been timed; this is a correctness
repair without a CPU speed claim. Internal `Buf` methods and capacity/waker
callbacks can still execute under locks. These tests do not establish general
callback reentrancy or panic rollback.

## Local transport checks

The [validation record](validation/README.md) reconstructs the checked source
from `26389ccf` and its retained overlay. Workspace coverage passes **3,849
native tests** across all 152 test executables and **175 doctests**. Six native
tests and three doctests retain their existing ignores; they are not qualified.
The final coverage combines Cargo partitions after two disk-guard interruptions.
It is not a completed one-shot workspace run.

All **68 transport safety tests in 14 groups** pass strict-provenance Miri.
The native transport, tonic adapter and greeter example pass strict all-target
Clippy. Workspace binaries, documentation contracts, formatting and warning-free
rustdoc also pass. The record retains detecting failures, interrupted attempts,
exact commands, source hashes and the coverage verifier. These are local checks;
the later transport source still needs its own hosted CI and release rehearsal.

## Performance decisions

The [rejected runtime screens](../rejected-runtime-candidates-20261030/README.md)
retain two map designs and the repeated string/bytes pre-scan. The pre-scan
reduced speculative allocation but regressed common CPU workloads. None of
these candidates shipped. The [October 29 record](../runtime-and-codegen-20261029/README.md)
remains the selected runtime and codegen measurement; its remaining losses
still apply.

## Before publication

The [release policy](../../RELEASE.md) requires final-source CI, a hosted dry
run with the three archives, SBOM and verified provenance, configured crates.io
Trusted Publishers, and a signed annotated tag that GitHub marks verified.
Local packaging cannot replace those checks.

Production qualification also needs the sustained resource/recovery campaign
(QG-06), the adopter security packet and sustained fuzz evidence (QG-08), and
an approved maintenance owner and response policy. Short tests and version
updates do not close those gates. The [work queue](../../../TODO.md),
[implementation status](../../status.md) and [support policy](../../support-policy.md)
track the remaining scope. No publication or release tag was attempted.
