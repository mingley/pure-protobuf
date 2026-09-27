#!/usr/bin/env bash
# Dev-loop measurement entry point (SB-03).
#
#   scripts/devloop.sh [--cells a,b] [--iters N] [--repeats N] [--baseline FILE] [--out FILE]
#
# Builds the release harness, runs the cell matrix, and optionally
# compares against a baseline with the scoreboard thresholds
# (2% improvement bar; 1% codec / 2% RPC regression limit).
# Exits nonzero on regression. Missing perf/strace/valgrind yield
# not_run metrics, never a pass.
set -euo pipefail

cd "$(dirname "$0")/.."

CELLS=""
ITERS="2000"
REPEATS="3"
BASELINE=""
OUT=""

while [ $# -gt 0 ]; do
  case "$1" in
    --cells) CELLS="$2"; shift 2;;
    --iters) ITERS="$2"; shift 2;;
    --repeats) REPEATS="$2"; shift 2;;
    --baseline) BASELINE="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

cargo build --manifest-path bench/devloop/Cargo.toml --release --offline
BIN="bench/devloop/target/release/devloop"

ARGS=(run --iters "$ITERS" --repeats "$REPEATS")
if [ -n "$CELLS" ]; then ARGS+=(--cells "$CELLS"); fi

if [ -n "$BASELINE" ]; then
  TMP="$(mktemp -t devloop.XXXXXX.json)"
  trap 'rm -f "$TMP"' EXIT
  "$BIN" "${ARGS[@]}" --out "$TMP"
  "$BIN" compare --baseline "$BASELINE" --current "$TMP"
  if [ -n "$OUT" ]; then cp "$TMP" "$OUT"; fi
else
  if [ -n "$OUT" ]; then ARGS+=(--out "$OUT"); fi
  "$BIN" "${ARGS[@]}"
fi
