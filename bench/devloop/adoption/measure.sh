#!/usr/bin/env bash
# Run every registered adoption codec cell through the established collector.
# Build the parent devloop release binary first, with genuine protoc 35.1.
set -euo pipefail

ADOPTION_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
ADOPTION_BIN="${PBRS_ADOPTION_DEVLOOP_BIN:-$ADOPTION_ROOT/bench/devloop/target/release/devloop}"
ADOPTION_OUT="${PBRS_ADOPTION_OUT:-$ADOPTION_ROOT/target/devloop/adoption-codec}"
cd "$ADOPTION_ROOT"
mkdir -p "$ADOPTION_OUT"

# Frozen initial dev-loop baseline: N=16, 2N=32, three repeats, the parent
# collector's unchanged 100-operation warmup and identical preparation.
# Keep eight bounded reports so completed measurements survive a later error.
ADOPTION_REGISTRY="$("$ADOPTION_BIN" list)"
for ADOPTION_CODEC in pbrs prost; do
    for ADOPTION_OP in fresh_encode owned_decode read_all fully_read_clone; do
        ADOPTION_IDS="$(while read -r ADOPTION_ID ADOPTION_KIND ADOPTION_IMPL; do
            case "$ADOPTION_ID" in
                codec.adoption."$ADOPTION_CODEC".*."$ADOPTION_OP") printf '%s\n' "$ADOPTION_ID" ;;
            esac
        done <<< "$ADOPTION_REGISTRY")"
        ADOPTION_COUNT="$(printf '%s\n' "$ADOPTION_IDS" | wc -l | tr -d ' ')"
        if [ "$ADOPTION_COUNT" != 64 ]; then
            echo "Expected 64 $ADOPTION_CODEC $ADOPTION_OP cells, found $ADOPTION_COUNT" >&2
            exit 1
        fi
        ADOPTION_CELLS="$(printf '%s\n' "$ADOPTION_IDS" | paste -sd, -)"
        ADOPTION_REPORT="$ADOPTION_OUT/$ADOPTION_CODEC-$ADOPTION_OP.json"
        echo "Measuring $ADOPTION_CODEC $ADOPTION_OP: 64 cells -> $ADOPTION_REPORT"
        "$ADOPTION_BIN" run --cells "$ADOPTION_CELLS" --iters 16 --repeats 3 --out "$ADOPTION_REPORT"
        echo "Completed $ADOPTION_CODEC $ADOPTION_OP"
    done
done
