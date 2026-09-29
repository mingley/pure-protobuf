# H2 engine decision

Date: 2026-09-29. Card: H2-01. Evidence:
[`docs/evidence/h2-costs.md`](../evidence/h2-costs.md).

## Decision

**Do not start a custom `pbrs-h2` engine now.** Keep the existing `h2` backend
behind the transport seam and pivot H2 lane work to upstream `h2` improvements
plus narrower pbrs-grpc transport/header/compression wins.

## Why

H2-01's decision bar was to proceed only if replacing `h2` modeled at least a
15% achievable gain on primary C/D cells or if `h2` blocked a required
multi-core design. The local evidence did not meet either condition:

- Small unary/server-stream deterministic callgrind cells attribute only about
  **0.8-1.3% self instructions** to h2 internals in the loopback dev-loop.
- DHAT attributes about **9 h2 allocations and 2.9 KiB per uncompressed RPC**,
  or **4-5% heap bytes**. That is worth upstream/local cleanup, but not a
  15% custom-engine model.
- Compressed cells are dominated by gzip/miniz and pbrs-grpc compression/string
  work; h2 direct allocation share falls below 0.25%.
- The many-stream scaling diagnostic was too noisy to prove an h2 mutex/frame
  queue wall. Four server cores did not improve one-connection throughput in
  the contended host run, but four connections on four cores reduced queue
  overflow; this is not enough to claim h2 blocks the required design.

## Consequences

- H2-03/H2-04 should stay blocked until new evidence crosses the H2-01 bar.
- `pbrs-grpc/src/transport/` remains the correct seam. Preserve it so a future
  engine can still be swapped in if dedicated-host evidence changes the model.
- Current work should focus on upstream `h2` patches and pbrs-grpc wins that
  are already visible in evidence: header construction/HPACK allocation,
  compression allocation, streaming pumps/tasks, and resource instrumentation.

## Upstream-first follow-up

1. Reduce `h2::frame::headers::HeaderBlock::into_encoding` allocation/growth
   for small fixed gRPC header sets.
2. Add h2-internal diagnostics for stream-store lock hold/wait time, queued
   frames, flow-control blocked time, and wakeups.
3. Narrow stream-store/frame-queue critical sections once lock-wait evidence
   identifies a hot path.
4. Improve write batching/vectored write behavior for many small frames on a
   single connection without changing HTTP/2 semantics.
5. Add a gRPC-shaped upstream benchmark/profile: no push, realistic header
   sets, one connection with many streams, and multi-connection scaling.

## Revisit trigger

Reopen the `pbrs-h2` architecture decision only with claim-grade evidence on a
dedicated Linux host showing one of:

- modeled removable `h2` CPU/instruction cost **>= 15%** on primary C/D cells;
- proven h2 stream-store/frame-queue contention preventing required
  thread-per-core or many-stream scaling;
- a correctness/security limit in `h2` that cannot be fixed upstream or gated
  behind the existing transport seam.
