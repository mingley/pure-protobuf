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
150 ms drain grace and a 60 ms deliberately paused reader of sixteen 2 KiB
response messages. The seed identifies deterministic request labels; this is
not a randomized fault campaign.

Each cycle records warmup, slow reader, overload, cancellation, deadline,
successful recovery probe and drain, following one initial process baseline.
Two unending uploads occupy both server RPC slots and a third unary call must
return `RESOURCE_EXHAUSTED`. Aborting both client futures must release calls
and accounted bytes/tokens; an independent unending upload must finish with
`DEADLINE_EXCEEDED`. Exact response contents/counts are checked, including all
slow-reader messages. Every cycle reconnects and tears down the server.

Raw JSONL events record Linux RSS and `VmHWM`, OS thread and descriptor counts,
Tokio alive tasks, lifecycle admitted-call starts/ends/current/peak, and each
tracker's current and exact lifetime peak bytes/tokens. The Python parent
samples `/proc/<child>` every 50 ms and retains all samples in the report.
Snapshots and sampled RSS can miss transient peaks; `VmHWM` is the process
lifetime RSS high-water, and tracker peaks are exact only after acquisitions
settle. Byte-permit tokens are not RPC slots or HTTP/2 streams. Admitted-call
counts are lifecycle observations, not all wire streams. Tokio alive tasks and
OS threads are different quantities. This single process cannot attribute
client/server RSS separately or quantify allocator/socket memory.

The predeclared post-drain tolerances are baseline plus 32 MiB RSS, one file
descriptor and two Tokio tasks. Calls, accounted bytes and byte-permit tokens
must return to exactly zero; admitted starts must equal ends and tracker peaks
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

The completed run and gate results are recorded below after execution. A short
instrumented shared-host run is diagnostic evidence only. The complete
24-hour profile, TLS/compression, independent peers, injected RST_STREAM/GOAWAY,
mixed 1 MiB messages, allocator high-water, kernel socket memory and
dedicated-host latency/goodput qualification remain `not_run`. Duration alone
cannot convert this deliberately narrower scenario into production acceptance.
