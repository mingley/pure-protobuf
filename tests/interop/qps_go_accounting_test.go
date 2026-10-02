package main

import (
	"context"
	"io"
	"math"
	"net"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/benchmark/stats"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	testpb "google.golang.org/grpc/interop/grpc_testing"
)

func TestAggregateScheduleMatchesNativeFixedVector(t *testing.T) {
	// Independent Python derivation is recorded in SB21 evidence; Rust has the
	// same fixed vector. Slots never multiply the aggregate arrival rate.
	want := []time.Duration{258292, 124501, 131192, 212648, 346233, 174957,
		324031, 333377, 117648, 247724, 46045, 213350}
	state := qpsSeed
	for i, expected := range want {
		if actual := qpsInterval(&state, 5000); actual != expected {
			t.Fatalf("interval %d: got %d, want %d", i, actual, expected)
		}
	}
}

func testAccounting() *qpsAccounting {
	return newQpsAccounting(stats.HistogramOptions{NumBuckets: 2500, GrowthFactor: .01,
		BaseBucketSize: 1.01}, &testpb.ClientConfig{})
}

func TestAccountingResetConservesCarryFailuresAndRejections(t *testing.T) {
	a := testAccounting()
	old := a.admit(true)
	a.admit(false)
	first := a.snapshot(true)
	if first.Latencies.Count != 0 || len(first.RequestResults) != 1 || first.RequestResults[0].Count != 1 {
		t.Fatalf("rejection fabricated a completion: %v", first)
	}
	a.outcome(old, 10, 25, codes.DeadlineExceeded)
	newEpoch := a.admit(true)
	a.outcome(newEpoch, 15, 35, codes.OK)
	a.admit(true) // unfinished
	a.admit(false)
	final := a.snapshot(false)
	if a.offered != 3 || a.dispatched != 2 || a.completed != 2 || a.incoming != 1 ||
		a.active != 1 || a.carried != 1 || a.failed != 1 || a.timedOut != 1 || a.rejected != 1 {
		t.Fatalf("window does not conserve calls: %+v", a)
	}
	if final.Latencies.Count != 2 || final.Latencies.Sum != 25 || a.scheduled.Sum != 60 ||
		len(final.RequestResults) != 2 {
		t.Fatalf("failures, latencies or official errors missing: %v", final)
	}
	if a.incoming+a.dispatched != a.completed+a.active {
		t.Fatal("completion conservation failed")
	}
}

type delayedQpsServer struct {
	testpb.UnimplementedBenchmarkServiceServer
}

type countedStreamServer struct {
	testpb.UnimplementedBenchmarkServiceServer
	streams, requests, replies, requestBytes, responseBytes atomic.Int64
}

func (s *countedStreamServer) StreamingCall(stream testpb.BenchmarkService_StreamingCallServer) error {
	s.streams.Add(1)
	for {
		request, err := stream.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		s.requests.Add(1)
		s.requestBytes.Add(int64(len(request.GetPayload().GetBody())))
		body := make([]byte, request.ResponseSize)
		if err := stream.Send(&testpb.SimpleResponse{Payload: &testpb.Payload{Body: body}}); err != nil {
			return err
		}
		s.replies.Add(1)
		s.responseBytes.Add(int64(len(body)))
	}
}

func TestStreamingArrivalUsesNewStreamOneMessageOneReplyAndExactPayload(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := grpc.NewServer()
	counted := &countedStreamServer{}
	testpb.RegisterBenchmarkServiceServer(server, counted)
	go server.Serve(listener)
	defer server.Stop()
	conn, err := grpc.NewClient(listener.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	slot := qpsSlot{client: testpb.NewBenchmarkServiceClient(conn)}
	for i := 0; i < 2; i++ {
		if code := slot.call(context.Background(), testpb.RpcType_STREAMING, 128, 256); code != codes.OK {
			t.Fatalf("stream arrival failed: %v", code)
		}
	}
	if counted.streams.Load() != 2 || counted.requests.Load() != 2 || counted.replies.Load() != 2 ||
		counted.requestBytes.Load() != 256 || counted.responseBytes.Load() != 512 {
		t.Fatalf("stream work unit/bytes do not match native: streams=%d requests=%d replies=%d bytes=%d/%d",
			counted.streams.Load(), counted.requests.Load(), counted.replies.Load(), counted.requestBytes.Load(), counted.responseBytes.Load())
	}
}

func (delayedQpsServer) UnaryCall(ctx context.Context, _ *testpb.SimpleRequest) (*testpb.SimpleResponse, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-time.After(20 * time.Millisecond):
		return &testpb.SimpleResponse{}, nil
	}
}

func TestAggregateSchedulerBoundsSlotsAndAccountsRejectedArrivals(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := grpc.NewServer()
	testpb.RegisterBenchmarkServiceServer(server, delayedQpsServer{})
	go server.Serve(listener)
	defer server.Stop()
	conn, err := grpc.NewClient(listener.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	a := testAccounting()
	ctx, cancel := context.WithCancel(context.Background())
	config := &testpb.ClientConfig{RpcType: testpb.RpcType_UNARY,
		LoadParams: &testpb.LoadParams{Load: &testpb.LoadParams_Poisson{Poisson: &testpb.PoissonParams{OfferedLoad: 1000}}}}
	if err := a.start(ctx, []*grpc.ClientConn{conn}, 2, config, 0, 0); err != nil {
		t.Fatal(err)
	}
	time.Sleep(100 * time.Millisecond)
	a.mu.Lock()
	if a.active > 2 || a.rejected == 0 || a.offered != a.dispatched+a.rejected {
		t.Fatalf("aggregate scheduler slot/rejection mismatch: %+v", a)
	}
	a.mu.Unlock()
	cancel()
	deadline := time.Now().Add(time.Second)
	for {
		a.mu.Lock()
		active := a.active
		a.mu.Unlock()
		if active == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("RPC tasks did not finish after cancellation")
		}
		time.Sleep(time.Millisecond)
	}
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.completed != a.dispatched || math.IsNaN(float64(a.service.Sum)) || a.scheduled.Sum < a.service.Sum {
		t.Fatal("final independent completion/latency accounting failed")
	}
}
