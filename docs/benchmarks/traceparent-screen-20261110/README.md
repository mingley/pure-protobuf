# Reconstruct the trace-context screen

This directory retains the source patch and probe from the
[screen](../traceparent-screen-20261110.md). The patch transforms source
`cab0e5803c80c163722b5ea7356327a812510edf` into the two changed source
files measured at `ea4f6e89f63532b117c2f8d541b4d7a4fb01a846`. The probe
files are byte-for-byte copies from the measured fixture.

The original resolved probe lockfile was **not retained**. The original run
started from the baseline workspace's lockfile, resolved it once without
`--locked`, then copied the result to the candidate build. The recorded
resolved lockfile hash was
`6f7b2770887a5fee8b9ee16ac5e2af0889a7aea6970a6d57044881f33f111df8`.
A new resolution can select different dependencies, so the source and probe
can be reconstructed but an identical dependency graph or result is not
guaranteed. The recorded run used Rust 1.99.1 on a shared Linux runner; host
and toolchain differences also affect results.

## Restore the two source trees

Run from a full-history checkout. The commands make two temporary worktrees
and verify the candidate's source blobs after applying the patch:

```sh
set -eu
repo_root="$(git rev-parse --show-toplevel)"
trace_screen_work="$(mktemp -d)"
trace_fixture="$repo_root/docs/benchmarks/traceparent-screen-20261110"

git -C "$repo_root" worktree add --detach "$trace_screen_work/base-src" cab0e5803c80c163722b5ea7356327a812510edf
git -C "$repo_root" worktree add --detach "$trace_screen_work/candidate-src" cab0e5803c80c163722b5ea7356327a812510edf
git -C "$trace_screen_work/candidate-src" apply --check "$trace_fixture/candidate.patch"
git -C "$trace_screen_work/candidate-src" apply "$trace_fixture/candidate.patch"
test "$(git -C "$trace_screen_work/candidate-src" hash-object --no-filters pbrs-grpc/src/otel/trace.rs)" = b2211f4bcacad066e1f61cbdd9bd7d22ef816f08
test "$(git -C "$trace_screen_work/candidate-src" hash-object --no-filters pbrs-grpc/tests/otel_trace.rs)" = ec25c88fcbe63387bc49c560f0e361d141fe4c9b
```

## Build and compare

Continue in the same shell. This creates separate probe directories, replaces
the `PBRSGRPC_PATH` placeholder with each worktree's `pbrs-grpc` path, and
uses separate targets for the two sources. The first build can update the
baseline lockfile; that resolved lockfile is then copied to the candidate.
The default builds measure CPU without the counting allocator, while the
`count_alloc` builds measure allocations separately.

```sh
for trace_variant in base candidate; do
  mkdir -p "$trace_screen_work/$trace_variant-probe/src"
  cp "$trace_fixture/main.rs" "$trace_screen_work/$trace_variant-probe/src/main.rs"
  python3 - "$trace_fixture/Cargo.toml" "$trace_screen_work/$trace_variant-src/pbrs-grpc" "$trace_screen_work/$trace_variant-probe/Cargo.toml" <<'PY'
import json
import pathlib
import sys

template = pathlib.Path(sys.argv[1]).read_text()
assert template.count('"PBRSGRPC_PATH"') == 1
replacement = json.dumps(str(pathlib.Path(sys.argv[2]).resolve()))
pathlib.Path(sys.argv[3]).write_text(template.replace('"PBRSGRPC_PATH"', replacement))
PY
done

cp "$trace_screen_work/base-src/Cargo.lock" "$trace_screen_work/base-probe/Cargo.lock"
CARGO_TARGET_DIR="$trace_screen_work/target-base" cargo build --release --manifest-path "$trace_screen_work/base-probe/Cargo.toml"
cp "$trace_screen_work/target-base/release/trace-context-probe" "$trace_screen_work/base"
CARGO_TARGET_DIR="$trace_screen_work/target-base" cargo build --release --locked --features count_alloc --manifest-path "$trace_screen_work/base-probe/Cargo.toml"
cp "$trace_screen_work/target-base/release/trace-context-probe" "$trace_screen_work/base-count"

cp "$trace_screen_work/base-probe/Cargo.lock" "$trace_screen_work/candidate-probe/Cargo.lock"
sha256sum "$trace_screen_work/base-probe/Cargo.lock" "$trace_screen_work/candidate-probe/Cargo.lock"
CARGO_TARGET_DIR="$trace_screen_work/target-candidate" cargo build --release --locked --manifest-path "$trace_screen_work/candidate-probe/Cargo.toml"
cp "$trace_screen_work/target-candidate/release/trace-context-probe" "$trace_screen_work/candidate"
CARGO_TARGET_DIR="$trace_screen_work/target-candidate" cargo build --release --locked --features count_alloc --manifest-path "$trace_screen_work/candidate-probe/Cargo.toml"
cp "$trace_screen_work/target-candidate/release/trace-context-probe" "$trace_screen_work/candidate-count"

python3 "$trace_fixture/compare.py" "$trace_screen_work/base" "$trace_screen_work/candidate" "$trace_screen_work/base-count" "$trace_screen_work/candidate-count" "$trace_screen_work/report.json"
```

The comparison runs ten alternating pairs of one million extractions for each
case. It writes raw samples to `report.json` and prints its own ratio of
separate medians. The [screen verifier](../traceparent-screen-20261110-verify.py)
instead computes the median of within-block ratios.

When finished, remove the worktrees and the temporary files:

```sh
git -C "$repo_root" worktree remove "$trace_screen_work/base-src"
git -C "$repo_root" worktree remove --force "$trace_screen_work/candidate-src"
rm -rf "$trace_screen_work"
```
