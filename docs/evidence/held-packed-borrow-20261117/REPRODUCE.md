# Reproduce the held packed-field screens

Use a Linux x86_64 checkout with Python 3, Git and Rust 1.85.0. This builds
scratch consumers against isolated source trees. It creates no commit or remote
branch and leaves the current checkout's files unchanged.

Choose `original` for the 26-case baseline/original/independent-baseline screen,
or `refined` for the 16-case baseline/original/refined screen. Both drivers
measure thread CPU and run separate allocation-instrumented binaries. Results
can vary on a different host; this does not reproduce a ranking.

The patches reconstruct the exact candidate trees from the reachable baseline.
The original and refined candidates are sibling commits, so the incremental
[refinement patch](patches/refinement.patch) applies to the original, while
[refined.patch](patches/refined.patch) applies to baseline. The commands verify
each complete Git tree before adding the scratch consumer.

```bash
set -euo pipefail
probe_screen=refined
probe_evidence_root="$PWD/docs/evidence/held-packed-borrow-20261117"
probe_scratch_root="$(mktemp -d)"
probe_output_root="$(mktemp -d)"
probe_baseline=532c3814a53fdb10092d64d6f85fa57db031bd18

cleanup_packed_worktrees() {
  for probe_variant in a b c
  do
    probe_snapshot="$probe_scratch_root/$probe_variant"
    if test -d "$probe_snapshot"
    then
      git worktree remove --force "$probe_snapshot"
    fi
  done
  rmdir "$probe_scratch_root"
}
trap cleanup_packed_worktrees EXIT

rustup toolchain install 1.85.0 --profile minimal
git fetch origin "$probe_baseline"
export CARGO_BUILD_JOBS=2
export CARGO_INCREMENTAL=0

for probe_variant in a b c
do
  probe_snapshot="$probe_scratch_root/$probe_variant"
  git worktree add --detach "$probe_snapshot" "$probe_baseline"
  probe_expected_tree=94e4146056e8237523b75b7ad10527fd5e40b6cf
  if test "$probe_variant" = b
  then
    git -C "$probe_snapshot" apply --index "$probe_evidence_root/patches/original.patch"
    probe_expected_tree=f1f2aaa210ee64ba4fc18f91db9dc2f0e6cdeeaf
  fi
  if test "$probe_variant" = c && test "$probe_screen" = refined
  then
    git -C "$probe_snapshot" apply --index "$probe_evidence_root/patches/refined.patch"
    probe_expected_tree=e85aed99dc2d60e2e40b7ac2837e8dd8626f059e
  fi
  test "$(git -C "$probe_snapshot" write-tree)" = "$probe_expected_tree"

  probe_package="$probe_snapshot/bench/packed-borrowed-probe"
  mkdir -p "$probe_package/src"
  cp "$probe_evidence_root/source/probe.rs" "$probe_package/src/main.rs"
  cp "$probe_evidence_root/source/Cargo.toml" "$probe_package/Cargo.toml"
  if test "$probe_variant" = a
  then
    cp "$probe_evidence_root/source/baseline.Cargo.lock" "$probe_package/Cargo.lock"
    cargo +1.85.0 build --release --manifest-path "$probe_package/Cargo.toml"
    cp "$probe_package/Cargo.lock" "$probe_output_root/generated.Cargo.lock"
  else
    cp "$probe_output_root/generated.Cargo.lock" "$probe_package/Cargo.lock"
    cargo +1.85.0 build --locked --release --manifest-path "$probe_package/Cargo.toml"
  fi
  cp "$probe_package/target/release/packed-borrowed-probe" "$probe_output_root/$probe_variant"
  cargo +1.85.0 build --locked --release --features inline-count --manifest-path "$probe_package/Cargo.toml"
  cp "$probe_package/target/release/packed-borrowed-probe" "$probe_output_root/$probe_variant-count"
done

python3 "$probe_evidence_root/source/$probe_screen-driver.py" \
  "$probe_output_root/a" "$probe_output_root/b" "$probe_output_root/c" \
  "$probe_output_root/a-count" "$probe_output_root/b-count" "$probe_output_root/c-count" \
  "$probe_output_root/results.json"
sha256sum "$probe_output_root/"* > "$probe_output_root/hashes.txt"
echo "Retained results and binaries: $probe_output_root"
```

The first build starts from the retained baseline workspace lock and lets Cargo
update it for the isolated consumer, exactly as the recorded workflows did.
The remaining builds use that generated lock with `--locked`. The original
generated locks are also in the linked Actions artifacts.

For the exact hosted commands, see the inert copies of the
[original workflow](source/original-workflow.yml) and
[follow-up workflow](source/refined-workflow.yml), plus their
[probe source](source/probe.rs), [manifest](source/Cargo.toml) and
[drivers](source/original-driver.py). These copies are evidence, not active
repository workflows. The allocator and Linux clock FFI are scratch
measurement utilities, outside the shipping runtime.

[Decisions and limitations](README.md) · [Full observations](RESULTS.md)
