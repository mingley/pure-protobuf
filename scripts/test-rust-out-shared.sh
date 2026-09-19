#!/usr/bin/env bash
# Run original Google rust/test/shared integration suites in rust_out_shared.
#
# Validates that all 19 official shared test suites compile and pass against
# the pure-protobuf (pbrs) kernel via `extern crate pbrs as protobuf` and
# `grpc_remap/protobuf-shim`.
#
# Pinned generator / runtime:
#   - protoc: v35.1 (SHA: 35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03)
#   - kernel runtime: pbrs v0.1.0 (pure-protobuf)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PIN_FILE="$ROOT/vendor/google/PIN"
SHA_FILE="$ROOT/vendor/google/SHA"
PIN="$(cat "$PIN_FILE" 2>/dev/null || echo "unknown")"
SHA="$(cat "$SHA_FILE" 2>/dev/null || echo "unknown")"

# Track whether lockfiles were clean initially so we can restore them if cargo modified them
RESTORE_SHARED_LOCK=0
if git -C "$ROOT" diff --quiet rust_out_shared/Cargo.lock 2>/dev/null; then
  RESTORE_SHARED_LOCK=1
fi
RESTORE_GRPC_LOCK=0
if git -C "$ROOT" diff --quiet grpc_remap/Cargo.lock 2>/dev/null; then
  RESTORE_GRPC_LOCK=1
fi

TMP_OUT="$(mktemp)"
cleanup() {
  rm -f "$TMP_OUT"
  if [[ "$RESTORE_SHARED_LOCK" -eq 1 ]]; then
    git -C "$ROOT" checkout -- rust_out_shared/Cargo.lock 2>/dev/null || true
  fi
  if [[ "$RESTORE_GRPC_LOCK" -eq 1 ]]; then
    git -C "$ROOT" checkout -- grpc_remap/Cargo.lock 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "============================================================"
echo " pure-protobuf: Original Upstream Shared Consumer Test Suite"
echo "============================================================"
echo "Upstream generator pin: $PIN ($SHA)"
if command -v protoc >/dev/null 2>&1; then
  echo "Local protoc:           $(protoc --version)"
else
  echo "Local protoc:           not found on PATH (build will fail if generated files missing)"
fi
echo "Runtime target:         pbrs v0.1.0 (remapped as protobuf)"
echo "------------------------------------------------------------"

# Ensure upstream protobuf source is fetched if missing
if [[ ! -d "$ROOT/third_party/protobuf/rust/test" ]]; then
  echo "Fetching pinned upstream protobuf source..."
  "$ROOT/scripts/fetch-protobuf.sh"
fi

# Validate grpc_remap/protobuf-shim compiles against pbrs
echo "Validating grpc_remap/protobuf-shim..."
cargo check -p protobuf --manifest-path "$ROOT/grpc_remap/Cargo.toml" --quiet

# Prepare cargo test flags
CARGO_FLAGS=()
HAS_OFFLINE_OR_LOCKED=0
USER_ARGS=()

for arg in "$@"; do
  if [[ "$arg" == "--offline" || "$arg" == "--locked" ]]; then
    HAS_OFFLINE_OR_LOCKED=1
  fi
  USER_ARGS+=("$arg")
done

if [[ "$HAS_OFFLINE_OR_LOCKED" -eq 0 ]]; then
  # Test if offline check works; if so, prefer --offline for hermetic runs
  if cargo check --manifest-path "$ROOT/rust_out_shared/Cargo.toml" --offline --quiet 2>/dev/null; then
    CARGO_FLAGS+=("--offline")
  fi
fi

echo "Running cargo test in rust_out_shared..."
cd "$ROOT/rust_out_shared"

# Run cargo test and capture output
set +e
cargo test "${CARGO_FLAGS[@]}" "${USER_ARGS[@]}" 2>&1 | tee "$TMP_OUT"
TEST_STATUS="${PIPESTATUS[0]}"
set -e

echo ""
echo "============================================================"
echo " Test Results by Crate"
echo "============================================================"

# Parse test results
PARSER_RESULT="$(python3 -c '
import re, sys

content = open(sys.argv[1]).read()

crates = {}
cur_crate = None
for line in content.splitlines():
    m_run = re.search(r"Running tests/([a-zA-Z0-9_]+)\.rs", line)
    if m_run:
        cur_crate = m_run.group(1)
    m_res = re.search(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed", line)
    if m_res and cur_crate:
        crates[cur_crate] = (m_res.group(1), int(m_res.group(2)), int(m_res.group(3)))
        cur_crate = None

total_passed = sum(c[1] for c in crates.values())
total_failed = sum(c[2] for c in crates.values())
crates_count = len(crates)

for name in sorted(crates.keys()):
    status, passed, failed = crates[name]
    mark = "✓" if status == "ok" and failed == 0 else "✗"
    print(f"  {mark} {name:<35} {status:>6} ({passed} passed, {failed} failed)")

print(f"__CRATES_COUNT__={crates_count}")
print(f"__TOTAL_PASSED__={total_passed}")
print(f"__TOTAL_FAILED__={total_failed}")
' "$TMP_OUT")"

echo "$PARSER_RESULT" | grep -v '^__.*__=' || true

CRATES_COUNT="$(echo "$PARSER_RESULT" | grep '^__CRATES_COUNT__=' | cut -d= -f2 || echo 0)"
TOTAL_PASSED="$(echo "$PARSER_RESULT" | grep '^__TOTAL_PASSED__=' | cut -d= -f2 || echo 0)"
TOTAL_FAILED="$(echo "$PARSER_RESULT" | grep '^__TOTAL_FAILED__=' | cut -d= -f2 || echo 0)"

echo ""
echo "============================================================"
echo " Documented Skipped Files & Exclusions"
echo "============================================================"
cat << 'SKIPS'
  - ctype_cord_test.rs:
      Reason: Google C++ Cord string type (ctype=CORD); internal C++ representation.
      Status: Excluded (pure Rust uses ProtoString / ProtoBytes).
  - gtest_matchers_test.rs:
      Reason: protobuf_gtest_matchers internal C++ test framework integration.
      Status: Excluded (pure Rust uses standard assertions / googletest Rust).
  - no_internal_access_test.rs:
      Reason: Asserts __internal == (); pbrs uses a module with SealedInternal trait.
      Status: Excluded by design (sealed module pattern).
  - package_disambiguation_test.rs:
      Reason: Empty test file in pinned upstream Google protobuf v35.1 release.
      Status: Excluded (no tests).
  - extensions_test.rs:
      Reason: Tests Edition 2024 custom extensions (extensions.proto).
      Status: Pending Edition 2024 descriptor/extension support (tasks CG-13, CG-14).
  - edition2023 str_view cpp VIEW:
      Reason: C++ string_view (pb.cpp.string_type=VIEW); tested as standard string.
      Status: Excluded (C++-specific).
  - proto! #[cfg(bzl)] qualified paths:
      Reason: Bazel-specific package path qualification (::crate::Type).
      Status: Excluded (Cargo workspace build).
SKIPS

echo "============================================================"
echo " Summary"
echo "============================================================"
echo "  Crates tested: $CRATES_COUNT / 19"
echo "  Total passed:  $TOTAL_PASSED"
echo "  Total failed:  $TOTAL_FAILED"
echo "============================================================"

if [[ "$TEST_STATUS" -ne 0 || "$TOTAL_FAILED" -gt 0 ]]; then
  echo "ERROR: One or more test crates failed!" >&2
  exit 1
fi

if [[ "$CRATES_COUNT" -eq 0 || "$TOTAL_PASSED" -eq 0 ]]; then
  echo "ERROR: Zero tests ran! Expected 19 test crates and >0 passed tests." >&2
  exit 1
fi

if [[ "$CRATES_COUNT" -ne 19 ]]; then
  echo "ERROR: Expected 19 crates to run, but found $CRATES_COUNT" >&2
  exit 1
fi

echo "SUCCESS: All 19 original upstream shared test crates passed against pbrs."
exit 0
