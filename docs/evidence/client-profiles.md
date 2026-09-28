# CL-01 client profiles

Date: 2026-09-28. Base SHA: `7aec79e22914cce4cd280f886ced4dae892d169f`.
Host: Apple M4 Pro, macOS, shared/contended. All wall/QPS numbers below are
`dev-loop, contended host` diagnostics; allocation counts are exact dev-loop
counts where reported. macOS has no `perf`/`strace` in this setup, so
instructions, syscalls and lock-wait counts are `not_run`.

## Method

- Prepared local fixtures with repo-approved `./scripts/fetch-protobuf.sh` and
  `./scripts/build-pinned-protoc.sh`; `protoc --version` was `libprotoc 35.1`.
- Dev-loop baseline:
  `PATH="$PWD/target/pinned-protoc-build:$PATH" CARGO_BUILD_JOBS=3 scripts/devloop.sh --cells rpc.pbrs.unary,rpc.tonic.unary,rpc.pbrs.server_stream,rpc.tonic.server_stream,rpc.pbrs.unary_compressed,rpc.pbrs.server_stream_compressed --repeats 3 --iters 2000 --out target/client-perf/cl-baseline-devloop.json`
- Sampled profiles:
  `./scripts/profile.sh --cell rpc.pbrs.unary --out target/client-perf/profile-rpc-pbrs-unary --tool sample --skip-build`
  and
  `./scripts/profile.sh --cell rpc.pbrs.server_stream --out target/client-perf/profile-rpc-pbrs-server-stream --tool sample --skip-build`.
- Independent server peer:
  `CARGO_BUILD_JOBS=3 python3 scripts/rpc-bench-matrix.py --server go --client native,tonic-pbrs --shape unary --output target/client-perf/rpc-bench-go-unary-standard.json`
  plus quick diagnostics for `stream` and `upload`. The full Go `--shape all`
  run and Go `ping_pong` timed out on this host; bidi ping-pong used the
  protocol fallback
  `--server native,tonic-pbrs --client native,tonic-pbrs --shape ping_pong`.

## Dev-loop baseline

| Cell | Heap allocs/RPC | Heap bytes/RPC | Wall ns/RPC | Copies/RPC | Read |
|---|---:|---:|---:|---:|---|
| `rpc.pbrs.unary` | 57.33 | 55,059 | 103,541 | 2 wire / 2 chunk / 2 encode | Native spends fewer allocs than tonic, but more bytes. |
| `rpc.tonic.unary` | 71.32 | 49,165 | 145,045 | n/a | Tonic is allocation-heavier but byte-lighter. |
| `rpc.pbrs.server_stream` | 74.34 | 65,963 | 117,907 | 5 wire / 5 chunk / 1 encode | Per-response stream path adds ~17 allocs over unary. |
| `rpc.tonic.server_stream` | 83.33 | 58,305 | 183,734 | n/a | Same pattern: tonic allocs more, fewer bytes. |
| `rpc.pbrs.unary_compressed` | 87.34 | 1,483,556 | 776,346 | 2 wire / 2 chunk / 2 serialize | Compression dominates bytes. |
| `rpc.pbrs.server_stream_compressed` | 937.39 | 25,084,004 | 984,352 | 53 wire / 57 chunk / 57 serialize | Streaming compression is allocation-dominant. |

Artifacts:

- `target/client-perf/cl-baseline-devloop.json`
- `target/client-perf/profile-rpc-pbrs-unary/{cell.json,top.txt,meta.json,sample.txt}`
- `target/client-perf/profile-rpc-pbrs-server-stream/{cell.json,top.txt,meta.json,sample.txt}`

## Sampled attribution

Sampling is wall-clock on loopback and dominated by parked Tokio/h2/server work,
so it is useful for ranking categories but not for a claim-grade CPU result.

| Rank | Unary sampled evidence | Server-stream sampled evidence | Hypothesis |
|---:|---|---|---|
| 1 | `__psynch_cvwait` 82.9%, `kevent` 7.5%, `writev` 4.1%, `recvfrom` 2.4% | `__psynch_cvwait` 81.6%, `kevent` 8.3%, `writev` 4.1%, `recvfrom` 2.5% | Closed-loop dev-loop waits on local h2/socket scheduling; use exact allocations as primary signal. |
| 2 | `HeaderBlock::into_encoding`, `HeaderBlock::load`, `HeaderMap::try_append2`, `hash_elem_using` | Same header symbols plus `Channel::server_streaming` | Request header construction and HPACK/header-map work are visible enough to justify CL-03. |
| 3 | `Channel::unary`, `run_unary_inner`, `malloc/free`, `pthread_mutex_*` | `run_server_stream_inner`, `schedule_task`, `wake_by_val`, `malloc/free` | CL-02 can remove response-path allocation and eventually the boxed call future/watch cancellation; CL-05 should target spawned streaming pumps/tasks. |
| 4 | `chunk_slices`, `encode_calls` from copy counters | More wire/chunk events per RPC | Streaming shape improvements need send/receive lifecycle changes, not protobuf serialization changes. |

## Independent-peer diagnostics

The Go peer built offline from `tests/interop/go`. Client saturation checks
passed, but the matrix reports are not leadership evidence because the runs are
shorter than the 60 s server-ceiling requirement. Treat these as same-host
diagnostics only.

| Fixed server | Shape | Native client | Tonic-pbrs client | Read |
|---|---|---:|---:|---|
| Go, standard | empty unary | 150.2 us p50, 2,435.7 us p99, 200.7 us client CPU/RPC | 184.6 us p50, 1,334.3 us p99, 237.5 us client CPU/RPC | Native uses less client CPU and better p50; tonic tail is lower in this run. |
| Go, standard | large unary | 975.5 us p50, 6,287.4 us p99, 2,007.3 us client CPU/RPC | 943.1 us p50, 5,124.3 us p99, 2,374.9 us client CPU/RPC | Native uses less client CPU; tonic p50/p99 are lower on this short run. |
| Go, quick | server-stream download | 60.6k msgs/s best line, 449.0 us client CPU/RPC | 44.0k msgs/s best line, 465.9 us client CPU/RPC | Native leads download throughput and CPU. |
| Go, quick | client-stream upload | 34.4k msgs/s best line, 424.5 us client CPU/RPC | 50.6k msgs/s best line, 392.2 us client CPU/RPC | Tonic leads upload CPU/throughput; supports CL-05. |
| Native/tonic fallback, quick | bidi ping-pong | Native client 1,105-1,132 us CPU/RPC | Tonic-pbrs client 1,004-1,060 us CPU/RPC | Tonic leads bidi client CPU; supports CL-05. |

Artifacts:

- `target/client-perf/rpc-bench-go-unary-standard.json`
- `target/client-perf/rpc-bench-go-unary.json`
- `target/client-perf/rpc-bench-go-stream.json`
- `target/client-perf/rpc-bench-go-upload.json`
- `target/client-perf/rpc-bench-go-ping_pong.json` (incomplete timeout)
- `target/client-perf/rpc-bench-fallback-ping_pong.json`

## Card hypotheses

| Card | Baseline hypothesis | Primary evidence to use |
|---|---|---|
| CL-02 | Default unary still pays for a boxed `Call` future, `watch` cancellation channel, `Channel` clone, and a successful-response path `String`. The path copy is the safest first target; boxed/watch removal needs a larger `Call` future refactor to preserve `CallHandle` semantics. | `rpc.pbrs.unary` allocations/RPC and response-hook tests. |
| CL-03 | Header construction allocates per RPC (`Uri::from_parts`, authority clone, timeout `String`, fixed `HeaderMap` inserts). Precomputing method/channel header templates and stack-encoding timeout should reduce unary allocation bytes and instructions. | `rpc.pbrs.unary` allocations/bytes and wire/header equivalence tests. |
| CL-04 | Pool/connection picking did not show a strong symbol in concurrency-1 profiles. It should be gated on multi-connection C2 cells or a Linux lock-wait run before large changes. | New/expanded multi-connection dev-loop or rpc-bench matrix evidence. |
| CL-05 | Client-streaming upload and bidi ping-pong favor tonic in client CPU; current code uses streaming pumps/oneshots/tasks. Removing pumps should target task spawns and allocations in C3 shapes. | Streaming lifecycle tests plus C3/rpc-bench upload+bidi diagnostics. |
| CL-06 | Cold-start/memory is not measured by these cells. Existing data only shows native RSS slightly above tonic in unary and streaming diagnostics. | Separate cold-start/memory card with first-RPC and RSS measurement. |

## Limits

- macOS lacks instruction/syscall/lock primary metrics here; allocation counts
  are exact, wall/QPS are secondary.
- Go unary standard runs still failed the script's 60 s server-ceiling rule;
  quick streaming runs also do not establish server ceilings.
- The full Go `--shape all` and Go `ping_pong` cells timed out, so bidi uses
  native/tonic fallback servers exactly as allowed by the assignment.
