# Reproduce the packed bool screens

The compressed reports contain the original JSON bytes. Run this from the repository root to verify and extract them into a temporary directory.

```bash
set -euo pipefail
packed_evidence="$PWD/docs/evidence/packed-bool-20261118"
packed_scratch="$(mktemp -d)"
for packed_run in original replay
do
  gzip -dc "$packed_evidence/$packed_run-results.json.gz" > "$packed_scratch/$packed_run-results.json"
done
(
  cd "$packed_scratch"
  sha256sum -c <<'SUMS'
5cc5e6fd79c8d42477fa9535312dc37b38422f3b70751a1bc5817723e0100503  original-results.json
bdc5bfa4409726e285aea33b15a535e96e47b2f12ba6117b30cf20d25ab74b74  replay-results.json
SUMS
)
```

## Build and run the original screen

Use a Linux x86_64 checkout with Git, Python and Rust 1.85.0. The patch and tree check reconstruct the precise candidate from the source pinned in the original workflow. Run this in the same shell as the verification above.

```bash
packed_base=9061b8ee0a75d32acb19ebf7090e4a95660012db
packed_tree=168b888b956e8440b0808f61c6679281946c3a2d
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
rustup toolchain install 1.85.0 --profile minimal
mkdir -p "$packed_scratch/bin"

cleanup_packed_sources() {
  for packed_variant in a b c
  do
    packed_source="$packed_scratch/source-$packed_variant"
    if test -d "$packed_source"
    then
      git worktree remove --force "$packed_source"
    fi
  done
}
trap cleanup_packed_sources EXIT

for packed_variant in a b c
do
  packed_source="$packed_scratch/source-$packed_variant"
  packed_probe="$packed_scratch/probe-$packed_variant"
  packed_target="$packed_scratch/target-$packed_variant"
  git worktree add --detach "$packed_source" "$packed_base"
  if test "$packed_variant" = b
  then
    gzip -dc "$packed_evidence/source/candidate.patch.gz" | git -C "$packed_source" apply --index -
    test "$(git -C "$packed_source" write-tree)" = "$packed_tree"
  fi
  mkdir -p "$packed_probe/src"
  cp "$packed_evidence/source/probe.rs" "$packed_probe/src/main.rs"
  cp "$packed_evidence/source/resolved.Cargo.lock" "$packed_probe/Cargo.lock"
  sed "s|@PACKED_SOURCE@|$packed_source|" "$packed_evidence/source/Cargo.toml" > "$packed_probe/Cargo.toml"
  CARGO_TARGET_DIR="$packed_target" cargo +1.85.0 build --locked --release --manifest-path "$packed_probe/Cargo.toml"
  cp "$packed_target/release/packed-bool-probe" "$packed_scratch/bin/$packed_variant"
  CARGO_TARGET_DIR="$packed_target" cargo +1.85.0 build --locked --release --features inline-count --manifest-path "$packed_probe/Cargo.toml"
  cp "$packed_target/release/packed-bool-probe" "$packed_scratch/bin/$packed_variant-count"
done

python3 "$packed_evidence/source/compare.py" "$packed_scratch/bin" "$packed_scratch/new-results.json"
echo "Retained results in $packed_scratch"
```

The [original workflow](source/packed-bool-screen-20261118.yml) contains the exact hosted commands, including the focused build and test gates. Its first consumer build started from the workspace lock, then the remaining builds used the [resolved lock](source/resolved.Cargo.lock) retained here. Executable addresses and timings can vary across builds and machines.

## Replay the archived binaries

The original Actions archive contains all six binaries until its December 18, 2026 expiration. Download the archive with GitHub CLI and give the unpacked directory to the audit script. The exact [replay workflow](source/packed-bool-replay-20261118.yml) verifies all source and binary hashes and checks the raw result. The [audit script](source/audit.py) summarizes the original pairs, captures the symbol and instruction layouts, and drives the same workloads for the selected eight cases.

```bash
mkdir -p "$packed_scratch/replay-input" "$packed_scratch/replay-output"
gh run download 44347181608 --repo mingley/pure-protobuf --name packed-bool-component-screen --dir "$packed_scratch/replay-input"
(
  cd "$packed_scratch/replay-input"
  sha256sum --check environment/input-sha256.txt
  sha256sum --check environment/results-sha256.txt
  chmod +x bin/a bin/b bin/c bin/a-count bin/b-count bin/c-count
)
python3 "$packed_evidence/source/audit.py" "$packed_scratch/replay-input" "$packed_scratch/replay-output"
```

The two frozen source trees, exact original archive and replay have independent content hashes in [source-pins.json](source-pins.json).
