# SB-21 Rust correctness and lint cleanup — 2026-10-02

Standalone `rpc-bench` passes all 180 tests and strict all-target Clippy at
`57a073ebd0fdf7246f9c198c4996975217f59813`. The cleanup starts from
`30da0badd14b027d1004b0889c98eb5479978707`. These are shared-host correctness
and tooling results. No throughput, latency, CPU comparison, headroom or
production qualification was performed. SB-21's existing diagnostics and
qualification dispositions remain unchanged.

The [handoff](sb21-rust-correctness-20261002/handoff.json) records the exact
source, commands, toolchain, dependency lock hash, results and original artifact
paths. The [raw archive](sb21-rust-correctness-20261002/raw-artifacts.tar.gz)
contains its 18 artifacts. [Member hashes](sb21-rust-correctness-20261002/member-sha256.txt)
and the [archive manifest](sb21-rust-correctness-20261002/archive-manifest.json)
cover every member and the saved handoff. The archive SHA-256 is
`c4c142804df91e85fa35449b6ab362431a304cce7ab2ad39937bc5d234097ef2`.

## Changes and semantic boundaries

The three lint commits (`011e68cb`, `ec712836`, `92926107`) replace duplicated
resource-module loading with one canonical module, simplify equivalent
conditionals and group borrowed run/channel options. The saved caller audit
reconstructs all 28 previous argument lists: eight latency, four QPS, twelve
streaming, two native channel and two reference load calls. CLI values, report
schema/order, defaults and measurement boundaries are preserved. `Counter`'s
new `Default` implementation calls `new()`, retaining its initial running state.

Existing short `std::sync::Mutex` sections retain synchronous completion/mark
accounting. Reviewed guards do not cross an await. Local procfs/cgroup snapshots
and the synchronous report-file API retain their capture/write order. Narrow
item-level lint expectations explain these operations; this work adds no broad
lint suppression, dependency or runtime-policy change. The synchronous child
process test uses `park_timeout` in place of the lint-disallowed sleep.

The separate correction at `9520e4998daeeda75dfd115c13d8e9153093db13`
requires the fresh-stream one-request/one-reply worker to receive a reply.
Previously a successful stream ending without a reply counted as success.
Clean zero-reply EOF now returns `UNKNOWN`, matching the Go client's EOF status
classification. Existing send/call/read errors still propagate. The request,
clone, await and sender-drop order is preserved.

The real no-reply peer fixture receives exactly one request and shuts down
cleanly. Its tracker assertions require exactly one failure, one latency sample
and zero successes. This fixture first failed against the unchanged helper and
then passed after the correction; both traces and the exact first-red source
patch are retained. The first-red source is `92926107` plus that patch, rather
than a clean committed source pin. Previously recorded benchmark peers returned
replies, and their old diagnostic artifacts remain intact.

The final fixture-only commit `57a073eb` updates an older assertion that tonic
clients reject TLS. The baseline already supports verified tonic TLS. A TLS
client without an explicit remote address correctly fails because the automatic
loopback server is plaintext; the fixture now requires that exact existing
exit/status message. Paired TLS arguments, unknown shapes and the tonic server
subcommand's native-only TLS policy remain asserted. The existing native and
tonic CA/name verification test also passes.

## Retained attempts and final gates

| Attempt | Source | Result |
|---|---|---|
| Original worker / binary strict lint runs | Original SB-21 record below | Failed with 37 / 47 errors; complete raw logs preserved. |
| No-reply regression before correction | `92926107` + archived patch | Exit 101; one intended failure, no passes, 86 filtered out. |
| No-reply regression after correction | `9520e499` | Exit 0; one pass. |
| First all-target test run | `9520e499` | Exit 101; 84 binary and one fairness passed; load-shapes six passed, one stale TLS fixture failed. Later targets did not run in this attempt. |
| First final-source Clippy attempt | `57a073eb` | Exit 101; `ENOSPC` creating the target fingerprint directory before source checking. |
| Final all-target test run | `57a073eb` | Exit 0; 180 passed, zero failed. |
| Retried all-target strict Clippy | `57a073eb` | Exit 0 with `-D warnings`. |
| Format, diff and 28-caller audit | `57a073eb` | Exit 0. |

The copied original lint logs have exactly the hashes retained by
[SB-21's arrival/accounting evidence](sb21-arrival-accounting-20261002.md),
whose recorded source is `1e6b1119182e1e4e90f2d93656765b3bf4929b81`.
They were copied read-only, rather than rerun or relabeled as passes. That record
also preserves the original all-target attempt's two `load_shapes.rs` lints.

Final test counts are 84 binary, one fairness, seven load-shapes, one tonic
fairness and 87 worker. The original binary's 94 becomes 84 because ten resource
tests were loaded twice. The original worker's 96 plus the new EOF regression,
minus those ten duplicate executions, becomes 87. All ten canonical resource
test cases remain in each applicable executable. This count reduction removes
duplicate execution, rather than omitting resource coverage.

GN12 independently reviewed the EOF correction at `9520e499` and the final
lint cleanup at `57a073eb` without edits or builds, and found no blocker.

## Reproduction

The final source was clean when tested. Tool versions were Rust
`1.99.0 (b940084d7 2026-09-28)`, Cargo `1.99.0 (5f94df478 2026-08-27)` and
protoc `35.1`. The standalone `rpc-bench/Cargo.lock` SHA-256 is
`8f294f7fcc95d32118344e33d996b7d0dddc95cb4e4c89d7287c42bf1878eae1`.
Cargo used one job and an isolated target; its observed final size was 908 MiB,
below the 2 GiB allocation. Other agents ran ordinary jobs=1 builds concurrently.
No measurement lease or comparative timing claim applies to these gates.

From the exact final source checkout, with the recorded tools available:

```sh
export CARGO_BUILD_JOBS=1
export CARGO_TARGET_DIR="$PWD/work/sb21-correctness-target"
cargo test --locked --manifest-path rpc-bench/Cargo.toml --all-targets -- --test-threads=1
cargo clippy --locked --manifest-path rpc-bench/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path rpc-bench/Cargo.toml --all -- --check
git diff --check
```

The original session first sourced `/workspace/pure-protobuf/work/toolchain/env.sh`
and used `/workspace/scratch/work/sb21lint/target/native`; the handoff retains
those exact original command paths. To inspect the archived failures and verify
their contents from the evidence directory:

```sh
mkdir -p work/sb21-correctness-raw
tar -xzf docs/evidence/sb21-rust-correctness-20261002/raw-artifacts.tar.gz -C work/sb21-correctness-raw
cd work/sb21-correctness-raw
sha256sum -c ../../docs/evidence/sb21-rust-correctness-20261002/member-sha256.txt
```

To reproduce the intended first-red EOF regression, use a separate checkout at
`92926107617672721798a889574b3cbedee12824`, apply the archived
`raw/no-reply-regression-before-fix.patch`, and run:

```sh
cargo test --locked --manifest-path rpc-bench/Cargo.toml --test worker fresh_stream_ok_eof_without_reply_counts_one_failure_and_no_success -- --exact --nocapture
```

That source-plus-patch is expected to fail. The same command at `9520e499`
passes. These correctness results do not qualify SB-21's offered-load headroom,
peer matrix, dedicated-host campaign or any production profile.
