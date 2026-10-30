# Coordinated release preparation, October 30

Prepared source candidates: pbrs 0.2.1, pbrs-grpc 0.1.0-alpha.3 and
protobuf-tonic 0.1.0-alpha.3. They are not published or production-promoted.

## Changes and local checks

- Restored CallHandle's published unwind traits with an assertion confined to
  its private cancellation-handle field. Internal AtomicWaker registration
  is not asserted unwind-safe. Explicit Clone and Debug preserve behavior.
- A regression checks Clone/Send/Sync/UnwindSafe/RefUnwindSafe and catches an
  executor wake panic, then verifies cancellation can be observed and retried.
  All six rpc_cancel tests and strict all-feature gRPC library Clippy passed.
- Updated three manifests and 21 maintained lockfiles. Adapter runtime and
  build dependencies require core 0.2.1. TOML comparison confirmed each lock
  diff changes only local package versions; all 21 locked offline metadata
  checks passed. Historical evidence and legacy benchmark locks were retained.
- Publisher safety tests passed all seven cases. A committed candidate
  subsequently completed the local packaging-only rehearsal described below;
  the script still rejects dirty crate source.

The first focused compile exposed that AssertUnwindSafe does not implement
Clone; that failure is retained separately. The corrected handle supplies an
explicit Clone and passes the detecting test. A version-update safety check
also rejected a legacy vendored lock before touching it; the final scope
excludes those unrelated historical versions.

## Hosted baseline audit

At 15e738499f0dc937b0e54fd4325271b75ba7e5f4, required CI run 40950371929
passed ten jobs and failed only the Public API semver job 123716584220.
It reported CallHandle no longer implementing UnwindSafe and RefUnwindSafe.
Core pbrs semver passed; the later tonic check was not reached in that job.
The repaired source must pass the hosted comparison on its own SHA. The
new required CI run at `26389ccf77bf9342fe6e605f1d797853acd6c39a` is
`41100856391`; at 2026-10-30 17:48 UTC all 11 required jobs were still queued.
This is a pending result, not a pass.

Separate runs failed shared-consumer arena expectations, the core Miri fixture
table-owner retagging, and adoption-RPC exact byte equality for map messages.
Those logs are retained in hosted/; other agents own their repairs.

The public crates.io API reported the older published versions pbrs 0.2.0
and both adapters 0.1.0-alpha.2. No new version was uploaded in this work.

## Local package rehearsal at 26389ccf

`DRY_RUN=1 RELEASE_TAG=v0.2.1 bash scripts/publish-crates.sh` completed
successfully at 2026-10-30 17:43:32–37 UTC. The script ran in a disposable
worktree, did not receive registry tokens, and did not take its upload branch.
All recorded inputs and the root HEAD remained unchanged. Its package mode
uses `--no-verify`: it is not a substitute for isolated consumer builds.

The three retained archives declare the new versions, include the exact
`26389ccf77bf9342fe6e605f1d797853acd6c39a` VCS SHA and contain both licenses.
An independent audit compared every shipped non-generated source file with
that Git object's blob. Normalized adapter runtime and build requirements
both use `pbrs` `0.2.1` and contain no source path. No work, target or Git cache
is included; the core also excludes docs, tests, benchmarks and third_party.

| Archive | Bytes | SHA-256 |
|---|---:|---|
| pbrs-0.2.1.crate | 584329 | ac5effbf3957968d4ad2b0bc71b703d0dec7529cf8bc778f5c75630d62e52f8a |
| protobuf-tonic-0.1.0-alpha.3.crate | 28483 | 15c8744758a328e28f3bade35573392186e57f9fa00e6d86835b2bd323e7ad63 |
| pbrs-grpc-0.1.0-alpha.3.crate | 1381298 | 808991c419d73043bfd08fa29f4f1e376b1364fddd5a7d0235ebce2330076eb4 |

The command report, publisher log, archives and per-file audit live in
`work/release-rehearsal-26389/`. This local run does not generate the hosted
SBOM or GitHub attestation and does not establish that any version was
published. No tag was created.

## Isolated package consumers at the same SHA

`cargo test --locked --offline --test package_consumer -- --nocapture
--test-threads=1` passed both tests at `26389ccf`. The consumers unpacked
all three archives outside the workspace, built them and executed the core
round trip (`ada`), native gRPC status check (`OK`) and tonic codec construction
(`ProtobufCodec`). The 750 MiB free-space guard did not fire; recorded inputs
and HEAD remained unchanged. The test process completed at 17:52:57 UTC.

The consumer test packages all three together, whereas the publisher dry run
uses a local registry patch for adapters. The core archive was byte-identical.
The adapters' source, normalized manifest and VCS members were all identical;
only their bundled `Cargo.lock` pbrs entry differed, adding the local registry
overlay's source and checksum. That checksum is the exact core archive digest.
An initial all-archive identity assertion exposed this expected lockfile
provenance difference; the scoped follow-up verified it without hiding any
code difference. Reports, full log and comparison are retained alongside the
local rehearsal. This does not create a hosted SBOM or attestation.

## Publication prerequisites still open

- Full exact-candidate required CI and applicable sustained qualification.
- A hosted dry-run producing packaged crates, SBOM and verifiable provenance.
- Configured crates.io Trusted Publishers for all three crates.
- A signed annotated release tag GitHub marks verified.

This environment has no configured GPG secret signing key, git signing key,
or SSH agent. Its gh token is invalid. The connected GitHub tools support the
CI audit but do not expose a workflow-dispatch or signed-tag operation.
Public GitHub HTTP fallback was refused by the session proxy. No identity
was fabricated, credential contents exposed, release gate weakened, or
publication attempted. A verified API commit would not satisfy the distinct
signed annotated-tag requirement.
