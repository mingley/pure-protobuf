#!/usr/bin/env bash
# SB-11: cross-stack client/server matrix harness.
#
# Server cells pin each server under test (1/2/4 CPUs via taskset on Linux)
# behind one independent pinned open-loop load generator and step offered
# load to the sustained QPS at a p99 SLO; client cells run each client
# under test against one fixed reference server at a matched offered rate
# with verified server headroom. Required peers: pbrs-grpc, tonic,
# grpc-go, grpc-c++. A required peer that cannot run fails the stage.
#
#   ./scripts/stack-matrix.sh --stage smoke [--out-dir DIR]
#       [--server-peers native,go] [--client-peers native] [--skip-build] [-v]
#
# Peer pins live in rpc-bench/peers/*.json; the stage report lands in
# <out-dir>/report.json with per-cell logs under <out-dir>/logs/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGE="smoke"
OUT_DIR="$ROOT/target/stack-matrix"
SERVER_PEERS=""
CLIENT_PEERS=""
SKIP_BUILD=0
VERBOSE=()

usage() {
  sed -n '2,20p' "$0"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --stage) STAGE="$2"; shift 2 ;;
    --stage=*) STAGE="${1#--stage=}"; shift ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --out-dir=*) OUT_DIR="${1#--out-dir=}"; shift ;;
    --server-peers) SERVER_PEERS="$2"; shift 2 ;;
    --server-peers=*) SERVER_PEERS="${1#--server-peers=}"; shift ;;
    --client-peers) CLIENT_PEERS="$2"; shift 2 ;;
    --client-peers=*) CLIENT_PEERS="${1#--client-peers=}"; shift ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -v|--verbose) VERBOSE=(-v); shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

case "$STAGE" in
  smoke|primary) ;;
  *) echo "unknown --stage '$STAGE' (want smoke or primary)" >&2; exit 2 ;;
esac

if [ "$SKIP_BUILD" -eq 0 ]; then
  # Release numbers always: smoke is wiring proof but must exercise the
  # same binaries. Bounded jobs like the other runners.
  (
    cd "$ROOT" && \
    CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target \
      cargo build --locked --release --manifest-path rpc-bench/Cargo.toml
  )
fi

ARGS=(--stage "$STAGE" --out-dir "$OUT_DIR")
if [ -n "$SERVER_PEERS" ]; then ARGS+=(--server-peers "$SERVER_PEERS"); fi
if [ -n "$CLIENT_PEERS" ]; then ARGS+=(--client-peers "$CLIENT_PEERS"); fi
ARGS+=("${VERBOSE[@]+"${VERBOSE[@]}"}")

set +e
python3 "$ROOT/bench/stack-matrix/run.py" "${ARGS[@]}"
CODE=$?
set -e

if [ -f "$OUT_DIR/report.json" ]; then
  python3 - "$OUT_DIR/report.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
print()
print(f"STACK-MATRIX {report['stage']}: matrix_complete={report.get('matrix_complete')}")
if report.get("fatal"):
    print(f"  fatal: {report['fatal']}")
for cell in report.get("cells", []):
    line = f"  {cell['id']:55} {cell['status']:11}"
    sustained = cell.get("sustained") or {}
    if sustained.get("success_qps") is not None:
        line += f" {sustained['success_qps']:>10}qps p99={sustained.get('p99_s', float('nan')) * 1000:.1f}ms"
        if sustained.get("server_cpu_per_rpc") is not None:
            line += f" srv={sustained['server_cpu_per_rpc'] * 1e6:.1f}us/rpc"
    elif sustained.get("client_cpu_per_rpc") is not None:
        line += f" cli={sustained['client_cpu_per_rpc'] * 1e6:.1f}us/rpc (soak)"
    if cell.get("reason"):
        line += f" ({cell['reason'][:100]})"
    print(line)
PY
fi

exit "$CODE"
