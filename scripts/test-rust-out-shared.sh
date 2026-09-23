#!/usr/bin/env bash
# Run original Google rust/test/shared integration suites in rust_out_shared.
#
# Validates that all 19 official shared test suites compile and pass against
# the pure-protobuf (pbrs) kernel via `extern crate pbrs as protobuf` and
# `grpc_remap/protobuf-shim`.
#
# Pinned generator / runtime (derived at run time, never hardcoded):
#   - protoc pin: vendor/google/PIN @ vendor/google/SHA (v35.1 baseline)
#   - generator flags: --rust_out with --rust_opt=experimental-codegen=enabled,kernel=upb
#   - kernel runtime: pbrs version from root Cargo.toml (pure-protobuf)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PIN_FILE="$ROOT/vendor/google/PIN"
SHA_FILE="$ROOT/vendor/google/SHA"
PIN="$(cat "$PIN_FILE" 2>/dev/null || echo "unknown")"
SHA="$(cat "$SHA_FILE" 2>/dev/null || echo "unknown")"
PBRS_VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | cut -d'"' -f2 || echo "unknown")"
REPO_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo "unknown")"
REPO_DIRTY=""
if ! git -C "$ROOT" diff --quiet HEAD --; then
  REPO_DIRTY=" (dirty tracked worktree; not reproducible from the SHA alone)"
fi

TMP_OUT="$(mktemp)"
cleanup() {
  rm -f "$TMP_OUT"
}
trap cleanup EXIT

echo "============================================================"
echo " pure-protobuf: Original Upstream Shared Consumer Test Suite"
echo "============================================================"
echo "Upstream generator pin: $PIN ($SHA)"
PROTOC_BIN="${PROTOC:-$(command -v protoc || true)}"
if [[ -z "$PROTOC_BIN" ]] || ! command -v "$PROTOC_BIN" >/dev/null 2>&1; then
  echo "Pinned protoc not found; run ./scripts/build-pinned-protoc.sh first" >&2
  exit 1
fi
PROTOC_BIN="$(command -v "$PROTOC_BIN")"
PROTOC_BIN="$(cd "$(dirname "$PROTOC_BIN")" && pwd)/$(basename "$PROTOC_BIN")"
PROTOC_VERSION="$("$PROTOC_BIN" --version)"
if [[ "$PROTOC_VERSION" != "libprotoc ${PIN#v}" ]] ||
   [[ "$("$PROTOC_BIN" --help)" != *'--rust_out=OUT_DIR'* ]]; then
  echo "Expected protoc $PIN with built-in Rust output, got $PROTOC_VERSION ($PROTOC_BIN)" >&2
  echo "Run ./scripts/build-pinned-protoc.sh and set PROTOC to its output binary" >&2
  exit 1
fi
export PROTOC="$PROTOC_BIN"
echo "Pinned protoc:          $PROTOC_VERSION ($PROTOC)"
echo "Runtime target:         pbrs v$PBRS_VERSION (remapped as protobuf) @ $REPO_SHA$REPO_DIRTY"
echo "Generator flags:        --rust_out with --rust_opt=experimental-codegen=enabled,kernel=upb"
echo "------------------------------------------------------------"

# Ensure upstream protobuf source is fetched if missing
if [[ ! -d "$ROOT/third_party/protobuf/rust/test" ]]; then
  echo "Fetching pinned upstream protobuf source..."
  "$ROOT/scripts/fetch-protobuf.sh"
fi

# Verify the upstream shared inventory is fully accounted for: every
# *_test.rs file must be either an included crate or a documented exclusion.
# This fails loudly on upstream drift instead of silently skipping new suites.
SHARED_DIR="$ROOT/third_party/protobuf/rust/test/shared"
INCLUDED_TESTS="accessors_map_test accessors_proto3_test accessors_repeated_test accessors_test bad_names_test child_parent_test edition2023_test enum_test fields_with_imported_types_test import_public_test message_copy_merge_test message_generics_test nested_types_test package_test proto_macro_test serialization_test simple_nested_test threading_test utf8_test"
EXCLUDED_TESTS="ctype_cord_test extensions_test gtest_matchers_test no_internal_access_test package_disambiguation_test"
echo "Verifying upstream shared inventory is fully accounted for..."
UNACCOUNTED=0
for uf in "$SHARED_DIR"/*_test.rs "$SHARED_DIR"/utf8/utf8_test.rs; do
  [[ -f "$uf" ]] || continue
  stem="$(basename "$uf" .rs)"
  case " $INCLUDED_TESTS $EXCLUDED_TESTS " in
    *" $stem "*) ;;
    *)
      echo "ERROR: upstream shared test '$stem' is neither included nor a documented exclusion" >&2
      UNACCOUNTED=1
      ;;
  esac
done
for stem in $INCLUDED_TESTS; do
  if [[ ! -f "$ROOT/rust_out_shared/tests/$stem.rs" ]]; then
    echo "ERROR: expected included test crate '$stem' missing from rust_out_shared/tests/" >&2
    UNACCOUNTED=1
  fi
done
if [[ "$UNACCOUNTED" -ne 0 ]]; then
  echo "ERROR: upstream shared inventory changed; update the included/excluded lists and docs/codegen-compatibility.md" >&2
  exit 1
fi
echo "Inventory OK: 19 included crates + 5 documented file exclusions cover all upstream shared tests."

# Validate grpc_remap/protobuf-shim compiles against pbrs
echo "Validating grpc_remap/protobuf-shim..."
cargo check -p protobuf --manifest-path "$ROOT/grpc_remap/Cargo.toml" --locked --quiet

# Prepare cargo test flags
CARGO_FLAGS=(--locked)
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
  if cargo check --manifest-path "$ROOT/rust_out_shared/Cargo.toml" --offline --locked --quiet 2>/dev/null; then
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
      Owner: Kernel. Reason: Google C++ Cord string type (ctype=CORD); internal C++ rope.
      Task: N/A (kernel-layout exclusion, not missing application behavior; cord fields use ProtoString/ProtoBytes).
  - gtest_matchers_test.rs:
      Owner: Test Infra. Reason: protobuf_gtest_matchers internal C++ test framework integration.
      Task: N/A (tests C++ matcher plumbing, not the application trait API).
  - no_internal_access_test.rs:
      Owner: Kernel. Reason: Asserts __internal == (); pbrs uses a module with SealedInternal trait.
      Task: N/A (excluded by design; sealed module pattern).
  - package_disambiguation_test.rs:
      Owner: Upstream. Reason: Empty test stub in pinned upstream Google protobuf v35.1 release.
      Task: N/A (no tests present).
  - extensions_test.rs:
      Owner: Codegen. Reason: Tests Edition 2024 custom extensions (extensions.proto).
      Task: CG-13, CG-14 (pending Edition 2024 descriptor/extension support).
  - edition2023 str_view cpp VIEW:
      Owner: Codegen. Reason: C++ string_view (pb.cpp.string_type=VIEW); tested as standard string.
      Task: N/A (C++-specific layout; application view API covered via as_view()/ProtoStr).
  - proto! #[cfg(bzl)] qualified paths:
      Owner: Build Tooling. Reason: Bazel-specific package path qualification (::crate::Type).
      Task: N/A (Bazel-specific; Cargo workspace build).
SKIPS
echo "  (Kernel-layout exclusions assert C++/arena implementation internals,"
echo "   not application API behavior; see docs/codegen-compatibility.md sections 4-5.)"

echo "============================================================"
echo " Summary"
echo "============================================================"
echo "  Crates tested: $CRATES_COUNT / 19"
echo "  Total passed:  $TOTAL_PASSED"
echo "  Total failed:  $TOTAL_FAILED"
echo "  Exclusions documented: 7 (5 files + 2 scoped; each with owner/reason/task)"
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
