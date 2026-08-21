#!/usr/bin/env bash
# Regenerate tests/google_gen from vendor/google rust/test + utf8 protos.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build --bin protoc-gen-pbrs
export PBRS_PLUGIN="$ROOT/target/debug/protoc-gen-pbrs"
OUT="$ROOT/tests/google_gen"
rm -rf "$OUT"
mkdir -p "$OUT"
export PURE_PROTOBUF_NO_REFLECT=1 PURE_PROTOBUF_NO_WKT=1 PURE_PROTOBUF_EMIT_DEPS=1
I=(-I "$ROOT/vendor/google" -I "$ROOT/third_party/protobuf/src")
run() {
  "$ROOT/scripts/gen.sh" "${I[@]}" -o "$OUT" "$@"
}

# unittest_proto3 first; optional would otherwise clobber it if generated together.
run "$ROOT/vendor/google/rust/test/unittest_proto3.proto"
mv "$OUT/unittest_proto3.rs" "$OUT/.unittest_proto3.rs.bak"
run "$ROOT/vendor/google/rust/test/unittest_proto3_optional.proto"
mv "$OUT/.unittest_proto3.rs.bak" "$OUT/unittest_proto3.rs"

run "$ROOT/vendor/google/rust/test/unittest.proto"
run "$ROOT/vendor/google/rust/test/unittest_import.proto"
run "$ROOT/vendor/google/rust/test/map_unittest.proto"
run "$ROOT/vendor/google/rust/test/enums.proto"
run "$ROOT/vendor/google/rust/test/nested.proto"
run "$ROOT/vendor/google/rust/test/child.proto"
run "$ROOT/vendor/google/rust/test/parent.proto"
run "$ROOT/vendor/google/rust/test/edition2023.proto"
run "$ROOT/vendor/google/rust/test/bad_names.proto"
# extensions.proto is Edition 2024; kernel max is 2023. SKIP (extensions_test.rs is a stub).
run "$ROOT/vendor/google/rust/test/fields_with_imported_types.proto"
run "$ROOT/vendor/google/rust/test/imported_types.proto"
run "$ROOT/vendor/google/rust/test/import_public_grandparent.proto"
run "$ROOT/vendor/google/rust/test/import_public_primary_src.proto"
run "$ROOT/vendor/google/rust/test/import_public_non_primary_src1.proto"
run "$ROOT/vendor/google/rust/test/import_public_non_primary_src2.proto"
run "$ROOT/vendor/google/rust/test/import_public.proto"
run "$ROOT/vendor/google/rust/test/import_public2.proto"
run "$ROOT/vendor/google/rust/test/package_import.proto"
run "$ROOT/vendor/google/rust/test/package.proto"
run "$ROOT/vendor/google/rust/test/no_package_import.proto"
run "$ROOT/vendor/google/rust/test/no_package.proto"

"$ROOT/scripts/gen.sh" -I "$ROOT/vendor/google/rust-tests/shared" -o "$OUT" \
  "$ROOT/vendor/google/rust-tests/shared/utf8/no_features_proto2.proto" \
  "$ROOT/vendor/google/rust-tests/shared/utf8/no_features_proto3.proto" \
  "$ROOT/vendor/google/rust-tests/shared/utf8/feature_verify.proto"

cargo fmt -- "$OUT"/*.rs
echo "wrote $OUT ($(ls -1 "$OUT" | wc -l | tr -d ' ') files)"
