#!/usr/bin/env bash
# Run the official gRPC interop test cases against the pbrs-grpc kernel.
#
# Three passes, each one catching a different class of bug:
#
#   kernel client -> kernel server   both halves agree with each other
#   kernel client -> Go server       the kernel client speaks real gRPC
#   Go client     -> kernel server   the kernel server speaks real gRPC
#
# The Go passes require a Go toolchain and the pinned Go reference peer
# in tests/interop/go (google.golang.org/grpc @ dd51b1c90aaf / v1.85.0-dev).
# Cross-language execution fails closed unless --self-only is explicitly passed.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

INTEROP_REPORT="$ROOT/scripts/interop-report.py"

# Pinned Go reference peer (matching tests/interop/cases.json):
# commit dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef (v1.85.0-dev)
GO_PEER_PIN="dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef"
GO_PEER_VERSION="v1.85.0-dev ($GO_PEER_PIN)"

# Cases that need nothing beyond the base TestService contract.
BASE_CASES=(
  empty_unary
  large_unary
  client_streaming
  server_streaming
  ping_pong
  empty_stream
  cancel_after_begin
  cancel_after_first_response
  timeout_on_sleeping_server
  custom_metadata
  status_code_and_message
  special_status_message
  unimplemented_method
  unimplemented_service
)

# Cases built on SimpleRequest.expect_compressed and response_compressed.
# grpc-go implements them on neither side: its interop server ignores both
# fields (v1.85.0-dev @ dd51b1c90aaf interop/test_utils.go, where UnaryCall reads neither),
# and its interop client rejects the case names outright. They therefore only run in the
# self-interop pass, against the one implementation here that does honour them.
COMPRESSION_CASES=(
  client_compressed_unary
  server_compressed_unary
  client_compressed_streaming
  server_compressed_streaming
)

CASES=("${BASE_CASES[@]}" "${COMPRESSION_CASES[@]}")

SELF_ONLY=0
SKIP_BUILD="${SKIP_BUILD:-${GRPC_INTEROP_SKIP_BUILD:-0}}"
LOG_DIR=""
GRPC_INTEROP_PORT_VAL=""
GRPC_INTEROP_GO_PORT_VAL=""
CASE_TIMEOUT_SEC="${CASE_TIMEOUT_SEC:-${GRPC_INTEROP_CASE_TIMEOUT:-15}}"
STARTUP_TIMEOUT_SEC="${STARTUP_TIMEOUT_SEC:-${GRPC_INTEROP_STARTUP_TIMEOUT:-5}}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-${GRPC_INTEROP_MAX_ATTEMPTS:-2}}"
OVERALL_FAILED=0
USE_TLS="${USE_TLS:-${GRPC_INTEROP_USE_TLS:-0}}"
TLS_CA_FILE="${TLS_CA_FILE:-${GRPC_INTEROP_TLS_CA_FILE:-}}"
TLS_CERT_FILE="${TLS_CERT_FILE:-${GRPC_INTEROP_TLS_CERT_FILE:-}}"
TLS_KEY_FILE="${TLS_KEY_FILE:-${GRPC_INTEROP_TLS_KEY_FILE:-}}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --self-only)
      SELF_ONLY=1
      shift
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    --cases)
      GRPC_INTEROP_CASES="$2"
      shift 2
      ;;
    --cases=*)
      GRPC_INTEROP_CASES="${1#*=}"
      shift
      ;;
    --log-dir|--output-dir)
      LOG_DIR="$2"
      shift 2
      ;;
    --log-dir=*|--output-dir=*)
      LOG_DIR="${1#*=}"
      shift
      ;;
    --port)
      GRPC_INTEROP_PORT_VAL="$2"
      shift 2
      ;;
    --port=*)
      GRPC_INTEROP_PORT_VAL="${1#*=}"
      shift
      ;;
    --use-tls|--use_tls)
      USE_TLS=1
      shift
      ;;
    --use-tls=*|--use_tls=*)
      val="${1#*=}"
      case "$val" in
        true|1|yes) USE_TLS=1 ;;
        false|0|no) USE_TLS=0 ;;
        *) echo "invalid boolean for --use-tls: $val" >&2; exit 1 ;;
      esac
      shift
      ;;
    --tls-ca-file|--tls_ca_file)
      TLS_CA_FILE="$2"
      shift 2
      ;;
    --tls-ca-file=*|--tls_ca_file=*)
      TLS_CA_FILE="${1#*=}"
      shift
      ;;
    --tls-cert-file|--tls_cert_file)
      TLS_CERT_FILE="$2"
      shift 2
      ;;
    --tls-cert-file=*|--tls_cert_file=*)
      TLS_CERT_FILE="${1#*=}"
      shift
      ;;
    --tls-key-file|--tls_key_file)
      TLS_KEY_FILE="$2"
      shift 2
      ;;
    --tls-key-file=*|--tls_key_file=*)
      TLS_KEY_FILE="${1#*=}"
      shift
      ;;
    --timeout)
      CASE_TIMEOUT_SEC="$2"
      shift 2
      ;;
    --timeout=*)
      CASE_TIMEOUT_SEC="${1#*=}"
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ $USE_TLS -eq 1 ]]; then
  DEFAULT_TLS_DIR="$ROOT/pbrs-grpc/tests/tls_data"
  TLS_CA_FILE="${TLS_CA_FILE:-$DEFAULT_TLS_DIR/ca.crt}"
  TLS_CERT_FILE="${TLS_CERT_FILE:-$DEFAULT_TLS_DIR/server.crt}"
  TLS_KEY_FILE="${TLS_KEY_FILE:-$DEFAULT_TLS_DIR/server.key}"
  TRANSPORT="http2_tls"
else
  TRANSPORT="http2_cleartext"
fi

if [[ -n "${GRPC_INTEROP_CASES:-}" ]]; then
  IFS=', ' read -r -a CASES <<< "$GRPC_INTEROP_CASES"
  IFS=', ' read -r -a BASE_CASES <<< "$GRPC_INTEROP_CASES"
fi

TIMESTAMP_PID="$(date +%Y%m%d_%H%M%S)_$$"
LOG_DIR="${LOG_DIR:-${GRPC_INTEROP_LOG_DIR:-$ROOT/target/interop-logs/$TIMESTAMP_PID}}"
mkdir -p "$LOG_DIR"
RESULTS_JSON="$LOG_DIR/results.json"
REPORT_JSON="$LOG_DIR/report.json"

find_free_port() {
  python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

PORT="${GRPC_INTEROP_PORT_VAL:-${GRPC_INTEROP_PORT:-}}"
if [[ -z "$PORT" ]]; then
  PORT="$(find_free_port)"
fi

GO_PORT="${GRPC_INTEROP_GO_PORT_VAL:-${GRPC_INTEROP_GO_PORT:-}}"
if [[ -z "$GO_PORT" ]]; then
  GO_PORT="$(find_free_port)"
  while [[ "$GO_PORT" == "$PORT" ]]; do
    GO_PORT="$(find_free_port)"
  done
fi

TRACKED_PIDS=()
SCRATCH_PATHS=()
INTERRUPTED=0
CURRENT_CASE=""
CURRENT_PEER=""
CURRENT_DIRECTION=""
CURRENT_LOG=""

cleanup() {
  local exit_status=$?
  for pid in "${TRACKED_PIDS[@]:-}"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill -TERM "$pid" 2>/dev/null || true
      for _ in {1..10}; do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.05
      done
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
      fi
      wait "$pid" 2>/dev/null || true
    fi
  done
  for p in "${SCRATCH_PATHS[@]:-}"; do
    if [[ -e "$p" ]]; then
      rm -rf "$p"
    fi
  done
  if [[ "$INTERRUPTED" -eq 1 && -n "${RESULTS_JSON:-}" ]]; then
    # A signal during a concurrent record write may leave partial JSON;
    # reset it so aggregation below still emits a failed report.
    if [[ ! -f "$RESULTS_JSON" ]] || ! python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$RESULTS_JSON" 2>/dev/null; then
      printf '{"results": []}\n' > "$RESULTS_JSON" 2>/dev/null || true
    fi
  fi
  if [[ "$INTERRUPTED" -eq 1 && ! -f "${REPORT_JSON:-}" && -n "${CURRENT_CASE:-}" && -n "${RESULTS_JSON:-}" ]]; then
    # A signal arrived while a case attempt was in flight: record the
    # interruption against that case so the retained report fails closed
    # instead of staying silent or passing on partial results.
    local int_args=(
      python3 "$INTEROP_REPORT" record
      --output "$RESULTS_JSON"
      --case "$CURRENT_CASE"
      --status failed
      --duration-ms 0.0
      --peer "${CURRENT_PEER:-pbrs-grpc}"
      --direction "${CURRENT_DIRECTION:-kernel_client_to_kernel_server}"
      --transport "$TRANSPORT"
      --exit-code 130
      --attempt-count 1
      --notes "run interrupted by signal during case execution"
    )
    if [[ -n "${CURRENT_LOG:-}" ]]; then
      int_args+=(--stdout-log "$CURRENT_LOG" --stderr-log "$CURRENT_LOG")
    fi
    "${int_args[@]}" >/dev/null 2>&1 || true
  fi
  if [[ -f "${RESULTS_JSON:-}" && ! -f "${REPORT_JSON:-}" ]]; then
    python3 "$INTEROP_REPORT" aggregate --results "$RESULTS_JSON" --output "$REPORT_JSON" >/dev/null 2>&1 || true
  fi
}

on_signal() {
  INTERRUPTED=1
  cleanup
  exit 130
}
trap cleanup EXIT
trap on_signal INT TERM

start_server() {
  local name="$1"
  local port="$2"
  local log_file="$3"
  shift 3
  local server_cmd=("$@")

  # 1. Check if port is already occupied before starting server
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    exec 3<&- 3>&-
    echo "FAIL: port $port is already occupied before starting $name" >&2
    echo "Port $port occupied" >> "$log_file"
    return 1
  fi

  # 2. Start server in background with stdout/stderr redirected to log_file
  "${server_cmd[@]}" > "$log_file" 2>&1 &
  local pid=$!
  TRACKED_PIDS+=($pid)

  # 3. Wait for server to listen on port with startup deadline
  local deadline=$((SECONDS + STARTUP_TIMEOUT_SEC))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "FAIL: $name (pid $pid) died during startup" >&2
      if [[ -f "$log_file" ]]; then
        cat "$log_file" >&2
      fi
      return 1
    fi
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.05
  done
  echo "FAIL: $name (pid $pid) never listened on $port within ${STARTUP_TIMEOUT_SEC}s" >&2
  if [[ -f "$log_file" ]]; then
    cat "$log_file" >&2
  fi
  return 1
}

record_server_failure() {
  local case="$1" peer="$2" direction="$3" reason="$4" log_file="$5"
  local case_log="$LOG_DIR/${peer}-${direction}-${case}-attempt1.log"
  echo "$reason" > "$case_log"
  if [[ -f "$log_file" ]]; then
    cat "$log_file" >> "$case_log"
  fi
  local record_args=(
    python3 "$INTEROP_REPORT" record
    --output "$RESULTS_JSON"
    --case "$case"
    --status failed
    --duration-ms 0.0
    --peer "$peer"
    --direction "$direction"
    --transport "$TRANSPORT"
    --stdout-log "$case_log"
    --stderr-log "$case_log"
    --exit-code 1
    --attempt-count 1
    --notes "$reason"
  )
  if [[ "$peer" == "grpc-go" ]]; then
    record_args+=(--peer-pin "$GO_PEER_PIN")
  fi
  "${record_args[@]}" >/dev/null
}

run_attempt() {
  local deadline_sec="$1"
  local log_path="$2"
  shift 2
  python3 -c '
import subprocess, sys, time, os, signal

deadline_sec = float(sys.argv[1])
log_path = sys.argv[2]
cmd = sys.argv[3:]

start = time.perf_counter()
exit_code = 1

with open(log_path, "wb") as log_file:
    try:
        proc = subprocess.Popen(
            cmd,
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True
        )
        try:
            exit_code = proc.wait(timeout=deadline_sec)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            exit_code = 124
            log_file.write(f"\nCommand timed out after {deadline_sec}s\n".encode("utf-8"))
        except KeyboardInterrupt:
            # The runner was interrupted: stop the owned client process group
            # instead of orphaning it, then report signal termination.
            try:
                os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            log_file.write(b"\nCommand interrupted by signal\n")
            sys.exit(130)
    except Exception as e:
        log_file.write(f"\nExecution error: {e}\n".encode("utf-8"))
        exit_code = 1

end = time.perf_counter()
duration_ms = (end - start) * 1000.0
print(f"{duration_ms:.2f} {exit_code}")
' "$deadline_sec" "$log_path" "$@"
}

run_case() {
  local client="$1"
  local host="$2"
  local port="$3"
  local case="$4"
  local peer="$5"
  local direction="$6"
  local flag_prefix="$7"
  shift 7
  local extra_args=("$@")

  CURRENT_CASE="$case"
  CURRENT_PEER="$peer"
  CURRENT_DIRECTION="$direction"

  local max_attempts="${MAX_ATTEMPTS:-2}"
  local attempt=1
  local first_attempt_status=""
  local first_duration_ms=0
  local total_duration_ms=0
  local last_log=""
  local last_exit_code=0
  local last_status="failed"

  local client_cmd=("$client" "${flag_prefix}server_host" "$host" "${flag_prefix}server_port" "$port" "${flag_prefix}test_case=$case")
  if [[ ${#extra_args[@]} -gt 0 ]]; then
    client_cmd+=("${extra_args[@]}")
  fi

  while (( attempt <= max_attempts )); do
    local log_file="$LOG_DIR/${peer}-${direction}-${case}-attempt${attempt}.log"
    last_log="$log_file"
    CURRENT_LOG="$log_file"

    local result
    result=$(run_attempt "$CASE_TIMEOUT_SEC" "$log_file" "${client_cmd[@]}")
    local duration_ms exit_code
    read -r duration_ms exit_code <<< "$result"

    total_duration_ms=$(python3 -c "print(round($total_duration_ms + $duration_ms, 2))")
    last_exit_code=$exit_code

    if [[ $exit_code -eq 0 ]]; then
      last_status="passed"
      break
    else
      last_status="failed"
      if [[ $attempt -eq 1 ]]; then
        first_attempt_status="failed"
        first_duration_ms=$duration_ms
        local rec1_args=(
          python3 "$INTEROP_REPORT" record
          --output "$RESULTS_JSON"
          --case "$case"
          --status failed
          --duration-ms "$duration_ms"
          --peer "$peer"
          --direction "$direction"
          --transport "$TRANSPORT"
          --stdout-log "$log_file"
          --stderr-log "$log_file"
          --exit-code "$exit_code"
          --attempt-count 1
        )
        if [[ "$peer" == "grpc-go" ]]; then
          rec1_args+=(--peer-pin "$GO_PEER_PIN")
        fi
        "${rec1_args[@]}" >/dev/null
      fi
    fi

    attempt=$((attempt + 1))
  done

  local final_attempts=$attempt
  if (( final_attempts > max_attempts )); then
    final_attempts=$max_attempts
  fi

  local record_args=(
    python3 "$INTEROP_REPORT" record
    --output "$RESULTS_JSON"
    --case "$case"
    --status "$last_status"
    --duration-ms "$total_duration_ms"
    --peer "$peer"
    --direction "$direction"
    --transport "$TRANSPORT"
    --stdout-log "$last_log"
    --stderr-log "$last_log"
    --exit-code "$last_exit_code"
    --attempt-count "$final_attempts"
  )
  if [[ -n "$first_attempt_status" && $final_attempts -gt 1 ]]; then
    record_args+=(--first-attempt-status "$first_attempt_status")
  fi
  if [[ "$peer" == "grpc-go" ]]; then
    record_args+=(--peer-pin "$GO_PEER_PIN")
  fi

  "${record_args[@]}" >/dev/null

  CURRENT_CASE=""
  CURRENT_PEER=""
  CURRENT_DIRECTION=""
  CURRENT_LOG=""

  if [[ "$last_status" == "passed" ]]; then
    return 0
  else
    return 1
  fi
}

run_kernel_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5"
  shift 5
  local cases=("$@")
  local pass_failed=0
  local client_extra=()
  if [[ $USE_TLS -eq 1 ]]; then
    client_extra=(
      "--use_tls=true"
      "--tls_ca_file=$TLS_CA_FILE"
      "--server_host_override=localhost"
    )
  fi
  for case in "${cases[@]}"; do
    if run_case "$KERNEL_CLIENT" "$host" "$port" "$case" "$peer" "$direction" "--" "${client_extra[@]}"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: kernel client against $label" >&2
    return 1
  fi
  echo "PASS: kernel client against $label (${#cases[@]} cases)"
}

run_go_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5"
  shift 5
  local cases=("$@")
  local pass_failed=0
  local client_extra=()
  if [[ $USE_TLS -eq 1 ]]; then
    client_extra=(
      "-use_tls=true"
      "-use_test_ca=true"
      "-ca_file=$TLS_CA_FILE"
      "-server_host_override=localhost"
    )
  else
    client_extra=("-use_tls=false")
  fi
  for case in "${cases[@]}"; do
    if run_case "$GO_CLIENT" "$host" "$port" "$case" "$peer" "$direction" "-" "${client_extra[@]}"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: Go client against $label" >&2
    return 1
  fi
  echo "PASS: Go client against $label (${#cases[@]} cases)"
}

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "== building kernel interop binaries =="
  cargo build --release -p pbrs-grpc \
    --bin pbrs-grpc-interop-server --bin pbrs-grpc-interop-client
fi

KERNEL_SERVER="${GRPC_INTEROP_KERNEL_SERVER:-$ROOT/target/release/pbrs-grpc-interop-server}"
KERNEL_CLIENT="${GRPC_INTEROP_KERNEL_CLIENT:-$ROOT/target/release/pbrs-grpc-interop-client}"

KERNEL_SERVER_ARGS=(--port "$PORT")
if [[ $USE_TLS -eq 1 ]]; then
  KERNEL_SERVER_ARGS+=(
    --use_tls=true
    --tls_cert_file "$TLS_CERT_FILE"
    --tls_key_file "$TLS_KEY_FILE"
  )
fi

echo "== kernel client -> kernel server =="
SERVER_PID=""
if ! start_server "kernel server" "$PORT" "$LOG_DIR/server-kernel.log" "$KERNEL_SERVER" "${KERNEL_SERVER_ARGS[@]}"; then
  echo "FAIL: kernel server failed to start on port $PORT" >&2
  OVERALL_FAILED=1
  for case in "${CASES[@]}"; do
    record_server_failure "$case" "pbrs-grpc" "kernel_client_to_kernel_server" "kernel server failed to start on port $PORT" "$LOG_DIR/server-kernel.log"
  done
else
  SERVER_PID="${TRACKED_PIDS[-1]}"
  run_kernel_client 127.0.0.1 "$PORT" "pbrs-grpc" "kernel_client_to_kernel_server" "kernel server" "${CASES[@]}" || OVERALL_FAILED=1
fi

if [[ $SELF_ONLY -eq 1 ]]; then
  echo "SKIP: cross-language passes (--self-only requested)"
  echo "WARNING: self-only output is insufficient for official cross-language qualification"

  echo "== aggregating interop results =="
  AGGREGATE_EXIT=0
  python3 "$INTEROP_REPORT" aggregate \
    --results "$RESULTS_JSON" \
    --output "$REPORT_JSON" || AGGREGATE_EXIT=$?

  if [[ $OVERALL_FAILED -ne 0 || $AGGREGATE_EXIT -ne 0 ]]; then
    echo "FAIL: self-interop qualification failed" >&2
    exit 1
  fi
  echo "PASS: kernel client against kernel server passed successfully"
  exit 0
fi

if ! command -v go >/dev/null 2>&1; then
  echo "FAIL: Go toolchain missing (required for cross-language interop qualification)" >&2
  exit 1
fi

GO_DIR="$ROOT/tests/interop/go"
GO_BIN_DIR="${GRPC_INTEROP_GO_BIN_DIR:-$ROOT/target/interop-go}"
mkdir -p "$GO_BIN_DIR"
GO_SERVER="${GRPC_INTEROP_GO_SERVER:-$GO_BIN_DIR/go-interop-server}"
GO_CLIENT="${GRPC_INTEROP_GO_CLIENT:-$GO_BIN_DIR/go-interop-client}"

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "== building Go gRPC interop peer ($GO_PEER_VERSION) =="
  if ! (cd "$GO_DIR" && go build -mod=readonly -o "$GO_SERVER" google.golang.org/grpc/interop/server \
        && go build -mod=readonly -o "$GO_CLIENT" google.golang.org/grpc/interop/client); then
    echo "FAIL: could not build the pinned Go peer from $GO_DIR" >&2
    exit 1
  fi

  if ! go version -m "$GO_SERVER" 2>/dev/null | grep -q "google.golang.org/grpc.*${GO_PEER_PIN:0:12}"; then
    echo "FAIL: built Go peer does not match pinned version ($GO_PEER_VERSION)" >&2
    exit 1
  fi
fi

GO_SERVER_ARGS=(-port "$GO_PORT")
if [[ $USE_TLS -eq 1 ]]; then
  GO_SERVER_ARGS+=(
    -use_tls=true
    -tls_cert_file "$TLS_CERT_FILE"
    -tls_key_file "$TLS_KEY_FILE"
  )
else
  GO_SERVER_ARGS+=(-use_tls=false)
fi

echo "== kernel client -> Go server =="
GO_SERVER_PID=""
if ! start_server "Go server" "$GO_PORT" "$LOG_DIR/server-go.log" "$GO_SERVER" "${GO_SERVER_ARGS[@]}"; then
  echo "FAIL: Go server failed to start on port $GO_PORT" >&2
  OVERALL_FAILED=1
  for case in "${BASE_CASES[@]}"; do
    record_server_failure "$case" "grpc-go" "kernel_client_to_go_server" "Go server failed to start on port $GO_PORT" "$LOG_DIR/server-go.log"
  done
else
  GO_SERVER_PID="${TRACKED_PIDS[-1]}"
  run_kernel_client 127.0.0.1 "$GO_PORT" "grpc-go" "kernel_client_to_go_server" "Go server" "${BASE_CASES[@]}" || OVERALL_FAILED=1
  for case in "${COMPRESSION_CASES[@]}"; do
    echo "  skip $case (grpc-go @ $GO_PEER_PIN does not implement it)"
    python3 "$INTEROP_REPORT" record \
      --output "$RESULTS_JSON" \
      --case "$case" \
      --status unsupported \
      --duration-ms 0.0 \
      --peer "grpc-go" \
      --direction "kernel_client_to_go_server" \
      --transport "$TRANSPORT" \
      --peer-pin "$GO_PEER_PIN" \
      --notes "grpc-go does not implement compression flags" >/dev/null
  done
fi

echo "== Go client -> kernel server =="
if [[ -z "$SERVER_PID" ]] || ! kill -0 "$SERVER_PID" 2>/dev/null; then
  if ! start_server "kernel server" "$PORT" "$LOG_DIR/server-kernel.log" "$KERNEL_SERVER" "${KERNEL_SERVER_ARGS[@]}"; then
    echo "FAIL: kernel server failed to start on port $PORT" >&2
    OVERALL_FAILED=1
    for case in "${BASE_CASES[@]}"; do
      record_server_failure "$case" "grpc-go" "go_client_to_kernel_server" "kernel server failed to start on port $PORT" "$LOG_DIR/server-kernel.log"
    done
  else
    SERVER_PID="${TRACKED_PIDS[-1]}"
  fi
fi

if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
  run_go_client 127.0.0.1 "$PORT" "grpc-go" "go_client_to_kernel_server" "kernel server" "${BASE_CASES[@]}" || OVERALL_FAILED=1
  for case in "${COMPRESSION_CASES[@]}"; do
    echo "  skip $case (grpc-go @ $GO_PEER_PIN does not implement it)"
    python3 "$INTEROP_REPORT" record \
      --output "$RESULTS_JSON" \
      --case "$case" \
      --status unsupported \
      --duration-ms 0.0 \
      --peer "grpc-go" \
      --direction "go_client_to_kernel_server" \
      --transport "$TRANSPORT" \
      --peer-pin "$GO_PEER_PIN" \
      --notes "grpc-go does not implement compression flags" >/dev/null
  done
fi

echo "== aggregating interop results =="
AGGREGATE_EXIT=0
python3 "$INTEROP_REPORT" aggregate \
  --results "$RESULTS_JSON" \
  --output "$REPORT_JSON" || AGGREGATE_EXIT=$?

if [[ $OVERALL_FAILED -ne 0 || $AGGREGATE_EXIT -ne 0 ]]; then
  echo "FAIL: interop qualification failed" >&2
  exit 1
fi

echo "PASS: all interop passes completed successfully"
exit 0

