#!/usr/bin/env bash
# Run the pbrs-grpc grpc_bench entries beside peer entries.
#
# Same methodology as the harness bench.sh: plaintext unary SayHello,
# 50 connections, 1000 concurrent calls, 5s warmup + 20s measured with
# pinned ghz, per-entry docker stats. Adds: CPU-count matrix, seeded
# randomized entry order per repeat, per-cell reports, host record.
#
# Env knobs (defaults = harness methodology + SB-17 matrix):
#   GRPC_BENCH_CHECKOUT  pinned checkout from sync.sh (default target/grpc-bench)
#   GRPC_SCENARIOS       e.g. "complex_proto" (images are tagged per scenario)
#   GRPC_CPUS            e.g. "1 2 4"
#   GRPC_ENTRIES         space-separated *_bench dirs (default: pbrs + SB-17 peers)
#   GRPC_REPEATS         repeats per (scenario, cpu) cell (default 1; order shuffled per repeat)
#   GRPC_SEED            shuffle seed (default 170017)
#   GRPC_BENCHMARK_DURATION / GRPC_BENCHMARK_WARMUP / GRPC_SERVER_RAM /
#   GRPC_CLIENT_CONNECTIONS / GRPC_CLIENT_CONCURRENCY / GRPC_CLIENT_QPS /
#   GRPC_CLIENT_CPUS / GRPC_GHZ_TAG / GRPC_IMAGE_NAME (harness defaults)
#   GRPC_SKIP_BUILD=1    reuse already-built images
#
# Peer build/run failures are recorded in failures.log and do not stop
# the matrix; any pbrs cell failure exits nonzero at the end.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CHECKOUT=${GRPC_BENCH_CHECKOUT:-$ROOT/target/grpc-bench}
PIN=${GRPC_BENCH_PIN:-48b6b958832c995eee021361bdf3544d7c356fcc}

SCENARIOS=${GRPC_SCENARIOS:-complex_proto}
CPUS_LIST=${GRPC_CPUS:-"1 2 4"}
ENTRIES=${GRPC_ENTRIES:-"rust_pbrs_mt_bench rust_pbrs_st_bench rust_tonic_mt_bench rust_tonic_st_bench rust_thruster_mt_bench rust_thruster_st_bench go_grpc_bench java_hotspot_grpc_g1gc_bench java_vertx_grpc_bench dotnet_grpc_bench cpp_grpc_mt_bench cpp_grpc_st_bench"}
REPEATS=${GRPC_REPEATS:-1}
SEED=${GRPC_SEED:-170017}
export GRPC_REPEATS="$REPEATS" GRPC_SEED="$SEED"

export GRPC_BENCHMARK_DURATION=${GRPC_BENCHMARK_DURATION:-"20s"}
export GRPC_BENCHMARK_WARMUP=${GRPC_BENCHMARK_WARMUP:-"5s"}
export GRPC_SERVER_RAM=${GRPC_SERVER_RAM:-"512m"}
export GRPC_CLIENT_CONNECTIONS=${GRPC_CLIENT_CONNECTIONS:-"50"}
export GRPC_CLIENT_CONCURRENCY=${GRPC_CLIENT_CONCURRENCY:-"1000"}
export GRPC_CLIENT_QPS=${GRPC_CLIENT_QPS:-"0"}
export GRPC_CLIENT_QPS=$(( GRPC_CLIENT_QPS / GRPC_CLIENT_CONCURRENCY ))
export GRPC_CLIENT_CPUS=${GRPC_CLIENT_CPUS:-"1"}
export GRPC_IMAGE_NAME="${GRPC_IMAGE_NAME:-grpc_bench}"
export GRPC_GHZ_TAG="${GRPC_GHZ_TAG:-0.114.0}"

# --network=host does not reach the host on Docker Desktop; ghz talks
# to the published port via the host gateway there instead.
if [ "$(uname)" = "Darwin" ]; then
  GHZ_NET_ARGS=""
  GHZ_TARGET=${GRPC_GHZ_TARGET:-host.docker.internal:50051}
else
  GHZ_NET_ARGS="--network=host"
  GHZ_TARGET=${GRPC_GHZ_TARGET:-127.0.0.1:50051}
fi

HEAD=$(cd "$CHECKOUT" && git rev-parse HEAD)
if [ "$HEAD" != "$PIN" ]; then
  echo "ERROR: checkout HEAD $HEAD != pinned $PIN; run sync.sh first." >&2
  exit 1
fi
for e in $ENTRIES; do
  if [ ! -d "$CHECKOUT/$e" ]; then echo "ERROR: missing entry $e" >&2; exit 1; fi
done

RESULTS_DIR="results/$(date '+%y%m%dT%H%M%S')-pbrs"
mkdir -p "$CHECKOUT/$RESULTS_DIR"
FAILURES="$CHECKOUT/$RESULTS_DIR/failures.log"
PBRS_FAILED=0
: > "$FAILURES"

cd "$CHECKOUT"

host_record() {
  {
    echo "date_utc=$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "uname=$(uname -a)"
    echo "cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu)"
    [ "$(uname)" = "Darwin" ] && sysctl -n machdep.cpu.brand_string 2>/dev/null | sed 's/^/cpu_model=/' || true
    echo "mem_bytes=$( ([ "$(uname)" = "Darwin" ] && sysctl -n hw.memsize) || grep MemTotal /proc/meminfo | awk '{print $2*1024}')"
    docker version --format 'docker_server={{.Server.Version}} docker_arch={{.Server.Arch}}' 2>/dev/null || echo "docker_server=unknown"
    echo "ghz_tag=$GRPC_GHZ_TAG"
    echo "grpc_bench_pin=$PIN"
    cat pbrs-entry.json 2>/dev/null | tr -d '\n' | sed 's/^/pins=/' || true
    echo ""
  } > "$RESULTS_DIR/host.txt"
}

build_entry() {
  local entry=$1 scenario=$2
  echo "==> Building $entry ($scenario)"
  if ! DOCKER_BUILDKIT=1 docker image build --force-rm \
      --file "$entry/Dockerfile" \
      --tag "$GRPC_IMAGE_NAME:${entry}-$scenario" \
      . >"$RESULTS_DIR/build-$entry.log" 2>&1; then
    echo "BUILD-FAIL $entry $scenario (see $RESULTS_DIR/build-$entry.log)" | tee -a "$FAILURES"
    return 1
  fi
  return 0
}

wait_on_tcp50051() {
  for ((i=1;i<=300;i++)); do
    if command -v nc >/dev/null; then nc -z localhost 50051 && return 0
    else (echo > /dev/tcp/localhost/50051) >/dev/null 2>&1 && return 0; fi
    sleep .1
  done
  return 1
}

ghz_run() { # $1 = duration, rest passed through
  local duration=$1; shift
  # shellcheck disable=SC2086
  docker run --name ghz --rm $GHZ_NET_ARGS \
    -v "${PWD}/proto:/proto:ro" -v "${PWD}/payload:/payload:ro" \
    --cpus "$GRPC_CLIENT_CPUS" \
    "ghcr.io/bojand/ghz:${GRPC_GHZ_TAG}" \
    --proto=/proto/helloworld/helloworld.proto \
    --call=helloworld.Greeter.SayHello \
    --disable-template-functions --disable-template-data \
    --insecure \
    --concurrency="$GRPC_CLIENT_CONCURRENCY" \
    --connections="$GRPC_CLIENT_CONNECTIONS" \
    --rps="$GRPC_CLIENT_QPS" \
    --duration "$duration" \
    --data-file /payload/payload \
    "$GHZ_TARGET" "$@"
}

run_cell() { # $1 entry, $2 cpus, $3 rep
  local entry=$1 cpus=$2 rep=$3
  local name="${entry}.cpus${cpus}.rep${rep}"
  echo "==> [$name] server cpus=$cpus"
  export GRPC_SERVER_CPUS="$cpus"
  docker run --name "$entry" --rm --cpus "$cpus" --memory "$GRPC_SERVER_RAM" \
    -e GRPC_SERVER_CPUS -e GRPC_SERVER_RAM -p 50051:50051 --detach --tty \
    "$GRPC_IMAGE_NAME:${entry}-$GRPC_REQUEST_SCENARIO" >/dev/null
  if ! wait_on_tcp50051; then
    echo "RUN-FAIL $name server unresponsive" | tee -a "$FAILURES"
    docker container stop "$entry" >/dev/null 2>&1 || true
    case "$entry" in rust_pbrs_*) PBRS_FAILED=1;; esac
    return 0
  fi
  if [ "$GRPC_BENCHMARK_WARMUP" != "0s" ]; then
    ghz_run "$GRPC_BENCHMARK_WARMUP" > /dev/null || true
  fi
  rm -f "$RESULTS_DIR/$entry.stats"
  ./collect_stats.sh "$entry" "$RESULTS_DIR" &
  local stats_pid=$!
  if ! ghz_run "$GRPC_BENCHMARK_DURATION" >"$RESULTS_DIR/${name}.report" 2>"$RESULTS_DIR/${name}.ghz-stderr"; then
    echo "RUN-FAIL $name ghz error" | tee -a "$FAILURES"
    case "$entry" in rust_pbrs_*) PBRS_FAILED=1;; esac
  else
    grep "Requests/sec" "$RESULTS_DIR/${name}.report" | sed -E 's/^ +/    /' || echo "    (no Requests/sec line)"
  fi
  docker container stop "$entry" >/dev/null 2>&1 || true
  kill -INT "$stats_pid" 2>/dev/null || true
  wait "$stats_pid" 2>/dev/null || true
  mv -f "$RESULTS_DIR/$entry.stats" "$RESULTS_DIR/${name}.stats" 2>/dev/null || true
  return 0
}

shuffled() { # $1 rep -> entries in seeded shuffled order
  # shellcheck disable=SC2086
  python3 -c "import random; es='''$ENTRIES'''.split(); random.Random($SEED+$1).shuffle(es); print(' '.join(es))"
}

host_record
BUILT=""
for scenario in $SCENARIOS; do
  export GRPC_REQUEST_SCENARIO="$scenario"
  if ! sh setup_scenario.sh "$scenario" true; then echo "Scenario setup failed." >&2; exit 1; fi
  if [ "${GRPC_SKIP_BUILD:-0}" != "1" ]; then
    for e in $ENTRIES; do
      case " $BUILT " in
        *" $e-$scenario "*) ;;
        *) if build_entry "$e" "$scenario"; then BUILT="$BUILT $e-$scenario"; fi ;;
      esac
    done
  fi
  for cpus in $CPUS_LIST; do
    rep=1
    while [ "$rep" -le "$REPEATS" ]; do
      for e in $(shuffled "$rep"); do
        if ! docker image inspect "$GRPC_IMAGE_NAME:${e}-$scenario" >/dev/null 2>&1; then
          echo "SKIP $e $scenario (no image)" | tee -a "$FAILURES"
          continue
        fi
        run_cell "$e" "$cpus" "$rep"
      done
      rep=$((rep + 1))
    done
  done
done

{
  echo "Benchmark Execution Parameters (pbrs entry run):"
  (cd "$CHECKOUT" && git log -1 --pretty="%h %cD %cn %s")
  for v in GRPC_BENCHMARK_DURATION GRPC_BENCHMARK_WARMUP GRPC_SERVER_RAM GRPC_CLIENT_CONNECTIONS GRPC_CLIENT_CONCURRENCY GRPC_CLIENT_QPS GRPC_CLIENT_CPUS GRPC_REQUEST_SCENARIO GRPC_GHZ_TAG GRPC_SEED GRPC_REPEATS; do
    eval "echo - $v=\$$v"
  done
  echo "- cpus_list=$CPUS_LIST scenarios=$SCENARIOS"
  echo "- entries=$ENTRIES"
  echo "- ghz_target=$GHZ_TARGET"
} > "$CHECKOUT/$RESULTS_DIR/bench.params"

echo "-----"
echo "Results in $CHECKOUT/$RESULTS_DIR"
if [ -s "$FAILURES" ]; then echo "Failures:"; cat "$FAILURES"; fi
[ "$PBRS_FAILED" = "0" ]
