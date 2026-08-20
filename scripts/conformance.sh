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
cargo build --release --bin conformance
BIN="$ROOT/target/release/conformance"
echo "===== required run 1 ====="
"$RUNNER" --maximum_edition 2023 "$BIN"
echo "===== required run 2 ====="
"$RUNNER" --maximum_edition 2023 "$BIN"
