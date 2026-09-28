#!/bin/bash
# Scheduled fuzz campaign runner (QG-02).
#
# Discovers every fuzz target from fuzz/Cargo.toml ([[bin]] names, so new
# targets are picked up automatically), builds them once, then runs each
# with a recorded per-target CPU budget. On a crash the campaign:
#   1. minimizes the crasher with libFuzzer tmin,
#   2. stores it as fuzz/corpus/<target>/crash-<sha16>.bin (fuzz-level
#      regression: every future campaign replays the corpus dir),
#   3. prints a ready-to-paste Rust const for the in-tree regression test
#      (tests/fuzz_parse.rs style; a human picks the test and asserts the
#      fixed behavior),
#   4. exits nonzero so scheduled CI goes red until triaged.
#
# Usage:
#   ./scripts/fuzz-campaign.sh [--seconds N] [--target T] [--jobs J]
#                              [--out DIR] [--build-only] [--no-triage]
# Defaults: 300 s/target, all targets, jobs=nproc, out=target/fuzz-logs.
# A short smoke run: ./scripts/fuzz-campaign.sh --seconds 10
#
# Requires nightly (cargo-fuzz) and clang with libFuzzer on Linux; on
# macOS the Xcode clang works for build/smoke but campaign numbers come
# from the scheduled Linux lane.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SECONDS_PER_TARGET=300
ONLY_TARGET=""
JOBS="$( (nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null) || echo 4)"
OUT=""
BUILD_ONLY=0
TRIAGE=1

usage() { sed -n '2,20p' "$0"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --seconds) SECONDS_PER_TARGET="$2"; shift 2 ;;
    --seconds=*) SECONDS_PER_TARGET="${1#--seconds=}"; shift ;;
    --target) ONLY_TARGET="$2"; shift 2 ;;
    --target=*) ONLY_TARGET="${1#--target=}"; shift ;;
    --jobs) JOBS="$2"; shift 2 ;;
    --jobs=*) JOBS="${1#--jobs=}"; shift ;;
    --out) OUT="$2"; shift 2 ;;
    --out=*) OUT="${1#--out=}"; shift ;;
    --build-only) BUILD_ONLY=1; shift ;;
    --no-triage) TRIAGE=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$SECONDS_PER_TARGET" in
  ''|*[!0-9]*) echo "--seconds must be a non-negative integer" >&2; exit 2 ;;
esac

OUT="${OUT:-$ROOT/target/fuzz-logs}"
FUZZ_DIR="$ROOT/fuzz"
mkdir -p "$OUT"

# Portable helpers (macOS bash 3.2 lacks sha256sum and timeout).
sha16() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -c1-16
  else
    shasum -a 256 "$1" | cut -c1-16
  fi
}
upper() { tr 'a-z' 'A-Z'; }
TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_BIN="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_BIN="gtimeout"
fi
# with_backstop SECS CMD...: run CMD under timeout(1) when available.
with_backstop() {
  local secs="$1"; shift
  if [ -n "$TIMEOUT_BIN" ]; then
    "$TIMEOUT_BIN" "$secs" "$@"
  else
    "$@"
  fi
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found" >&2; exit 2
fi
if ! cargo fuzz --version >/dev/null 2>&1; then
  echo "cargo-fuzz not found (cargo install cargo-fuzz)" >&2; exit 2
fi
# cargo-fuzz needs nightly; fail fast with a clear message otherwise.
if ! cargo +nightly --version >/dev/null 2>&1; then
  echo "nightly toolchain not found (rustup toolchain install nightly)" >&2; exit 2
fi

# Target discovery: name = "..." lines inside [[bin]] sections of fuzz/Cargo.toml.
TARGETS=()
while IFS= read -r t; do
  [ -n "$t" ] && TARGETS+=("$t")
done < <(awk '/^\[\[bin\]\]/{inbin=1;next} /^\[/{inbin=0} inbin && /^name = /{gsub(/"/,""); print $3}' "$FUZZ_DIR/Cargo.toml")
if [ -n "$ONLY_TARGET" ]; then
  found=0
  for t in "${TARGETS[@]}"; do
    if [ "$t" = "$ONLY_TARGET" ]; then found=1; break; fi
  done
  if [ "$found" -eq 0 ]; then echo "unknown target: $ONLY_TARGET (have: ${TARGETS[*]})" >&2; exit 2; fi
  TARGETS=("$ONLY_TARGET")
fi
if [ "${#TARGETS[@]}" -eq 0 ]; then echo "no fuzz targets in fuzz/Cargo.toml" >&2; exit 2; fi
echo "targets: ${TARGETS[*]}"
echo "budget: ${SECONDS_PER_TARGET}s per target, jobs=$JOBS"

export CARGO_BUILD_JOBS="$JOBS"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
MANIFEST="$OUT/campaign-$RUN_ID.json"
{
  echo "{"
  echo "  \"run_id\": \"$RUN_ID\","
  echo "  \"seconds_per_target\": $SECONDS_PER_TARGET,"
  echo "  \"targets\": ["
} > "$MANIFEST"

build_targets() {
  # `cargo fuzz build` takes zero (all) or one target; build each singly.
  : > "$OUT/build.log"
  for t in "$@"; do
    echo "--- building: $t ---" | tee -a "$OUT/build.log"
    ( cd "$FUZZ_DIR" && cargo +nightly fuzz build "$t" ) 2>&1 | tee -a "$OUT/build.log"
  done
}

build_targets "${TARGETS[@]}"
if [ "$BUILD_ONLY" -eq 1 ]; then
  echo "build-only: all ${#TARGETS[@]} targets built"
  exit 0
fi

# Crash triage for one target. Prints a report; returns 1 if NEW crashers.
triage() {
  local target="$1" sha f minimized const_body
  local artdir="$FUZZ_DIR/artifacts/$target"
  local new_crashes=0
  [ -d "$artdir" ] || return 0
  shopt -s nullglob
  for f in "$artdir"/crash-* "$artdir"/oom-* "$artdir"/timeout-*; do
    [ -f "$f" ] || continue
    echo "--- minimizing $f ---"
    minimized="$f"
    marker="$(mktemp)"
    tmin_out="$(cd "$FUZZ_DIR" && cargo +nightly fuzz tmin "$target" "$f" -- -runs=100000 2>&1)" || true
    # tmin drops minimized-from-* next to the artifacts; take one made now.
    newest="$(ls -t "$artdir"/minimized-from-* 2>/dev/null | while read -r c; do
      if [ "$c" -nt "$marker" ]; then echo "$c"; break; fi
    done)"
    rm -f "$marker"
    if [ -n "$newest" ]; then
      minimized="$newest"
    else
      echo "tmin produced nothing usable; keeping the original artifact"
      echo "$tmin_out" | tail -3
    fi
    sha="$(sha16 "$minimized")"
    local kept="fuzz/corpus/$target/crash-$sha.bin"
    if [ -f "$ROOT/$kept" ]; then
      echo "known crasher (already in $kept)"
      continue
    fi
    new_crashes=1
    mkdir -p "$ROOT/fuzz/corpus/$target"
    cp "$minimized" "$ROOT/$kept"
    const_body="$(python3 -c "import sys; print(', '.join(f'0x{b:02x}' for b in open(sys.argv[1],'rb').read()))" "$minimized")"
    {
      echo ""
      echo "=== NEW CRASH: $target ==="
      echo "artifact: $f"
      echo "minimized: $minimized ($(wc -c < "$minimized") bytes)"
      echo "retained: $kept (replayed by future campaigns)"
      echo "paste as a regression const (pick the covering test, assert fixed behavior):"
      echo "const CRASH_$(echo "${target}_${sha}" | upper): &[u8] = &[${const_body}];"
    } | tee -a "$OUT/triage-$RUN_ID.txt"
  done
  shopt -u nullglob
  return "$new_crashes"
}

fail=0
first=1
for target in "${TARGETS[@]}"; do
  seed="$FUZZ_DIR/corpus/$target"
  corpus="$OUT/corpus-work/$target"
  mkdir -p "$seed" "$corpus"
  # Seed the scratch dir from the checked-in seeds and retained crashers.
  # libFuzzer writes new units to the scratch dir only, never into the repo;
  # only minimized crashers are copied back (see triage), so reruns and
  # future campaigns replay every regression without polluting git.
  for s in "$seed"/*; do
    if [ -f "$s" ]; then cp -n "$s" "$corpus"/; fi
  done
  log="$OUT/$target.log"
  echo "--- fuzzing $target for ${SECONDS_PER_TARGET}s (corpus: $corpus) ---"
  set +e
  # The wall-clock backstop below is best-effort: -max_total_time is the
  # real bound; missing timeout(1) (stock macOS) just drops the backstop.
  ( cd "$FUZZ_DIR" && with_backstop "$((SECONDS_PER_TARGET + 120))" \
      cargo +nightly fuzz run "$target" -- -max_total_time="$SECONDS_PER_TARGET" "$corpus" ) >"$log" 2>&1
  code=$?
  set -e
  if [ "$code" -ne 0 ] && [ "$TRIAGE" -eq 1 ]; then
    if ! triage "$target"; then
      echo "$target: NEW CRASHERS (see $OUT/triage-$RUN_ID.txt)"
      fail=1
    else
      echo "$target: exit $code, no new crashers"
      fail=1
    fi
  elif [ "$code" -ne 0 ]; then
    echo "$target: exit $code (--no-triage, see $log)"
    fail=1
  else
    echo "$target: clean"
  fi
  if [ "$first" -eq 1 ]; then first=0; else echo "," >> "$MANIFEST"; fi
  printf '    {"target": "%s", "seconds": %s, "exit": %s}' "$target" "$SECONDS_PER_TARGET" "$code" >> "$MANIFEST"
done
{
  echo ""
  echo "  ]"
  echo "}"
} >> "$MANIFEST"
echo "manifest: $MANIFEST"

if [ "$fail" -ne 0 ]; then
  echo "CAMPAIGN FAILED (see $OUT)" >&2
  exit 1
fi
echo "CAMPAIGN CLEAN: ${#TARGETS[@]} targets x ${SECONDS_PER_TARGET}s"
