# Load accounting and endpoint diagnostics (SB-27a)

`rpc-bench load` now saves its report and exits 1 when a run has failed,
timed-out, rejected, unstarted, unfinished, or zero successful calls. Every
offered call must complete successfully for exit 0. Invalid timing options
exit 2 instead of panicking. Saved reports now include completed, unstarted,
and unfinished counts; older report formats remain readable.

This closes the benchmark accounting slice. Production readiness and the
performance floor against tonic remain open.

## Reproduced defect

The frozen pre-fix binary requested gzip responses from an identity-response
server. All three client profiles recorded zero successes and failures, yet
exited 0:

| Client | Failed RPCs | Old exit | Fixed exit |
|---|---:|---:|---:|
| Native / pbrs | 227 | 0 | 1 |
| Tonic / pbrs | 199 | 0 | 1 |
| Tonic / prost | 117 | 0 | 1 |

The fixed runs retain their complete error reports. NaN rate/duration and
infinite RTT previously panicked with exit 101; they now return parse errors.
Regression tests also reject out-of-range timing values, timeouts, queue
rejections, unfinished work, and empty runs.

Both frozen binaries, commands, stdout/stderr, original and fixed metrics,
lockfile, source snapshots, and check logs are in the
[archive](grpc-load-terminal-accounting/records.tar.gz). The pre-fix binary
already contains the RX-11 slice decoders. It is an accounting baseline,
not the old compression-performance binary.

## RPC completion matrix

The Linux [runner](../../scripts/grpc-load-smoke.py) passed all 256 cells and
263,694 RPCs on 2026-10-06:

- Native/native, native/tonic-prost, tonic-prost/native, and tonic-prost pairs.
- Unary, client streaming, server streaming, and bidirectional ping-pong.
- Empty, 1 KiB, 64 KiB, and 1 MiB payload bodies.
- Plaintext and verified TLS; identity and gzip encoding.

Each cell used one connection, one concurrent RPC, two Tokio workers, and a
0.25-second measurement. Streams exchanged four messages. For server-stream
and bidi cells, body size describes replies; for uploads, it describes request
messages. Unary uses the size in both directions. Clients verify response
encoding, lengths, counts, upload aggregates, and final status.

Order seed: `11011`. Raw endpoint CPU counters and sampled RSS are retained.
Client CPU covers its entire process lifetime; server CPU covers the client's
lifetime, including connection setup and cleanup. CPU counters have 10 ms
resolution on this host. These short, single-repeat, zero-filled-payload runs
are completion diagnostics. They do not measure steady-state instructions,
allocations, wakeups, or syscalls, or establish dedicated-host latency/QPS.
The runner pins the executable digest but leaves source-to-binary verification
false for prebuilt binaries. Build records and source snapshots are separate
artifacts; final documentation and scale-role diagnostic wording also changed
after the measured executable was built.

## Resource recovery

The existing finite-limit runner passed 64 cycles in 30.692 seconds on clean
local snapshot `a4d8b7494b492a4f516bde4840dce4d767d77d20`. This snapshot is a
local qualification commit; the main branch was not advanced. Its parent is
`452bfad7a1239e6a408af1f8cccd4dc5e6d0bee0`.

The child had 1 GiB address space, 128 descriptors, 4,096 same-UID
processes/threads, and 60 CPU seconds. It exercised slow readers, overload,
cancellation, deadlines, recovery, and drain. Tracked bytes, permit tokens,
and observed streaming calls returned to zero after each cycle. RSS,
descriptors, and tasks stayed within the predeclared recovery tolerances.
The final drain had zero Tokio tasks. Process limits, every phase observation,
independent process samples, commands, and executable digest are retained.

This plaintext diagnostic leaves `qualified: false` and the 24-hour soak
`not_run`. It does not cover the complete TLS/fault/load production campaign.

## Checks and remaining work

The final Rust benchmark suite passed 182 tests. Six Python guards reject
missing or contradictory accounting. Strict standalone Clippy, format,
documentation, and plan checks passed. The first test build missed an
`Instant` import, and the first Clippy run rejected blocking file IO in the
synchronous regression test; both failures and their fixes are retained.
Five altered archives were also rejected after recomputing their hashes:
missing terminal counts, invented CPU deltas, missing cells, a false
qualification flag, and an unrecovered permit token.

Verify artifact hashes, coverage, terminal counts, CPU deltas, and resource
recovery without executing the archived binaries:

```sh
python3 docs/evidence/grpc-load-terminal-accounting/check.py
```

SB-27 still owns the complete per-side cost matrix, loss ledger, native-prost
profile, higher concurrency, and steady-state comparisons. SV-09 and CL-08
retain the known streaming instruction and allocated-byte losses to tonic.
QG-06 retains the production soak. The RX-11 isolated decoder measurements
are recorded separately.
