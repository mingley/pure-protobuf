# Server dispatch decision (SV-03)

## Decision

Keep the current per-RPC spawn model for now. Do **not** inline arbitrary
handler dispatch on the HTTP/2 connection task in this slice.

## Evidence

SV-01/SV-02 found two exact allocation wins in request construction and
response finalization without changing scheduling:

- `rpc.pbrs.unary`: 57.335 -> 55.340 allocs/RPC.
- `rpc.pbrs.server_stream`: 74.335 -> 72.335 allocs/RPC.

The remaining per-RPC spawn is a plausible cost, but the available macOS
profile for `rpc.pbrs.unary` was dominated by scheduler and IO waits:

| symbol | share |
|---|---:|
| `__psynch_cvwait` | 92.5% |
| `kevent` | 2.7% |
| `writev` | 2.4% |
| `__recvfrom` | 1.6% |

This profile does not provide enough CPU attribution to justify a scheduling
architecture change.

## Rationale

Polling user handlers on the connection task can improve spawn/task-allocation
cost only if the handler is known to be fast and nonblocking. Today the public
`Service::call` API returns an arbitrary async future, and handlers may:

- perform application IO;
- wait on timers;
- receive streaming messages;
- spawn or coordinate side work;
- hold interceptor-visible context and metadata for arbitrary durations.

Inlining that future on the connection task would create head-of-line risk for
other streams on the same HTTP/2 connection unless a cooperative budget and a
safe fallback-to-spawn policy are designed and tested.

The current spawn boundary preserves fairness and cancellation semantics by
keeping connection progress independent from user handler progress.

## Follow-up design requirements

A future SV-03 implementation can be reconsidered only with:

1. a deterministic fairness test proving a slow unary handler cannot stall
   unrelated streams beyond a documented budget;
2. a cooperative polling budget and fallback-to-spawn mechanism;
3. dev-loop evidence showing D2 allocation/throughput improvement with no
   primary p99 regression beyond 5%;
4. targeted lifecycle/cancellation/resource-bound tests.

## Status

Rejected for this wave; recorded as a measured no-adoption decision. The server
allocation wins from SV-02/SV-05 were adopted instead.
