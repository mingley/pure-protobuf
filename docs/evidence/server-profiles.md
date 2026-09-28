# Server profiles (SV-01)

## Scope

Server-side profiling used the in-repository dev-loop and `rpc-bench` harnesses
on the shared macOS host at `7aec79e22914cce4cd280f886ced4dae892d169f`.
This is **dev-loop, contended-host** evidence, not claim-grade performance
evidence.

The independent load generator was `rpc-bench load` in a separate process
against a native server process. Go/C++ peers were not used for this local
server-cost pass; the goal here was pbrs-grpc server attribution and allocation
reduction, not cross-stack ranking.

## Method

Baseline artifacts:

- `target/sv-server/devloop-sv01-base.json`: exact allocation/copy counts for
  `rpc.pbrs.{unary,server_stream,unary_compressed,server_stream_compressed}`.
- `target/sv-server/rpc-bench/*.json`: separate-process native server cells,
  plaintext/TLS, 1 and 4 server shards, unary and server-streaming.
- `target/profile/rpc.pbrs.unary-20260928T234240Z/`: macOS `sample` profile.

Setup notes:

- `third_party/protobuf` was fetched with `scripts/fetch-protobuf.sh`.
- `v4_tat/build.rs` reads `protoc` from `PATH`, so the pinned
  `target/pinned-protoc-build/protoc` was prepended to `PATH`.
- The dev-loop target cache was deleted once to remove stale v36-generated
  files after an initial toolchain mismatch.

## Baseline allocation and copy counts

Primary signal is exact allocation counts. Instructions/syscalls were not
available on macOS.

| cell | allocs/op | bytes/op | copy calls/op | copy bytes/op | notes |
|---|---:|---:|---:|---:|---|
| `rpc.pbrs.unary` | 57.335 | 55,059 | 2 wire + 2 encode | 20,480 wire + 20,540 encode per 10k profile iters | 1 request frame + 1 response frame |
| `rpc.pbrs.server_stream` | 74.335 | 65,963 | 5 wire + 1 encode | 2,051 wire + 1,030 encode per op | 4 replies per RPC |
| `rpc.pbrs.unary_compressed` | 87.345 | 1,483,557 | 2 wire + 2 serialize | 89,646 wire + 89,654 serialize per op | gzip dominates allocation bytes |
| `rpc.pbrs.server_stream_compressed` | 937.415 | 25,084,169 | 53 wire + 57 serialize | 89,435 wire + 89,675 serialize per op | compression internals dominate |

The `sample` profile for unary was mostly scheduler/IO wait on this host:
`__psynch_cvwait` 92.5%, `kevent` 2.7%, `writev` 2.4%, `recvfrom` 1.6%.
It is useful as a flamegraph artifact, but it does not reliably rank CPU work
inside the server on macOS.

## Separate-process load-generator sanity cells

Each row used ~1000 offered QPS for 2 seconds with 4 client connections.
There were no RPC failures or queue overflows.

| cell | successful | failed | QPS | sched p50 / p99 | latency p50 / p99 |
|---|---:|---:|---:|---:|---:|
| plaintext 1-core unary | 1993 | 0 | 996.7 | 1.09 ms / 6.84 ms | 2.10 ms / 9.88 ms |
| plaintext 4-core unary | 1991 | 0 | 995.1 | 1.03 ms / 6.80 ms | 1.48 ms / 12.21 ms |
| plaintext 1-core server-stream | 1993 | 0 | 996.7 | 1.16 ms / 8.26 ms | 2.34 ms / 17.98 ms |
| plaintext 4-core server-stream | 1990 | 0 | 994.7 | 0.99 ms / 3.01 ms | 1.62 ms / 5.26 ms |
| TLS 1-core unary | 1990 | 0 | 994.9 | 0.97 ms / 22.19 ms | 1.28 ms / 28.87 ms |
| TLS 4-core unary | 1992 | 0 | 995.5 | 0.95 ms / 2.33 ms | 1.29 ms / 3.13 ms |
| TLS 1-core server-stream | 1993 | 0 | 996.6 | 0.97 ms / 4.30 ms | 1.67 ms / 7.70 ms |
| TLS 4-core server-stream | 1993 | 0 | 996.6 | 0.93 ms / 2.29 ms | 1.57 ms / 3.54 ms |

The 1-core TLS unary p99 is noisy on this shared host; all cells completed the
scheduled offered load with zero failures, so they are retained as diagnostic
sanity checks only.

## Ranked limiting costs and hypotheses

| rank | cost source | evidence | hypothesis |
|---:|---|---|---|
| 1 | Compression allocations | Compressed server-stream cell is ~937 allocs/op and ~25 MB allocated/op. | Compression-cell wins need RX/SV streaming-specific compression work, not the immediate unary fast path. |
| 2 | Redundant per-RPC server state | `incoming_rpc` allocated a default `ByteBudgetTracker`, then `Single`/`Router` overwrote it before the handler. | SV-02 can remove one allocation per RPC by stamping the dispatcher's real tracker during `Rpc` construction. |
| 3 | Empty OK trailer detail block | Successful responses allocated `Status` detail storage just to attach an empty `Metadata` trailer map before encoding OK trailers. | SV-02/SV-05 can skip allocating status detail unless OK trailers are nonempty. |
| 4 | Per-RPC path/authority/scheme ownership | `run_unary_request` and `run_streaming_request` still allocate owned path/authority/scheme strings for interceptor-visible `Request` context. | SV-02 follow-up: requires changing request/context ownership outside this slice; not done here to preserve public behavior and scope. |
| 5 | Per-RPC spawn | `serve_io` still spawns one task per accepted RPC before routing. | SV-03 should not inline dispatch on the connection task without a fairness budget; see `docs/decisions/server-dispatch.md`. |
| 6 | Router boxing/string map lookup | `Router` still boxes heterogeneous services and hashes full service paths. | SV-04 static routing remains deferred by ownership rules; no router changes here. |
| 7 | h2 internals / scheduler waits | macOS sample mostly shows waits and HTTP/2 IO (`writev`, `recvfrom`, `kevent`). | H2-01/H2 lane owns deeper transport-engine attribution. |

## Card hypotheses

| card | hypothesis |
|---|---|
| SV-02 | Remove the throwaway `ByteBudgetTracker` and empty OK-trailer status detail allocation; expect exact allocation count to fall on unary and uncompressed streaming cells with unchanged interceptor-visible context/metadata. |
| SV-03 | Do not adopt inline connection-task dispatch yet: per-RPC spawn is real cost, but safe removal needs a cooperative budget and slow-handler fairness proof. Decision recorded separately. |
| SV-04 | Static generated routing likely reduces router boxing/string hashing, but it is deferred and outside this worker's write scope. |
| SV-05 | Immediate error paths already use trailers-only responses; the measurable local trailer win is skipping empty OK trailer detail allocation. More HPACK/static-header work belongs to the H2/static-routing lane. |
| SV-06 | Streaming compression and batch policy dominate compressed server-stream allocation counts; bounded streaming work should focus there, not on unary request prep. |
| SV-07 | Accept/memory work needs SB-12-style connection-scale cells; no accept bottleneck surfaced in the low-QPS diagnostic cells. |

## Limits

- This profile used a contended macOS loopback host; it is dev-loop evidence.
- No Linux `perf`/`strace` instructions or syscall data were available.
- No Go/C++ server peer drove this SV-01 pass.
- TLS cells used repository test certificates only.
