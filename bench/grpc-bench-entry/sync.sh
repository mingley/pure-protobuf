#!/usr/bin/env bash
# Sync the pbrs-grpc grpc_bench entries into a pinned local checkout.
#
# Usage: sync.sh [checkout-dir]
#   Default checkout: <repo>/target/grpc-bench (override with GRPC_BENCH_CHECKOUT).
# Clones LesnyRumcajs/grpc_bench at GRPC_BENCH_PIN when missing, refuses to
# touch a checkout at any other commit, then copies the two entry dirs plus
# a `git archive` snapshot of pure-protobuf HEAD as pbrs-src/.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
PIN=${GRPC_BENCH_PIN:-48b6b958832c995eee021361bdf3544d7c356fcc}
CHECKOUT=${1:-${GRPC_BENCH_CHECKOUT:-$ROOT/target/grpc-bench}}

if [ ! -d "$CHECKOUT/.git" ]; then
  echo "==> Cloning grpc_bench at $PIN into $CHECKOUT"
  mkdir -p "$(dirname "$CHECKOUT")"
  git clone https://github.com/LesnyRumcajs/grpc_bench.git "$CHECKOUT"
  (cd "$CHECKOUT" && git checkout --quiet "$PIN")
fi

HEAD=$(cd "$CHECKOUT" && git rev-parse HEAD)
if [ "$HEAD" != "$PIN" ]; then
  echo "ERROR: checkout HEAD $HEAD != pinned $PIN; refusing to sync." >&2
  exit 1
fi
if [ -n "$(cd "$CHECKOUT" && git status --porcelain -- . ':(exclude)results' | grep -v '^??' || true)" ]; then
  echo "ERROR: checkout has tracked modifications; refusing to sync." >&2
  exit 1
fi

PBRS_REV=$(cd "$ROOT" && git rev-parse HEAD)
echo "==> Syncing entries + pbrs-src@$PBRS_REV"
cp -r "$HERE/rust_pbrs_mt_bench" "$HERE/rust_pbrs_st_bench" "$CHECKOUT/"
rm -rf "$CHECKOUT/pbrs-src"
mkdir -p "$CHECKOUT/pbrs-src"
(cd "$ROOT" && git archive HEAD) | tar -x -C "$CHECKOUT/pbrs-src"
cat > "$CHECKOUT/pbrs-entry.json" <<EOF
{
  "grpc_bench_pin": "$PIN",
  "pbrs_rev": "$PBRS_REV"
}
EOF
echo "==> Ready: $CHECKOUT"
