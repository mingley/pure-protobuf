#!/usr/bin/env bash
# Bounded compile/smoke check for the shipping HTTP/2 receive-codec fuzz target.
# This records a requested wall-time budget, not sustained CPU qualification.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${1:-$ROOT/target/h2-frame-fuzz-smoke}"
mkdir -p "$OUT/corpus" "$OUT/artifacts"
OUT="$(cd "$OUT" && pwd)"
cp "$ROOT"/fuzz/corpus/h2_frame/*.bin "$OUT/corpus/"

{
  git -C "$ROOT" rev-parse HEAD
  rustc +nightly --version
  cargo +nightly fuzz --version
  echo "scope=modified embedded h2 receive codec; no connection, network or TLS"
  echo "requested_wall_seconds=15 max_input_bytes=65536 timeout_seconds=5 rss_limit_mb=512"
  echo "sustained_cpu_qualification=not_run sb26_seed_mapping=not_run"
} > "$OUT/provenance.txt"

(
  cd "$ROOT/fuzz"
  cargo +nightly fetch --locked
  cargo +nightly fuzz build h2_frame
) 2>&1 | tee "$OUT/build.log"

(
  cd "$ROOT/fuzz"
  timeout 120s cargo +nightly fuzz run h2_frame "$OUT/corpus" -- \
    -max_total_time=15 -timeout=5 -rss_limit_mb=512 -max_len=65536 \
    -artifact_prefix="$OUT/artifacts/"
) 2>&1 | tee "$OUT/fuzz.log"
