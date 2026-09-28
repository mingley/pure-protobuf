#!/usr/bin/env bash
# Build the pinned upstream protoc, which has built-in experimental Rust output.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIN="$(cat "$ROOT/vendor/google/PIN")"
SHA="$(cat "$ROOT/vendor/google/SHA")"
BUILD="$ROOT/target/pinned-protoc-build"
PROTOC="$BUILD/protoc"
STAMP="$BUILD/.pbrs-protoc-pin"

"$ROOT/scripts/fetch-protobuf.sh"
ACTUAL_SHA="$(git -C "$ROOT/third_party/protobuf" rev-parse HEAD)"
if [[ "$ACTUAL_SHA" != "$SHA" ]]; then
  echo "Pinned protobuf source mismatch: expected $SHA, got $ACTUAL_SHA" >&2
  exit 1
fi

if [[ -x "$PROTOC" && -f "$STAMP" && "$(cat "$STAMP")" == "$PIN $SHA" ]] &&
   [[ "$("$PROTOC" --version)" == "libprotoc ${PIN#v}" ]] &&
   [[ "$("$PROTOC" --help)" == *'--rust_out=OUT_DIR'* ]]; then
  echo "Reusing pinned protoc at $PROTOC"
else
  cmake -S "$ROOT/third_party/protobuf" -B "$BUILD" \
    -Dprotobuf_BUILD_CONFORMANCE=OFF \
    -Dprotobuf_BUILD_TESTS=OFF
  cmake --build "$BUILD" --parallel "${PBRS_PROTOC_JOBS:-2}" --target protoc
fi

if [[ "$("$PROTOC" --version)" != "libprotoc ${PIN#v}" ]] ||
   [[ "$("$PROTOC" --help)" != *'--rust_out=OUT_DIR'* ]]; then
  echo "Pinned protoc is missing or does not support built-in Rust output: $PROTOC" >&2
  exit 1
fi
printf '%s %s\n' "$PIN" "$SHA" > "$STAMP"
echo "Pinned protoc ready: $PROTOC ($("$PROTOC" --version))"
