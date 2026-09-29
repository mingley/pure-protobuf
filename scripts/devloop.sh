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
#
# SB-23: bench/devloop is its own workspace, so a root `cargo fetch`
# does not populate its dependency closure (notably the crates.io
# `protobuf` pin). Fetch the devloop manifest first so cold caches
# (fresh CI runners) resolve; the fetch is failure-tolerant so offline
# dev machines with a warm cache still work. The build itself stays
# --offline for determinism. Build/run failures are reported: the
# harness log tail goes to stderr and, when --out is given, a
# devloop/1-compatible error report (cells: [], plus an "error" field
# readers ignore) is written there so CI still uploads an artifact
# instead of exiting silent before compare/upload.
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

MANIFEST="bench/devloop/Cargo.toml"

# Write a devloop/1-compatible error report: empty cells plus an
# "error" field (unknown to the Report struct, so `compare` still
# parses it). $1=out path, $2=stage, $3=detail.
write_error_report() {
  local out="$1" stage="$2" detail="$3"
  local commit
  if [ -n "${PBRS_DEVLOOP_COMMIT:-}" ]; then
    commit="$PBRS_DEVLOOP_COMMIT"
  else
    commit="$(git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)"
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "devloop: python3 missing; cannot write error report to $out" >&2
    return 0
  fi
  PBRS_ERR_OUT="$out" PBRS_ERR_STAGE="$stage" PBRS_ERR_DETAIL="$detail" \
    PBRS_ERR_COMMIT="$commit" python3 - <<'PY'
import json, os, platform, shutil, subprocess

def cmd(argv):
    try:
        return subprocess.run(
            argv, capture_output=True, text=True, timeout=10).stdout.strip()
    except Exception:
        return ""

report = {
    "schema": "devloop/1",
    "host": {
        "os": platform.system(),
        "arch": platform.machine(),
        "cpu": "unknown",
        "rustc": cmd(["rustc", "--version"]) or "unknown",
        "perf": shutil.which("perf") is not None,
        "strace": shutil.which("strace") is not None,
        "valgrind": shutil.which("valgrind") is not None,
    },
    "devloop_commit": os.environ["PBRS_ERR_COMMIT"],
    "cells": [],
    "error": "%s: %s" % (os.environ["PBRS_ERR_STAGE"],
                         os.environ["PBRS_ERR_DETAIL"]),
}
with open(os.environ["PBRS_ERR_OUT"], "w") as f:
    f.write(json.dumps(report, indent=2) + "\n")
print("devloop: wrote error report to %s" % os.environ["PBRS_ERR_OUT"])
PY
}

FETCH_LOG="$(mktemp -t devloop-fetch.XXXXXX.log)"
if cargo fetch --manifest-path "$MANIFEST" >"$FETCH_LOG" 2>&1; then
  rm -f "$FETCH_LOG"
else
  echo "devloop: warning: cargo fetch failed (offline?); trying offline build with cached deps" >&2
  head -n 5 "$FETCH_LOG" >&2 || true
  rm -f "$FETCH_LOG"
fi

BUILD_LOG="$(mktemp -t devloop-build.XXXXXX.log)"
if ! cargo build --manifest-path "$MANIFEST" --release --offline >"$BUILD_LOG" 2>&1; then
  echo "devloop: harness build failed (offline release build of $MANIFEST)" >&2
  tail -n 30 "$BUILD_LOG" >&2
  DETAIL="$(grep -m1 '^error' "$BUILD_LOG" || echo "see build log $BUILD_LOG")"
  if [ -n "$OUT" ]; then
    write_error_report "$OUT" "build" "$DETAIL"
  fi
  echo "devloop: reported build failure; not running cells" >&2
  exit 1
fi
rm -f "$BUILD_LOG"
BIN="bench/devloop/target/release/devloop"

ARGS=(run --iters "$ITERS" --repeats "$REPEATS")
if [ -n "$CELLS" ]; then ARGS+=(--cells "$CELLS"); fi

if [ -n "$BASELINE" ]; then
  TMP="$(mktemp -t devloop.XXXXXX.json)"
  trap 'rm -f "$TMP"' EXIT
  if ! "$BIN" "${ARGS[@]}" --out "$TMP"; then
    echo "devloop: harness run failed" >&2
    write_error_report "$TMP" "run" "devloop run exited nonzero"
    if [ -n "$OUT" ]; then cp "$TMP" "$OUT"; fi
    exit 1
  fi
  BUDGET_ARGS=()
  if [ -f bench/devloop/baselines/picker.json ]; then
    BUDGET_ARGS=(--budget bench/devloop/baselines/picker.json)
  fi
  "$BIN" compare --baseline "$BASELINE" --current "$TMP" "${BUDGET_ARGS[@]}"
  if [ -n "$OUT" ]; then cp "$TMP" "$OUT"; fi
else
  if [ -n "$OUT" ]; then ARGS+=(--out "$OUT"); fi
  if ! "$BIN" "${ARGS[@]}"; then
    echo "devloop: harness run failed" >&2
    if [ -n "$OUT" ]; then
      write_error_report "$OUT" "run" "devloop run exited nonzero"
    fi
    exit 1
  fi
fi
