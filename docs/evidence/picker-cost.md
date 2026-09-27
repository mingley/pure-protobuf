# Picker cost qualification (CH-11)

Every shipped LB policy has a dev-loop cell measuring steady-state pick
cost, and every cell passes the absolute budget in
[`picker.json`](../../bench/devloop/baselines/picker.json): 0 allocations
and 0 blocking lock waits per pick. The perf lane (`perf.yml`) runs the
full suite and fails on any budget breach (`devloop compare --budget`).

## Cells

One cell per policy, all following the CH-10 harness: the policy is
preloaded to its ready steady state outside the allocation guard, then
the guarded loop runs one pick per op.

| Cell | Steady state |
|---|---|
| `lb.pick_first.pick` | Single ready address (passthrough-equivalent hot path) |
| `lb.round_robin.pick` | 3 ready addresses |
| `lb.weighted_round_robin.pick` | 3 ready addresses, hourly weight recompute (scheduler builds once in warmup; the cadence is deployment-tunable, default 1s) |
| `lb.ring_hash.pick` | 3 ready addresses, varying request hash per op |
| `lb.least_request.pick` | 3 ready addresses, default choice count 2 |
| `lb.priority.pick` | Single priority over a round_robin child, flat 3-address update |
| `lb.outlier_detection.pick` | round_robin child, hourly sweep interval, nothing ejected |
| `lb.random_subsetting_experimental.pick` | Subset size 2 over 3 addresses, round_robin child |

Wrappers use a round_robin child: wrapper-under-wrapper nesting is
parse-rejected, so a leaf child is the only runtime shape.

## What the harness caught

The baseline (before CH-11 fixes) allocated on 6 of 8 cells:

| Policy | Allocs/pick before | Cause | Fix |
|---|---|---|---|
| round_robin | 0 | — | — |
| weighted_round_robin | 1 | `ready` Vec collected per pick | Fast path: when nothing is down/unhealthy/pending the ready set is the address list; degraded picks keep the slow path |
| ring_hash | 1 | Per-pick `seen` HashSet | Removed: usability is pure over the locked state, so a repeated ring entry re-checks to the same answer; the address clones once, on return |
| least_request | 1 | `ready` Vec collected per pick | Walk to the r-th ready address instead of indexing a collected Vec (identical sampling distribution) |
| priority | 5 | Slot-list clone + observation Vec + boxed child pick/readiness per pick | Index iteration with split borrows in `choose`; leaf-direct child dispatch |
| outlier_detection | 2 | `live` Vec per pick + boxed child pick | Fast path when nothing is ejected; leaf-direct child dispatch |
| random_subsetting | 1 | Boxed child pick | Leaf-direct child dispatch |

The `Box::pin` on wrapper child calls exists to break the recursive
future type (wrappers recurse through the `LbPolicy` enum, E0733).
`LbPolicy::{pick_direct, pick_hash_direct, readiness_direct}` box only
wrapper arms and await leaf arms directly; since nesting is
parse-rejected, the box never fires at runtime and leaf children pick
with zero allocation. Behavior is unchanged: the full `pbrs-grpc`
suite (385 lib + integration targets) passes unmodified.

## Reading the numbers

- `allocs` / `locks` are absolute ceilings that hold on every host.
- `instructions` stays relative (base-vs-head compare): uncontended
  mutex acquisitions are pure atomics and land there, and counts
  differ per microarchitecture.
- `locks` counts blocking futex waits via strace on Linux CI; macOS
  runs report `not_run` and the budget check skips them. A hot path
  must never block, so the ceiling is 0 where measured.
- Degraded paths (failover, ejection, scheduler rebuilds, failure
  backoff) may allocate: the budget covers the steady-state pick,
  which is what every RPC pays.

Reproduce locally:

```sh
scripts/devloop.sh --cells lb.pick_first.pick,lb.round_robin.pick,lb.weighted_round_robin.pick,lb.ring_hash.pick,lb.least_request.pick,lb.priority.pick,lb.outlier_detection.pick,lb.random_subsetting_experimental.pick --iters 2000 --repeats 2 --out /tmp/picker.json
bench/devloop/target/release/devloop compare --baseline /tmp/picker.json --current /tmp/picker.json --budget bench/devloop/baselines/picker.json
```
