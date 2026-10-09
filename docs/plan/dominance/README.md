# Client and server comparison plan

Compare pbrs-grpc with tonic and prost on each workload. The target is lower
CPU, allocation count, and wall time for clients and servers separately,
using both prost and pbrs messages. Track each loss until a rerun shows improvement.

**Coordinator:** Michael Ingley.
**Relationship to other programs:** the
[performance plan](../world-class/README.md) sets the performance strategy,
and the [adoption program](../adoption/README.md) sets integration targets.
This plan tracks results for each workload and side.
**Cards:** [`../world-class/tasks.json`](../world-class/tasks.json).
**Rules:** [worker protocol](../world-class/README.md#worker-protocol) and the
[benchmark contract](../../benchmark-contract.md).

## What "better in every case" means

A **cell** combines one choice from each of these dimensions:

| Dimension | Values |
|---|---|
| Message layer | prost messages on pbrs-grpc; pbrs messages on pbrs-grpc |
| Baseline | tonic 0.14 with prost 0.14 at defaults, settings matched per the SB-01 fairness rules |
| Side | client and server, measured in separate processes |
| Call shape | unary; client streaming; server streaming; bidirectional ping-pong; bidirectional pipelined |
| Payload | empty, 1 KiB, 64 KiB, 1 MiB, plus the production-shaped corpora from SB-26 |
| Security | h2c; TLS 1.3 with the same cipher suite on both sides |
| Compression | identity; gzip |
| Concurrency | 1 call in flight; 16 streams on 1 connection; 64 connections with 16 streams each; saturation |
| Lifecycle | steady state; cold connect to first RPC; idle connection and open stream |

Each cell reports these metrics:

- **Deterministic (dev-loop):** instructions, allocations, allocated bytes,
  task wakeups, context switches and syscalls per RPC, per side.
- **Wall time (dedicated hosts):** p50 and p99 latency at fixed offered
  load, sustainable QPS per core within the SLO, streaming messages per
  second per core, time to first RPC, and RSS per connection and per stream.

The bar has three levels:

| Level | Rule |
|---|---|
| **Floor** | No cell loses on any metric beyond its measured noise band. A cell that ties must also tie on its allocations. |
| **Margin** | Primary cells meet the adoption targets: RPCs at most 0.7× instructions and 0.6× allocations, and message decode-plus-read at most 0.8× prost's instructions and 0.5× its allocations. They also meet the §7 margins in the benchmark contract. |
| **Both messages** | pbrs messages on pbrs-grpc beat prost messages on pbrs-grpc in every codec and RPC cell, so choosing pbrs messages is never a trade-off. |

The dev-loop floor is checked on every change. Wall-time rows need dedicated
hosts (SB-28), and claims still follow the benchmark contract.

## Loss ledger

SB-27 generates the ledger from the matrix and fails if any loss has no
owning card. These losses are already recorded:

| Cell | Metric | pbrs-grpc | tonic + prost | Evidence | Owner |
|---|---|---:|---:|---|---|
| prost messages, server streaming, 1 KiB | instructions per RPC | 170,930 | 150,409 | [TC-03](../../evidence/tc-03-prost.md) | SV-09 |
| prost messages, unary and server streaming | bytes per RPC | 52,021 and 75,173 | 49,177 and 58,058 | [TC-03](../../evidence/tc-03-prost.md) | CL-08 |
| Cold connect to first RPC, 1k connections | median | 2.280 ms | 1.760 ms | [SB-12](../../evidence/sb-12.md) | CL-07 |
| Empty unary | p99 latency | favors tonic | | [scoreboard C1](../../scoreboard.md) | CL-09 |
| Bidirectional ping-pong | throughput | favors tonic | | [scoreboard C3](../../scoreboard.md) | CL-09 |
| pbrs messages, deep or `Any`-heavy, reading every field | codec instructions | 2–5× prost | | [adoption program](../adoption/README.md) | PK-26, PK-27, PK-29 |
| pbrs messages in RPCs whose handlers read every field | instructions per RPC | +10% to +60% | | [adoption program](../adoption/README.md) | PK-27, PK-29 |
| Encoding an 80-byte string field | instructions per op | 219.6 | 203.9 (prost) | [codec profiles](../../evidence/codec-profiles.md) | OP-02 |
| Cloning built messages | instructions | +34% to +66% | | PK-19 notes | PK-19 |
| Generated code and build time | B3, B4, B6 | broad losses | | [scoreboard](../../scoreboard.md) | GN-02, GN-03, GN-11 |
| Gzip unary | instructions | gzip is 49.8% of the cell | | [h2 costs](../../evidence/h2-costs.md) | RX-11 |

The [2026-10-08 record](../../evidence/grpc-readiness-20261008/README.md)
includes functional checks for all five shapes, four payload sizes, h2c/TLS,
identity/gzip, and 1 or 16 calls on one connection. Per-side instruction and
allocation measurements cover only 1 KiB, h2c, identity, and one call. They
retain server instruction and byte losses. The other performance cells,
many-connection loads, read-all corpora, and dedicated-host timings remain open.

## Where the CPU goes

The [SV-09 profile](../../evidence/sv-09.md) identifies large future construction
and copies as current candidates. Earlier scheduler percentages in
[h2 costs](../../evidence/h2-costs.md) are not current per-side attribution;
inclusive instruction shares also include transport and application work.
RX-10a counts library task spawns. Wakeups, channel handoffs, and per-side
instruction attribution remain open under RX-10.

Measure structural changes to futures, task hops, and buffer ownership
against the existing controls before adopting them. [RX-08](../../evidence/rx-08.md)
found only sub-2% local fixes. H2-04 stays blocked unless H2-16 finds a
modeled gain of at least 15%. Owned arenas stay rejected
([decision](../../decisions/owned-arena.md)).

## Phases

| Phase | Cards | Done when |
|---|---|---|
| 0. See every cell | SB-26, SB-27, RX-10 instrumentation | The ledger covers the whole matrix, with per-side wakeup and syscall counts. |
| 1. Flip known losses | SV-09, CL-08, CL-07, CL-09, PK-26, PK-27, PK-29, OP-02, PK-19 | Every seeded ledger row meets the floor in dev-loop. |
| 2. Cut structural cost | RX-10 decisions, SV-08, RX-07, RX-11, RX-12, AD-04 and the cards they file | Primary cells meet the margin in dev-loop. |
| 3. Pass on both message layers | PK-07, PK-25, PK-20, PK-14, GN-02, GN-03, GN-11 | pbrs messages beat prost messages in every codec and RPC cell. |
| 4. Prove on dedicated hosts | SB-28, then SB-22 and SB-16 | Wall-time rows meet the floor with contract statistics on x86_64 and arm64. |
| 5. Keep it won | SB-29 | Every won cell is a blocking gate, and new losses need an owning card. |

## Program rules

- **One product path.** Fix losses in code that production traffic runs.
  Never add benchmark-only paths ([contract §10.3](../../benchmark-contract.md#103-no-benchmark-specific-escape-hatches)).
- **Matched work.** Compare cells only with matched settings (SB-01),
  identical handler work and equal read checksums. Handlers read whole
  messages.
- **Separate sides.** Attribute client and server costs in separate
  processes; a client win cannot hide a server loss.
- **Show every result.** The ledger lists ties and losses beside wins, and a
  loss leaves the ledger only when the rerun that flips it is retained.
- **Escalate.** New dependencies, public API changes, default changes and
  threshold changes need maintainer approval.
