# H2-01 h2 cost attribution

Date: 2026-09-28/29. Base SHA: `349c00f1c13a9f75e736b9ef3c939a9bb8023797`.
Resolved transport crate: `h2` 0.4.19 (`Cargo.toml` allows `0.4.18`, `Cargo.lock`
resolves 0.4.19). All wall/QPS rows below are **dev-loop, contended host**
diagnostics on the shared Apple Silicon machine. Load averages while running the
short peer/scaling cells ranged from about 60 to 213; the scaling pass recorded
`load averages: 130.25 106.28 82.31` before and `102.83 102.05 81.61` after.

## Verdict

**NO-GO for starting a custom `pbrs-h2` engine now.** The measured h2 share in
small uncompressed cells is visible but well below the H2-01 bar: callgrind
self-instruction share is about **0.8-1.3% h2** for small unary/server-streaming
and DHAT attributes about **9 h2 allocations / 2.9 KiB per RPC**. Compressed
cells are dominated by gzip/miniz and pbrs-grpc compression/string work, not h2.
The contended 1-connection scaling probe did not prove that h2 blocks a required
multi-core design.

Proceed with upstream `h2` patches and current-backend pbrs-grpc work instead
of H2-03/H2-04. Revisit a custom engine only after a dedicated Linux benchmark
shows either **>=15% modeled gain on primary C/D cells** from removing h2 or a
hard correctness/design block in h2's stream scheduler.

## Method

Deterministic counts:

- Linux dev-loop in the repository container:
  `./scripts/devloop-linux.sh --cells rpc.pbrs.unary,rpc.pbrs.server_stream,rpc.pbrs.unary_compressed,rpc.pbrs.server_stream_compressed --iters 200 --repeats 1 --out target/h2-costs/devloop-linux-pbrs-rpc.json`
- Manual valgrind/callgrind and DHAT over the dev-loop binary with 80 measured
  iterations, written under `target/h2-costs/manual/`.
- macOS `sample` profiles for `rpc.pbrs.unary` and `rpc.pbrs.server_stream`:
  `target/h2-costs/profile-rpc-pbrs-{unary,server-stream}/`.
- `/usr/bin/time -l` context-switch proxy:
  `target/h2-costs/wakeups/`.

Diagnostic peers and scaling:

- `python3 scripts/rpc-bench-matrix.py --quick --server-peer native,tonic-pbrs,go --client-peer native --shape unary --output target/h2-costs/rpc-bench-peer-unary.json`
- `python3 scripts/rpc-bench-matrix.py --quick --server-peer native --client-peer native --shape stream --output target/h2-costs/rpc-bench-native-stream.json`
- `python3 scripts/rpc-bench-matrix.py --quick --server-peer native --client-peer native --shape upload --output target/h2-costs/rpc-bench-native-upload.json`
- Ad-hoc native server scaling using `rpc-bench server --cores {1,4}` and
  `rpc-bench load --distribution constant --rate 10000 --duration-secs 5
  --connections {1,4} --max-in-flight 512`, artifacts in
  `target/h2-costs/scaling/`.

## Deterministic dev-loop counts

`scripts/devloop-linux.sh` reports exact heap/copy counts and callgrind
instruction counts. `strace` is not installed in the container, so syscall and
lock-wait metrics are `not_run`.

| Cell | Callgrind IR/RPC | Heap allocs/RPC | Heap bytes/RPC | Copy/counter read |
|---|---:|---:|---:|---|
| `rpc.pbrs.unary` | 142,674 | 35.01 | 53,879 | 2 wire, 2 chunk slices, 2 encode calls |
| `rpc.pbrs.server_stream` | 176,455 | 50.03 | 64,626 | 5 wire, 5 chunk slices, 1 encode call |
| `rpc.pbrs.unary_compressed` | 7,516,628 | 65.02 | 1,482,376 | gzip/miniz dominates bytes and IR |
| `rpc.pbrs.server_stream_compressed` | 12,918,076 | 914.67 | 25,093,351 | streaming gzip dominates allocations |

## Instruction attribution

The table below is raw callgrind **self** instruction share from the manual
captures. It includes runtime scheduler/timing overhead from the loopback
closed-loop harness, so it should be read as diagnostic attribution rather than
claim-grade CPU share. TLS was not enabled in these cells.

| Cell | h2 | pbrs-grpc | tokio | TLS | compression | bytes/http | syscall / pthread / mio | Read |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| unary | 1.25% | 3.29% | 54.23% | 0.00% | 0.00% | 0.24% | 0.64% | h2 visible mainly in HPACK/header encoding. |
| server-stream | 0.83% | 2.38% | 59.05% | 0.00% | 0.00% | 0.14% | 0.61% | More stream lifecycle than h2 frame work. |
| unary compressed | 0.01% | 0.86% | 8.31% | 0.00% | 49.78% | 0.04% | 0.11% | gzip/miniz dominates. |
| server-stream compressed | 0.01% | 3.02% | 40.70% | 0.00% | 14.36% | 0.13% | 0.20% | compression + Tokio dominate. |

Representative raw callgrind symbols:

- Small unary: `<h2::frame::headers::HeaderBlock>::into_encoding` is the largest
  h2 symbol at 0.24% self IR in the manual capture.
- macOS `sample` for server-streaming captured `h2::client::ResponseFuture::poll`,
  `h2::proto::streams::OpaqueStreamRef::drop`, `h2::share::RecvStream::poll_data`,
  and h2 send-request lock/unlock samples, but the profile was wall-clock wait
  dominated.

## Allocation attribution

DHAT allocation stacks attribute only allocations whose stack contains `h2::` as
h2. This misses allocator overhead but gives a stable upper-bound-style signal
for direct h2 allocation sites.

| Cell | Total DHAT allocs/RPC | h2 allocs/RPC | h2 bytes/RPC | h2 bytes share | Dominant allocation sites |
|---|---:|---:|---:|---:|---|
| unary | 41.61 | 8.96 | 2,914 | 5.13% | client `GreeterClient::say_hello`, server `serve_io_in`, header maps, pbrs lazy strings |
| server-stream | 56.91 | 8.96 | 2,917 | 4.31% | client/server stream setup, channel, response frames |
| unary compressed | 72.16 | 9.11 | 3,334 | 0.22% | miniz/gzip state plus pbrs compression decode/encode |
| server-stream compressed | 935.34 | 13.56 | 32,034 | 0.13% | gzip/miniz buffers dominate |

This points to pbrs-grpc/header/compression work first. h2 allocations are real
but not large enough to model a 15% primary-cell win by replacement.

## Wall profiles and wakeup proxy

The macOS samples are **dev-loop, contended host** diagnostics, not CPU
attribution. Unary sampling finished too quickly for a useful call graph. The
server-stream sample was dominated by parked runtime waits:

| Cell | Wall sample highlights | Wake/context proxy |
|---|---|---|
| unary | raw sample contained no useful collapsed symbols at 3,000 iters | `/usr/bin/time -l`: 24,362 involuntary context switches for 3,000 RPCs = **8.12/RPC**; Mach messages sent+received 16,989 = **5.66/RPC** |
| server-stream | `__psynch_cvwait` 90.6%, `kevent` 4.5%, `writev` 2.6%, `recvfrom` 1.7%; h2/pbrs symbols appear only below the wait stack | 44,407 involuntary context switches for 3,000 RPCs = **14.80/RPC**; Mach messages sent+received 22,654 = **7.55/RPC** |

The context-switch/Mach-message counts are a wakeup proxy only. No exact Linux
per-RPC wakeup counter was available in this local pass.

## Peer and shape diagnostics

All rows below are short **dev-loop, contended host** diagnostics. The scripts
explicitly reported `NOT QUALIFIED` because quick smoke runs cannot establish a
server ceiling.

| Cell | Native p50 / p99 | Native CPU/RPC | Peer read |
|---|---:|---:|---|
| native server, native client, empty unary | 239 / 939 us | client 946 us; server 340 us | Baseline short run, 20 successes |
| tonic-pbrs server, native client, empty unary | 271 / 1,252 us | client 811 us; server 313 us | Slightly lower server CPU/RPC but worse p50 in this noisy sample |
| Go server, native client, empty unary | 486 / 2,927 us | client 962 us; server 661 us | Worse in this short run |
| native server, native client, large unary | 1,668 / 2,977 us | client 3,783 us; server 1,361 us | Native large p50 near Go, better server CPU/RPC than Go |
| Go server, native client, large unary | 1,614 / 1,940 us | client 3,849 us; server 2,644 us | Go lower latency in this short noisy large-payload row, higher server CPU/RPC |
| native server-stream 1 KiB download | 24.6k-49.4k msgs/s | client 472 us/RPC; server 63 us/RPC | Not a ceiling; 20-RPC smoke |
| native client-stream 1 KiB upload | 28.6k-40.9k msgs/s | client 592 us/RPC; server 40 us/RPC | Not a ceiling; 20-RPC smoke |

The grpc_bench Docker peer images present on the machine included
`rust_tonic`, `go_grpc`, `cpp_grpc`, `java_vertx`, `rust_thruster`, and local
`rust_pbrs` images under `grpc_bench:*`. I did not run the Docker suite because
the host load was above 100 and the repository-native matrix already produced
diagnostic peer rows.

## Multi-core / many-stream contention probe

This pass used one native server process with `--cores` and a constant 10k QPS
offered unary load. It is not a perfect 256-concurrent-stream benchmark because
`rpc-bench load` does not expose closed-loop concurrency directly; `--max-in-flight
512` and constant-rate scheduling created a many-stream pressure diagnostic.

| Server cores | Client connections | Successes | Queue overflows | Throughput | p50 / p99 e2e | Scheduling p99 | Read |
|---:|---:|---:|---:|---:|---:|---:|---|
| 1 | 1 | 49,991 | 0 | 9,996 QPS | 1.85 / 20.13 ms | 5.02 ms | Met offered rate with no queue overflow. |
| 1 | 4 | 49,527 | 473 | 9,902 QPS | 3.14 / 65.79 ms | 21.55 ms | More connections worsened tails under host contention. |
| 4 | 1 | 49,140 | 860 | 9,828 QPS | 2.90 / 66.70 ms | 15.42 ms | 4 server cores did not improve one-connection scaling in this run. |
| 4 | 4 | 49,894 | 105 | 9,977 QPS | 1.85 / 36.31 ms | 9.18 ms | Best 4-core row, but still noisy and not a clean h2 mutex proof. |

Interpretation: this does **not** prove that h2's stream store/frame queue is
the limiter. The test saturated the contended local scheduler as much as the
transport; one connection did not benefit from four server cores, while four
connections reduced queue overflows at four cores. A claim-grade contention
answer needs a dedicated Linux host, pinned CPUs, and lock-wait instrumentation
around h2's stream store.

## Upstream h2 patch list

Given the no-go verdict, the concrete follow-up should be upstream-first:

1. **HPACK/header encoding allocation cuts:** reduce `HeaderBlock::into_encoding`
   allocation/growth for small fixed gRPC request/response headers; reserve
   exact block capacity where possible and avoid per-header temporary churn.
2. **Stream store lock scope and diagnostics:** add h2-internal counters or
   tracing for stream-store lock hold/wait time, then narrow critical sections
   around send-request/response-future paths; this aligns with h2 issue-class
   work such as stream store mutex refactors.
3. **Frame queue batching/vectored write policy:** expose or improve batching
   so many small DATA/HEADERS frames on one connection flush with fewer wakeups
   without changing HTTP/2 semantics.
4. **gRPC SETTINGS/profile helper:** upstream a benchmark/example profile that
   disables push, uses realistic gRPC header sets, and exercises one
   connection/many streams and multi-connection scaling.
5. **Public instrumentation hooks:** expose optional, zero-cost-off metrics for
   stream count, queued frames, wakeups, and flow-control blocked durations so
   downstream crates do not need a custom engine only to see bottlenecks.

## Limits

- The host was extremely contended; all wall/QPS values are diagnostic only.
- Callgrind attribution is self-instruction based and includes Tokio scheduler
  and dev-loop timing overhead.
- DHAT h2 allocation attribution is stack-string based.
- No exact Linux wakeup counter or h2 lock-wait counter was available.
- TLS was not included in callgrind/DHAT cells. TLS currently measures as 0% in
  the tables because those cells were plaintext.
