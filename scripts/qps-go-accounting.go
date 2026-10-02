// Benchmark-only arrival/accounting overlay for grpc-go @ dd51b1c90aaf.
// Upstream transports, codecs, servers and closed-loop client paths are unchanged.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"math"
	"os"
	"sort"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/benchmark"
	"google.golang.org/grpc/benchmark/stats"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/internal/syscall"
	testpb "google.golang.org/grpc/interop/grpc_testing"
	"google.golang.org/grpc/status"
)

const qpsSeed uint64 = 0x5eed20260918

// Same integer PRNG and rounded, positive exponential intervals as rpc-bench.
func qpsInterval(state *uint64, rate float64) time.Duration {
	*state += 0x9e3779b97f4a7c15
	z := *state
	z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9
	z = (z ^ (z >> 27)) * 0x94d049bb133111eb
	z ^= z >> 31
	u := float64((z>>11)+1) / float64(uint64(1)<<53)
	n := math.Round(-math.Log(u) / rate * 1e9)
	if n < 1 {
		n = 1
	}
	if n >= float64(math.MaxInt64) {
		return time.Duration(math.MaxInt64)
	}
	return time.Duration(n)
}

type qpsAccounting struct {
	mu                                                                            sync.Mutex
	options                                                                       stats.HistogramOptions
	service, scheduled                                                            *stats.Histogram
	started                                                                       time.Time
	rusage                                                                        *syscall.Rusage
	epoch, offered, dispatched, completed, successful, failed, rejected, timedOut uint64
	incoming, active, carried                                                     uint64
	errors                                                                        map[int32]int64
}

func newQpsAccounting(options stats.HistogramOptions, config *testpb.ClientConfig) *qpsAccounting {
	return &qpsAccounting{options: options, service: stats.NewHistogram(options),
		scheduled: stats.NewHistogram(options), started: time.Now(), rusage: syscall.GetRusage(),
		errors: make(map[int32]int64)}
}

func (a *qpsAccounting) admit(accepted bool) uint64 {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.offered++
	if accepted {
		a.dispatched++
		a.active++
	} else {
		a.rejected++
		a.errors[int32(codes.ResourceExhausted)]++
	}
	return a.epoch
}

func (a *qpsAccounting) outcome(epoch uint64, service, scheduled time.Duration, code codes.Code) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.service.Add(int64(service))
	a.scheduled.Add(int64(scheduled))
	a.completed++
	a.active--
	if epoch != a.epoch {
		a.carried++
	}
	if code == codes.OK {
		a.successful++
	} else {
		a.failed++
		a.errors[int32(code)]++
		if code == codes.DeadlineExceeded {
			a.timedOut++
		}
	}
}

func qpsHistogram(h *stats.Histogram) *testpb.HistogramData {
	buckets := make([]uint32, len(h.Buckets))
	for i, bucket := range h.Buckets {
		buckets[i] = uint32(bucket.Count)
	}
	min := float64(h.Min)
	max := float64(h.Max)
	if h.Count == 0 {
		min = 0
		max = 0
	}
	return &testpb.HistogramData{Bucket: buckets, MinSeen: min, MaxSeen: max,
		Sum: float64(h.Sum), SumOfSquares: float64(h.SumOfSquares), Count: float64(h.Count)}
}

func qpsHistogramJSON(h *stats.Histogram) map[string]any {
	d := qpsHistogram(h)
	return map[string]any{"count": h.Count, "bucket": d.Bucket, "sum": d.Sum,
		"min_seen": d.MinSeen, "max_seen": d.MaxSeen}
}

// One lock determines both official stats and independent completion-mark counts.
// Carry-in survives reset; arrivals and RPCs continue without a fabricated drain.
func (a *qpsAccounting) snapshot(reset bool) *testpb.ClientStats {
	a.mu.Lock()
	defer a.mu.Unlock()
	now := time.Now()
	elapsed := now.Sub(a.started).Seconds()
	latest := syscall.GetRusage()
	user, system := syscall.CPUTimeDiff(a.rusage, latest)
	result := &testpb.ClientStats{Latencies: qpsHistogram(a.service), TimeElapsed: elapsed,
		TimeUser: user, TimeSystem: system}
	keys := make([]int, 0, len(a.errors))
	for key := range a.errors {
		keys = append(keys, int(key))
	}
	sort.Ints(keys)
	for _, key := range keys {
		result.RequestResults = append(result.RequestResults, &testpb.RequestResultCount{
			StatusCode: int32(key), Count: a.errors[int32(key)]})
	}
	window := map[string]any{
		"schema_version": 1, "window_kind": "completion_mark", "window_epoch": a.epoch,
		"reset": reset, "window_seconds": elapsed, "drain_seconds": 0,
		"offered": a.offered, "dispatched": a.dispatched, "completed": a.completed,
		"successful": a.successful, "failed": a.failed, "rejected": a.rejected,
		"timed_out": a.timedOut, "incoming_in_flight": a.incoming,
		"carried_in_completed": a.carried, "unfinished": a.active,
		"service_latency_nanos":   qpsHistogramJSON(a.service),
		"scheduled_latency_nanos": qpsHistogramJSON(a.scheduled),
		"arrival_schedule":        "aggregate SplitMix64 Poisson", "arrival_seed": fmt.Sprintf("%x", qpsSeed),
		"peer": "overlaid pinned grpc-go benchmark client",
	}
	encoded, err := json.Marshal(window)
	if err != nil {
		panic(err)
	}
	fmt.Fprintf(os.Stdout, "QPS_ACCOUNTING %s\n", encoded)
	if reset {
		a.service, a.scheduled = stats.NewHistogram(a.options), stats.NewHistogram(a.options)
		a.started, a.rusage = now, latest
		a.epoch++
		a.offered, a.dispatched, a.completed, a.successful = 0, 0, 0, 0
		a.failed, a.rejected, a.timedOut, a.carried = 0, 0, 0, 0
		a.incoming = a.active
		a.errors = make(map[int32]int64)
	}
	return result
}

type qpsSlot struct {
	client testpb.BenchmarkServiceClient
}

// Unary requests use upstream protobuf/transport with a five-second RPC deadline.
// Native STREAMING arrivals each open a stream, send one message and receive one
// reply. Preserve that unit of work rather than upstream persistent-stream loops.
func (slot *qpsSlot) call(ctx context.Context, rpcType testpb.RpcType, reqSize, respSize int) codes.Code {
	callCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	request := &testpb.SimpleRequest{ResponseType: testpb.PayloadType_COMPRESSABLE,
		ResponseSize: int32(respSize)}
	// Match native's absent empty payload, including its protobuf wire presence.
	if reqSize > 0 {
		request.Payload = benchmark.NewPayload(testpb.PayloadType_COMPRESSABLE, reqSize)
	}
	if rpcType == testpb.RpcType_UNARY {
		_, err := slot.client.UnaryCall(callCtx, request)
		return status.Code(err)
	}
	stream, err := slot.client.StreamingCall(callCtx)
	if err != nil {
		return status.Code(err)
	}
	if err := stream.Send(request); err != nil {
		return status.Code(err)
	}
	_, err = stream.Recv()
	stream.CloseSend()
	return status.Code(err)
}

func (a *qpsAccounting) start(ctx context.Context, conns []*grpc.ClientConn, perChannel int,
	config *testpb.ClientConfig, reqSize, respSize int) error {
	rate := config.GetLoadParams().GetPoisson().GetOfferedLoad()
	if math.IsNaN(rate) || math.IsInf(rate, 0) || rate <= 0 || len(conns) == 0 || perChannel <= 0 {
		return status.Error(codes.InvalidArgument, "invalid aggregate Poisson rate/slot count")
	}
	if len(conns) > 64 || perChannel > 256/len(conns) {
		return status.Error(codes.ResourceExhausted, "aggregate Poisson slot limit exceeds native worker bounds")
	}
	if config.RpcType != testpb.RpcType_UNARY && config.RpcType != testpb.RpcType_STREAMING {
		return status.Error(codes.InvalidArgument, "unsupported overlaid benchmark RPC type")
	}
	if config.GetPayloadConfig().GetBytebufParams() != nil {
		return status.Error(codes.InvalidArgument, "overlaid Poisson client requires protobuf simple payloads")
	}
	free := make(chan *qpsSlot, len(conns)*perChannel)
	for _, conn := range conns {
		for i := 0; i < perChannel; i++ {
			free <- &qpsSlot{client: testpb.NewBenchmarkServiceClient(conn)}
		}
	}
	go func() {
		state := qpsSeed
		scheduled := time.Now()
		for {
			scheduled = scheduled.Add(qpsInterval(&state, rate))
			timer := time.NewTimer(time.Until(scheduled))
			select {
			case <-ctx.Done():
				timer.Stop()
				return
			case <-timer.C:
			}
			if ctx.Err() != nil {
				return
			}
			select {
			case slot := <-free:
				epoch := a.admit(true)
				arrival := scheduled
				go func() {
					start := time.Now()
					code := slot.call(ctx, config.RpcType, reqSize, respSize)
					end := time.Now()
					a.outcome(epoch, end.Sub(start), end.Sub(arrival), code)
					free <- slot
				}()
			default:
				a.admit(false)
			}
		}
	}()
	return nil
}
