# Frozen collector scope audit (read-only)

Source: clean measured `ab62da78eb48b4e3cdc6dbdd44555a9dfe656cf3`.
No collector, fixture, runtime, threshold or campaign input changes and no
additional heavy runs were performed for this audit.

| Counter | Captured process and phases | Report calculation |
|---|---|---|
| Instructions | Whole Callgrind child, starting at process startup; runtime/setup, warmup, measured loop, JSON and teardown all contribute | `(I(2N)-I(N))/N`, then median of three repeats |
| Allocations/bytes and copy counters | Global process counters bracketed by `AllocGuard`; resets after the fixed warmup, stops before JSON; all worker threads share the guard | N child's bracketed total/N, then median |
| Syscalls / legacy `locks` | A separate bare child under `strace -c -f`, from exec through exit and every thread; runtime/setup, warmup, loop, JSON/teardown contribute | Whole strace summary total/N, then median; no N/2N subtraction |

References in `bench/devloop/src/main.rs`: tool invocation 1811–1832,
N/2N collection/subtraction 1904–1933, separate strace run 1944–1959,
normalization 2018–2031, allocation guard 80–101, primary warmup/loop/stop
1532–1570, runtime creation 2268–2274.

The collector starts Callgrind with its default whole-process instrumentation;
there are no start/stop client requests. `AllocGuard` changes only allocator and
copy-counter state, not Callgrind or tracing. Differential subtraction removes
fixed work only approximately; thread scheduling/teardown need not be constant.
The original RPC runtime uses Tokio's default worker count. The separate
`rpc.adoption.*` branch explicitly chooses two workers; that difference is not
silently applied to the old primary.

The selected `rpc.prost.server_stream` helper always performs exactly **one**
fixed warmup RPC. The old helper does not consume parsed `--warmup` or
`--prepare-iters` values. Both CG children receive preparation=400, but that
argument does not create an extra preparation phase for this fixed-payload RPC.
The separate strace child receives N only. The allocator-only adoption ledger's
warmup100/preparation128 protocol is a separate collector path and remains as
recorded in its evidence.

`bench/devloop/src/counters.rs:37–59` reads the `calls` column of the strace
summary, sums `futex` and `futex_waitv`, and ignores operation, result, error,
waiting duration and caller stack. Wake calls and unsuccessful waits count.
The legacy field cannot identify blocking lock acquisitions or application-lock
contention. Both client/server tasks, Tokio workers and process lifecycle
contribute. This explains the scope; it does not prove which callers caused
any measured drift.

The transparent `capture-callgrind.py:20–28` records only Callgrind invocations.
Six reports × three repeats run **54 benchmark processes**: 36 CG N/2N children
whose complete raw stderr/graphs are retained, plus **18 separate strace N
children whose individual raw summaries are not retained** by the frozen
collector. Reports retain syscall/futex medians and compare verdicts, and the
parent report's immutable binary guards bracket those trace launches. However,
there is no individual raw trace command/status/summary to reconstruct those
medians or distinguish wake/wait/error operations independently. Successful
measured metrics imply the collector accepted three successful valid summaries;
that inference is not a retained raw exit record. Tool-probe processes are
outside this benchmark-child count.

This is an additional explicit provenance gap alongside the unchanged-control
instability. The original exit1 and unchanged candidate exit1 remain failed;
the baseline unchanged exit0 still fails absolute1% stability. No failures are
removed, budgets waived, favorable report selected, alternate window introduced
or new runtime win qualified. A future separately reviewed collector campaign
must retain trace evidence and specify counter scope; this audit does not alter
the original comparator or prescribe replacement thresholds.
