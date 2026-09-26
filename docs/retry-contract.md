# gRPC Retry Safety & Commitment Boundaries Contract

**Task:** RT-01 ("Specify and reproduce retry commitment boundaries")  
**Pinned Standards:** gRFC A6 (Client-side retry support in gRPC) at `grpc/proposal@6342be729b96478a2897ceb208a8cddcd832a17b` (per [plan pins](plan/README.md)), RFC 7540 / RFC 9113 (HTTP/2)  
**Work Package:** RT Lane (Reliability & Retry Correctness), Wave 0  
**Test Harness:** `pbrs-grpc/tests/retry_safety.rs`  

---

## 1. Overview and Problem Statement

gRPC defines two distinct retry mechanisms:
1. **Policy-based retries (Service Config):** Configured per-method, restricted to explicitly idempotent or retry-safe methods, bounded by max attempts and hedging policies, retrying specific gRPC status codes (e.g. `UNAVAILABLE`).
2. **Transparent retries:** Automatic retries performed by the gRPC client transport kernel without requiring user configuration, **only when it is mathematically certain that server application logic has never seen or processed the request**.

The central hazard in distributed RPC systems is **ambiguous connection loss**. If an HTTP/2 connection or TCP socket drops after request bytes have been transmitted, the absence of a response from the server is **not** proof of non-execution. The server may have already processed the request, committed database mutations, charged a customer, or triggered external side effects.

Prior to RT-01, the `pbrs-grpc` client kernel conflated "transport connection died" with "safe to transparently retry". This document specifies the required call lifecycle states, analyzes the baseline behavior, defines the exact gRFC A6 commitment rules, and approves the transport-evidence-based replay specification for RT-02.

## 1b. Shipped policy engine (unary)

`Channel::service_config` attaches the A6 document; `ServiceConfig::parse`
validates it eagerly per A21. Unary calls resolve the method entry and then:

- `retryPolicy` retries a failed attempt while attempts remain, the code is in
  `retryableStatusCodes`, throttling allows it, and no `DoNotRetry` pushback
  arrived. Backoff is jittered exponential; a `Delay` pushback overrides it.
- `perAttemptRecvTimeout` bounds each attempt; its expiry retries on its own,
  without consulting the retryable set.
- `hedgingPolicy` fans out delayed duplicate attempts; the first `OK` (or the
  first fatal status) commits, non-fatal statuses keep waiting, and an
  exhausted race fails with the last non-fatal error.
- `retryThrottling` debits every failed unary call and credits every success;
  retries and hedged sends past the first need more than half the bucket.
- Transparent retry (at most once, pre-commit only) still runs first and never
  consumes a policy attempt.

Policy retry replays the already-encoded request frame, so it never
re-serializes and never exceeds the method's caps. Server-streaming policy
retry and streaming throttling accounting are follow-up work; client-streaming
and bidi stay call-site retries because the kernel holds no replay buffer.

---

## 2. Call and Attempt Lifecycle State Machine

Each RPC attempt traverses a sequence of discrete lifecycle states. The boundaries between these states govern whether a transparent retry is permissible.

```
       [ Client Initiates RPC ]
                  │
                  ▼
              (Queued) ───[ Connect error / timeout ]───► [ Transparent Retry Safe ]
                  │
                  ▼ (Open connection & send HEADERS)
            (HeadersSent) ───[ REFUSED_STREAM / GOAWAY last < id ]───► [ Transparent Retry Safe ]
                  │
                  ▼ (Send request payload DATA)
            (BodyStarted) ───[ Ambiguous connection loss ]───► [ MUST NOT RETRY (Committed) ]
                  │
                  ▼ (Server sends response HEADERS)
         (ResponseCommitted) ───[ Stream error / Conn drop ]───► [ MUST NOT RETRY (Committed) ]
                  │
                  ▼ (Receive DATA & trailers)
          (TrailersReceived) ───► [ Terminal State ]
```

### State Definitions

1. **`Queued`:**
   - The RPC is created and queued waiting for channel readiness, connection establishment, TLS handshake, or stream concurrency limits (`SETTINGS_MAX_CONCURRENT_STREAMS`).
   - *Commitment:* Uncommitted. No bytes have been sent to any remote server for this attempt.
2. **`HeadersSent`:**
   - The client has serialized and written the HTTP/2 `HEADERS` frame (`:method = POST`, `:path = /<service>/<method>`, `:scheme`, `:authority`, metadata) to the wire.
   - An HTTP/2 stream ID has been allocated.
   - *Commitment:* Conditionally uncommitted. If the remote peer responds with `REFUSED_STREAM` or `GOAWAY` indicating the stream was never processed, the attempt remains uncommitted.
3. **`BodyStarted` (Request Data Transmitted):**
   - The client has written one or more HTTP/2 `DATA` frames (or the complete unary length-prefixed request message) onto the wire.
   - For unary and server-streaming calls in `pbrs-grpc`, the entire request frame is sent immediately after headers.
   - *Commitment:* **Committed against transparent retry.** Server application logic may have read the stream and commenced execution.
4. **`ResponseCommitted`:**
   - The client has received the initial HTTP/2 response `HEADERS` frame (HTTP status 200, `content-type: application/grpc`) or Trailers-Only headers.
   - *Commitment:* **Committed.** The server has accepted the request and produced response headers. Any subsequent failure is a failure of the active stream, never a candidate for transparent retry.
5. **`TrailersReceived`:**
   - The client has received the trailing `HEADERS` containing `grpc-status` and optional `grpc-message` / error details.
   - Terminal state of the RPC attempt.
6. **`Cancelled`:**
   - The RPC was cancelled by caller action (`CallHandle::cancel`) or request deadline expiration (`DEADLINE_EXCEEDED`).
   - Terminal state. Must never trigger retry.

---

## 3. Pre-Fix Baseline Analysis (`pbrs-grpc/src/client.rs`)

This section records the exact pre-RT-02 baseline outcome (§6 holds the post-fix assertions).

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
`run_unary` and `run_server_stream` first execute `open(...)` (which sends `HEADERS`), then immediately call `send_bytes(...)` (which sends the entire serialized request `DATA` frame with `end_stream = true`), and then await `resp_fut.await`.

If the connection drops (TCP reset, broken pipe, remote peer crash, network timeout) while awaiting `resp_fut.await`:
1. `h2::Error` is classified as `err.is_io() == true`.
2. `Status::from_h2` calls `status.mark_transport()`.
3. `status.is_transport()` returns `true`.
4. `unary` and `server_streaming` discard the dead connection slot, redial a new TCP/TLS connection to the server, and **re-transmit the request frame**.
5. If the server application handler already received the first request and began execution, the server executes the handler a **second time**.

### Empirical Proof in Baseline Tests
In `pbrs-grpc/tests/retry_safety.rs`, deterministic test fixtures reproduce this exact regression:
- **`scenario_b_unary_drops_after_request_dispatched_baseline_retries`**:
  The server receives the request, increments an atomic execution counter from 0 to 1, and abruptly drops the TCP connection before sending response headers. The client catches `is_transport()`, redials, and executes again. The counter becomes **2**.
- **`scenario_b_server_streaming_drops_after_request_dispatched_baseline_retries`**:
  Same outcome for server-streaming; execution counter becomes **2**.

### Contrast with Streaming RPCs
In contrast, `client_streaming` and `bidi` calls use `open_retrying()`. `open_retrying()` only retries if `open()` fails *before* request `DATA` frames are sent. Once `open()` returns, caller-held `StreamSender` takes over and no automatic transport retry is attempted.

---

## 4. Official gRFC A6 Transparent Retry Rules

Under [gRFC A6](https://github.com/grpc/proposal/blob/6342be729b96478a2897ceb208a8cddcd832a17b/A6-client-retries.md) (pinned `grpc/proposal@6342be7`), transparent retries are governed by strict eligibility rules.

### Permitted Transparent Retries
A transparent retry of an attempt is permitted **if and only if** the client has positive proof that the server application logic never started processing the request:

1. **Pre-Stream / Connect Failures:**
   - Failure to acquire a connection or stream slot.
   - Handshake failure (connection refused, DNS failure, TLS negotiation failure).
   - Connection closed by peer while idle in the pool before request `HEADERS` were dispatched.
2. **HTTP/2 `REFUSED_STREAM`:**
   - RFC 7540 §8.1.4 explicitly specifies:  
     *"REFUSED_STREAM: The endpoint refused the stream prior to performing any application processing on the stream."*
   - Transparent retry on `REFUSED_STREAM` is guaranteed safe regardless of whether request data was transmitted.
3. **HTTP/2 `GOAWAY` with `last_stream_id < stream_id`:**
   - If the server sends a `GOAWAY` frame where `last_stream_id` is less than the stream ID allocated for this RPC, the HTTP/2 specification guarantees the server never processed the stream.

### Prohibited Transparent Retries
Transparent retry is **strictly forbidden** in all of the following conditions:

1. **Ambiguous Connection Loss After Request Data Sent:**
   - Once request `DATA` has been sent on the wire and the stream opened, any transport error without `REFUSED_STREAM` or `GOAWAY` stream-id evidence is ambiguous. It must be treated as potentially executed and must **not** be transparently retried.
2. **Response Headers Received (`ResponseCommitted`):**
   - Once response `HEADERS` have been received from the server, the RPC is committed. Subsequent connection loss or stream resets must fail the call directly to the caller.
3. **Stream Resets with Other Error Codes:**
   - `RST_STREAM` with `CANCEL`, `INTERNAL_ERROR`, `PROTOCOL_ERROR`, or `STREAM_CLOSED` indicate failures after stream acceptance and cannot be transparently retried.
4. **Explicit Application Error Statuses:**
   - If the server sends trailers with `grpc-status: UNAVAILABLE` (or any other code), server application logic ran and generated that status. It cannot be transparently retried (only policy retries may evaluate application status codes).
5. **Caller Cancellation or Timeout:**
   - Client-side `CallHandle::cancel` or local deadline expiration immediately terminates the RPC.

---

## 5. Transport-Evidence-Based Replay Rules for RT-02

For task RT-02, `pbrs-grpc` must implement the following design:

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

| State at Failure | Error Cause | Transparent Retry Allowed? | Reason |
|---|---|:---:|---|
| `Queued` / Connect | `ECONNREFUSED` / Timeout | **YES** | Server never saw request |
| `Queued` (Idle Slot) | Server closed idle connection | **YES** | Request never written to socket |
| `HeadersSent` | `REFUSED_STREAM` | **YES** | RFC 7540 §8.1.4 non-processing guarantee |
| `HeadersSent` / `BodyStarted` | `GOAWAY (last < stream_id)` | **YES** | RFC 7540 §6.8 non-processing guarantee |
| `BodyStarted` | IO Error / Broken Pipe / TCP Reset | **NO** | Ambiguous: server may have executed |
| `BodyStarted` | `RST_STREAM(CANCEL / INTERNAL)` | **NO** | Stream was accepted before reset |
| `ResponseCommitted` | Any stream / transport error | **NO** | Response headers already arrived |
| Any | Local cancellation / Deadline | **NO** | Caller cancelled or timed out |

### 3. Never Promise Exactly-Once Semantics
Transport-level protocols cannot deliver exactly-once semantics across network partitions. If a server receives a request, commits mutations, and crashes before sending headers:
- At-most-once safety requires the client to **fail fast** (`Status::unavailable("connection lost after request transmission")`) rather than replaying.
- Exactly-once guarantees require application-level idempotency tokens or deduplication layers (e.g. transactional deduplication tables keyed by request UUID), not transport replay.

---

## 6. Regression Baseline & Verification Matrix

The test suite in `pbrs-grpc/tests/retry_safety.rs` establishes the verification baseline:

| Test Scenario | Condition | Verified Behavior | Regression Baseline Role |
|---|---|---|---|
| **Scenario A1** (`scenario_a_connection_refused_wait_for_ready_retries_safely`) | `ECONNREFUSED` on initial dial with `wait_for_ready` | Connect retries until server starts; execution counter = 1 | Verifies safe transparent retry before headers |
| **Scenario A2** (`scenario_a_refused_stream_retries_safely`) | Server sends HTTP/2 `REFUSED_STREAM` on attempt 1 | Client transparently redials to attempt 2; execution counter = 1 | Verifies safe transparent retry on explicit refusal |
| **Scenario B (Unary)** (`scenario_b_unary_drops_after_request_dispatched_baseline_retries`) | Server receives request, increments counter to 1, drops connection before headers | Client redials; server executes attempt 2; counter reaches 2 | **Regression reproducer**: Proves unsafe duplicate execution baseline in RT-01. RT-02 will assert counter remains 1. |
| **Scenario B (Streaming)** (`scenario_b_server_streaming_drops_after_request_dispatched_baseline_retries`) | Server receives request, increments counter to 1, drops connection before headers | Client redials; server executes attempt 2; counter reaches 2 | **Regression reproducer**: Proves unsafe duplicate execution on server-streaming. |
| **Scenario C (Unary)** (`scenario_c_unary_response_headers_committed_no_retry_on_stream_error`) | Server sends response headers (response committed), then `RST_STREAM` | Client returns `Err(Status::unavailable)`; counter remains 1 | Confirms no retry after response commitment on unary |
| **Scenario C (Streaming)** (`scenario_c_server_streaming_response_headers_committed_no_retry`) | Server sends response headers + 1 item, then `RST_STREAM` | Client returns `Err(Status::cancelled)`; counter remains 1 | Confirms no retry after response commitment on streaming |

All 9 test cases in `pbrs-grpc/tests/retry_safety.rs` (scenarios A–C for RT-02, scenario D for RT-03) pass deterministically; scenarios A–C are the RT-02 regression harness.
