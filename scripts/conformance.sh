#!/usr/bin/env bash
# Build official conformance_test_runner from the vendored pin and run it
# against this crate's conformance binary (required ×2, then recommended).
# Uses cmake's protoc from that same build; does not require system protoc.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
./scripts/fetch-protobuf.sh
BUILD="$ROOT/target/conformance-build"
RUNNER="$BUILD/conformance_test_runner"
PROTOC="$BUILD/protoc"
PIN="$(cat "$ROOT/vendor/google/PIN")"
SHA="$(cat "$ROOT/vendor/google/SHA")"
STAMP="$BUILD/.pbrs-protobuf-pin"
EXPECTED_STAMP="$PIN $SHA"
if [[ ! -x "$RUNNER" || ! -x "$PROTOC" || ! -f "$STAMP" || "$(cat "$STAMP")" != "$EXPECTED_STAMP" ]]; then
  cmake -S third_party/protobuf -B "$BUILD" \
    -Dprotobuf_BUILD_CONFORMANCE=ON \
    -Dprotobuf_BUILD_TESTS=OFF
  PARALLEL="${NPROC:-$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)}"
  cmake --build "$BUILD" --parallel "$PARALLEL" \
    --target conformance_test_runner --target protoc
  printf '%s\n' "$EXPECTED_STAMP" > "$STAMP"
fi
if [[ ! -x "$PROTOC" ]]; then
  echo "cmake protoc missing at $PROTOC (this path must not use system protoc)" >&2
  exit 1
fi
# cmake's protoc matches the runner pin; put it first so build.rs can
# regenerate the FDS. Vendored vendor/google/conformance_fds.bin is the
# fallback if this binary is absent at cargo-build time.
export PATH="$BUILD:$PATH"
if [[ "$(command -v protoc)" != "$PROTOC" ]]; then
  echo "expected cmake protoc $PROTOC first on PATH, got $(command -v protoc)" >&2
  exit 1
fi
echo "using cmake protoc: $PROTOC ($("$PROTOC" --version))"
OUT="${CONFORMANCE_OUTPUT_DIR:-$ROOT/target/conformance-out}"
OUT_REQ1="$OUT/required_1"
OUT_REQ2="$OUT/required_2"
OUT_REC="$OUT/recommended"
mkdir -p "$OUT" "$OUT_REQ1" "$OUT_REQ2" "$OUT_REC"

GIT_COMMIT="$(git rev-parse HEAD 2>/dev/null || echo "${GITHUB_SHA:-unknown}")"
GIT_DIRTY=0
if ! git diff --quiet HEAD --; then
  GIT_DIRTY=1
fi
TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EDITION="2023"

write_summary() {
  python3 "$ROOT/scripts/conformance-report.py" \
    "$OUT" "$PIN" "$SHA" "$GIT_COMMIT" "$TIMESTAMP" "$EDITION" "$GIT_DIRTY"
}

trap 'write_summary || echo "::error::conformance report incomplete" >&2' EXIT

cargo build --release --bin conformance
BIN="$ROOT/target/release/conformance"

OVERALL_FAILED=0

run_pass() {
  local title="$1"
  local out_sub="$2"
  shift 2
  local log_file="$out_sub/runner.log"
  mkdir -p "$out_sub"
  echo "===== $title ====="
  local exit_code=0
  "$@" 2>&1 | tee "$log_file" || exit_code=$?
  if [[ $exit_code -ne 0 ]]; then
    echo "FAIL: $title failed with exit code $exit_code" >&2
    OVERALL_FAILED=1
    return $exit_code
  fi
}

run_pass "required run 1" "$OUT_REQ1" "$RUNNER" --maximum_edition 2023 --output_dir "$OUT_REQ1" "$BIN" || true
if [[ $OVERALL_FAILED -eq 0 ]]; then
  run_pass "required run 2" "$OUT_REQ2" "$RUNNER" --maximum_edition 2023 --output_dir "$OUT_REQ2" "$BIN" || true
fi
if [[ $OVERALL_FAILED -eq 0 ]]; then
  run_pass "recommended" "$OUT_REC" "$RUNNER" --enforce_recommended --maximum_edition 2023 --output_dir "$OUT_REC" "$BIN" || true
fi

if [[ $OVERALL_FAILED -ne 0 ]]; then
  echo "FAIL: conformance qualification failed" >&2
  exit 1
fi

trap - EXIT
if ! write_summary; then
  echo "FAIL: binary/JSON or text conformance evidence missing or inconsistent" >&2
  exit 1
fi
echo "PASS: all conformance passes completed successfully"
