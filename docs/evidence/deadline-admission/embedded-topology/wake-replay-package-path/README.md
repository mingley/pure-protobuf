# Packaged helper path overlay

The original WakeGuard replay helper passes in its source workspace but, with
explicit old-byte snapshots, fails inside a versioned package directory. It
tries to read `<unpacked>/pbrs-grpc/src/h2_backend/admission_tests.rs`, while the
actual fixture is under `<unpacked>/pbrs-grpc-0.1.0-alpha.2/src/h2_backend`.
The complete failing traceback, successful workspace replay and guarded input
hashes are retained. The correction changes only the current-byte assertion to
`HERE / Path(path).name`, matching the existing Clippy replay helper's lookup.

Read-only replay passes after the correction in the source workspace, with
explicit old-byte snapshots in that workspace, and in the versioned-directory
fixture with explicit old-byte snapshots. The fixture copies six exact backend
inputs; it is not a freshly generated Cargo package. GN13 owns the subsequent
actual normalized package/consumer gates. No Cargo, compiler, Rust test or
performance command ran for this correction, and no Rust runtime, manifests,
locks, original import mapping or original overlay records change.

The original wake-guard-1.88-overlay.json intentionally keeps its historical
helper hash and successful original workspace replay identity. The separate
packaged wake-replay-package-path-overlay.json and exact patch record the old
and corrected helper hashes and that original JSON hash. All earlier source,
package and gate identities remain tied to their original bytes.

`python3 check-artifacts.py` verifies the exact one-line transformation, recorded
red/green outputs and input guards, then reproduces the failing original helper
and passing corrected helper in a temporary versioned-directory fixture. The
explicit old-byte snapshots support this replay without repository git history.
