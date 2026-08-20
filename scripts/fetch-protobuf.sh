#!/usr/bin/env bash
# Clone protocolbuffers/protobuf at the vendored pin into third_party/protobuf.
# The tree is gitignored (~115MiB); this is how CI/dev gets conformance_test_runner.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PIN="$(cat "$ROOT/vendor/google/PIN")"
SHA="$(cat "$ROOT/vendor/google/SHA")"
DEST="$ROOT/third_party/protobuf"
if [[ -f "$DEST/version.json" ]] && grep -q '"protoc_version": "35.1"' "$DEST/version.json"; then
  echo "protobuf $PIN already at $DEST"
  exit 0
fi
rm -rf "$DEST"
git clone --depth 1 --branch "$PIN" https://github.com/protocolbuffers/protobuf.git "$DEST"
got="$(git -C "$DEST" rev-parse HEAD)"
if [[ "$got" != "$SHA" ]]; then
  echo "pin mismatch: wanted $SHA got $got" >&2
  exit 1
fi
echo "fetched $PIN ($SHA) -> $DEST"
