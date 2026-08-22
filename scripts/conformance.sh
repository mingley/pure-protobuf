#!/usr/bin/env bash
# Build official conformance_test_runner from the vendored pin and run it
# against this crate's conformance binary (required, twice).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
./scripts/fetch-protobuf.sh
BUILD="$ROOT/target/conformance-build"
RUNNER="$BUILD/conformance_test_runner"
if [[ ! -x "$RUNNER" ]]; then
  cmake -S third_party/protobuf -B "$BUILD" -Dprotobuf_BUILD_CONFORMANCE=ON
  cmake --build "$BUILD" --target conformance_test_runner
fi
# cmake's protoc matches the runner pin; put it first so build.rs can
# regenerate the FDS when system protoc is missing.
if [[ -x "$BUILD/protoc" ]]; then
  export PATH="$BUILD:$PATH"
fi
OUT="${CONFORMANCE_OUTPUT_DIR:-$ROOT/target/conformance-out}"
mkdir -p "$OUT"
cargo build --release --bin conformance
BIN="$ROOT/target/release/conformance"
echo "===== required run 1 ====="
"$RUNNER" --maximum_edition 2023 --output_dir "$OUT" "$BIN"
echo "===== required run 2 ====="
"$RUNNER" --maximum_edition 2023 --output_dir "$OUT" "$BIN"
