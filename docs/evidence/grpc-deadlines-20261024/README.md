# Server deadlines and gzip encoding

Measured runtime: `7c02eb387a6785d6e914146babe34f0902a6dd62`.
Comparison baseline: `beb4ecd9a9b2143c440892607d5b486318062b61`.
Full-workspace repository: `8144576f74bcf290951a65235e3bb50f24710b71`.

The runtime fixes accepted deadline ownership, committed unary status accounting
and response termination, uses bounded gzip output reads, and keeps Router's
selected service future small. All 22 measured native Prost instruction scopes
beat fresh same-side tonic. Allocation-byte losses remain; the current resource
days are running with results pending. Production readiness and broad superiority
remain unproved.

## Validation

| Check | Actual source | Result |
|---|---|---|
| Full all-feature workspace | Repository `8144576f` | **3,812 passed, zero failed, nine existing ignores**, 144 target summaries; 1,480.912 seconds |
| ASAN and leak detection | Runtime `7c02eb38` | **787 passed, zero failed, one existing ignore**, nine selected binaries |
| Eleven prechecks | Runtime `7c02eb38` | Passed; strict lint/rustdoc, examples, 193 benchmark tests, minimum Rust 1.85 default / 1.87 Zstd; gRPC run 3,250 passed, four existing ignores |
| General release preview | Runtime `7c02eb38` | **33.418 seconds**, balanced cancellation/drain |
| Deadline release preview | Runtime-equivalent `7c02eb38` | **30.792 seconds**, 31 cycles of the same 27 cases; final FD 8, tasks 0, retained cache/leases 0 |

These test scopes overlap. Selected sanitizers and repeated fixture cycles do not
add distinct full-workspace tests. The workspace repository differs from runtime
source only in the registry test fixture and eleven prior published evidence/index
paths. Before/after proofs compare all **3,790 Git blobs, modes and sizes** and the
complete physical checkout: **840,672,050 bytes**. Production, codegen, benchmark,
manifest and lock blobs match. Results preserve both source identities.

## Measurements

Linux x86_64, Rust 1.99.1, shared host. Equal Prost endpoint handlers unless labeled
pbrs interop. Requested bytes are cumulative allocation requests, not RSS.

| Current native side | Workload | Requested bytes/RPC | Fresh tonic/prost | Difference |
|---|---|---:|---:|---:|
| Client | Bidi, 1 KiB identity | 54.23 KiB | 40.19 KiB | +34.92% |
| Server | Bidi, 1 KiB identity | 43.58 KiB | 32.95 KiB | +32.26% |
| Client | Bidi, 1 MiB gzip | 19.41 MiB | 16.67 MiB | +16.40% |
| Server | Bidi, 1 MiB gzip | 18.40 MiB | 15.67 MiB | +17.43% |

The separate pbrs interop server uses **43.37 versus 15.67 MiB/RPC (+176.88%)**
for 1 MiB gzip bidi. Additional handler work prevents isolated codec/transport
attribution. The [benchmark page](../../benchmarks.md) lists all 22 instruction
scopes: 20 gzip scopes are 3.19–16.52% below tonic; both repeats win in every scope.

Source increases remain visible: server unary bytes **+5.12%** with TLS/16 in
flight, client identity bidi allocation count **+2.95%**, and plaintext server
identity unary instructions **+16.14%** versus the fresh baseline. The complete
ledger retains every positive change and repeat range without a 2% cutoff.
Callgrind uses release binaries with allocation counting; these are not shipping
CPU, latency or throughput measurements.

| Completed comparison | Captures | Successful RPCs, both sources |
|---|---:|---:|
| Native counters | 1,088 | 40,800 |
| Instructions | 264 | 3,960 |
| Fresh process | 96 | 144 |
| **Canonical total** | **1,448** | **44,904** |
| Separate three-source Router control | 48 | 1,800 |

All offered/dispatched/completed/successful counts reconcile; errors, timeouts,
overflow, unstarted and unfinished calls are zero. Router control counts stay
separate. Its runtime-only `40f219fa`→`7c02eb38` change reduces server requested
bytes **19.76% unary / 7.72% bidi**; small allocation-count increases remain.
The private release layout proof, Router 4,288→544 bytes with Single unchanged
at 544, is separate from these real allocator-request observations.

Native counts use 25/50 calls, instructions 10/20, two paired repeats. Fresh-process
1/2-call controls use four repeats and **include setup**. Identity server process
bytes rise **0.49% / 0.90%** for one/two RPCs; no startup subtraction, isolated
cold-RPC cost or hardware-cold-cache claim is made. Gzip level 6, zero-filled
payloads, four messages per streaming leg, 400 KiB server send buffers, queues 16;
native accounting/channelz stay enabled. Entropy/read-all/overload coverage remains
limited; combined-source changes do not isolate each mechanism.

## Ownership and framing

One absolute deadline begins at header acceptance and covers cooperative service
waits and shape selection. Late caps only tighten it. The outer owner drops the
application future before ending lifecycle accounting; unary accounting uses the
writer's committed status. Timers cannot preempt synchronous application polls.
Native termination preserves headers/current HTTP/2 frame length and discards
unsent tails safely. Web status is BODY DATA: completed-message boundaries preserve
text carry; an unfinished message/trailer body needs a reset rather than another
status frame. Peer credit and a blocked socket can still prevent wire progress.
New terminal-path payload destruction/wakers run outside engine locks; the broader
ordinary-success DATA-owner Drop concern remains open and unreproduced.

## Resource days

At **2026-10-24 09:27:55 UTC**, both current runtime `7c02eb38` days were running,
using repository validation `8144576f`. General worker PID 584451 started around
09:23 UTC; deadline worker PID 584649 around 09:24 UTC. The independent 09:26 UTC
observation verified actual child limits: AS 1 GiB, FD 128, same-UID processes
4,096, CPU 86,430 seconds. Deadline cycles repeat the same 27 cases, not new tests.
Retained binary/launch/progress provenance matches; protected loaded-image procfs
selectors were unavailable and are not claimed as direct loaded-byte hashes.

No terminal report or 24-hour pass exists in this snapshot. Earlier `beb4ecd9`
[general](../grpc-decode-reuse-soak-20261023/README.md) and
[Prost-cache](../grpc-prost-cache-diagnostic-20261023/README.md) days passed their
own source/scopes and do not qualify this runtime. The raw archive retains the
once-read day snapshot and independent actual-worker observation by value.

## Earlier failures

The raw record keeps the three detecting deadline failures, initial compile
failure, partial-write fixture failures, missing lifecycle-start repair, unary
five-test red/green and withdrawn Tonic-observer review finding. Original outcomes
are never renamed by later passes. Old `40f219fa` captures passed, but its ASAN and
workspace were not run before supersession.

The first `7c02eb38` workspace finished **3,811/1/9**: sparse checkout excluded
tracked evidence and broke documentation links. Complete materialization fixed
that setup. Its second run finished **3,811/1/9**: documentation passed, but a
registry test raced concurrent default registration between two observations.
The controlled red detected that fixture race; its test-only correction passed
the diagnostic plus all 29 service-config tests. The final repository run above
passed. Both failed states/logs, exact source maps, fixture/ELF snapshots and
independent review remain available in [results.json](results.json) and raw history.

## Reproduce and remaining work

Use the pinned Rust/Cargo/protoc environment and recorded commands in the archive;
build the exact commits with the unchanged harness. Original runtime/baseline
snapshots and the actual final workspace fixture/patch are separate. ELF bytes
are omitted; hashes and source/build provenance remain. Verify the capsule with:

```sh
python3 check.py
```

[Manifest](manifest.json) · [Raw sources, commands, logs, counters and profiles](raw.tar.gz)
· [Execution and ownership](../../decisions/grpc-execution.md)

Finish the current days, remaining features/specification coverage, required
platforms/consumers/releases and dedicated-host shipping CPU/latency/throughput
comparisons. Current byte losses and source increases remain optimization work;
passing these local checks does not complete those gates.
