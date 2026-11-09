# gRPC Retry Safety & Commitment Boundaries Contract

Transparent retry requires proof that the server application never processed
the request. Once an attempt enters `BodyStarted`, an ambiguous connection
failure cannot be retried transparently. An explicit service-config policy
may allow re-execution; the application must make that safe.

This contract defines the implementation states and evidence needed to enforce
that distinction. For application setup, start with the
[gRPC guide](grpc.md) and [service-config contract](service-config.md).

**Task:** RT-01 ("Specify and reproduce retry commitment boundaries")  
**Pinned Standards:** gRFC A6 (Client-side retry support in gRPC) at `grpc/proposal@6342be729b96478a2897ceb208a8cddcd832a17b` (per [plan pins](plan/README.md)), RFC 7540 / RFC 9113 (HTTP/2)  
**Work Package:** RT Lane (Reliability & Retry Correctness), Wave 0  
**Test Harness:** `pbrs-grpc/tests/retry_safety.rs`  

---

## 1. Overview and Problem Statement

gRPC has two retry mechanisms:

1. **Policy-based retries (service config):** per-method retries for methods
   that are explicitly idempotent or retry-safe. The policy bounds attempts
   and hedging, and it names retryable gRPC status codes such as
   `UNAVAILABLE`.
2. **Transparent retries:** automatic transport-kernel retries that need no
   user configuration. They are permitted **only when the client can prove that
   server application logic never saw or processed the request**.

The central hazard is **ambiguous connection loss**. If an HTTP/2 connection
or TCP socket drops after request bytes were sent, no response does **not**
mean no execution. The server may already have processed the request,
committed database mutations, charged a customer, or triggered external side
effects.

Before RT-01, the `pbrs-grpc` client kernel treated "transport connection
died" as "safe to transparently retry." This page defines that boundary,
records the baseline bug and current hedge limitation, maps gRFC A6 commitment
rules, and describes the transport-evidence replay rules for RT-02.

## 1b. Shipped policy engine (unary)

`Channel::service_config` attaches the A6 document.
`ServiceConfig::parse` validates it eagerly per A21. Unary calls resolve the
method entry, then apply the configured policy:

- `retryPolicy` retries a failed attempt while attempts remain, the status code
  is in `retryableStatusCodes`, throttling allows it, and no `DoNotRetry`
  pushback arrived. Backoff is jittered exponential. A `Delay` pushback
  overrides the backoff. In a valid parsed gRPC status block, negative,
  unparseable, non-ASCII, out-of-range, or repeated pushback values stop
  further retries. The parser also accepts redundant leading zeros, which
  differ from the pinned A6 wire syntax.
- `perAttemptRecvTimeout` bounds each attempt. When it expires, the attempt
  retries on its own, without consulting the retryable status set.
- `hedgingPolicy` sends duplicate unary attempts. The first completed `OK`
  or fatal status wins, and an exhausted race fails with the last non-fatal
  error. Ordinary response headers do not yet commit the hedge race: another
  attempt can continue or win afterward. When attempts remain and throttling
  permits, each non-fatal response without pushback advances the next send
  immediately. Non-negative pushback schedules the next send after that delay;
  a later in-flight response can replace the schedule. Once processed,
  `DoNotRetry` stops new policy hedges; already-started attempts can complete
  or transparently redial under the existing limits. Cancellation and the
  overall deadline interrupt the wait. If another outcome, including
  pushback, is queued while a send waits for the throttling check, the new
  hedge can be sent before that outcome is processed. With an unset hedge delay, initial fanout checks
  throttling before the event loop; cancellation and deadline expiry can wait
  for that check.
- `retryThrottling` debits every failed unary call and credits every success.
  Retries and hedged sends past the first need more than half the bucket.
- Transparent retry runs first, is at most once per attempt, requires proof
  of non-execution before response commitment, and does not consume a policy
  attempt. Each hedged attempt retains its own response commitment when
  deciding whether to redial; ordinary headers still do not commit the entire
  hedge race as described above.

Policy retry replays the already encoded request frame. It does not
re-serialize the request, and it does not exceed the method's caps.

Server-streaming policy retry replays from already buffered request bytes with
the same unary decision helper. Ordinary initial response headers commit the
attempt before any response message; valid non-OK gRPC trailers-only failures
can remain eligible for policy retry. Streaming tests cover both accept shapes:

- trailers-only failures take the retry path;
- stream-created-then-cancelled matches the explicit
  streaming-transport-cancel verdict.

Client-streaming and bidirectional streaming stay call-site retries because
the kernel has no replay buffer.

Every policy outcome (unary attempts, hedging sends, and streaming calls)
records into channel-scoped lock-free `RetryStats` counters. They are exported
through `Channel::retry_stats` for GF-01/A96 channelz and OpenTelemetry (OTel)
export.

Streaming throttling accounting is approximate. Credit and debit tap the
headers read because the receipt of an `OK` status is read through the
unobservable transport at commit. A6-faithful streaming throttling accounting
is explicit follow-up work.

---

## 2. Call and Attempt Lifecycle State Machine

Sequential unary and server-streaming attempts move through these lifecycle
states. The hedging limitations above still apply.

```
       [ Client Initiates RPC ]
                  │
                  ▼
              (Queued) ───[ Connection-attempt failure ]───► [ Transparent retry may be eligible ]
                  │
                  ▼ (Stream admitted; request HEADERS queued)
            (HeadersSent) ───[ REFUSED_STREAM / GOAWAY last < id ]───► [ Transparent Retry Safe ]
                  │
                  ▼ (Enter request-body send phase)
            (BodyStarted) ───[ Ambiguous connection loss ]───► [ No transparent retry ]
                  │  └──[ Explicit non-processing evidence ]──► [ Transparent retry may be eligible ]
                  │
                  ▼ (Client receives ordinary initial response HEADERS)
         (ResponseCommitted) ───[ Stream error / Conn drop ]───► [ No automatic retry ]
                  │
                  ▼ (Receive DATA & trailers)
          (TrailersReceived) ───► [ Terminal State ]
```

### State Definitions

1. **`Queued`:**
   - The RPC exists and waits for channel readiness, connection establishment,
     Transport Layer Security (TLS) handshake, or stream concurrency limits
     (`SETTINGS_MAX_CONCURRENT_STREAMS`).
   - *Commitment:* Uncommitted. No request `HEADERS` or `DATA` have been sent
     for this attempt.
2. **`HeadersSent`:**
   - The HTTP/2 transport has accepted the request `HEADERS`
     (`:method = POST`, `:path = /<service>/<method>`, `:scheme`,
     `:authority`, metadata). They may still be queued for transmission.
   - An HTTP/2 stream ID has been allocated.
   - *Commitment:* Conditionally uncommitted. If the remote peer responds with
     `REFUSED_STREAM` or `GOAWAY` showing the stream was never processed, the
     attempt remains uncommitted.
3. **`BodyStarted` (Request-Body Send Phase):**
   - The client has entered the request-body send phase. Data may be queued,
     blocked by flow control, or already on the wire.
   - Ambiguous loss in this phase does not prove non-execution and cannot
     trigger transparent retry. Explicit peer evidence of non-processing can
     still allow it before response commitment, subject to retry and deadline
     limits.
4. **`ResponseCommitted`:**
   - In sequential unary and server-streaming calls, ordinary initial response
     headers commit the attempt against policy and transparent retry.
   - A valid non-OK trailers-only response is a terminal status and may remain
     eligible for policy retry. It does not by itself prove non-execution for
     transparent retry.
   - In sequential paths, the current client conservatively treats non-200
     responses as committed, including some HTTP fallback responses other
     clients treat as trailers-only.
5. **`TrailersReceived`:**
   - The client has received `grpc-status` and optional `grpc-message` or error
     details in trailers or a trailers-only response.
   - Terminal state of the RPC attempt.
6. **`Cancelled`:**
   - The caller cancelled the RPC (`CallHandle::cancel`) or the request
     deadline expired (`DEADLINE_EXCEEDED`).
   - Terminal state. Must never trigger retry.

---

## 3. Pre-Fix Baseline Analysis (`pbrs-grpc/src/client.rs`)

This section records the exact pre-RT-02 baseline outcome. Section 6 records
the post-fix assertions.

### Baseline Implementation
Before RT-02, `unary` and `server_streaming` in `pbrs-grpc/src/client.rs` executed the following loop:

```rust
let mut retried = false;
loop {
    let live = channel.grab(cancel_rx.clone(), deadline, wait).await?;
    let (slot, gen) = (live.slot, live.gen);
    match run_unary(...).await {
        Err(status)
            if !retried
                && status.is_transport()
                && channel.inner.endpoint.can_redial() =>
        {
            retried = true;
            channel.inner.discard(slot, gen).await;
        }
        result => return result,
    }
}
```

In `pbrs-grpc/src/status.rs`, `status.is_transport()` is defined as:
```rust
pub(crate) fn from_h2(err: impl Into<h2::Error>) -> Self {
    let err = err.into();
    let mut status = Self::unavailable(err.to_string());
    if h2_lost_connection(&err) {
        status.mark_transport();
    }
    status.with_cause(err)
}

fn h2_lost_connection(err: &h2::Error) -> bool {
    err.is_io() || err.is_go_away() || err.reason() == Some(h2::Reason::REFUSED_STREAM)
}
```

### The Safety Flaw
`run_unary` and `run_server_stream` first called `open(...)`, which sends
`HEADERS`. They then immediately called `send_bytes(...)`, which sends the
entire serialized request `DATA` frame with `end_stream = true`, and awaited
`resp_fut.await`.

If the connection dropped while awaiting `resp_fut.await` (TCP reset, broken
pipe, remote peer crash, or network timeout), this happened:

1. `h2::Error` was classified as `err.is_io() == true`.
2. `Status::from_h2` called `status.mark_transport()`.
3. `status.is_transport()` returned `true`.
4. `unary` and `server_streaming` discarded the dead connection slot, redialed
   a new TCP/TLS connection to the server, and **re-transmitted the request
   frame**.
5. If the server application handler had received the first request and
   started execution, the server executed the handler a **second time**.

### Empirical Proof in Baseline Tests
In `pbrs-grpc/tests/retry_safety.rs`, deterministic test fixtures reproduce this exact regression:
- **`scenario_b_unary_drops_after_request_dispatched_baseline_retries`**:
  The server receives the request, increments an atomic execution counter from
  0 to 1, and abruptly drops the TCP connection before sending response
  headers. The client catches `is_transport()`, redials, and executes again.
  The counter becomes **2**.
- **`scenario_b_server_streaming_drops_after_request_dispatched_baseline_retries`**:
  Server-streaming has the same outcome; the execution counter becomes **2**.

### Contrast with Streaming RPCs
`client_streaming` and bidirectional (`bidi`) calls use `open_retrying()`.
That helper only retries when `open()` fails *before* request `DATA` frames are
sent. Once `open()` returns, the caller-held `StreamSender` takes over and no
automatic transport retry is attempted.

---

## 4. Official gRFC A6 Transparent Retry Rules

Under [gRFC A6](https://github.com/grpc/proposal/blob/6342be729b96478a2897ceb208a8cddcd832a17b/A6-client-retries.md)
(pinned `grpc/proposal@6342be7`), strict eligibility rules govern transparent
retries.

### Permitted Transparent Retries
Transparent retry requires positive proof that the server application never
processed the request. It remains subject to retry, replay and deadline limits.
Examples of non-processing evidence include:

1. **Pre-Stream / Connect Failures:**
   - Failure to acquire a connection or stream slot.
   - Handshake failure (connection refused, DNS failure, TLS negotiation failure).
   - Connection closed by peer while idle in the pool, before request
     `HEADERS` were dispatched.
2. **HTTP/2 `REFUSED_STREAM`:**
   - RFC 7540 §8.1.4 explicitly specifies:  
     *"REFUSED_STREAM: The endpoint refused the stream prior to performing any application processing on the stream."*
   - Transparent retry on `REFUSED_STREAM` is guaranteed safe even if request
     data was transmitted.
3. **HTTP/2 `GOAWAY` with `last_stream_id < stream_id`:**
   - If the server sends a `GOAWAY` frame where `last_stream_id` is less than
     the stream ID allocated for this RPC, HTTP/2 guarantees the server never
     processed the stream.

### Prohibited Transparent Retries
Transparent retry is **strictly forbidden** in all of the following conditions:

1. **Ambiguous Connection Loss During the Request-Body Phase:**
   - Once the request-body send phase begins, a transport error without proof of
     non-processing is ambiguous. Treat the request as potentially executed; it
     must **not** be transparently retried.
2. **Response Headers Received (`ResponseCommitted`):**
   - For sequential attempts, ordinary initial response `HEADERS` commit the
     RPC. Later connection loss or stream resets fail the call. A trailers-only
     terminal status is handled separately.
3. **Stream Resets with Other Error Codes:**
   - `RST_STREAM` with `CANCEL`, `INTERNAL_ERROR`, `PROTOCOL_ERROR`, or
     `STREAM_CLOSED` does not prove non-processing. It cannot by itself trigger
     transparent retry.
4. **Explicit Application Error Statuses:**
   - A `grpc-status`, including `UNAVAILABLE`, does not prove the application
     did not process the request. The status alone cannot trigger transparent
     retry; a configured policy may evaluate it for policy retry.
5. **Caller Cancellation or Timeout:**
   - Client-side `CallHandle::cancel` or local deadline expiration immediately terminates the RPC.

---

## 5. Transport-Evidence-Based Replay Rules for RT-02

For task RT-02, `pbrs-grpc` must implement this design:

### 1. Classification of Transport Errors
Replace the single boolean `status.is_transport()` with fine-grained transport evidence:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransportEvidence {
    /// Failure occurred before request HEADERS were dispatched (e.g. idle pool drop).
    PreHeaders,
    /// Remote peer explicitly sent HTTP/2 REFUSED_STREAM.
    RefusedStream,
    /// Remote peer sent GOAWAY with last_stream_id < stream_id.
    GoawayUnprocessed,
    /// Connection dropped after request bytes were sent without proof of non-execution.
    AmbiguousLoss,
}
```

### 2. Transparent Retry Decision Matrix

Eligibility also depends on retry and replay availability, cancellation and
remaining deadline. `YES` means the evidence permits a retry, not that a retry
is guaranteed.

| State at Failure | Error Cause | Transparent Retry Eligible? | Reason |
|---|---|:---:|---|
| `Queued` / Connect | `ECONNREFUSED` / connection-attempt timeout | **YES** | Server never saw request |
| `Queued` (Idle Slot) | Server closed idle connection | **YES** | Request never written to socket |
| `HeadersSent` / `BodyStarted` | `REFUSED_STREAM` | **YES** | RFC 7540 §8.1.4 non-processing guarantee |
| `HeadersSent` / `BodyStarted` | `GOAWAY (last < stream_id)` | **YES** | RFC 7540 §6.8 non-processing guarantee |
| `BodyStarted` | IO Error / Broken Pipe / TCP Reset | **NO** | Ambiguous: server may have executed |
| `BodyStarted` | `RST_STREAM(CANCEL / INTERNAL)` | **NO** | Reset does not prove non-processing |
| `ResponseCommitted` | Any stream / transport error | **NO** | Response headers already arrived |
| Any | Local cancellation / Deadline | **NO** | Caller cancelled or timed out |

### 3. Never Promise Exactly-Once Semantics
Transport-level protocols cannot deliver exactly-once semantics across network
partitions. If a server receives a request, commits mutations, and crashes
before sending headers:

- At-most-once safety requires the client to **fail fast**
  (`Status::unavailable("connection lost after request transmission")`) rather
  than replaying.
- Exactly-once guarantees require application-level idempotency tokens or
  deduplication layers, such as transactional deduplication tables keyed by
  request UUID. They do not come from transport replay.

---

## 6. Regression Baseline & Verification Matrix

The test suite in `pbrs-grpc/tests/retry_safety.rs` establishes the
verification baseline:

| Test Scenario | Condition | Verified Behavior | Regression Baseline Role |
|---|---|---|---|
| **Scenario A1** (`scenario_a_connection_refused_wait_for_ready_retries_safely`) | `ECONNREFUSED` on initial dial with `wait_for_ready` | Connect retries until server starts; execution counter = 1 | Verifies safe transparent retry before headers |
| **Scenario A2** (`scenario_a_refused_stream_retries_safely`) | Server sends HTTP/2 `REFUSED_STREAM` on attempt 1 | Client transparently redials to attempt 2; execution counter = 1 | Verifies safe transparent retry on explicit refusal |
| **Scenario B (Unary)** (`scenario_b_unary_drops_after_request_dispatched_baseline_retries`) | Server receives request, increments counter to 1, drops connection before headers | Client redials; server executes attempt 2; counter reaches 2 | **Regression reproducer**: Proves unsafe duplicate execution baseline in RT-01. RT-02 will assert counter remains 1. |
| **Scenario B (Streaming)** (`scenario_b_server_streaming_drops_after_request_dispatched_baseline_retries`) | Server receives request, increments counter to 1, drops connection before headers | Client redials; server executes attempt 2; counter reaches 2 | **Regression reproducer**: Proves unsafe duplicate execution on server-streaming. |
| **Scenario C (Unary)** (`scenario_c_unary_response_headers_committed_no_retry_on_stream_error`) | Server sends response headers (response committed), then `RST_STREAM` | Client returns `Err(Status::unavailable)`; counter remains 1 | Confirms no retry after response commitment on unary |
| **Scenario C (Streaming)** (`scenario_c_server_streaming_response_headers_committed_no_retry`) | Server sends response headers + 1 item, then `RST_STREAM` | Client returns `Err(Status::cancelled)`; counter remains 1 | Confirms no retry after response commitment on streaming |

All 9 test cases in `pbrs-grpc/tests/retry_safety.rs` pass deterministically.
Scenarios A-C are the RT-02 regression harness; scenario D covers RT-03.
