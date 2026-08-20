#!/usr/bin/env bash
# Regenerate src/generated from third_party/protobuf conformance + WKT protos.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build --bin protoc-gen-pure-protobuf
PLUGIN="$ROOT/target/debug/protoc-gen-pure-protobuf"
OUT="$ROOT/src/generated"
SRC="$ROOT/third_party/protobuf/src"
TREE="$ROOT/third_party/protobuf"
export PURE_PROTOBUF_SHARED_POOL=1
run() {
  local proto="$1"
  protoc --plugin=protoc-gen-pure-protobuf="$PLUGIN" --pure-protobuf_out="$OUT" \
    -I "$SRC" -I "$TREE" "$proto"
}
run "$SRC/google/protobuf/any.proto"
run "$SRC/google/protobuf/duration.proto"
run "$SRC/google/protobuf/timestamp.proto"
run "$SRC/google/protobuf/struct.proto"
run "$SRC/google/protobuf/wrappers.proto"
run "$SRC/google/protobuf/field_mask.proto"
run "$SRC/google/protobuf/empty.proto"
run "$SRC/google/protobuf/test_messages_proto3.proto"
run "$SRC/google/protobuf/test_messages_proto2.proto"
run "$TREE/conformance/test_protos/test_messages_edition2023.proto"
run "$TREE/conformance/test_protos/test_messages_edition_unstable.proto"
run "$TREE/editions/golden/test_messages_proto2_editions.proto"
run "$TREE/editions/golden/test_messages_proto3_editions.proto"
cargo fmt -- "$OUT"/*.rs
echo "wrote $OUT"
