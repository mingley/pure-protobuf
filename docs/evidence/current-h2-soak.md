# Current-backend finite-limit resource diagnostic (QG-06a)

This tooling records a bounded current-`h2` resource/recovery smoke. It never
certifies production readiness or closes QG-06's 24-hour acceptance. The
shipping transport, dependency graph and defaults are unchanged.

## Reproduction

Use Linux with the repository's pinned Rust toolchain, from a clean checkout.
The output directory must be new and outside source, or under an ignored
directory. Compilation is outside the child process limits.

```sh
export CARGO_BUILD_JOBS=1
python3 scripts/current-h2-soak.py \
  --source "$(git rev-parse HEAD)" \
  --duration 30 --seed 20261002 \
  --memory-bytes 1073741824 --max-fds 128 --max-uid-processes 4096 \
  --output target/current-h2-smoke/run-001
python3 scripts/current-h2-soak.py \
  --validate target/current-h2-smoke/run-001/report.json
```

The runner rejects missing, abbreviated or dirty source pins, including
untracked source, before building. Cargo's current `compiler-artifact` JSON
must identify exactly one executable for this test and manifest; a stale
binary path is never used as fallback. Source/lockfile/tree pins are checked
again after building and after execution. The report retains executable SHA,
Rust/Cargo/Python versions, host/kernel details, exact commands, effective
Linux process limits, exit status and failures. An existing output directory
is rejected so earlier attempts cannot be overwritten.

## Frozen scope and accounting

The two-worker Tokio process contains both client and server on TCP loopback,
without TLS or compression. Per cycle, it admits at most two RPCs, permits four
connections/eight HTTP/2 streams per connection, caps encoded/decoded messages
at 64 KiB, sets a 16 KiB per-stream send threshold and independently attaches
256 KiB client/server byte trackers last. It uses a 300 ms server deadline,
150 ms drain grace and a 60 ms deliberately paused reader of 128 separate 2 KiB
response messages. The seed identifies deterministic request labels; this is
not a randomized fault campaign.

The client advertises a 1 KiB stream receive window and a 4 KiB connection
receive target at connection establishment. The RFC's initial connection
credit still applies; the fixture does not assume it starts below 65,535 bytes.
Received streams decode directly on the reader without a pump or decoded queue.
A four-message bounded server producer emits a 256 KiB response to a small
request. Its positive sent-message count must remain unchanged between the
30 ms and 60 ms pause observations, with the producer unfinished. Resuming reads
must deliver all 128 exact messages and finish the producer. This independent
progress observation demonstrates application backpressure; a server lifecycle
callback alone cannot show whether bytes remain in bounded HTTP/2 buffers.
Client outbound request-stream queues hold at most one message, a distinct knob.

Each cycle records warmup, slow reader, overload, cancellation, deadline,
successful recovery probe and drain, following one initial process baseline.
Two unending uploads occupy both server RPC slots and a third unary call must
return `RESOURCE_EXHAUSTED`. Aborting both client futures must release calls
and accounted bytes/tokens; an independent unending upload must finish with
`DEADLINE_EXCEEDED`. Exact response contents/counts are checked, including all
slow-reader messages. Every cycle reconnects and tears down the server.

Raw JSONL events record Linux RSS and `VmHWM`, OS thread and descriptor counts,
Tokio alive tasks, observed streaming-call starts/ends/current/peak, and each
tracker's current and exact lifetime peak bytes/tokens. The Python parent
samples `/proc/<child>` every 50 ms and retains all samples in the report.
Snapshots and sampled RSS can miss transient peaks; `VmHWM` is the process
lifetime RSS high-water, and tracker peaks are exact only after acquisitions
settle. Byte-permit tokens are not RPC slots or HTTP/2 streams. Admitted-call
counts cover the `ClientHello`/`ServerHello` lifecycle callbacks in this bounded
scenario, not unary calls, all wire streams or a general RPC-slot gauge. The
rejected unary also emits start/end callbacks, which are deliberately excluded
from this streaming-call accounting. Tokio alive tasks and
OS threads are different quantities. This single process cannot attribute
client/server RSS separately or quantify allocator/socket memory.

The predeclared post-drain tolerances are baseline plus 32 MiB RSS, one file
descriptor and two Tokio tasks. Calls, accounted bytes and byte-permit tokens
must return to exactly zero; observed streaming starts must equal ends and tracker peaks
must remain below their 256 KiB budgets. These are diagnostic recovery bounds,
not proposed production budgets. The independent report validator rejects
missing gauges, incomplete/duplicated/reordered phases, nonfinite limits,
failed children and tolerance violations. It refuses any `qualified: true`
or a passed 24-hour disposition.

Limits apply to the test binary only: 1 GiB virtual address space, 128 file
descriptors, 4,096 same-UID processes/threads and requested duration plus 30 CPU
seconds, with core dumps disabled and an independent duration-plus-15-second
wall watchdog. `RLIMIT_NPROC` counts the whole UID and does not cap Tokio tasks;
choose a finite value compatible with other same-UID processes. The harness
changes only its child limits, never other running processes.

## Builder compatibility finding

At the assigned base `cf3eee22c6b06324e299f81b76915651471d8025`, identical
HTTP/2 send-buffer settings do not imply identical aggregate admission:

| Construction | Send threshold | Tracker limit/identity |
|---|---:|---|
| Fresh `Server` or `Router` | 1 MiB | Unlimited |
| `.config(ServerConfig::default())` | 1 MiB | Preserves existing tracker |
| Explicit `.max_send_buffer_size(1 MiB)` | 1 MiB | Replaces tracker with 1 MiB limit |
| Attach tracker then `.config(nondefault_send)` | Nondefault | Replaces shared tracker |
| `.config(nondefault_send)` then attach tracker | Nondefault | Preserves shared tracker and separate budget |

Three ordinary integration tests characterize the existing Server/Router
behavior and shared identity with live held byte permits. Independent default
equality and tracker preservation are desirable API properties that the base
does not satisfy. Changing the coupling is a compatibility decision reserved
for a separately scoped correction; this patch neither changes defaults nor
silently imposes a new aggregate budget. The harness sets transport knobs
first and attaches its separate tracker last.

## Results and remaining qualification

All three attempts remain visible; the two failed fixture versions are not
rewritten as passes. The final source pin is
`ac6811e73f13130a064a0e9c8662503c961d0fc2`; the named native regression gates
exercise the unchanged shipping code from assigned base `cf3eee22`.

| Attempt | Frozen source | Exact fixture | Result/raw evidence |
|---|---|---|---|
| v1 | `34ef27e528e102aa8b1f6bd610acb4d124363efe` | 16 replies, all lifecycle callbacks, active-call assertion | [Failed report](current-h2-smoke-v1/report.json), [stderr](current-h2-smoke-v1/test.stderr.log), [events](current-h2-smoke-v1/events.jsonl) |
| v2 | `ae4ac38326a8a4d0a1b94d25b0e7dda66fbe5ad4` | 16 replies, selected streaming callbacks, outbound queue 1, active-call assertion | [Failed report](current-h2-smoke-v2/report.json), [stderr](current-h2-smoke-v2/test.stderr.log), [events](current-h2-smoke-v2/events.jsonl) |
| v3 | `ac6811e73f13130a064a0e9c8662503c961d0fc2` | 128 replies, bounded producer with independent stable-progress/done observations | [Passed diagnostic report](current-h2-smoke-v3/report.json), [stdout](current-h2-smoke-v3/test.stdout.log), [events](current-h2-smoke-v3/events.jsonl) |

The v1 lifecycle peak of three included the rejected unary's start/end callbacks;
it was not evidence of three admitted RPC slots. Both early versions assumed
that a paused application reader must keep the server lifecycle active. v1
completed five cycles before failing this assertion; v2 completed ten. Source
review showed that received streams are pull based and the client queue knob
controls outbound requests. A small response can also finish at the server
after entering bounded HTTP/2 buffers. The failed active-call assumption was
replaced by the independently observed bounded producer in v3. The secondary
shutdown `RecvError` in both failed stderr logs followed the assertion dropping
its shutdown sender; it is preserved rather than presented as an independent
production defect. No deadline, message cap, byte budget, process limit or
recovery tolerance was relaxed.

The 30-second v3 diagnostic passed with child/runner exits zero and no validator
failures. It ran for 30.52 seconds on a shared host with concurrent jobs=1 Rust
builds, making no comparative CPU, latency or throughput claim. It recorded
477 phase events and 604 independent `/proc` samples across 68 complete cycles,
272 selected streaming-call starts/ends, 68 expected overflow rejections and
68 expected deadline expirations. All 8,704 slow-reader responses were checked
for exact contents and count. During each pause, producer progress remained
unchanged at 20–27 messages between the 30 ms/60 ms observations, and the producer
was unfinished. Every cycle completed all 128 messages after reading resumed.

| Resource | Baseline | Load/observed peak | Every final drain |
|---|---:|---:|---:|
| Process RSS | 5,767,168 bytes | 11,644,928 bytes (`VmHWM` and phase maximum) | At most 11,644,928 bytes; below baseline + 32 MiB |
| File descriptors | 7 | 10 | 7 |
| OS threads | 4 (test harness plus two runtime workers) | 4 | 4 |
| Tokio alive tasks | 0 | 7 | 0 |
| Observed streaming calls | 0 | 2 | 0; 272 starts = 272 ends cumulatively |
| Accounted server/client bytes | 0/0 | Exact lifetime peaks 16,448/29 bytes | 0/0 |
| Server/client byte-permit tokens | 0/0 | Exact lifetime peaks 8/1 | 0/0 |

Tracker peaks cover each tracker lifetime, including its warmup; they do not
bound HTTP/2-owned buffers, application memory or RSS. The low client accounted
peak is not an estimate of all received-response memory. All values remain
separately labeled. The executable launch SHA256 was
`e25d6216a30ac5d6326aeffee12f67701b265c3a07b69e635ee1d3beaaf2e6a1`, checked
unchanged after execution. Effective soft/hard limits were 1 GiB address space,
128 descriptors, 4,096 same-UID processes/threads and 60 CPU seconds. The full
commands, Cargo artifact records, Rust 1.99.0/Cargo 1.99.0/Python 3.12.14 pins,
source/tree/lockfile pins, kernel and host details are retained in the reports.

| Gate | Result/evidence |
|---|---|
| `cargo test --locked -p pbrs-grpc --test resource_qualification --test hostile --test message_size --test rpc --test gaps --test tls --test serving` | [1,193 passed](current-h2-native-gates.log), one explicitly ignored diagnostic |
| Current ordinary characterization executable `--test-threads=1` | [3 passed, 1 deliberately ignored](current-h2-characterization-final.log); the ignored diagnostic was separately executed above |
| `cargo clippy --locked -p pbrs-grpc --test resource_qualification -- -D warnings` | [Passed](current-h2-clippy-final.log); the [initial two lint failures](current-h2-clippy-initial-failure.log) are retained |
| `python3 -m unittest discover -s tests -p test_current_h2_soak.py -v` | [16 passed](current-h2-python-guards.log), including malformed evidence, incomplete phases, source/binary drift, finite limits, recovery and blocked-producer guards |
| `cargo fmt --all --check`; `git diff --check` | Passed |
| `python3 scripts/current-h2-soak.py --validate docs/evidence/current-h2-smoke-v3/report.json` | Passed with `qualified: false` |
| Temporary intended builder invariants | [2 expected failures](current-h2-intended-invariants.log), with [exact temporary assertions](current-h2-intended-invariants.rs.txt); the shipping compatibility policy was not changed |

The complete 24-hour profile, TLS/compression, independent peers, injected
RST_STREAM/GOAWAY, mixed 1 MiB messages, allocator high-water, kernel socket
memory and dedicated-host latency/goodput qualification remain `not_run`.
QG-06 remains open. Duration alone cannot convert this narrower instrumented
scenario into production acceptance.
