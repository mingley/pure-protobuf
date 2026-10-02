#!/usr/bin/env bash
# Regenerate only the registered flat bindings; never replace handwritten mod.rs.
# Usage: regen-generated.sh [--check]
# First run scripts/conformance.sh to provision the pinned source/compiler.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CHECK=0
usage() {
  echo "usage: $0 [--check]" >&2
  exit 2
}
case "${1:-}" in
  "") [[ $# -eq 0 ]] || usage ;;
  --check) CHECK=1; [[ $# -eq 1 ]] || usage ;;
  *) usage ;;
esac

OUT="$ROOT/src/generated"
TREE="$ROOT/third_party/protobuf"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
[[ "$TARGET" == /* ]] || TARGET="$ROOT/$TARGET"
BUILD="$ROOT/target/conformance-build"
PROTOC="$BUILD/protoc"
PIN="$(cat "$ROOT/vendor/google/PIN")"
SHA="$(cat "$ROOT/vendor/google/SHA")"
fail() {
  echo "pbrs regen: $*" >&2
  exit 1
}

# Version alone is insufficient: the source checkout and conformance build's
# provenance stamp must agree with the repository's immutable upstream pin.
[[ -d "$TREE" ]] || fail "missing pinned source; run scripts/conformance.sh first"
GOT_SHA="$(git -C "$TREE" rev-parse HEAD)"
[[ "$GOT_SHA" == "$SHA" ]] || fail "source pin mismatch: wanted $SHA, got $GOT_SHA"
git -C "$TREE" diff --quiet HEAD -- || fail "pinned protobuf source has tracked changes"
[[ -z "$(git -C "$TREE" ls-files --others --exclude-standard -- '*.proto')" ]] ||
  fail "pinned protobuf source has untracked proto inputs"
[[ -x "$PROTOC" && -f "$BUILD/.pbrs-protobuf-pin" ]] ||
  fail "missing pinned protoc build; run scripts/conformance.sh first"
[[ "$(cat "$BUILD/.pbrs-protobuf-pin")" == "$PIN $SHA" ]] ||
  fail "protoc build pin mismatch; rebuild with scripts/conformance.sh"
[[ "$("$PROTOC" --version)" == "libprotoc ${PIN#v}" ]] ||
  fail "protoc version does not match $PIN"

PROTOS=(
  src/google/protobuf/any.proto
  src/google/protobuf/duration.proto
  src/google/protobuf/timestamp.proto
  src/google/protobuf/struct.proto
  src/google/protobuf/wrappers.proto
  src/google/protobuf/field_mask.proto
  src/google/protobuf/empty.proto
  src/google/protobuf/test_messages_proto3.proto
  src/google/protobuf/test_messages_proto2.proto
  conformance/test_protos/test_messages_edition2023.proto
  conformance/test_protos/test_messages_edition_unstable.proto
  editions/golden/test_messages_proto2_editions.proto
  editions/golden/test_messages_proto3_editions.proto
)
FILES=()
for proto in "${PROTOS[@]}"; do
  name="${proto##*/}"
  FILES+=("${name%.proto}.rs")
done
# Fail when the public registry changes until this input list is reconciled.
# In particular, pb_struct's explicit path must still map to struct.rs.
REGISTERED="$(python3 - "$OUT/mod.rs" <<'PY'
import re
import sys
from pathlib import Path
registry = Path(sys.argv[1]).read_text()
modules = re.findall(r'((?:#\[[^\n]*\]\s*)*)pub mod (\w+);', registry)
files = []
for attributes, name in modules:
    paths = re.findall(r'#\[path\s*=\s*"([^"]+)"\]', attributes)
    files.append(paths[0] if paths else f'{name}.rs')
print('\n'.join(sorted(files)))
PY
)"
[[ "$REGISTERED" == "$(printf '%s\n' "${FILES[@]}" | LC_ALL=C sort)" ]] ||
  fail "bundled input list differs from the handwritten module registry"

# Ambient consumer configuration must not change the checked bundled profile.
for variable in "${!PURE_PROTOBUF_@}"; do
  unset "$variable"
done
cargo build --locked --bin protoc-gen-pbrs --target-dir "$TARGET"
PLUGIN="$TARGET/debug/protoc-gen-pbrs"
[[ -x "$PLUGIN" ]] || fail "built plugin is missing at $PLUGIN"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/pbrs-regen.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
for proto in "${PROTOS[@]}"; do
  "$PROTOC" --plugin=protoc-gen-pbrs="$PLUGIN" \
    --pbrs_out="shared_pool=true:$STAGE" \
    -I "$TREE/src" -I "$TREE" "$TREE/$proto"
done

STAGED_FILES=()
for name in "${FILES[@]}"; do
  [[ -f "$STAGE/$name" ]] || fail "generator did not emit registered binding $name"
  [[ ! -L "$OUT/$name" ]] || fail "registered binding is a symlink: $name"
  STAGED_FILES+=("$STAGE/$name")
done
rustfmt --edition 2021 --config-path "$ROOT/rustfmt.toml" "${STAGED_FILES[@]}"

# Check all outputs before writing any binding. Hierarchies and generator
# registries stay in the disposable stage; unknown checkout files stay intact.
DRIFT=0
for name in "${FILES[@]}"; do
  if ! cmp -s "$STAGE/$name" "$OUT/$name"; then
    if [[ "$CHECK" -eq 1 ]]; then
      echo "pbrs regen: generated binding drift: src/generated/$name" >&2
      DRIFT=1
    else
      cp "$STAGE/$name" "$OUT/$name"
    fi
  fi
done
[[ "$DRIFT" -eq 0 ]] || exit 1
if [[ "$CHECK" -eq 1 ]]; then
  echo "checked ${#FILES[@]} bundled bindings ($PIN $SHA)"
else
  echo "regenerated ${#FILES[@]} bundled bindings ($PIN $SHA)"
fi
