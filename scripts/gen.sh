#!/usr/bin/env bash
# Generate pbrs Rust from .proto files (protoc-gen-pbrs).
# Usage: gen.sh [-I DIR]... -o OUT file.proto...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
  echo "usage: $0 [-I DIR]... -o OUT file.proto..." >&2
  exit 2
}

INCLUDES=()
OUT=""
PROTOS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -I)
      [[ $# -ge 2 ]] || usage
      INCLUDES+=("-I" "$2")
      shift 2
      ;;
    -o)
      [[ $# -ge 2 ]] || usage
      OUT="$2"
      shift 2
      ;;
    -h | --help) usage ;;
    --)
      shift
      PROTOS+=("$@")
      break
      ;;
    -*) usage ;;
    *)
      PROTOS+=("$1")
      shift
      ;;
  esac
done

[[ -n "$OUT" && ${#PROTOS[@]} -gt 0 ]] || usage

if [[ ${#INCLUDES[@]} -eq 0 ]]; then
  for p in "${PROTOS[@]}"; do
    INCLUDES+=("-I" "$(dirname "$p")")
  done
fi

plugin() {
  if [[ -n "${PBRS_PLUGIN:-}" ]]; then
    echo "$PBRS_PLUGIN"
    return
  fi
  if command -v protoc-gen-pbrs >/dev/null 2>&1; then
    command -v protoc-gen-pbrs
    return
  fi
  for cand in "$ROOT/target/debug/protoc-gen-pbrs" "$ROOT/target/release/protoc-gen-pbrs"; do
    if [[ -x "$cand" ]]; then
      echo "$cand"
      return
    fi
  done
  if [[ -f "$ROOT/Cargo.toml" ]]; then
    (cd "$ROOT" && cargo build --bin protoc-gen-pbrs >/dev/null)
    echo "$ROOT/target/debug/protoc-gen-pbrs"
    return
  fi
  echo "pbrs gen: protoc-gen-pbrs not found (set PBRS_PLUGIN or PATH)" >&2
  exit 1
}

PLUGIN="$(plugin)"
command -v protoc >/dev/null 2>&1 || {
  echo "pbrs gen: protoc not on PATH" >&2
  exit 1
}
mkdir -p "$OUT"
protoc --plugin=protoc-gen-pbrs="$PLUGIN" --pbrs_out="$OUT" "${INCLUDES[@]}" "${PROTOS[@]}"
echo "wrote $OUT"
