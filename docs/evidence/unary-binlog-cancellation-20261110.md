# Unary binary-log cancellation

When registered, unary client binary logs record an application cancellation
request rather than inferring it from a returned `CANCELLED` status. The
earlier implementation could mistake a peer status for a local request. A
status can also be reused by application codec code, so the cancellation
observation belongs to the call, not the status.

A client `Cancel` event may race a server trailer. The vendored A16 schema
allows later events after cancellation; a winner-only check would lose a real
request when a response wins that race.

## How it works

- Before exposing a call handle, the client selects cancellation logging
  capability from whether the channel has a configured logger. This choice is
  fixed for the call. Methods excluded by that logger's filter still use the
  logging-capable cancellation path, although they emit no records.
- The filter is resolved and a matching call logger is created only when the
  unary future is polled past local preprocessing and concurrency admission.
  A never-polled drop or earlier local failure can therefore omit a
  cancellation event. Explicit cancellation of an excluded call can allocate
  boxed logging state and takes additional registry locks; reused flags can
  retain that state for later calls. Dropping a call without a registered logger
  avoids initializing that state while preserving cancellation and wakeups.
- The logger is registered with the call's cancellation flag. Repeated signals
  claim at most one client `Cancel` event. A cancellation ordered before
  finalization can be emitted after a trailer; a detached handle used after
  finalization cannot create a new event.
- If no initial-header callback has completed and one is active, cancellation
  waits for at least one callback to return. Other hedged callbacks may finish
  later. Header guards remain alive if the hedge coordinator finishes first.
- Wake attempts and sink callbacks run outside the registry lock. Wakeup is
  attempted before the cancellation sink, including when a primary waker
  panics. Ordinary sink panics are caught; a panic payload whose destructor
  also panics is not covered.
- Closing the last sender wakes waiters without recording a cancellation.

The `Sink` contract requires cheap, nonblocking, infallible callbacks.
Logging-capable calls track active wake passes, including reentrant passes.
This does not make arbitrary blocking sinks safe: another wake can begin after
an event is claimed. A pending event can retain its logger while the first
header callback is blocked; that retention has no time bound.

## Validation and limits

`tests/binlog.rs` covers peer `CANCELLED` responses, a reused status,
cancellation during decoding and blocked upload, cancellation before polling,
filtering, call-ID laziness and late handles. `rpc_cancel` tests cover
registration races, wake and header ordering, reentrant and panicking
callbacks, and sender closure. `tests/hedge_binlog_order.rs` exercises
overlapping header callbacks and coordinator finalization on supervised
hedges. Its blocking sinks are bounded test fixtures; production sinks must
follow the nonblocking contract.

The [713 preflight](https://github.com/mingley/pure-protobuf/actions/runs/42739518997)
checked the selected source and its real hedge-order races, assertion-specific
negative controls, Rust 1.85 and documentation. The
[713 endpoint record](../benchmarks/unary-cancel-713-screen-20261110.md)
compares that candidate with the 713 runtime baseline: in its unpolled construct-and-drop
microbenchmark, configured but excluded and enabled logging were slower in
all ten blocks. Those measurements are specific to that workload and runner.
Earlier [rebased endpoint results](../benchmarks/rebased-unary-cancel-screen-20261110.md)
cover 1e-based no-logger and excluded-method calls. Two early-filter
alternatives were measured separately; the no-capture version slowed excluded
held-call construction in every measured block without a decisive endpoint
win. Those [experiments](../benchmarks/unary-filter-no-capture-screen-20261110.md)
are not the selected implementation.

The [layout and allocation record](../benchmarks/unary-filter-layout-20261110.md)
measured the cancellation `Inner` at 80 bytes versus 72 for the earlier
baseline on x86-64. In a separate retained-call construction check, each
additional live unpolled call requested 5,728 bytes versus 5,720 at baseline.
These observations do not measure peak memory or drop-time logging-state
allocation.

A16 support remains partial. Unary cancellations before logger registration
may be omitted, streaming can still misattribute a peer `CANCELLED`, retry or
hedge logs can include uncommitted attempt events, and early upload rejection
trailers can be omitted. These gaps need separate implementation and
qualification before claiming full A16 conformance.
