#!/usr/bin/env bash
# Approved CL08 frozen collector controls. Execute only during coordinator lease.
set -euo pipefail
cd /workspace/scratch/work/cl08
source /workspace/pure-protobuf/work/toolchain/env.sh
export PATH="$PWD/work/bin:$PATH"
export PBRS_REPLAY_VALGRIND=/workspace/pure-protobuf/work/toolchain/deb/usr/bin/valgrind
CELLS=$(cat work/profiles/callgrind/smoke-cells.txt)
BASE_BINARY=/workspace/scratch/work/sv09/work/bin/devloop-before
CANDIDATE_BINARY="$PWD/work/bin/devloop-candidate"
BASE_HASH=73fa59c733f0637b22ce8d58f035aa45cb13f0b7efc4c8e9bb4cdddcef54e0b9
CANDIDATE_HASH=efef1e7b44fd9b6f6defe08b0396879571035a65c411aa522c1f1b9fc5ad7f93
for REVISION in before after; do
  if [ "$REVISION" = before ]; then
    BINARY="$BASE_BINARY"
    EXPECTED="$BASE_HASH"
    SOURCE=cf3eee22c6b06324e299f81b76915651471d8025
  else
    BINARY="$CANDIDATE_BINARY"
    EXPECTED="$CANDIDATE_HASH"
    SOURCE=2321d736c0e9ed1b3323d24d99b2cc437c594774
  fi
  test "$(sha256sum "$BINARY" | cut -d' ' -f1)" = "$EXPECTED"
  test ! -e "work/profiles/callgrind/$REVISION.json"
  test ! -e "work/profiles/callgrind/$REVISION.raw.jsonl"
  export PBRS_REPLAY_RAW="$PWD/work/profiles/callgrind/$REVISION.raw.jsonl"
  export PBRS_DEVLOOP_COMMIT="$SOURCE"
  "$BINARY" run --cells "$CELLS" --iters 200 --repeats 3 --out "work/profiles/callgrind/$REVISION.json" \
    2>&1 | tee "work/logs/cl08-callgrind-$REVISION.log"
  test "$(sha256sum "$BINARY" | cut -d' ' -f1)" = "$EXPECTED"
done
