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
STAMP="$BUILD/.pbrs-protobuf-pin"
if [[ ! -x "$RUNNER" || ! -x "$PROTOC" || ! -f "$STAMP" || "$(cat "$STAMP")" != "$PIN" ]]; then
  cmake -S third_party/protobuf -B "$BUILD" \
    -Dprotobuf_BUILD_CONFORMANCE=ON \
    -Dprotobuf_BUILD_TESTS=OFF
  cmake --build "$BUILD" --parallel "$(nproc)" \
    --target conformance_test_runner --target protoc
  printf '%s\n' "$PIN" > "$STAMP"
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
mkdir -p "$OUT"
cargo build --release --bin conformance
BIN="$ROOT/target/release/conformance"
echo "===== required run 1 ====="
"$RUNNER" --maximum_edition 2023 --output_dir "$OUT" "$BIN"
echo "===== required run 2 ====="
"$RUNNER" --maximum_edition 2023 --output_dir "$OUT" "$BIN"
echo "===== recommended ====="
"$RUNNER" --enforce_recommended --maximum_edition 2023 --output_dir "$OUT" "$BIN"
