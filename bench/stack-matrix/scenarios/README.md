# SB-12 connection-scale scenarios

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
`unsupported` (tonic TLS, unresolvable TLS material) | `not_run`
(optional peer unrunnable) | `fail` (peer died, RSS unreadable,
handshake errors without an OS cause). Missing *required* peer binaries
fail the run before any cell, matching SB-11.

## Claim hosts

Linux, server pinned to 1 CPU, `RLIMIT_NOFILE >= 200k` for the 100k
point (opener + server each hold N fds), `ip_local_port_range` wider
than N. Non-Linux / unpinned runs record that in `pinning` and stay
diagnostic. `grpc-bench-echo.json` in this directory is the SB-21
contract-shaped echo scenario, untouched by SB-12.
