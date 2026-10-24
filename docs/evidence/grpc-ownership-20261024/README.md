# Ownership, batching and current costs

Current source: `933bf8775b70d24d91fb46a7992eae343b60f0ea`. Safety-only source: `ca4ef720f8a90134334101e0106a8a68808fc19f`.
Before: `7c02eb387a6785d6e914146babe34f0902a6dd62`. Linux x86_64, Rust 1.99.1, shared host.

The repair moves completed DATA owners outside backend locks, preserves reset
ownership before response-shape selection and rejects malformed protobuf group
ends. Batching reduces singleton reservations. Current shipping unary-server CPU,
large gzip bytes and some bulk/source costs still lose. These results do not prove
production readiness or broad superiority.

## Validation

| Check | Actual result |
|---|---|
| Full all-feature workspace, clean `933bf877` | **3,840 passed / 0 failed / 9 existing ignores**, 145 summaries; 1,350.798 seconds |
| ASAN/leak detection, 13 selected ELFs | **961 passed / 0 failed / 1 existing ignore** |
| Official v35.1 conformance, maximum Edition 2023 | Required twice and recommended once: each **5,631 binary/JSON + 909 text successes**, zero skipped/expected/unexpected failures |
| Strict lint/fmt | Precommit source snapshots pass; final Git-blob equivalence retains their actual source labels |
| Warning-strict rustdoc | Clean current source passes in 79.402 seconds |
| Rust 1.85 core / 1.87 optional Zstd | Clean current source passes in 174.605 / 94.340 seconds |

Workspace before/after proofs match **3,797 physical tracked blobs, modes and
sizes, 910,511,102 bytes**. A separate equivalence record binds all five tested
batch files and twenty kernel/generated files to final Git blobs; it does not
rename dirty precommit executions as clean committed runs. Test scopes overlap.
Selected sanitizer tests do not add distinct full-workspace test counts.

## Current costs

[The benchmark tables](../../benchmarks.md) give all thirty instruction scopes and
selected gains/losses. Bytes below are allocation requests/RPC, not RSS.

| Side | Workload | Current | Fresh tonic/prost | Difference |
|---|---|---:|---:|---:|
| Client | Bidi 1 KiB identity | 42.40 KiB | 40.11 KiB | +5.70% |
| Server | Bidi 1 KiB identity | 31.32 KiB | 32.95 KiB | −4.94% |
| Client | Bidi 1 MiB gzip | 19.41 MiB | 16.67 MiB | +16.40% |
| Server | Bidi 1 MiB gzip | 18.40 MiB | 15.67 MiB | +17.43% |

Safety→current singleton bidi saves **21.28% client / 27.92% server** requested
bytes. The conditional four-singleton model predicts 12,480 / 12,420 bytes per
RPC only if each message flushes individually. Actual paired reductions are
11,395.76–12,080.36 client / 12,415.52–12,433.76 server; no exact allocation-site
attribution follows.

Bulk has tradeoffs: 64-message pipelined server bytes increase **1.15% / 26.75%**
in the two repeats versus Safety, and **15.67% / 26.90%** versus fresh tonic.
Those are repeat ranges for one server scope, not client/server percentages.
Source instruction medians rise in 22 of 30 scopes versus before; unary server
rises 17.08% and client-stream server 17.66%. Every increase and mixed repeat stays
in the complete ledgers, including values below the 2% instruction band.

All thirty current native instruction groups are lower than fresh tonic in both
repeats, with median savings of 3.24–55.77%. **Shipping CPU remains a loss:** the
quiet four-repeat unary replay has native raw CPU/call at N=10,000 of
57/60/53/54 µs versus tonic's 41/41/45/40. All four raw pairs lose. N/2N slope
ratios are 1.55/1.79/1.00/1.37, retaining one equal pair. The earlier 48-capture
run had 2.08–2.89× slopes and 13.6–18.3% lower throughput; that slope magnitude
does not repeat. Replay timing is mixed and unqualified. Lower instruction and
allocation counts do not resolve the physical CPU loss.

## Measurement scopes

| Phase | Captures | Successful RPCs | Scope |
|---|---:|---:|---|
| Native counters | 576 | 21,600 | Three sources, N25/50, two paired repeats |
| Callgrind | 540 | 8,100 | Three sources, N10/20, two paired repeats |
| Separate bulk | 240 | 9,000 | Safety/current; 1/2/3/4/64-message partitions |
| Separate shipping builds | 48 | 360,000 | Counters/profilers off; N5000/10000 |
| Separate quiet unary replay | 16 | 120,000 | Same source/settings; four paired repeats |
| Separate selected syscalls | 8 | 12,000 | Traced, timing unqualified |
| Separate fd-aware traces | 8 | 600 | Own-child traces, timing unqualified |

Native plus Callgrind total **1,116 captures / 29,700 RPCs**. Other phases stay
separate. Actual offered/dispatched/completed/successful counts reconcile with
zero RPC failures, timeouts, overflow, unstarted or unfinished calls. Tracing
errors/restarts and unresolved calls at SIGTERM teardown are separate from RPC
accounting and remain in the raw traces.

Both endpoints use Prost for these transport comparisons. Gzip uses level 6,
zero-filled payloads and four streaming messages; server send buffers are
400 KiB, queues 16. The older pbrs interop results include different handler work
and are not substituted into this comparison. Current cold-process, entropy,
read-all, overload, physical-network and broader platform coverage remain open.

## What the repairs establish

The owner baseline receives valid output/status and recovers on the same
connection, but its copied and chained DATA owners cannot reenter the engine
during Drop. The repair retains original retired frames in a fixed outer batch;
locks exit before owners are destroyed, including the tested staging-unwind and
IO-error paths. Credit/EOS, partial tails, exactly-once Drop and many-frame controls
remain. A full retirement batch does not itself force a socket write or flush.
Generic current-frame Buf operations/panic owners and other legacy queues remain
separate audit work. The private scratch value sizes are layout diagnostics,
not heap requests, machine stack-frame size, RSS or measured speed.

Reset guards retain the selected peer/local cause through pre-shape cancellation
and unwind. Protobuf validation now passes decoded field numbers into a bounded,
allocation-free unknown-group skipper. Its five baseline structural failures
become nine passing root controls; valid nested groups, opaque unknown LEN bytes,
max field numbers and the shared recursion limit remain covered. Legacy numberless
low-level skip APIs keep their historical behavior. Required split-merge and eager
owned read-all design remain separate work.

## Failures and chronology

The raw record retains original reset/owner failures, stronger-owner compile and
fixture-EOF failures, then their explicit successors. The kernel's first test
attempt has 3 passes/6 failures: one opaque-LEN control accidentally used known
field 99. Corrected test-only tags produce 4 passes/5 real failures before the
repair and 9 passes after it. No earlier outcome is relabeled as passed.

The native controller captured all 576 successful rows, then exited **1** because
sparse source checkouts omitted plan files needed for ledger owner annotations.
Root materialized the exact Git blobs. A separate recovery validates all raw
counts/stdout/build pins, preserves four existing reports, reconstructs only two
missing reports and recomputes three ledgers. Its state is explicitly recovered,
with original_controller_passed=false. All 3,486 original input refs remain unchanged.

## Scheduling trace and remaining CPU question

Four native-server fd traces contain WAIT_BITSET_PRIVATE waits and zero
WAIT_PRIVATE entries matching the engine-lock acquisition path. Extra writes
resolve to eventfd notifications. TCP operation counts stay near two/RPC on both
peers; adjacent TCP operations change threads more often on native. These are
observations under tracing/background work, not task-wake/migration counts or a
complete CPU cause. A later [separate ready-set experiment](../grpc-ready-set-20261024/README.md)
passes its controlled same-parent-poll test and 360,000 RPCs, but has mixed
physical CPU results. It is not adopted and does not inherit this source
qualification; runtime `933bf877` remains unchanged.

## Resource days

Fresh `933bf877` release previews pass **34.370 / 30.796 measured seconds**. The
deadline preview completes 31 cycles of the same 30 cases, including the three
pre-shape reset cases in its shared paused-clock process. Repetition does not
create distinct tests.

Both 24-hour runs are **running, results pending**, in the closed **2026-10-24
22:48:16 UTC** observation. General cycles advance 43→44 and deadline cycles
128→132. The last observed general cleanup has zero active calls, balanced
started/ended counts and zero byte tokens; deadline cleanup has FD 8, tasks 0,
retained cache 0 and balanced leases. Independently read kernel limits are
1 GiB address space, 128 FDs, 4,096 same-UID processes and 86,430 CPU seconds.
Boot, PID start ticks, generation and fresh heartbeats match.

Retained ELF hashes and execution argv are verified. Reading either process
mapped executable through `/proc/PID/exe` is permission-denied, so that separate
observation remains unavailable. The archive keeps dated snapshots by value,
not hashes of advancing logs. There is no terminal day result or host-retention
guarantee; these observations do not establish a 24-hour pass.

Earlier `7c02eb38` workers and publishers are absent. Preserved progress stopped
before the currently observed boot, giving strong environment-discontinuity
evidence. Exit cause is unavailable; no application failure or 24-hour pass is
established. No attempt is resumed or treated as running from a stale JSON state.
The older `beb4ecd9` completed days qualify only their own source/scopes.

## Reproduce and verify

[Results](results.json) link exact source/build/tool hashes, commands, complete
ledgers, profiles, failures, physical-checkout proofs and tracing records. The raw
archive omits ELF bytes and duplicate old C12 capture trees; retained build/ELF
hashes identify them. Source snapshots and helper generations remain distinct.

```sh
python3 check.py
```

[Manifest](manifest.json) · [Raw sources, commands, logs, counters and profiles](raw.tar.gz)
· [Execution decision](../../decisions/grpc-execution.md)

The checker verifies regular archive members and hashes without extracting files.
Complete current-source sustained resources, CPU/byte/bulk improvements, remaining
features/specification coverage, platforms/consumers/releases and controlled
per-side statistics before claiming production readiness or an overall ranking.
