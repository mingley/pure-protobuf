# SB-17: pbrs-grpc entry in the public grpc_bench harness

## What

`bench/grpc-bench-entry/` adds two servers to a pinned local
checkout of `LesnyRumcajs/grpc_bench`:

- `rust_pbrs_mt_bench`: multi-thread, workers = `GRPC_SERVER_CPUS`.
- `rust_pbrs_st_bench`: single-thread (`current_thread` runtime).

Both mirror the tonic entries: plaintext unary `SayHello` echo of
the inner `Hello` message, jemalloc allocator, release
`opt-level=3 codegen-units=1 lto=true`, serves `0.0.0.0:50051`.
`sync.sh` pins the checkout to
`48b6b958832c995eee021361bdf3544d7c356fcc` and snapshots
pure-protobuf HEAD as `pbrs-src/`; `run.sh` reproduces the harness
`bench.sh` ghz methodology (plaintext unary, 50 connections, 1000
concurrent calls, 5s warmup + 20s measured, ghz 0.114.0) with a
1/2/4-CPU server axis, seeded shuffled entry order per repeat, and
per-cell reports. Local pre-check: 200/200 OK via ghz plus a typed
client echo assertion (`ECHO-OK`).

Drive-by product fix (required to build the Linux images): the
crate-root `#![deny(unsafe_code)]` rejected the RX-02 Linux
`sched_setaffinity` pin, so pbrs-grpc could not compile on Linux at
all (the module is cfg'd out on macOS, which hid it).
`pbrs-grpc/src/rt/per_core.rs` now carries the same scoped
`#[allow(unsafe_code)]` as the `TCP_USER_TIMEOUT` precedent.

## Accept mapping

- Published methodology reproduced: harness ghz flags verbatim
  (proto/call/concurrency/connections/duration/warmup/data-file),
  host details recorded (`host.txt` per results dir). Results below
  are labeled **diagnostic reproduction**; claims use SB-21
  variants. One recorded deviation: `GRPC_CLIENT_CPUS=8` instead of
  the harness default 1 (see "client ceiling" below); the
  methodology-faithful 1-CPU-client run is kept alongside.
- Reference points re-measured on the same host: all SB-17 peers
  ran on this host; no number below is compared across hosts. The
  2026-04-22 leaders (149,776 / 102,754 req/s) come from a stronger
  host; the gap to this Docker-Desktop-macOS setup is environment,
  not a finding.
- No upstream PR opened (operator action requiring approval).

## Host

Apple M4 Pro, 14 CPUs, 48 GiB RAM, macOS 25.6.0, Docker Desktop
29.8.0 (linux/arm64 VM), ghz 0.114.0 (linux/amd64 image, emulated),
pbrs-src `07e8e534`, scenario `complex_proto`, server RAM 512m.

## Run A: harness-faithful (1-CPU client), 33/33 cells

Results dir `results/260928T082729-pbrs`. Every entry at every CPU
count landed at 13-16k req/s with ~50ms average latency and no CPU
scaling — the signature of a saturated client, not differentiated
servers. Probe on `rust_pbrs_mt` (4 server CPUs): 1 client CPU ->
15.4k req/s, 4 client CPUs -> 38.7k, 8 client CPUs -> 41.4k. The
1-CPU emulated ghz client is the ceiling. Run A is kept as the
methodology record; Run B carries the comparison.

## Run B: 8-CPU client (recorded deviation), 33/33 cells

Results dir `results/260928T090642-pbrs`. Requests/sec (median of
1; p50/p99 and mean server CPU% alongside):

| entry | 1 CPU | 2 CPU | 4 CPU |
| --- | --- | --- | --- |
| pbrs mt | 37,680 (19/107ms, 63%) | 34,638 (21/125ms, 86%) | 30,390 (25/118ms, 82%) |
| pbrs st | 45,427 (16/68ms, 70%) | 43,313 (16/99ms, 67%) | 32,603 (17/134ms, 31%) |
| tonic mt | 36,192 (20/123ms, 47%) | 30,515 (23/119ms, 75%) | 24,183 (26/258ms, 59%) |
| tonic st | 46,553 (15/87ms, 56%) | 42,951 (16/94ms, 59%) | 32,354 (17/174ms, 40%) |
| thruster mt | 39,212 (15/110ms, 38%) | 35,354 (17/109ms, 48%) | 26,283 (18/175ms, 43%) |
| thruster st | 44,500 (14/99ms, 51%) | 3,687* (14/136ms, 3%) | 35,439 (18/111ms, 44%) |
| go | 44,866 (17/82ms, 74%) | 44,144 (15/90ms, 113%) | 30,056 (14/167ms, 108%) |
| java g1gc | 47,936 (14/74ms, 76%) | 8,935* (16/109ms, 35%) | 33,503 (20/140ms, 202%) |
| vertx | 46,572 (12/99ms, 66%) | 40,435 (14/85ms, 111%) | 31,961 (15/159ms, 111%) |
| cpp mt | 41,579 (18/87ms, 80%) | 38,492 (18/105ms, 80%) | 26,673 (19/202ms, 122%) |
| cpp st | 39,629 (18/120ms, 66%) | 36,436 (19/119ms, 84%) | 8,212* (19/466ms, 44%) |
| dotnet | no image (see below) | — | — |

`*` degraded cells with timeouts/errors (477
DeadlineExceeded for java-g1gc/2CPU; 123 DeadlineExceeded + 3
Unavailable for thruster-st/2CPU; 67 Unavailable/Canceled for
cpp-st/4CPU). Reported as measured, not retried or dropped.
pbrs cells: 608k-912k OK each with 0-269 Unavailable/Canceled
(<0.05%, teardown-race signature shared with peers).

Read with care: throughput *falls* as server CPUs rise for most
entries while server CPU stays far below cap — server + 8-CPU
client oversubscribe the shared Docker Desktop VM, so the ceiling
moves off-server. Single repeat, no confidence intervals: ordering
is indicative, not a claim. Within those limits, pbrs-st tracks
tonic-st within noise at all three CPU counts, and pbrs-mt leads
tonic-mt by 4-26%.

## dotnet

`dotnet_grpc_bench` does not build on this host: the peer's pinned
`Grpc.Tools` 2.76.0 runs its bundled `linux_arm64/protoc`, which
segfaults (exit 139) in the container. Peer toolchain issue, not
touched (peer entries are not modified). Recorded in
`failures.log`; the other 11 peers + 2 pbrs entries ran all cells.

## Reproduce

```sh
./bench/grpc-bench-entry/sync.sh
GRPC_CLIENT_CPUS=8 ./bench/grpc-bench-entry/run.sh   # informative
./bench/grpc-bench-entry/run.sh                      # harness-faithful
```

## Gates

`plan-lint.py` OK at completion; entry Rust formatted with
`rustfmt --edition 2021`; repo `clippy`/`fmt` unaffected (entries
are standalone crates outside the workspace). No new committed
tests: the harness's own ghz reports + typed echo pre-check are
the verification.
