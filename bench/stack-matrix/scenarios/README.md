# Cross-stack benchmark scenarios

These scenarios extend the [cross-stack matrix](../../../docs/evidence/stack-matrix-sb11.md).
They are local validation tools, with explicit gaps before a claim campaign.

| Goal | Example from the repository root | Read the result as |
|---|---|---|
| Check connection-scale plumbing | `./scripts/stack-matrix.sh --scenario conn-scale-idle --smoke --server-peers native` | A small ladder diagnostic; inspect every cell's status |
| Check overload accounting | `./scripts/stack-matrix.sh --scenario extended-overload --smoke --server-peers native` | Accounting/recovery evidence for the supported peer and shape |
| Check the runners without launching peers | `python3 bench/stack-matrix/scenarios/conn_scale.py --self-test` and `python3 bench/stack-matrix/scenarios/extended_cells.py --self-test` | Logic checks, not live performance measurements |
| Execute frozen echo orders | `python3 bench/stack-matrix/run.py --scenario bench/stack-matrix/scenarios/grpc-bench-echo.json --scenario-smoke` | All frozen repeats execute; claim eligibility still requires independent evidence |

Only `fail` makes the scenario runners exit nonzero. A zero exit can therefore
contain `invalid`, `unsupported`, or `not_run` cells. Read those statuses before
calling a requested matrix complete. No scenario here is a leadership result
merely because its smoke command passed.

## SB-12 connection scale

Measures connection scale, memory per connection/stream, and handshake
rate for `pbrs-grpc` (peer `native`) and the SB-11 peers. Runner:
[`conn_scale.py`](conn_scale.py), invoked via
`scripts/stack-matrix.sh --scenario <name>`.

## Scenarios

| File | Kind | Scoreboard | Claim ladder |
|---|---|---|---|
| `conn-scale-idle.json` | idle-conn RSS/conn | D5 | 1k / 10k / 100k idle conns |
| `conn-scale-active-streams.json` | active-stream RSS/stream | D5, C6 | 1k / 10k / 100k held bidi streams |
| `conn-scale-tls-handshake.json` | TLS handshakes/s/core | D6, C7 | 2000 handshakes x 3 reps, 1 CPU |
| `conn-scale-storm.json` | storm accepts/s | D6 | 10 s burst x 3 reps |

Cell order is frozen: peers in file order x ladder ascending. Every
scenario keeps `repeats: 3`; the runner refuses memory kinds with fewer
(accept 2).

## Method in one paragraph

One fresh server per ladder point, spawned with the exact SB-11 argv and
pinning (`run.server_command`, `pin.wrap`). The opener is peer-agnostic
stdlib Python: TCP (+TLS with a verifying client) connect, HTTP/2 client
preface, wait for server SETTINGS, hold. Each memory point starts with a
discarded full-shape warmup round (first-touch allocator/runtime growth
otherwise lands in repeat 1), then 3 measured repeats. Storm workers
hold a bounded rotating pool so fd use stays flat at any rate. Idle RSS/conn is
`(loaded-baseline)/established` per repeat, median over repeats (accept
2); streams add held-open `FullDuplexCall` HEADERS (no `END_STREAM`, no
`DATA`) and divide the idle->streams delta by streams held. Claim
analysis should use the slope across ladder points (1k->10k->100k), not
just per-point delta/N, since fixed costs dominate small N. Handshake
rate is completed full handshakes over wall time divided by pinned CPUs.
Storm rate counts only preface-roundtrip-established connections, so it
is the accept+handshake-processing rate, not SYN acceptance.

## Accept evidence

- **Accept (1)**: every report embeds `os_limits` (RLIMIT_NOFILE/NPROC,
  Linux `fs.file-max`, `fs.nr_open`, `somaxconn`, `ip_local_port_range`,
  `tcp_max_syn_backlog`, `tcp_tw_reuse`; Darwin `kern.maxfiles*`,
  `kern.ipc.somaxconn`, portrange). Preflight budget checks plus runtime
  `EMFILE`/`ENFILE`/`ENOBUFS`/`ENOMEM`, refused-with-live-server, and
  server-stderr fd notes mark the cell `invalid`, never `fail`; only
  `fail` exits nonzero. Prove it without tuning the host:
  `python3 conn_scale.py --self-test`, or run any scenario under a
  lowered fd ceiling (`ulimit -n 512` covers the opener; the preflight
  check then marks ladder points invalid deterministically).
- **Accept (2)**: memory cells carry per-repeat
  `rss_baseline/loaded/delta_bytes` plus the median/min/max summary, and
  `--smoke` keeps 3 repeats with tiny ladders so CI exercises the gate.

## Statuses

`pass` | `invalid` (OS ceiling or server-side saturation: preflight
budget, errno, `<99%` established, `>1%` streams reset) |
`unsupported` (unresolvable TLS material) | `not_run`
(optional peer unrunnable) | `fail` (peer died, RSS unreadable,
handshake errors without an OS cause). Missing *required* peer binaries
fail the run before any cell, matching SB-11.

## Claim hosts

Use Linux with the server pinned to 1 CPU and record per-process/system file
descriptor limits. The 100k point requires more than larger fd limits: one
source address connecting to one destination address/port cannot supply
100,000 distinct TCP source ports. The current single-endpoint opener needs
a multi-address or multi-endpoint topology extension before that point can be
qualified. Do not prescribe an impossible `ip_local_port_range` wider than
100,000, or count an OS-limit row as a server loss.

Non-Linux / unpinned runs record that in `pinning` and stay diagnostic.
`grpc-bench-echo.json` is loaded directly by `run.py --scenario PATH`.
`--cells ID,ID` preserves the selected cells' order within every frozen repeat;
`--scenario-smoke` shortens durations but retains all repeats and the seed.
The report records frozen/effective parameters, source SHA-256, actual order,
and separate repeat logs. `--claim` blocks before launching endpoints because
this runner currently uses loopback and lacks verified runtime peer settings.
`--verify-fairness REPORT --claim` rejects configured/scripted settings,
missing repeats, resource windows, or headroom evidence.

## SB-19 extended cells

Adds the contract-section-3 axes the SB-11 harness does not cover:
client-streaming and pipelined-bidi shapes, gzip on/off, emulated 1 ms
and 10 ms RTT profiles, and overload/recovery cells. Runner:
[`extended_cells.py`](extended_cells.py), invoked via
`scripts/stack-matrix.sh --scenario <name>`. Server cells only: every
cell pins one server peer under test behind the fixed native open-loop
Poisson generator, reusing the SB-11 spawn/pin/preflight machinery.

## Scenarios

| File | Kind | Scoreboard | Cells |
|---|---|---|---|
| `extended-shapes.json` | fixed-rate new shapes | C3, D3 | client_stream 1 KiB x 2000 (runs); bidi_pipelined empty x 256 (frozen, unsupported) |
| `extended-gzip.json` | gzip on/off pairs | E4 | unary + server_stream 64 KiB x identity/gzip; native and tonic codecs runnable, official-peer gzip unsupported |
| `extended-rtt.json` | RTT holdouts | E5 | unary/client_stream/server_stream x 1 ms + 10 ms, verify-then-measure per repeat |
| `extended-overload.json` | 2x saturation + recovery | D7 | unary 1 KiB: saturation search, overload phase, recovery probes |

Cell order is frozen: peers in file order x cells in file order. Every
scenario keeps `repeats: 3`; the runner refuses fewer (accept-style
gate, mirroring SB-12). These are validation-stage cells: pass means
the cell ran and every offered call reconciles, never a leadership
gate; claim-grade statistics (5+ paired runs, contract section 6) are
SB-22's job, which re-freezes rates after a pilot.

## Method in one paragraph

One fresh server per cell, spawned with the exact SB-11 argv and pinning.
Fixed-rate cells run one discarded warmup probe then 3 measured probes
at the frozen offered rate. RTT cells add a recorded setting plus a
measured ping probe (short constant-rate unary-empty `load` through the
same `--latency-rtt-ms` loopback-proxy path) before the warmup and
before every repeat; a repeat whose measured p50 does not evidence the
configured delay is `invalid`. Overload cells search geometrically for the first rate the server
cannot sustain -- server errors/timeouts, or rejections persisting at
the escalated cap (a bounded generator sheds load before a slowing
server errors, so rejection persistence within a fixed concurrency
budget is the observable knee; the firing signal is recorded per
repeat). Rejected steps escalate `--max-in-flight` once, CPU-saturated
steps invalidate as inconclusive; then a baseline at the last clean
rate, one overload phase at 2x saturation, and probes back at normal
until clean or the budget exhausts. The report carries goodput
(application bytes, framing excluded), error/timeout/rejection rates,
p99 with retained failures, and recovery time (or `recovered: false`).

## Frozen-but-unrunnable cells

`load --compression=identity|gzip` and `load-server` support all four
RPC shapes for native and both tonic codecs. Gzip requires gzip response
metadata on every call; a peer silently returning identity fails validation.
`transport_smoke.py` covers all nine plaintext directions and native TLS,
and captures plaintext gRPC message compression flags in both directions.
Both tonic codecs support TLS with the existing pure-Rust Graviola provider,
explicit CA/name verification and the TLS 1.3 AES-128-GCM suite.
Pipelined bidi and configured official-peer gzip responses remain
unsupported and block any claim covering those cells.

## Accept evidence

- **Accept (1)**: every report reconciles offered vs
  successful/failed/rejected per probe; overload summaries carry
  `median_overload_goodput_mib_s`, `median_overload_rejection_rate`
  (+ error/timeout rates) and `median_recovery_time_s` with
  `recovered_repeats`. Prove it without claim hosts:
  `python3 extended_cells.py --self-test`, or run any scenario smoke
  (`--smoke --server-peers native`, >=3 repeats, tiny durations).
- **Accept (2)**: every RTT repeat embeds its `rtt_verify` record
  (configured ms, mechanism, baseline TCP ping, measured ping p50,
  verdict); unverified repeats are `invalid`, and the report carries
  `rtt_authoritative: false` with the claim note that authoritative
  RTT needs separate hosts plus tc netem.

## Statuses

`pass` | `invalid` (RTT unverified, saturation unreached, accounting
mismatch, generator saturated: precondition unmet, never a peer loss)
| `unsupported` (gzip-on, pipelined bidi) | `not_run` (optional peer
unrunnable) | `fail` (peer died, probe crashed). Missing *required*
peer binaries fail the run before any cell, matching SB-11. Only
`fail` exits nonzero.

## Claim hosts

Linux, pinned CPUs; loopback-proxy RTT stays bench-only emulation
everywhere (authoritative RTT needs separate hosts, contract 3.6/8).
`load` payloads are zero bytes, so future gzip-on cells measure
near-maximum compressibility -- any gzip claim must name that pattern.
