#!/usr/bin/env bash
# Regenerate tests/google_gen from vendor/google/rust/test unittest protos.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build --bin protoc-gen-pure-protobuf
PLUGIN="$ROOT/target/debug/protoc-gen-pure-protobuf"
OUT="$ROOT/tests/google_gen"
rm -rf "$OUT"
mkdir -p "$OUT"
export PURE_PROTOBUF_NO_REFLECT=1 PURE_PROTOBUF_NO_WKT=1 PURE_PROTOBUF_EMIT_DEPS=1
protoc --plugin=protoc-gen-pure-protobuf="$PLUGIN" --pure-protobuf_out="$OUT" \
  -I "$ROOT/vendor/google" \
  "$ROOT/vendor/google/rust/test/unittest_proto3.proto"
mv "$OUT/unittest_proto3.rs" /tmp/pure-protobuf-u3.rs
protoc --plugin=protoc-gen-pure-protobuf="$PLUGIN" --pure-protobuf_out="$OUT" \
  -I "$ROOT/vendor/google" \
  "$ROOT/vendor/google/rust/test/unittest_proto3_optional.proto"
mv /tmp/pure-protobuf-u3.rs "$OUT/unittest_proto3.rs"
protoc --plugin=protoc-gen-pure-protobuf="$PLUGIN" --pure-protobuf_out="$OUT" \
  -I "$ROOT/vendor/google" \
  "$ROOT/vendor/google/rust/test/unittest.proto"
cargo fmt -- "$OUT"/*.rs "$ROOT/tests/google_shared.rs"
echo "wrote $OUT"
