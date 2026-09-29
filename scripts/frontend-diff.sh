#!/usr/bin/env bash
# Differential harness for the GN-08 pinned .proto frontend corpus.
#
# Baseline mode (no frontend present) records pinned-protoc descriptor output
# for every corpus entry and checks each outcome against corpus.json:
#   ./scripts/frontend-diff.sh --corpus pinned
#
# Compare mode diffs a frontend under test against that baseline:
#   ./scripts/frontend-diff.sh --corpus pinned --frontend <bin>
# where <bin> implements: BIN --mode parse|link|full -I DIR... [--out FDS] FILE...
# (parse compares accept/reject verdicts; link/full compare descriptor bytes).
#
# Modes: parse (verdict only), link (FileDescriptorSet bytes), full (FDS with
# source info + retained options). No network access at test time: the corpus
# .proto tree is vendored (regen: tests/frontend_corpus/fetch-corpus.sh) and
# protoc comes from scripts/build-pinned-protoc.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORPUS=""
FRONTEND=""
OUT="$ROOT/target/frontend-diff"
MODES=()
ENTRIES=()

usage() {
  sed -n '2,17p' "$ROOT/scripts/frontend-diff.sh"
  echo "Options:"
  echo "  --corpus pinned     corpus selector (only 'pinned' exists)"
  echo "  --mode M            run one mode (parse|link|full); repeatable; default all"
  echo "  --frontend BIN      compare mode: diff BIN against the baseline"
  echo "  --entry ID          run one entry (e.g. 'envoy:envoy/type/v3/percent.proto'); repeatable"
  echo "  --out DIR           output directory (default target/frontend-diff)"
  echo "  --list              print entry ids and exit (no protoc build)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --corpus) CORPUS="${2:?--corpus needs a value}"; shift 2 ;;
    --mode) MODES+=("${2:?--mode needs a value}"); shift 2 ;;
    --frontend) FRONTEND="${2:?--frontend needs a value}"; shift 2 ;;
    --entry) ENTRIES+=("${2:?--entry needs a value}"); shift 2 ;;
    --out) OUT="${2:?--out needs a value}"; shift 2 ;;
    --list)
      python3 -c "import json; [print(e['id']) for e in json.load(open('$ROOT/tests/frontend_corpus/corpus.json'))['entries']]"
      exit 0
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$CORPUS" != "pinned" ]]; then
  echo "frontend-diff: --corpus pinned is required (got '${CORPUS:-<none>}')" >&2
  exit 2
fi
if [[ ${#MODES[@]} -gt 0 ]]; then
  for m in "${MODES[@]}"; do
    case "$m" in parse|link|full) ;; *) echo "bad --mode: $m" >&2; exit 2 ;; esac
  done
fi

# Reproducible baseline compiler (pinned v35.1; reuses a stamped build).
"$ROOT/scripts/build-pinned-protoc.sh" >&2
PROTOC="$ROOT/target/pinned-protoc-build/protoc"

ARGS=(--root "$ROOT" --protoc "$PROTOC" --out "$OUT")
if [[ ${#MODES[@]} -gt 0 ]]; then
  for m in "${MODES[@]}"; do ARGS+=(--mode "$m"); done
fi
if [[ ${#ENTRIES[@]} -gt 0 ]]; then
  for e in "${ENTRIES[@]}"; do ARGS+=(--entry "$e"); done
fi
if [[ -n "$FRONTEND" ]]; then ARGS+=(--frontend "$FRONTEND"); fi

exec python3 "$ROOT/tests/frontend_corpus/diff.py" "${ARGS[@]}"
