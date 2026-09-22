#!/usr/bin/env bash
# Run the official gRPC interop test cases in mixed peer directions:
#   1. Native client (pbrs-grpc-interop-client) -> Tonic server
#   2. Tonic client                             -> Native server (pbrs-grpc-interop-server)
#
# Two codec matrices, recorded under separate peers so they never mix:
#   peer=tonic               prost 0.14 messages (wire independence)
#   peer=tonic-pbrs-adapter  protobuf-tonic adapter over pbrs messages
#
# Covers official cases:
#   empty_unary
#   large_unary
#   client_streaming
#   server_streaming
#   ping_pong
#   empty_stream
#   cancel_after_begin
#   cancel_after_first_response (native-client direction only; see below)
#   timeout_on_sleeping_server
#   custom_metadata
#   status_code_and_message
#   special_status_message
#   unimplemented_method
#   unimplemented_service
#
# Named limitations, recorded as explicit unsupported rows (never fabricated
# passes): the four gzip cases in both directions (tonic's public API cannot
# express the official expect_compressed/response_compressed semantics), and
# tonic-client cancel_after_first_response (no mid-stream client cancel
# handle in tonic's public API with which to observe terminal CANCELLED).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

INTEROP_REPORT="$ROOT/scripts/interop-report.py"

DEFAULT_CASES=(
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

# Official gzip cases: tonic's public API exposes neither per-message
# compression control nor observation, so the expect/response_compressed
# semantics cannot run here. They get explicit unsupported rows, mirroring
# the grpc-go compression precedent in scripts/grpc-interop.sh.
GZIP_CASES=(
  client_compressed_unary
  server_compressed_unary
  client_compressed_streaming
  server_compressed_streaming
)

GZIP_NOTES="tonic public API cannot express official expect_compressed/response_compressed semantics; explicit unsupported, not fabricated coverage"
CANCEL_CLIENT_NOTES="tonic public client API has no mid-stream cancel handle to observe terminal CANCELLED; explicit unsupported, not fabricated coverage"

CASES=("${DEFAULT_CASES[@]}")
FILTER_SET=0

SKIP_BUILD="${SKIP_BUILD:-${GRPC_INTEROP_SKIP_BUILD:-0}}"
LOG_DIR=""
GRPC_INTEROP_PORT_VAL=""
CASE_TIMEOUT_SEC="${CASE_TIMEOUT_SEC:-${GRPC_INTEROP_CASE_TIMEOUT:-15}}"
STARTUP_TIMEOUT_SEC="${STARTUP_TIMEOUT_SEC:-${GRPC_INTEROP_STARTUP_TIMEOUT:-5}}"
MAX_ATTEMPTS="${MAX_ATTEMPTS:-${GRPC_INTEROP_MAX_ATTEMPTS:-2}}"
OVERALL_FAILED=0
TRANSPORT="http2_cleartext"

while [[ $# -gt 0 ]]; do
  case "$1" in
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

if [[ -n "${GRPC_INTEROP_CASES:-}" ]]; then
  IFS=', ' read -r -a CASES <<< "$GRPC_INTEROP_CASES"
  FILTER_SET=1
fi

# True when a case belongs in this run: everything by default, or only the
# named filter entries when --cases/GRPC_INTEROP_CASES is set.
wanted() {
  local needle="$1"
  if [[ $FILTER_SET -eq 0 ]]; then
    return 0
  fi
  local c
  for c in "${CASES[@]}"; do
    if [[ "$c" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

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

TRACKED_PIDS=()

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
  if [[ -f "${RESULTS_JSON:-}" && ! -f "${REPORT_JSON:-}" ]]; then
    python3 "$INTEROP_REPORT" aggregate --results "$RESULTS_JSON" --output "$REPORT_JSON" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

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

stop_server() {
  local pid="$1"
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
}

record_server_failure() {
  local case="$1" peer="$2" direction="$3" reason="$4" log_file="$5"
  local case_log="$LOG_DIR/${peer}-${direction}-${case}-attempt1.log"
  echo "$reason" > "$case_log"
  if [[ -f "$log_file" ]]; then
    cat "$log_file" >> "$case_log"
  fi
  python3 "$INTEROP_REPORT" record \
    --output "$RESULTS_JSON" \
    --case "$case" \
    --status failed \
    --duration-ms 0.0 \
    --peer "$peer" \
    --direction "$direction" \
    --transport "$TRANSPORT" \
    --stdout-log "$case_log" \
    --stderr-log "$case_log" \
    --exit-code 1 \
    --attempt-count 1 \
    --notes "$reason" >/dev/null
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
        python3 "$INTEROP_REPORT" record \
          --output "$RESULTS_JSON" \
          --case "$case" \
          --status failed \
          --duration-ms "$duration_ms" \
          --peer "$peer" \
          --direction "$direction" \
          --transport "$TRANSPORT" \
          --stdout-log "$log_file" \
          --stderr-log "$log_file" \
          --exit-code "$exit_code" \
          --attempt-count 1 >/dev/null
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

  "${record_args[@]}" >/dev/null

  if [[ "$last_status" == "passed" ]]; then
    return 0
  else
    return 1
  fi
}

run_native_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5"
  shift 5
  local cases=("$@")
  local pass_failed=0
  for case in "${cases[@]}"; do
    if run_case "$KERNEL_CLIENT" "$host" "$port" "$case" "$peer" "$direction" "--"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: native client against $label" >&2
    return 1
  fi
  echo "PASS: native client against $label (${#cases[@]} cases)"
}

run_tonic_client() {
  local host="$1" port="$2" peer="$3" direction="$4" label="$5" codec="$6"
  shift 6
  local cases=("$@")
  local pass_failed=0
  for case in "${cases[@]}"; do
    if run_case "$TONIC_BIN" "$host" "$port" "$case" "$peer" "$direction" "--" "--codec=$codec"; then
      echo "  ok   $case"
    else
      echo "  FAIL $case"
      pass_failed=1
      OVERALL_FAILED=1
    fi
  done
  if [[ $pass_failed -ne 0 ]]; then
    echo "FAIL: tonic client against $label" >&2
    return 1
  fi
  echo "PASS: tonic client against $label (${#cases[@]} cases)"
}

record_case_unsupported() {
  local case="$1" peer="$2" direction="$3" notes="$4"
  python3 "$INTEROP_REPORT" record \
    --output "$RESULTS_JSON" \
    --case "$case" \
    --status unsupported \
    --duration-ms 0.0 \
    --peer "$peer" \
    --direction "$direction" \
    --transport "$TRANSPORT" \
    --notes "$notes" >/dev/null
}

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "== building native interop binaries =="
  cargo build --release -p pbrs-grpc \
    --bin pbrs-grpc-interop-server --bin pbrs-grpc-interop-client

  echo "== building tonic interop binary =="
  cargo build --release --manifest-path "$ROOT/tests/interop/tonic/Cargo.toml" --target-dir "$ROOT/target"
fi

KERNEL_SERVER="${GRPC_INTEROP_KERNEL_SERVER:-$ROOT/target/release/pbrs-grpc-interop-server}"
KERNEL_CLIENT="${GRPC_INTEROP_KERNEL_CLIENT:-$ROOT/target/release/pbrs-grpc-interop-client}"
TONIC_BIN="${GRPC_INTEROP_TONIC_BIN:-$ROOT/target/release/tonic-interop}"
if [[ ! -x "$TONIC_BIN" && -x "$ROOT/tests/interop/tonic/target/release/tonic-interop" ]]; then
  TONIC_BIN="$ROOT/tests/interop/tonic/target/release/tonic-interop"
fi
if [[ ! -x "$TONIC_BIN" && -x "$ROOT/target/debug/tonic-interop" ]]; then
  TONIC_BIN="$ROOT/target/debug/tonic-interop"
fi
if [[ ! -x "$TONIC_BIN" && -x "$ROOT/tests/interop/tonic/target/debug/tonic-interop" ]]; then
  TONIC_BIN="$ROOT/tests/interop/tonic/target/debug/tonic-interop"
fi

run_codec_matrix() {
  local peer="$1" codec="$2"
  echo "== matrix: peer=$peer codec=$codec =="

  # Pass 1: Native client (pbrs-grpc-interop-client) -> Tonic server
  echo "== Native client -> Tonic server ($codec) =="
  TONIC_SERVER_PORT="$(find_free_port)"
  TONIC_SERVER_PID=""
  if ! start_server "Tonic server ($codec)" "$TONIC_SERVER_PORT" "$LOG_DIR/server-tonic-$codec.log" "$TONIC_BIN" server --port "$TONIC_SERVER_PORT" --codec "$codec"; then
    echo "FAIL: Tonic server ($codec) failed to start on port $TONIC_SERVER_PORT" >&2
    OVERALL_FAILED=1
    for case in "${CASES[@]}"; do
      record_server_failure "$case" "$peer" "native_client_to_tonic_server" "Tonic server ($codec) failed to start on port $TONIC_SERVER_PORT" "$LOG_DIR/server-tonic-$codec.log"
    done
  else
    TONIC_SERVER_PID="${TRACKED_PIDS[-1]}"
    # Gzip cases never run here: they only ever record explicit unsupported
    # rows below, even when named by --cases.
    NATIVE_CLIENT_CASES=()
    for case in "${CASES[@]}"; do
      skip=0
      for g in "${GZIP_CASES[@]}"; do
        if [[ "$case" == "$g" ]]; then skip=1; break; fi
      done
      if [[ $skip -eq 0 ]]; then NATIVE_CLIENT_CASES+=("$case"); fi
    done
    run_native_client 127.0.0.1 "$TONIC_SERVER_PORT" "$peer" "native_client_to_tonic_server" "Tonic server ($codec)" "${NATIVE_CLIENT_CASES[@]}" || OVERALL_FAILED=1
    stop_server "$TONIC_SERVER_PID"
  fi
  for case in "${GZIP_CASES[@]}"; do
    if wanted "$case"; then
      echo "  skip $case (tonic $codec: no per-message compression API)"
      record_case_unsupported "$case" "$peer" "native_client_to_tonic_server" "$GZIP_NOTES"
    fi
  done

  # Pass 2: Tonic client -> Native server (pbrs-grpc-interop-server).
  # cancel_after_first_response cannot run here: tonic's public client API
  # has no mid-stream cancel handle, so it records an explicit unsupported
  # row instead of a fabricated pass.
  echo "== Tonic client ($codec) -> Native server =="
  NATIVE_SERVER_PORT="$(find_free_port)"
  NATIVE_SERVER_PID=""
  if ! start_server "Native server" "$NATIVE_SERVER_PORT" "$LOG_DIR/server-native-$codec.log" "$KERNEL_SERVER" --port "$NATIVE_SERVER_PORT"; then
    echo "FAIL: Native server failed to start on port $NATIVE_SERVER_PORT" >&2
    OVERALL_FAILED=1
    for case in "${CASES[@]}"; do
      record_server_failure "$case" "$peer" "tonic_client_to_native_server" "Native server failed to start on port $NATIVE_SERVER_PORT" "$LOG_DIR/server-native-$codec.log"
    done
  else
    NATIVE_SERVER_PID="${TRACKED_PIDS[-1]}"
    TONIC_CLIENT_CASES=()
    for case in "${CASES[@]}"; do
      if [[ "$case" == "cancel_after_first_response" ]]; then
        continue
      fi
      skip=0
      for g in "${GZIP_CASES[@]}"; do
        if [[ "$case" == "$g" ]]; then skip=1; break; fi
      done
      if [[ $skip -eq 0 ]]; then TONIC_CLIENT_CASES+=("$case"); fi
    done
    run_tonic_client 127.0.0.1 "$NATIVE_SERVER_PORT" "$peer" "tonic_client_to_native_server" "Native server" "$codec" "${TONIC_CLIENT_CASES[@]}" || OVERALL_FAILED=1
    stop_server "$NATIVE_SERVER_PID"
  fi
  if wanted "cancel_after_first_response"; then
    echo "  skip cancel_after_first_response (tonic client: no cancel handle)"
    record_case_unsupported "cancel_after_first_response" "$peer" "tonic_client_to_native_server" "$CANCEL_CLIENT_NOTES"
  fi
  for case in "${GZIP_CASES[@]}"; do
    if wanted "$case"; then
      echo "  skip $case (tonic $codec: no per-message compression API)"
      record_case_unsupported "$case" "$peer" "tonic_client_to_native_server" "$GZIP_NOTES"
    fi
  done
}

run_codec_matrix "tonic" "prost"
run_codec_matrix "tonic-pbrs-adapter" "pbrs"

echo "== aggregating tonic interop results =="
AGGREGATE_EXIT=0
python3 "$INTEROP_REPORT" aggregate \
  --results "$RESULTS_JSON" \
  --output "$REPORT_JSON" || AGGREGATE_EXIT=$?

if [[ $OVERALL_FAILED -ne 0 || $AGGREGATE_EXIT -ne 0 ]]; then
  echo "FAIL: tonic mixed-peer interop qualification failed" >&2
  exit 1
fi

echo "PASS: all tonic mixed-peer interop passes completed successfully"
exit 0
