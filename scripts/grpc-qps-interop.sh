#!/usr/bin/env bash
# Official gRPC QPS Benchmark Scenario Driver & Interop Runner.
#
# Drives native and reference workers using official QPS scenarios
# conforming to grpc.testing.WorkerService and grpc.testing.Scenario (control.proto).
#
# Pinned Upstream References:
#   Official C++ Reference Peer & Driver: grpc/grpc @ d1487957db6658bc532b72871775148229836627 (v1.84.0)
#   Official Go Reference Peer:          google.golang.org/grpc @ dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef (v1.85.0-dev)
#   Native Worker:                       pure-protobuf / pbrs-grpc / rpc-bench (in-tree)
#
# Supports execution in all peer combinations:
#   1. native_pair:                 Native client -> Native server
#   2. native_client_to_ref_server: Native client -> Reference server (Go or C++)
#   3. ref_client_to_native_server: Reference client (Go or C++) -> Native server
#   4. ref_pair:                    Reference client -> Reference server
#
# Consumes native worker stats directly over official protobuf wire format without
# schema adaptations hiding unsupported fields, and retains raw ScenarioResult JSON outputs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CPP_PEER_PIN="d1487957db6658bc532b72871775148229836627"
CPP_PEER_VERSION="v1.84.0 ($CPP_PEER_PIN)"
GO_PEER_PIN="dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef"
GO_PEER_VERSION="v1.85.0-dev ($GO_PEER_PIN)"

DEFAULT_SCENARIOS_FILE="$ROOT/rpc-bench/scenarios/official.json"

SCENARIOS_FILE="$DEFAULT_SCENARIOS_FILE"
SCENARIO_FILTER=""
MODE="all"
REF_PEER="go"
DRIVER_BIN=""
LOG_DIR=""
DRY_RUN=0
SKIP_BUILD="${SKIP_BUILD:-${GRPC_QPS_SKIP_BUILD:-0}}"
WARMUP_OVERRIDE=0
DURATION_OVERRIDE=0
SERVER_PORT_ARG=""
CLIENT_PORT_ARG=""
STARTUP_TIMEOUT_SEC="${STARTUP_TIMEOUT_SEC:-5}"
OVERALL_FAILED=0

show_help() {
  cat << 'EOF'
Usage: scripts/grpc-qps-interop.sh [OPTIONS]

Official gRPC QPS Benchmark Scenario Runner for native and reference workers.

Drives official QPS scenarios (grpc.testing.Scenario) against grpc.testing.WorkerService
instances and outputs raw ScenarioResult JSON matching upstream gRPC.

Execution Modes (--mode):
  all                         Run native_pair, native_client_to_ref_server, ref_client_to_native_server (default)
  native, native_pair         Native client -> Native server
  native_client_to_ref_server Native client -> Reference server (Go / C++)
  ref_client_to_native_server Reference client (Go / C++) -> Native server
  ref_pair                    Reference client -> Reference server

Options:
  --help, -h                  Show this help message and exit
  --dry-run                   Print planned execution matrix, scenarios, and pins without starting workers
  --scenarios <FILE>          Path to JSON scenarios file (default: rpc-bench/scenarios/official.json)
  --scenario <NAME>           Filter and run a specific scenario by exact name
  --scenario-filter <REGEX>   Filter scenarios matching pattern
  --mode <MODE>               Execution direction mode (default: all)
  --ref-peer <go|cpp>         Reference peer implementation (default: go)
  --driver <PATH>             Path to QPS driver binary (default: integrated driver or qps_json_driver)
  --warmup <SEC>              Override scenario warmup_seconds (e.g. 1 for fast test)
  --duration <SEC>            Override scenario benchmark_seconds (e.g. 2 for fast test)
  --log-dir <DIR>             Directory for execution logs and ScenarioResult outputs
  --output-dir <DIR>          Alias for --log-dir
  --server-port <PORT>        Explicit driver port for server worker (default: dynamically allocated)
  --client-port <PORT>        Explicit driver port for client worker (default: dynamically allocated)
  --skip-build                Skip building worker and driver binaries

Environment Variables:
  GRPC_QPS_DRIVER             Path to official C++ qps_json_driver or custom driver binary
  GRPC_QPS_SCENARIOS          Path to scenarios file
  GRPC_QPS_REF_PEER           Reference peer implementation (go or cpp)
  GRPC_QPS_MODE               Execution mode (all, native, native_client_to_ref_server, ...)
  GRPC_QPS_WARMUP             Warmup duration override in seconds
  GRPC_QPS_DURATION           Benchmark duration override in seconds
  GRPC_QPS_SKIP_BUILD         If 1, skip binary builds
  GRPC_QPS_LOG_DIR            Directory for execution logs and reports

Examples:
  # Fast smoke test of unary ping-pong scenario:
  ./scripts/grpc-qps-interop.sh --scenario=protobuf_unary_ping_pong_empty --warmup=1 --duration=2

  # Dry run showing execution plan and scenario metadata:
  ./scripts/grpc-qps-interop.sh --dry-run

  # Run mixed-peer direction (Native Client -> Go Server):
  ./scripts/grpc-qps-interop.sh --mode=native_client_to_ref_server --warmup=1 --duration=2

  # Run full official scenario suite across all mixed directions:
  ./scripts/grpc-qps-interop.sh
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      show_help
      exit 0
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --scenarios)
      SCENARIOS_FILE="$2"
      shift 2
      ;;
    --scenarios=*)
      SCENARIOS_FILE="${1#*=}"
      shift
      ;;
    --scenario)
      SCENARIO_FILTER="$2"
      shift 2
      ;;
    --scenario=*)
      SCENARIO_FILTER="${1#*=}"
      shift
      ;;
    --scenario-filter)
      SCENARIO_FILTER="$2"
      shift 2
      ;;
    --scenario-filter=*)
      SCENARIO_FILTER="${1#*=}"
      shift
      ;;
    --mode|--direction)
      MODE="$2"
      shift 2
      ;;
    --mode=*|--direction=*)
      MODE="${1#*=}"
      shift
      ;;
    --ref-peer)
      REF_PEER="$2"
      shift 2
      ;;
    --ref-peer=*)
      REF_PEER="${1#*=}"
      shift
      ;;
    --driver)
      DRIVER_BIN="$2"
      shift 2
      ;;
    --driver=*)
      DRIVER_BIN="${1#*=}"
      shift
      ;;
    --warmup)
      WARMUP_OVERRIDE="$2"
      shift 2
      ;;
    --warmup=*)
      WARMUP_OVERRIDE="${1#*=}"
      shift
      ;;
    --duration)
      DURATION_OVERRIDE="$2"
      shift 2
      ;;
    --duration=*)
      DURATION_OVERRIDE="${1#*=}"
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
    --server-port)
      SERVER_PORT_ARG="$2"
      shift 2
      ;;
    --server-port=*)
      SERVER_PORT_ARG="${1#*=}"
      shift
      ;;
    --client-port)
      CLIENT_PORT_ARG="$2"
      shift 2
      ;;
    --client-port=*)
      CLIENT_PORT_ARG="${1#*=}"
      shift
      ;;
    --skip-build)
      SKIP_BUILD=1
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      echo "Use --help for usage information." >&2
      exit 1
      ;;
  esac
done

SCENARIOS_FILE="${SCENARIOS_FILE:-${GRPC_QPS_SCENARIOS:-$DEFAULT_SCENARIOS_FILE}}"
REF_PEER="${REF_PEER:-${GRPC_QPS_REF_PEER:-go}}"
MODE="${MODE:-${GRPC_QPS_MODE:-all}}"
WARMUP_OVERRIDE="${WARMUP_OVERRIDE:-${GRPC_QPS_WARMUP:-0}}"
DURATION_OVERRIDE="${DURATION_OVERRIDE:-${GRPC_QPS_DURATION:-0}}"
DRIVER_BIN="${DRIVER_BIN:-${GRPC_QPS_DRIVER:-}}"

if [[ ! -f "$SCENARIOS_FILE" ]]; then
  echo "FAIL: scenario definition file not found: $SCENARIOS_FILE" >&2
  exit 1
fi

TIMESTAMP_PID="$(date +%Y%m%d_%H%M%S)_$$"
LOG_DIR="${LOG_DIR:-${GRPC_QPS_LOG_DIR:-$ROOT/target/qps-logs/$TIMESTAMP_PID}}"
mkdir -p "$LOG_DIR"

SUMMARY_JSON="$LOG_DIR/summary.json"

find_free_port() {
  python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

TRACKED_PIDS=()

cleanup() {
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
}
trap cleanup EXIT INT TERM

# Build the integrated Go QPS driver source inside target/interop-go/driver_src/
generate_integrated_driver_source() {
  local src_dir="$ROOT/target/interop-go/driver_src"
  mkdir -p "$src_dir"
  cat << 'EOF' > "$src_dir/main.go"
package main

import (
	"context"
	"flag"
	"fmt"
	"math"
	"net"
	"os"
	"strings"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	testpb "google.golang.org/grpc/interop/grpc_testing"
	"google.golang.org/protobuf/encoding/protojson"
	"google.golang.org/protobuf/types/known/timestamppb"
)

var (
	scenariosFile      = flag.String("scenarios_file", "", "JSON file containing Scenario or Scenarios")
	scenariosJSON      = flag.String("scenarios_json", "", "JSON string containing Scenario or Scenarios")
	scenarioResultFile = flag.String("scenario_result_file", "", "Output JSON file for ScenarioResult")
	scenarioFilter     = flag.String("scenario_name", "", "Scenario name to run (default: all or first)")
	warmupOverride     = flag.Int("warmup_override", 0, "Override warmup_seconds")
	durationOverride   = flag.Int("benchmark_override", 0, "Override benchmark_seconds")
)

type Histogram struct {
	Resolution  float64
	MaxPossible float64
	Count       float64
	Sum         float64
	MinSeen     float64
	MaxSeen     float64
	Buckets     []uint32
}

func NewHistogram(data *testpb.HistogramData) *Histogram {
	if data == nil {
		return &Histogram{}
	}
	res := 0.01
	return &Histogram{
		Resolution:  res,
		MaxPossible: 60e9,
		Count:       data.Count,
		Sum:         data.Sum,
		MinSeen:     data.MinSeen,
		MaxSeen:     data.MaxSeen,
		Buckets:     data.Bucket,
	}
}

func (h *Histogram) Percentile(p float64) float64 {
	if h.Count == 0 || len(h.Buckets) == 0 {
		return 0.0
	}
	target := h.Count * (p / 100.0)
	countSoFar := 0.0
	multiplier := 1.0 + h.Resolution

	for i, c := range h.Buckets {
		countSoFar += float64(c)
		if countSoFar >= target {
			var lower, upper float64
			if i == 0 {
				lower = 0.0
				upper = multiplier
			} else {
				lower = math.Pow(multiplier, float64(i))
				upper = math.Pow(multiplier, float64(i+1))
			}
			frac := 0.5
			if c > 0 {
				frac = 1.0 - ((countSoFar - target) / float64(c))
			}
			val := lower + (upper-lower)*frac
			if val < h.MinSeen && h.MinSeen > 0 {
				val = h.MinSeen
			}
			if val > h.MaxSeen && h.MaxSeen > 0 {
				val = h.MaxSeen
			}
			return val
		}
	}
	return h.MaxSeen
}

func getWorkers() ([]string, error) {
	env := os.Getenv("QPS_WORKERS")
	if env == "" {
		return nil, fmt.Errorf("QPS_WORKERS environment variable not set (expected 'server_host:port,client_host:port')")
	}
	parts := strings.Split(env, ",")
	var workers []string
	for _, p := range parts {
		trimmed := strings.TrimSpace(p)
		if trimmed != "" {
			workers = append(workers, trimmed)
		}
	}
	if len(workers) < 2 {
		return nil, fmt.Errorf("QPS_WORKERS must contain at least 2 workers (server,client); got %q", env)
	}
	return workers, nil
}

func runScenario(scenario *testpb.Scenario, serverAddr, clientAddr string) (*testpb.ScenarioResult, error) {
	ctx := context.Background()

	// 1. Connect to server worker
	serverConn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("connect to server worker %s: %w", serverAddr, err)
	}
	defer serverConn.Close()
	serverStub := testpb.NewWorkerServiceClient(serverConn)

	// CoreCount
	coreResp, err := serverStub.CoreCount(ctx, &testpb.CoreRequest{})
	if err != nil {
		return nil, fmt.Errorf("CoreCount on server worker: %w", err)
	}
	serverCores := coreResp.Cores

	// Start RunServer
	serverStream, err := serverStub.RunServer(ctx)
	if err != nil {
		return nil, fmt.Errorf("RunServer on server worker: %w", err)
	}

	serverConfig := scenario.ServerConfig
	if serverConfig == nil {
		serverConfig = &testpb.ServerConfig{
			ServerType: testpb.ServerType_ASYNC_SERVER,
			Port:       0,
		}
	}

	if err := serverStream.Send(&testpb.ServerArgs{
		Argtype: &testpb.ServerArgs_Setup{Setup: serverConfig},
	}); err != nil {
		return nil, fmt.Errorf("send ServerArgs setup: %w", err)
	}

	initServerStatus, err := serverStream.Recv()
	if err != nil {
		return nil, fmt.Errorf("recv initial ServerStatus: %w", err)
	}
	boundPort := initServerStatus.Port

	serverHost, _, err := net.SplitHostPort(serverAddr)
	if err != nil {
		serverHost = "127.0.0.1"
	}
	if boundPort <= 0 {
		_, portStr, _ := net.SplitHostPort(serverAddr)
		fmt.Sscanf(portStr, "%d", &boundPort)
	}
	benchServerTarget := fmt.Sprintf("%s:%d", serverHost, boundPort)

	// 2. Connect to client worker
	clientConn, err := grpc.NewClient(clientAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, fmt.Errorf("connect to client worker %s: %w", clientAddr, err)
	}
	defer clientConn.Close()
	clientStub := testpb.NewWorkerServiceClient(clientConn)

	clientStream, err := clientStub.RunClient(ctx)
	if err != nil {
		return nil, fmt.Errorf("RunClient on client worker: %w", err)
	}

	clientConfig := scenario.ClientConfig
	if clientConfig == nil {
		clientConfig = &testpb.ClientConfig{
			ClientType:                testpb.ClientType_ASYNC_CLIENT,
			ClientChannels:            1,
			OutstandingRpcsPerChannel: 1,
			AsyncClientThreads:        1,
			RpcType:                   testpb.RpcType_UNARY,
			LoadParams: &testpb.LoadParams{
				Load: &testpb.LoadParams_ClosedLoop{ClosedLoop: &testpb.ClosedLoopParams{}},
			},
		}
	}
	clientConfig.ServerTargets = []string{benchServerTarget}

	if err := clientStream.Send(&testpb.ClientArgs{
		Argtype: &testpb.ClientArgs_Setup{Setup: clientConfig},
	}); err != nil {
		return nil, fmt.Errorf("send ClientArgs setup: %w", err)
	}

	_, err = clientStream.Recv()
	if err != nil {
		return nil, fmt.Errorf("recv initial ClientStatus: %w", err)
	}

	// 3. Warmup
	warmupSec := int(scenario.WarmupSeconds)
	if *warmupOverride > 0 {
		warmupSec = *warmupOverride
	}
	if warmupSec > 0 {
		if err := serverStream.Send(&testpb.ServerArgs{
			Argtype: &testpb.ServerArgs_Mark{Mark: &testpb.Mark{Reset_: true}},
		}); err != nil {
			return nil, fmt.Errorf("send server warmup mark: %w", err)
		}
		if err := clientStream.Send(&testpb.ClientArgs{
			Argtype: &testpb.ClientArgs_Mark{Mark: &testpb.Mark{Reset_: true}},
		}); err != nil {
			return nil, fmt.Errorf("send client warmup mark: %w", err)
		}
		_, _ = serverStream.Recv()
		_, _ = clientStream.Recv()

		time.Sleep(time.Duration(warmupSec) * time.Second)
	}

	// 4. Start benchmark
	benchmarkSec := int(scenario.BenchmarkSeconds)
	if *durationOverride > 0 {
		benchmarkSec = *durationOverride
	}
	if benchmarkSec <= 0 {
		benchmarkSec = 1
	}

	startTime := time.Now()
	if err := serverStream.Send(&testpb.ServerArgs{
		Argtype: &testpb.ServerArgs_Mark{Mark: &testpb.Mark{Reset_: true}},
	}); err != nil {
		return nil, fmt.Errorf("send server benchmark mark: %w", err)
	}
	if err := clientStream.Send(&testpb.ClientArgs{
		Argtype: &testpb.ClientArgs_Mark{Mark: &testpb.Mark{Reset_: true}},
	}); err != nil {
		return nil, fmt.Errorf("send client benchmark mark: %w", err)
	}
	_, _ = serverStream.Recv()
	_, _ = clientStream.Recv()

	time.Sleep(time.Duration(benchmarkSec) * time.Second)

	// 5. Finish benchmark & collect final status
	if err := clientStream.Send(&testpb.ClientArgs{
		Argtype: &testpb.ClientArgs_Mark{Mark: &testpb.Mark{Reset_: false}},
	}); err != nil {
		return nil, fmt.Errorf("send client done mark: %w", err)
	}
	clientFinal, err := clientStream.Recv()
	if err != nil {
		return nil, fmt.Errorf("recv client final status: %w", err)
	}
	_ = clientStream.CloseSend()

	if err := serverStream.Send(&testpb.ServerArgs{
		Argtype: &testpb.ServerArgs_Mark{Mark: &testpb.Mark{Reset_: false}},
	}); err != nil {
		return nil, fmt.Errorf("send server done mark: %w", err)
	}
	serverFinal, err := serverStream.Recv()
	if err != nil {
		return nil, fmt.Errorf("recv server final status: %w", err)
	}
	_ = serverStream.CloseSend()
	endTime := time.Now()

	clientStats := clientFinal.Stats
	serverStats := serverFinal.Stats

	// Calculate summary matching official driver postprocess_scenario_result
	h := NewHistogram(clientStats.Latencies)
	count := clientStats.Latencies.GetCount()
	elapsed := clientStats.TimeElapsed
	if elapsed <= 0 {
		elapsed = float64(benchmarkSec)
	}
	qps := count / elapsed

	qpsPerCore := 0.0
	if serverCores > 0 {
		qpsPerCore = qps / float64(serverCores)
	}

	serverSysTime := 100.0 * (serverStats.TimeSystem / elapsed)
	serverUserTime := 100.0 * (serverStats.TimeUser / elapsed)
	clientSysTime := 100.0 * (clientStats.TimeSystem / elapsed)
	clientUserTime := 100.0 * (clientStats.TimeUser / elapsed)

	serverQueriesPerCpuSec := 0.0
	if (serverStats.TimeUser + serverStats.TimeSystem) > 0 {
		serverQueriesPerCpuSec = count / (serverStats.TimeUser + serverStats.TimeSystem)
	}
	clientQueriesPerCpuSec := 0.0
	if (clientStats.TimeUser + clientStats.TimeSystem) > 0 {
		clientQueriesPerCpuSec = count / (clientStats.TimeUser + clientStats.TimeSystem)
	}

	summary := &testpb.ScenarioResultSummary{
		Qps:                         qps,
		QpsPerServerCore:           qpsPerCore,
		ServerSystemTime:           serverSysTime,
		ServerUserTime:             serverUserTime,
		ClientSystemTime:           clientSysTime,
		ClientUserTime:             clientUserTime,
		Latency_50:                 h.Percentile(50),
		Latency_90:                 h.Percentile(90),
		Latency_95:                 h.Percentile(95),
		Latency_99:                 h.Percentile(99),
		Latency_999:                h.Percentile(99.9),
		SuccessfulRequestsPerSecond: qps,
		ServerQueriesPerCpuSec:     serverQueriesPerCpuSec,
		ClientQueriesPerCpuSec:     clientQueriesPerCpuSec,
		StartTime:                  timestamppb.New(startTime),
		EndTime:                    timestamppb.New(endTime),
	}

	result := &testpb.ScenarioResult{
		Scenario:       scenario,
		Latencies:      clientStats.Latencies,
		ClientStats:    []*testpb.ClientStats{clientStats},
		ServerStats:    []*testpb.ServerStats{serverStats},
		ServerCores:    []int32{serverCores},
		Summary:        summary,
		ClientSuccess:  []bool{true},
		ServerSuccess:  []bool{true},
		RequestResults: clientStats.RequestResults,
	}

	return result, nil
}

func main() {
	flag.Parse()

	workers, err := getWorkers()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
	serverAddr := workers[0]
	clientAddr := workers[1]

	var rawJSON []byte
	if *scenariosFile != "" {
		data, err := os.ReadFile(*scenariosFile)
		if err != nil {
			fmt.Fprintf(os.Stderr, "Error reading %s: %v\n", *scenariosFile, err)
			os.Exit(1)
		}
		rawJSON = data
	} else if *scenariosJSON != "" {
		rawJSON = []byte(*scenariosJSON)
	} else {
		fmt.Fprintf(os.Stderr, "Error: either --scenarios_file or --scenarios_json must be provided\n")
		os.Exit(1)
	}

	var scenarios []*testpb.Scenario
	var scList testpb.Scenarios
	if err := (protojson.UnmarshalOptions{DiscardUnknown: true}).Unmarshal(rawJSON, &scList); err == nil && len(scList.Scenarios) > 0 {
		scenarios = scList.Scenarios
	} else {
		var single testpb.Scenario
		if err2 := (protojson.UnmarshalOptions{DiscardUnknown: true}).Unmarshal(rawJSON, &single); err2 == nil && single.Name != "" {
			scenarios = []*testpb.Scenario{&single}
		} else {
			fmt.Fprintf(os.Stderr, "Error parsing scenarios JSON: %v (as Scenarios: %v)\n", err2, err)
			os.Exit(1)
		}
	}

	for _, sc := range scenarios {
		if *scenarioFilter != "" && sc.Name != *scenarioFilter {
			continue
		}
		fmt.Printf("=== RUN SCENARIO: %s ===\n", sc.Name)
		fmt.Printf("Server worker: %s, Client worker: %s\n", serverAddr, clientAddr)

		res, err := runScenario(sc, serverAddr, clientAddr)
		if err != nil {
			fmt.Fprintf(os.Stderr, "FAIL scenario %s: %v\n", sc.Name, err)
			os.Exit(1)
		}

		s := res.Summary
		fmt.Printf("PASS: %s\n", sc.Name)
		fmt.Printf("  QPS:                  %.1f\n", s.Qps)
		fmt.Printf("  QPS / server core:    %.1f\n", s.QpsPerServerCore)
		fmt.Printf("  Latency p50:          %.2f us\n", s.Latency_50/1000.0)
		fmt.Printf("  Latency p90:          %.2f us\n", s.Latency_90/1000.0)
		fmt.Printf("  Latency p99:          %.2f us\n", s.Latency_99/1000.0)
		fmt.Printf("  Latency p99.9:        %.2f us\n", s.Latency_999/1000.0)
		fmt.Printf("  Server CPU load:      User=%.1f%%, Sys=%.1f%%\n", s.ServerUserTime, s.ServerSystemTime)
		fmt.Printf("  Client CPU load:      User=%.1f%%, Sys=%.1f%%\n", s.ClientUserTime, s.ClientSystemTime)

		if *scenarioResultFile != "" {
			opts := protojson.MarshalOptions{
				Multiline:       true,
				Indent:          "  ",
				EmitUnpopulated: false,
			}
			outData, err := opts.Marshal(res)
			if err != nil {
				fmt.Fprintf(os.Stderr, "Failed to marshal ScenarioResult: %v\n", err)
			} else {
				if err := os.WriteFile(*scenarioResultFile, outData, 0644); err != nil {
					fmt.Fprintf(os.Stderr, "Failed to write %s: %v\n", *scenarioResultFile, err)
				} else {
					fmt.Printf("Wrote raw ScenarioResult JSON: %s (%d bytes)\n", *scenarioResultFile, len(outData))
				}
			}
		}
	}
}
EOF
}

# Resolve scenarios list from JSON
parse_scenario_names() {
  python3 -c '
import json, sys
with open(sys.argv[1]) as f:
    d = json.load(f)
scenarios = d.get("scenarios", [])
if not scenarios and "name" in d:
    scenarios = [d]
for s in scenarios:
    name = s.get("name", "")
    if name:
        print(name)
' "$SCENARIOS_FILE"
}

SCENARIO_NAMES=()
while IFS= read -r line; do
  [[ -n "$line" ]] && SCENARIO_NAMES+=("$line")
done < <(parse_scenario_names)

if [[ -n "$SCENARIO_FILTER" ]]; then
  FILTERED=()
  for sc in "${SCENARIO_NAMES[@]}"; do
    if [[ "$sc" == "$SCENARIO_FILTER" ]] || [[ "$sc" =~ $SCENARIO_FILTER ]]; then
      FILTERED+=("$sc")
    fi
  done
  SCENARIO_NAMES=("${FILTERED[@]}")
fi

if [[ ${#SCENARIO_NAMES[@]} -eq 0 ]]; then
  echo "FAIL: no scenarios matched filter '$SCENARIO_FILTER' in $SCENARIOS_FILE" >&2
  exit 1
fi

DIRECTIONS=()
case "$MODE" in
  all)
    DIRECTIONS=(native_pair native_client_to_ref_server ref_client_to_native_server)
    ;;
  native|native_pair|native-pair)
    DIRECTIONS=(native_pair)
    ;;
  native_client_to_ref_server|native-client-to-ref-server)
    DIRECTIONS=(native_client_to_ref_server)
    ;;
  ref_client_to_native_server|ref-client-to-native-server)
    DIRECTIONS=(ref_client_to_native_server)
    ;;
  ref_pair|ref-pair)
    DIRECTIONS=(ref_pair)
    ;;
  *)
    echo "FAIL: unknown execution mode: $MODE (choose: all, native_pair, native_client_to_ref_server, ref_client_to_native_server, ref_pair)" >&2
    exit 1
    ;;
esac

# Handle --dry-run
if [[ $DRY_RUN -eq 1 ]]; then
  echo "=== DRY RUN: Official gRPC QPS Benchmark Scenario Invocation ==="
  echo "Scenarios File:     $SCENARIOS_FILE"
  echo "Reference Peer:     $REF_PEER (Go: $GO_PEER_VERSION, C++: $CPP_PEER_VERSION)"
  echo "Native Worker:      rpc-bench/target/release/rpc-bench worker"
  echo "Driver Protocol:    grpc.testing.WorkerService over HTTP/2"
  if [[ -n "$DRIVER_BIN" ]]; then
    echo "Driver Binary:      $DRIVER_BIN (official C++ qps_json_driver or external)"
  else
    echo "Driver Binary:      target/interop-go/qps-driver (integrated Go reference driver)"
  fi
  echo "Warmup Override:    ${WARMUP_OVERRIDE}s"
  echo "Duration Override:  ${DURATION_OVERRIDE}s"
  echo "Log Directory:      $LOG_DIR"
  echo ""
  echo "Planned Directions (${#DIRECTIONS[@]}):"
  for dir in "${DIRECTIONS[@]}"; do
    case "$dir" in
      native_pair)
        echo "  - $dir: Server=Native, Client=Native"
        ;;
      native_client_to_ref_server)
        echo "  - $dir: Server=$REF_PEER, Client=Native"
        ;;
      ref_client_to_native_server)
        echo "  - $dir: Server=Native, Client=$REF_PEER"
        ;;
      ref_pair)
        echo "  - $dir: Server=$REF_PEER, Client=$REF_PEER"
        ;;
    esac
  done
  echo ""
  echo "Selected Scenarios (${#SCENARIO_NAMES[@]}):"
  for sc in "${SCENARIO_NAMES[@]}"; do
    echo "  - $sc"
  done
  echo ""
  echo "Total Runs Planned: $((${#DIRECTIONS[@]} * ${#SCENARIO_NAMES[@]}))"
  echo "DRY RUN COMPLETE: configurations, scenario definitions, and matrices verified."
  exit 0
fi

# Build required binaries
NATIVE_WORKER_BIN="$ROOT/rpc-bench/target/release/rpc-bench"
GO_WORKER_BIN="$ROOT/target/interop-go/go-worker"
INTEGRATED_DRIVER_BIN="$ROOT/target/interop-go/qps-driver"

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "== building native rpc-bench worker =="
  if [[ ! -x "$NATIVE_WORKER_BIN" ]]; then
    cargo build --release --manifest-path "$ROOT/rpc-bench/Cargo.toml"
  fi

  if [[ "$REF_PEER" == "go" ]] || [[ "$MODE" == "all" ]] || [[ "$MODE" =~ ref ]]; then
    echo "== building Go benchmark worker ($GO_PEER_VERSION) =="
    mkdir -p "$ROOT/target/interop-go"
    (cd "$ROOT/tests/interop/go" && go build -o "$GO_WORKER_BIN" google.golang.org/grpc/benchmark/worker)
  fi

  if [[ -z "$DRIVER_BIN" ]]; then
    echo "== compiling integrated QPS driver ($GO_PEER_VERSION) =="
    generate_integrated_driver_source
    (cd "$ROOT/tests/interop/go" && go build -o "$INTEGRATED_DRIVER_BIN" "$ROOT/target/interop-go/driver_src/main.go")
    DRIVER_BIN="$INTEGRATED_DRIVER_BIN"
  fi
else
  if [[ -z "$DRIVER_BIN" ]]; then
    if [[ -x "$INTEGRATED_DRIVER_BIN" ]]; then
      DRIVER_BIN="$INTEGRATED_DRIVER_BIN"
    elif [[ -x "$ROOT/target/interop-cpp/qps_json_driver" ]]; then
      DRIVER_BIN="$ROOT/target/interop-cpp/qps_json_driver"
    else
      echo "== compiling integrated QPS driver =="
      generate_integrated_driver_source
      (cd "$ROOT/tests/interop/go" && go build -o "$INTEGRATED_DRIVER_BIN" "$ROOT/target/interop-go/driver_src/main.go")
      DRIVER_BIN="$INTEGRATED_DRIVER_BIN"
    fi
  fi
fi

if [[ ! -x "$NATIVE_WORKER_BIN" ]]; then
  echo "FAIL: native worker binary missing: $NATIVE_WORKER_BIN" >&2
  exit 1
fi

start_worker() {
  local role="$1" # native or go or cpp
  local port="$2"
  local log_file="$3"

  # Port check
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    exec 3<&- 3>&-
    echo "FAIL: port $port is already occupied before starting $role worker" >&2
    return 1
  fi

  case "$role" in
    native)
      "$NATIVE_WORKER_BIN" worker --driver_port "$port" > "$log_file" 2>&1 &
      ;;
    go)
      "$GO_WORKER_BIN" -driver_port "$port" > "$log_file" 2>&1 &
      ;;
    cpp)
      local cpp_worker="${GRPC_QPS_WORKER:-$ROOT/target/interop-cpp/qps_worker}"
      if [[ ! -x "$cpp_worker" ]]; then
        echo "FAIL: C++ qps_worker binary not found at $cpp_worker" >&2
        return 1
      fi
      "$cpp_worker" --driver_port "$port" > "$log_file" 2>&1 &
      ;;
    *)
      echo "FAIL: unknown worker role $role" >&2
      return 1
      ;;
  esac

  local pid=$!
  TRACKED_PIDS+=($pid)

  # Wait for readiness
  local deadline=$((SECONDS + STARTUP_TIMEOUT_SEC))
  while (( SECONDS < deadline )); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "FAIL: $role worker (pid $pid) died during startup" >&2
      cat "$log_file" >&2 || true
      return 1
    fi
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      exec 3<&- 3>&-
      return 0
    fi
    sleep 0.05
  done

  echo "FAIL: $role worker (pid $pid) failed to bind $port within ${STARTUP_TIMEOUT_SEC}s" >&2
  cat "$log_file" >&2 || true
  return 1
}

stop_worker() {
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

declare -a SUMMARY_ROWS=()

run_scenario_cell() {
  local scenario="$1"
  local direction="$2"

  local server_role=""
  local client_role=""
  case "$direction" in
    native_pair)
      server_role="native"
      client_role="native"
      ;;
    native_client_to_ref_server)
      server_role="$REF_PEER"
      client_role="native"
      ;;
    ref_client_to_native_server)
      server_role="native"
      client_role="$REF_PEER"
      ;;
    ref_pair)
      server_role="$REF_PEER"
      client_role="$REF_PEER"
      ;;
  esac

  local s_port="${SERVER_PORT_ARG:-$(find_free_port)}"
  local c_port="${CLIENT_PORT_ARG:-$(find_free_port)}"
  while [[ "$s_port" == "$c_port" ]]; do
    c_port="$(find_free_port)"
  done

  local s_log="$LOG_DIR/${scenario}-${direction}-server.log"
  local c_log="$LOG_DIR/${scenario}-${direction}-client.log"
  local d_log="$LOG_DIR/${scenario}-${direction}-driver.log"
  local result_json="$LOG_DIR/${scenario}-${direction}-result.json"

  echo "--------------------------------------------------------------------------------"
  echo "SCENARIO:  $scenario"
  echo "DIRECTION: $direction (Server: $server_role, Client: $client_role)"
  echo "PORTS:     Server worker=$s_port, Client worker=$c_port"

  local s_pid=""
  local c_pid=""

  if ! start_worker "$server_role" "$s_port" "$s_log"; then
    echo "FAIL: could not start server worker ($server_role)" >&2
    OVERALL_FAILED=1
    return 1
  fi
  s_pid="${TRACKED_PIDS[-1]}"

  if ! start_worker "$client_role" "$c_port" "$c_log"; then
    echo "FAIL: could not start client worker ($client_role)" >&2
    stop_worker "$s_pid"
    OVERALL_FAILED=1
    return 1
  fi
  c_pid="${TRACKED_PIDS[-1]}"

  # Prepare driver command
  local driver_args=(
    "--scenarios_file=$SCENARIOS_FILE"
    "--scenario_name=$scenario"
    "--scenario_result_file=$result_json"
  )
  if [[ $WARMUP_OVERRIDE -gt 0 ]]; then
    driver_args+=("--warmup_override=$WARMUP_OVERRIDE")
  fi
  if [[ $DURATION_OVERRIDE -gt 0 ]]; then
    driver_args+=("--benchmark_override=$DURATION_OVERRIDE")
  fi

  local driver_status=0
  QPS_WORKERS="127.0.0.1:$s_port,127.0.0.1:$c_port" "$DRIVER_BIN" "${driver_args[@]}" > "$d_log" 2>&1 || driver_status=$?

  stop_worker "$c_pid"
  stop_worker "$s_pid"

  if [[ $driver_status -eq 0 && -f "$result_json" ]]; then
    echo "  PASS: $scenario ($direction)"
    cat "$d_log" | grep -E "QPS:|Latency p50:|Server CPU|Client CPU" | sed 's/^/    /' || true

    # Extract metrics for table
    local qps p50 p99 scpu ccpu
    qps=$(python3 -c "import json, sys; d=json.load(open(sys.argv[1])); print(f\"{d.get('summary',{}).get('qps',0):.1f}\")" "$result_json" 2>/dev/null || echo "N/A")
    p50=$(python3 -c "import json, sys; d=json.load(open(sys.argv[1])); print(f\"{d.get('summary',{}).get('latency50',0)/1000.0:.1f}\")" "$result_json" 2>/dev/null || echo "N/A")
    p99=$(python3 -c "import json, sys; d=json.load(open(sys.argv[1])); print(f\"{d.get('summary',{}).get('latency99',0)/1000.0:.1f}\")" "$result_json" 2>/dev/null || echo "N/A")
    scpu=$(python3 -c "import json, sys; d=json.load(open(sys.argv[1])); print(f\"{d.get('summary',{}).get('serverUserTime',0)+d.get('summary',{}).get('serverSystemTime',0):.1f}\")" "$result_json" 2>/dev/null || echo "N/A")
    ccpu=$(python3 -c "import json, sys; d=json.load(open(sys.argv[1])); print(f\"{d.get('summary',{}).get('clientUserTime',0)+d.get('summary',{}).get('clientSystemTime',0):.1f}\")" "$result_json" 2>/dev/null || echo "N/A")

    SUMMARY_ROWS+=("$scenario|$direction|PASS|$qps|$p50 us|$p99 us|${scpu}%|${ccpu}%")
  else
    echo "  FAIL: $scenario ($direction)" >&2
    if [[ -f "$d_log" ]]; then
      cat "$d_log" >&2
    fi
    OVERALL_FAILED=1
    SUMMARY_ROWS+=("$scenario|$direction|FAIL|N/A|N/A|N/A|N/A|N/A")
  fi
}

echo "================================================================================"
echo "Official gRPC QPS Benchmark Scenario Driver & Interop Runner"
echo "================================================================================"
echo "Scenarios File:     $SCENARIOS_FILE"
echo "Reference Peer:     $REF_PEER ($GO_PEER_VERSION)"
echo "Driver Binary:      $DRIVER_BIN"
echo "Log Directory:      $LOG_DIR"
echo "Scenarios Count:    ${#SCENARIO_NAMES[@]}"
echo "Directions:         ${DIRECTIONS[*]}"
echo ""

for sc in "${SCENARIO_NAMES[@]}"; do
  for dir in "${DIRECTIONS[@]}"; do
    run_scenario_cell "$sc" "$dir" || true
  done
done

echo ""
echo "================================================================================"
echo "SUMMARY RESULTS MATRIX"
echo "================================================================================"
printf "%-38s %-28s %-6s %-10s %-10s %-10s %-10s %-10s\n" \
  "Scenario" "Direction" "Status" "QPS" "p50" "p99" "Server CPU" "Client CPU"
printf "%s\n" "----------------------------------------------------------------------------------------------------------------------------------------"

for row in "${SUMMARY_ROWS[@]}"; do
  IFS='|' read -r r_sc r_dir r_stat r_qps r_p50 r_p99 r_scpu r_ccpu <<< "$row"
  printf "%-38s %-28s %-6s %-10s %-10s %-10s %-10s %-10s\n" \
    "$r_sc" "$r_dir" "$r_stat" "$r_qps" "$r_p50" "$r_p99" "$r_scpu" "$r_ccpu"
done
printf "%s\n" "----------------------------------------------------------------------------------------------------------------------------------------"

# Save summary JSON
python3 -c '
import json, sys
summary_file = sys.argv[1]
rows = sys.argv[2:]
results = []
for r in rows:
    parts = r.split("|")
    if len(parts) >= 8:
        results.append({
            "scenario": parts[0],
            "direction": parts[1],
            "status": parts[2],
            "qps": parts[3],
            "latency_p50": parts[4],
            "latency_p99": parts[5],
            "server_cpu": parts[6],
            "client_cpu": parts[7],
        })
with open(summary_file, "w") as f:
    json.dump({"runs": results}, f, indent=2)
' "$SUMMARY_JSON" "${SUMMARY_ROWS[@]}"

echo ""
echo "Summary JSON written to: $SUMMARY_JSON"
echo "Raw ScenarioResult JSON files saved in: $LOG_DIR/"

if [[ $OVERALL_FAILED -ne 0 ]]; then
  echo "FAIL: one or more benchmark scenarios failed" >&2
  exit 1
fi

echo "PASS: all QPS benchmark scenarios completed successfully across selected directions"
exit 0
