# TC-15 adaptive HTTP/2 flow-control windows

Date: 2026-09-28  
Host label: dev-loop, contended host

## Hypothesis

An opt-in BDP-estimating receive-window mode can recover much of the throughput
lost by small fixed HTTP/2 windows while preserving the existing 16 MiB fixed
defaults and resource caps.

## Implementation

- Added `ServerConfig::adaptive_window(true)` and
  `ChannelConfig::adaptive_window(true)`, plus
  `adaptive_window_initial_size` and `adaptive_window_max_size`.
- Default behavior is unchanged: both server and client still use fixed 16 MiB
  stream and connection windows unless adaptive mode is explicitly enabled.
- Adaptive mode owns h2's single user `PingPong` for that connection and shares
  it with keepalive when keepalive is configured. Fixed-window connections keep
  the previous keepalive task path.
- BDP growth follows the hyper/grpc-go shape: count inbound DATA during a PING
  RTT, update moving RTT/bandwidth, and double the sample when it reaches about
  two thirds of the current estimate, capped by `adaptive_window_max_size`
  (16 MiB by default).
- `rpc-bench load` gained bench-only `--window-mode={default,small,adaptive}`,
  `--small-window-size`, and `--latency-rtt-ms` flags. The latency flag uses a
  loopback TCP proxy in the benchmark process only; it is not in the shipping
  transport path.

## Checks and measurements

Pre-edit baseline on the original fixed default (debug build, no proxy):

| Cell | Window | Successes | p50 e2e | p99 e2e | Path |
|---|---:|---:|---:|---:|---|
| 8 MiB unary | default fixed 16 MiB | 53 | 44.069 ms | 64.116 ms | `files/tc15-baseline/default-large-unary.json` |
| 8 MiB server-streaming | default fixed 16 MiB | 99 | 24.839 ms | 33.718 ms | `files/tc15-baseline/default-large-stream.json` |
| empty unary | default fixed 16 MiB | 7356 | 0.307 ms | 0.699 ms | `files/tc15-baseline/default-small-unary.json` |

Post-change release build, no proxy, 0.2 s closed-loop cells:

| Cell | Window mode | Successes | Throughput | p50 e2e | p99 e2e | JSON |
|---|---|---:|---:|---:|---:|---|
| 8 MiB unary | fixed small 65,535 B | 4 | 11.7 qps | 340.069 ms | 340.855 ms | `files/tc15-measure-final2/large-unary-small.json` |
| 8 MiB unary | adaptive | 8 | 24.0 qps | 166.945 ms | 175.440 ms | `files/tc15-measure-final2/large-unary-adaptive.json` |
| 8 MiB unary | default fixed 16 MiB | 19 | 87.2 qps | 44.941 ms | 61.784 ms | `files/tc15-measure-final2/large-unary-default.json` |
| 8 MiB server-streaming | fixed small 65,535 B | 4 | 12.6 qps | 317.523 ms | 317.836 ms | `files/tc15-measure-final2/large-stream-small.json` |
| 8 MiB server-streaming | adaptive | 8 | 30.9 qps | 151.291 ms | 151.837 ms | `files/tc15-measure-final2/large-stream-adaptive.json` |
| 8 MiB server-streaming | default fixed 16 MiB | 33 | 155.9 qps | 21.287 ms | 59.324 ms | `files/tc15-measure-final2/large-stream-default.json` |
| empty unary | fixed small 65,535 B | 2753 | 13,710.4 qps | 0.235 ms | 1.410 ms | `files/tc15-measure-final2/small-unary-small.json` |
| empty unary | adaptive | 4002 | 19,999.0 qps | 0.187 ms | 0.468 ms | `files/tc15-measure-final2/small-unary-adaptive.json` |
| empty unary | default fixed 16 MiB | 3114 | 15,550.9 qps | 0.200 ms | 1.439 ms | `files/tc15-measure-final2/small-unary-default.json` |

Latency-proxy, 0.2 ms RTT, constant one-call 8 MiB cells:

| Cell | Window mode | Successes | p50 e2e | JSON |
|---|---|---:|---:|---|
| 8 MiB unary | fixed small 65,535 B | 1 | 760.419 ms | `files/tc15-measure-final2/latency-large-unary-small.json` |
| 8 MiB unary | adaptive | 1 | 624.415 ms | `files/tc15-measure-final2/latency-large-unary-adaptive.json` |
| 8 MiB unary | default fixed 16 MiB | 1 | 45.542 ms | `files/tc15-measure-final2/latency-large-unary-default.json` |
| 8 MiB server-streaming | fixed small 65,535 B | 1 | 451.403 ms | `files/tc15-measure-final2/latency-large-stream-small.json` |
| 8 MiB server-streaming | adaptive | 1 | 424.117 ms | `files/tc15-measure-final2/latency-large-stream-adaptive.json` |
| 8 MiB server-streaming | default fixed 16 MiB | 1 | 28.891 ms | `files/tc15-measure-final2/latency-large-stream-default.json` |

Commands used for the latency rows:

```bash
CARGO_BUILD_JOBS=3 cargo run --manifest-path rpc-bench/Cargo.toml --release --quiet -- \
  load --distribution=constant --rate=1 --max-in-flight=1 --duration-secs=0.2 \
  --latency-rtt-ms=0.2 --transport=native --window-mode=<small|adaptive|default> \
  --req-bytes=8388608 --resp-bytes=8388608 --max-message-size=16777216

CARGO_BUILD_JOBS=3 cargo run --manifest-path rpc-bench/Cargo.toml --release --quiet -- \
  load --distribution=constant --rate=1 --max-in-flight=1 --duration-secs=0.2 \
  --latency-rtt-ms=0.2 --transport=native --window-mode=<small|adaptive|default> \
  --shape=server_stream --stream-msgs=8 --resp-bytes=1048576 --max-message-size=2097152
```

## Verdict

Adaptive windows are viable as an opt-in feature and recover roughly 2x over a
65 KiB fixed window on the local 8 MiB cells. They do not beat the existing
fixed 16 MiB default on these loopback measurements, so the default stays fixed
and unchanged.

This is a partial tonic/hyper parity result rather than a reason to switch
defaults. The implementation intentionally keeps an explicit cap so the resource
budget formulas remain bounded by configuration.

## Limits and follow-ups

- Wall-time data only, on a dev-loop contended host. Treat relative large-payload
  numbers as directional, not claim-grade.
- The latency proxy is bench-only and useful for single-call cells. Higher
  offered-load proxy runs were noisy and could stall under the shared host, so
  this evidence does not claim an adaptive win under sustained RTT-injected
  load.
- Hyper's default adaptive starting window is 1 MiB. pbrs-grpc currently starts
  adaptive mode at the RFC initial window (65,535 B) unless overridden; a follow-up
  should evaluate changing that default after a stable RTT-injected harness exists.
