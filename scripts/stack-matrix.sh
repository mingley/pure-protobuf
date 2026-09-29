#!/usr/bin/env bash
# SB-11: cross-stack client/server matrix harness.
# SB-12: connection-scale scenarios (idle RSS, stream RSS, TLS rate, storm).
#
# --stage path (SB-11): server cells pin each server under test (1/2/4 CPUs
# via taskset on Linux) behind one independent pinned open-loop load
# generator and step offered load to the sustained QPS at a p99 SLO;
# client cells run each client under test against one fixed reference
# server at a matched offered rate with verified server headroom.
# Required peers: pbrs-grpc, tonic, grpc-go, grpc-c++. A required peer
# that cannot run fails the stage.
#
#   ./scripts/stack-matrix.sh --stage smoke [--out-dir DIR]
#       [--server-peers native,go] [--client-peers native] [--skip-build] [-v]
#
# --scenario path (SB-12): run one bench/stack-matrix/scenarios/conn-scale-*
# scenario (conn-scale-idle, conn-scale-active-streams,
# conn-scale-tls-handshake, conn-scale-storm) for the SB-11 server peers.
# --smoke keeps >=3 repeats with tiny ladders; only fail exits nonzero
# (invalid/unsupported/not_run are not losses).
#
#   ./scripts/stack-matrix.sh --scenario conn-scale-idle [--smoke]
#       [--out-dir DIR] [--server-peers native] [--repeats N] [--skip-build] [-v]
#
# Peer pins live in rpc-bench/peers/*.json; the stage/scenario report lands
# in <out-dir>/report.json with per-cell logs under <out-dir>/logs/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STAGE="smoke"
SCENARIO=""
SMOKE=0
REPEATS=""
OUT_DIR="$ROOT/target/stack-matrix"
SERVER_PEERS=""
CLIENT_PEERS=""
SKIP_BUILD=0
INCLUDE_OPTIONAL=0
VERBOSE=()

usage() {
  sed -n '2,30p' "$0"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --stage) STAGE="$2"; shift 2 ;;
    --stage=*) STAGE="${1#--stage=}"; shift ;;
    --scenario) SCENARIO="$2"; shift 2 ;;
    --scenario=*) SCENARIO="${1#--scenario=}"; shift ;;
    --smoke) SMOKE=1; shift ;;
    --repeats) REPEATS="$2"; shift 2 ;;
    --repeats=*) REPEATS="${1#--repeats=}"; shift ;;
    --include-optional) INCLUDE_OPTIONAL=1; shift ;;
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

if [ -n "$SCENARIO" ]; then
  case "$SCENARIO" in
    conn-scale-*) ;;
    *) echo "unknown --scenario '$SCENARIO' (want bench/stack-matrix/scenarios/conn-scale-*)" >&2; exit 2 ;;
  esac
  if [ ! -f "$ROOT/bench/stack-matrix/scenarios/$SCENARIO.json" ]; then
    echo "scenario file missing: bench/stack-matrix/scenarios/$SCENARIO.json" >&2; exit 2
  fi
  if [ -n "$CLIENT_PEERS" ]; then
    echo "--client-peers is SB-11-only; SB-12 scenarios vary the server peer" >&2; exit 2
  fi
else
  case "$STAGE" in
    smoke|primary) ;;
    *) echo "unknown --stage '$STAGE' (want smoke or primary)" >&2; exit 2 ;;
  esac
  if [ "$SMOKE" -eq 1 ] || [ -n "$REPEATS" ]; then
    echo "--smoke/--repeats need --scenario (SB-12)" >&2; exit 2
  fi
fi

if [ "$SKIP_BUILD" -eq 0 ]; then
  # Release numbers always: smoke is wiring proof but must exercise the
  # same binaries. Bounded jobs like the other runners.
  (
    cd "$ROOT" && \
    CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target \
      cargo build --locked --release --manifest-path rpc-bench/Cargo.toml
  )
fi

if [ -n "$SCENARIO" ]; then
  ARGS=(--scenario "$SCENARIO" --out-dir "$OUT_DIR")
  if [ -n "$SERVER_PEERS" ]; then ARGS+=(--server-peers "$SERVER_PEERS"); fi
  if [ "$SMOKE" -eq 1 ]; then ARGS+=(--smoke); fi
  if [ -n "$REPEATS" ]; then ARGS+=(--repeats "$REPEATS"); fi
  if [ "$INCLUDE_OPTIONAL" -eq 1 ]; then ARGS+=(--include-optional); fi
  ARGS+=("${VERBOSE[@]+"${VERBOSE[@]}"}")
  set +e
  python3 "$ROOT/bench/stack-matrix/scenarios/conn_scale.py" "${ARGS[@]}"
  CODE=$?
  set -e
else
  ARGS=(--stage "$STAGE" --out-dir "$OUT_DIR")
  if [ -n "$SERVER_PEERS" ]; then ARGS+=(--server-peers "$SERVER_PEERS"); fi
  if [ -n "$CLIENT_PEERS" ]; then ARGS+=(--client-peers "$CLIENT_PEERS"); fi
  if [ "$INCLUDE_OPTIONAL" -eq 1 ]; then ARGS+=(--include-optional); fi
  ARGS+=("${VERBOSE[@]+"${VERBOSE[@]}"}")

  set +e
  python3 "$ROOT/bench/stack-matrix/run.py" "${ARGS[@]}"
  CODE=$?
  set -e
fi

if [ -f "$OUT_DIR/report.json" ]; then
  python3 - "$OUT_DIR/report.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
print()
if report.get("schema") == "sb12-conn-scale/1":
    print(f"SB-12 {report['scenario']}: matrix_complete={report.get('matrix_complete')}")
    if report.get("fatal"):
        print(f"  fatal: {report['fatal']}")
    for cell in report.get("cells", []):
        line = f"  {cell['id']:50} {cell['status']:11}"
        summary = cell.get("summary") or {}
        if summary.get("median_bytes_per_connection") is not None:
            line += f" {summary['median_bytes_per_connection']:>10.0f}B/conn"
        elif summary.get("median_bytes_per_stream") is not None:
            line += f" {summary['median_bytes_per_stream']:>10.0f}B/stream"
        elif summary.get("median_handshakes_per_s_per_core") is not None:
            line += f" {summary['median_handshakes_per_s_per_core']:>10.0f}hs/s/core"
        elif summary.get("median_accepts_per_s") is not None:
            line += f" {summary['median_accepts_per_s']:>10.0f}acc/s"
        if cell.get("reason"):
            line += f" ({cell['reason'][:100]})"
        print(line)
else:
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
