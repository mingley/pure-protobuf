# RX-01: minimal runtime seam

## What

`pbrs-grpc/src/rt/` isolates task spawn, timer creation, and socket IO
bounds behind crate-private traits:

- `Runtime`: `now`, `sleep_until`, `timeout` (unified `TimedOut` error),
  `interval` (with `MissedTickBehavior::Delay` preset), `spawn`. All static,
  declared with `impl Trait` returns so each implementation keeps its own
  future types.
- `TokioRuntime`: every method compiles to the direct Tokio call
  (`tokio::time::sleep_until` / `tokio::spawn` returned directly, not
  wrapped). `timeout` maps Tokio's `Elapsed` to `rt::TimedOut` in one
  setup-path async layer; `tick` returns Tokio's `Tick` future directly.
- `Interval`: periodic ticker trait; `Io`: socket IO bound (blanket impl
  over the exact Tokio IO bound, named for the seam).
- `manual::ManualRuntime` (test-only, `#[cfg(test)]`): thread-local manual
  clock moved only by `ManualRuntime::advance`. `!Send` guard installs /
  clears one clock per thread; tests run on the current-thread runtime.
  `spawn` deliberately delegates to Tokio (only time is faked).

Rerouted, keeping every existing name and call shape: same-name wrappers
return the generic `*_in::<TokioRuntime>` core future directly (plain
`fn`, no extra async layer, no new bounds), in `client/call.rs`
(`send_request_frame`, `open`, `prefer_deadline`, `first_of`, `race`,
`grab`, `open_retrying`), `server/connection.rs` (`serve_one`, `serve_io`,
`sleep_until_opt`), `keepalive.rs` (`spawn`), and `timeout.rs`
(`TokioRuntime::now()` inside the unchanged public helpers). No public API
change; no out-of-scope file touched.

## Accept mapping

- Zero measurable dev-loop regression: devloop-compare exit 0; hot-path
  futures keep identical types via direct delegation.
- Deterministic double: 4 `rt::manual` tests (sleep parks until advance,
  timeout expires with no wall wait, completion-wins ties, interval parks)
  plus `client::call` proof that `first_of` / `prefer_deadline` honor the
  manual clock.

## Gates

lib 395 (390 + 5 new), rpc 11, gaps 6, hostile 37, tls 20, telemetry 25,
binlog 32, health 180, resolver 30, doctests 171, self-interop PASS,
devloop-compare exit 0, clippy `-D warnings` clean, fmt clean.
