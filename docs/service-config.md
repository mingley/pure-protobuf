# Service-Config Retry Semantics (Approved Contract)

Use a validated JSON service config to enable retries for selected methods.
Without a policy, the kernel performs at most one transparent retry and only
with proof the server did not process the request. A policy may re-execute the
handler; applications must provide their own idempotency or deduplication.

This is the implementation contract for selection, replay, throttling, and
pushback. Unary retry and hedging ship, as does server-streaming retry before
commitment. Client-streaming and bidirectional replay remain application work;
streaming throttling accounting is approximate (§8).

- **Task:** FL-06 ("Approve explicit service-config retry semantics")
- **Pinned Standards:** gRFC A6 (client retries) at
  `grpc/proposal@6342be729b96478a2897ceb208a8cddcd832a17b`, gRFC A21
  (service-config error handling), gRFC A24 (LB policy selection)
- **Work Package:** FL lane (Fleet & LB behavior), Wave 4
- **Depends on:** RT-03 (absolute deadline), RT-06 (byte budget), FL-04
  (round_robin and churn recovery)
- **Feeds:** FL-07 (parse/validate), FL-08 (unary execution), FL-09
  (pushback/amplification budgets)
- **Related Documents:** [retry-contract.md](retry-contract.md) (commitment
  boundaries, transparent retry), [resource-budgets.md](resource-budgets.md)
  (byte admission), [grfc.md](grfc.md) (A6 status row)
- **Status:** decision record. It approves semantics only; it does not change
  code. The existing `ServiceConfig` parser and unary/hedging executors are
  reviewed against this contract, not the other way around.

---

## 1. Scope and opt-in principle

The service config is a JSON document, attached with
`Channel::service_config` (or delivered later by a resolver), that supplies:

1. per-method defaults: `timeout`, `waitForReady`, message caps,
   `retryPolicy` / `hedgingPolicy`;
2. channel-wide `retryThrottling`;
3. `loadBalancingConfig` selection (A24; policies land in FL-03/FL-04/CH lanes);
4. `healthCheckConfig` (A17 pointer only; semantics live with health checking).

Retry rules:

- **No policy, no policy retry.** A method without `retryPolicy` gets exactly
  the pre-existing behavior: at most one transparent redial under the
  [retry contract](retry-contract.md) evidence rules, nothing else. FL-07 must
  keep the no-config path byte-identical.
- **Policy is method-scoped and explicit.** The operator names the methods (or
  service/default wildcards, §3) whose handlers are safe to re-execute. The
  kernel cannot infer idempotency.
- **Retry and hedging are mutually exclusive** on one method: a document
  setting both on the same entry is invalid. (Hedging execution already ships
  for unary; this card approves retry semantics and records hedging only where
  the two share machinery: throttling, pushback, observability.)

## 2. Transparent retry vs application policy vs cross-endpoint picks

Three mechanisms move a call to a new attempt. They have different safety
arguments and must never be conflated:

| Mechanism | Trigger | Safety argument | Attempt budget |
|---|---|---|---|
| Transparent retry | Transport failure with proof of non-execution: pre-headers loss, `REFUSED_STREAM`, or `GOAWAY(last < stream_id)` | Server application provably never saw the request ([retry-contract.md](retry-contract.md) §4–5) | At most once per attempt; never consumes a policy attempt |
| Application policy retry | Terminal attempt outcome whose code is in `retryableStatusCodes` (or a per-attempt timeout), attempts remain, throttling allows, no `DoNotRetry` | Operator asserted the method is retry-safe; re-execution is accepted, not disproven | Consumes one of `maxAttempts` |
| Cross-endpoint pick | Any (re)acquire of a connection: initial dial, transparent redial, or policy attempt | LB/routing decision, not a safety argument: picking a different endpoint does not make an ambiguous execution safe | Governed by the enclosing mechanism |

Consequences:

- A policy retry re-acquires through the normal LB path (FL-04 discard
  routing, per-address backoff, health gating where enabled), so a retry may
  land on another endpoint. That is a routing outcome, not a correctness
  claim: if the first attempt executed remotely, the retry executes again.
- An ambiguous execution — connection loss after request bytes were sent
  without `REFUSED_STREAM`/`GOAWAY` evidence — is surfaced to the caller as
  `UNAVAILABLE` ("connection lost after request transmission" family) with an
  **unknown outcome**: the handler may or may not have run. Callers must be
  able to distinguish this from success; the kernel must never silently
  convert it into either a success or a transparent replay. Only an explicit
  `retryPolicy` listing `UNAVAILABLE` may turn a later such failure into
  another attempt, and then only as a counted policy attempt.
- Interceptor or caller-initiated re-invocation ("call-site retry") is outside
  this contract: the kernel sees it as a new call.

## 3. Method selection and precedence

Each `methodConfig` entry carries a `name` list of `{service, method}`
selectors; an empty field is a wildcard. Resolution for a call:

1. an exact `service` + `method` match wins;
2. else a service-wide default (`service` set, `method` empty) wins;
3. else the global default (both empty) wins;
4. else no entry covers the call (no policy, no defaults).

Selector rules:

- A missing `name` key, or an explicitly empty `name` array, covers everything
  (global default).
- `method` without `service` is invalid.
- The same `(service, method)` pair in two entries is invalid (duplicate).
- Precedence is by specificity, not document order: a later service default
  still beats an earlier global default.

Non-retry defaults in an entry: `waitForReady` (bool), `timeout`
(protobuf-JSON duration, §5), `maxRequestMessageBytes` /
`maxResponseMessageBytes` (unsigned integers). Message caps tighten only: a
method entry never loosens an explicit channel cap (tightest-wins in
`wire_for`).

## 4. Retry policy fields and validation

`retryPolicy` is valid only with all of:

| Field | Type | Rule |
|---|---|---|
| `maxAttempts` | integer | Required. Must be ≥ 2 (a policy that cannot retry is a config error, not a silent no-op). Values above 5 are clamped to 5 per A6. Non-integers and values < 2 are invalid. |
| `initialBackoff` | duration string | Required. Must parse per §5. |
| `maxBackoff` | duration string | Required. Must parse per §5. |
| `backoffMultiplier` | number | Required. Must be positive and finite. |
| `retryableStatusCodes` | string array | Required. Must be non-empty. Each entry is a canonical status name (`Code::from_str`, numeric `0`–`16` spellings accepted); unknown names are invalid. |
| `perAttemptRecvTimeout` | duration string | Optional. When present must parse per §5. |

Backoff between attempts is jittered exponential: for retry index `n`
(`n = 0` is the first retry),

```
delay(n) = min(initialBackoff * multiplier^n, maxBackoff) * U[0.8, 1.2)
```

i.e. ±20% uniform jitter per A6. The jitter source needs decorrelation, not
cryptographic strength.

`maxAttempts` counts attempts including the first: `maxAttempts: 3` permits
one initial attempt plus two retries.

## 5. Durations, bytes, and unknown fields

- Durations use protobuf-JSON form: `"3.2s"` — decimal seconds with up to 9
  fractional digits and a mandatory `s` suffix. Negative, suffix-less,
  over-precise (> 9 fraction digits), non-numeric, or overflowing values are
  invalid. `"0s"` is valid (zero backoff / immediate per-attempt timeout is
  the operator's choice).
- Byte limits must be unsigned integers fitting `usize`.
- Unknown top-level and per-method fields are **ignored** (forward
  compatibility), never errors. Unknown LB policy names parse as skippable
  entries per A24. Unknown *values* in known retry fields (bad code names,
  malformed durations) are validation errors, not skips.
- Whole-document validation is eager (`ServiceConfig::parse` fails with
  `InvalidArgument` naming the first bad entry); per A21 a resolver-supplied
  update that fails validation keeps the last good document.

## 6. Attempt, deadline, and replay-byte budgets

### 6.1 Attempt budget

- Attempts are 1-based; `attempts_made >= maxAttempts` ends the call with the
  last outcome. Exhaustion of a retryable outcome is recorded distinctly
  (`exhausted`) from fail-fast on a non-retryable code.
- A transparent redial inside an attempt does not consume a policy attempt.
- Cancellation (`CallHandle::cancel`) and overall-deadline expiry end the
  call immediately from any state, including backoff and pushback sleeps:
  no further attempt is sent and the sleep is interrupted.

### 6.2 Deadline budget (RT-03)

One absolute deadline spans queueing, connect, TLS, attempts, backoff, and
replay. Every attempt sends the *remaining* timeout derived from the original
deadline, never a reset full duration.

- `perAttemptRecvTimeout`, when set, caps each attempt: the attempt deadline
  is `min(overall deadline, attempt start + perAttemptRecvTimeout)`. Its
  expiry retries on its own — it does not consult `retryableStatusCodes`
  (so `DEADLINE_EXCEEDED` need not be listed) — but it still consumes an
  attempt, still honors throttling and `DoNotRetry`, and still ends the call
  when the overall deadline is gone.
- Overall-deadline expiry surfaces `DEADLINE_EXCEEDED` (or the transport's
  terminal mapping); it is never converted into a success and never retried
  past the deadline. Overflow/rounding in remaining-time computation must
  fail toward expiry, never extend the deadline.

### 6.3 Replay-byte budget (RT-06)

Policy retry replays the already-encoded request frame; it never
re-serializes the message:

- Unary and server-streaming calls encode once (`encode_msg` under the
  method's `wire_for` caps) and hold one `Bytes` frame across attempts.
- Each attempt acquires a byte-budget permit for the frame length before
  sending; permits release on every exit path (success, failure,
  cancellation, encode error, peer reset). Oversized frames fail locally
  with `RESOURCE_EXHAUSTED` before any send and are not retryable outcomes.
- The replay buffer is bounded by the outbound encoding cap (default 4 MiB),
  already accounted in the per-RPC term of
  [resource-budgets.md](resource-budgets.md) §3.2. Backoff sleeps hold the
  frame but no connection slot, stream, or byte permit.

## 7. Commitment: what may and may not retry

Commitment here means "this call will not be re-sent by the kernel":

- **Unary:** the call commits when response headers arrive (success or
  application status) — a policy retry then happens only as a *new attempt*
  evaluated against §4/§6, never as transparent replay. Transport failure
  before commitment follows the [retry-contract.md](retry-contract.md)
  evidence matrix exactly once, then falls through to the policy decision.
- **Server-streaming:** the call commits when the first response message
  arrives (or on success completion). Only trailers-only failures before
  any message take the retry path, reusing the unary decision helper.
  Mid-stream failures never retry.
- **Client-streaming and bidirectional:** arbitrary replay stays
  **unsupported**. The kernel holds no replay buffer for caller-driven
  outbound streams; application code that wants retries re-invokes the call
  (call-site retry). FL-08/FL-09 must not smuggle kernel replay into these
  shapes.
- Committed, oversized, cancelled, or expired calls cannot retry. A retryable
  code on an already-committed stream is a terminal outcome, not a retry
  trigger.

## 8. Throttling

Channel-wide `retryThrottling` (`maxTokens`, `tokenRatio`, both required,
positive, finite) is a token bucket shared by every method on the channel
(lineage-shared across clones):

- Starts full at `maxTokens`. Each failed call removes 1 token; each
  successful call refunds `tokenRatio`, capped at `maxTokens`.
- A retry — or a hedged send past the first — is allowed only while the
  balance is **strictly greater than half** of `maxTokens`. The first attempt
  of any call is never throttled.
- Accounting: success = terminal `OK`; failure = any terminal non-OK,
  including cancellation and deadline expiry. Local rejections that never
  ran an attempt (refused interceptor, unencodable message, full
  concurrency semaphore, local byte-budget refusal) are not call outcomes
  and are not recorded.
- Streaming throttling accounting is approximate (it taps headers-read
  because `OK` receipt flows through the unobservable transport at
  commit); A6-faithful streaming accounting is explicit follow-up work, not
  a silent claim.
- Throttled decisions are observable (`throttled` counter, §10); the call
  fails with its last outcome, not with a synthetic throttling status.

## 9. Server pushback

The server may steer retries with the `grpc-retry-pushback-ms` trailer:

| Trailer value | Meaning |
|---|---|
| `-1` | `DoNotRetry`: no further attempt is sent; the call fails with its last outcome and records `pushback_refusals`. Checked before the retryable-code set and before throttling. |
| non-negative integer `ms` | `Delay(ms)`: the next retry waits this long *instead of* the computed jittered backoff, and records `pushback_delays`. Still consumes an attempt and still requires a retryable outcome (or per-attempt timeout) with attempts remaining. |
| malformed | Ignored (not an error, no pushback applied). |

Pushback travels on the `Status` itself, never in user metadata (`grpc-*`
keys are kernel-owned). Cancellation and overall-deadline expiry interrupt
a pushback sleep exactly like a backoff sleep.

## 10. Observability

Every unary and server-streaming call records into channel-scoped lock-free
`RetryStats`, whether or not a policy is attached (so exporters can compute
retry rates). Counters: `calls`, `policy_retries` (attempts actually sent,
per-attempt timeouts included), `transparent_retries`, `hedged_sends` past
the first, `throttled`, `pushback_delays`, `pushback_refusals`,
`per_attempt_timeouts`, `exhausted`, `committed_ok`, `committed_err`.
Invariant: `committed_ok + committed_err == calls` once in-flight calls
settle.

Further surface, all per-attempt aware:

- `Channel::retry_stats()` snapshot (always on, no sampling flag).
- Lifecycle observer hooks (`on_attempt_start`, attempt guards) and binary
  logging (attempts share one call id) for per-attempt traces.
- Channelz stream/message accounting per attempt.
- OTel export of these counters is GF-01/A96 work; this card approves the
  counter meanings, not the exporter.

## 11. No exactly-once promise

Transport replay cannot deliver exactly-once execution across network
partitions: a server may process a request, commit mutations, and crash
before responding. Therefore:

- Transparent retry is allowed only with proof of non-execution (§2); all
  other transport failures fail fast with an unknown outcome.
- Policy retry is an operator assertion that re-execution is safe, not a
  kernel guarantee of single execution.
- Exactly-once requires application-level idempotency (request tokens,
  deduplication tables). Nothing in this contract provides it, and no API
  or document in the FL lane may claim it.

## 12. Reviewed public API shape

The API below is the approved surface for FL-07/08/09. Field and method
names are reviewed; FL-07 may add constructors or error variants but must
not rename or widen without re-review.

```rust
// Parse + inspect (FL-07). All parse failures: Status::invalid_argument
// naming the first invalid entry.
ServiceConfig::parse(json: &str) -> Result<ServiceConfig, Status>
ServiceConfig::parse_value(value: &serde_json::Value) -> Result<ServiceConfig, Status>
ServiceConfig::method_config(&self, service: &str, method: &str) -> Option<&MethodConfig>
ServiceConfig::lb_policies(&self) -> &[LbPolicyConfig]
ServiceConfig::retry_throttling(&self) -> Option<&RetryThrottling>
ServiceConfig::is_empty(&self) -> bool

pub struct MethodName { pub service: String, pub method: String }
pub struct MethodConfig {
    pub names: Vec<MethodName>,
    pub wait_for_ready: Option<bool>,
    pub timeout: Option<Duration>,
    pub max_request_message_bytes: Option<usize>,
    pub max_response_message_bytes: Option<usize>,
    pub retry_policy: Option<RetryPolicy>,
    pub hedging_policy: Option<HedgingPolicy>,
}
pub struct RetryPolicy {
    pub max_attempts: u32,              // parsed values > 5 stored as 5
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f64,        // always positive, finite
    pub per_attempt_recv_timeout: Option<Duration>,
    pub retryable_status_codes: BTreeSet<Code>,
}
pub struct RetryThrottling { pub max_tokens: f64, pub token_ratio: f64 }

// Attach + observe (FL-08/FL-09).
Channel::service_config(self, json: &str) -> Result<Self, Status>  // channel unchanged on Err
Channel::service_config_doc(&self) -> Option<ServiceConfig>
Channel::retry_stats(&self) -> RetryStats

// Backoff + pushback primitives.
retry_backoff(initial: Duration, max: Duration, multiplier: f64, retry_index: u32) -> Duration
Status::retry_pushback(&self) -> Option<Pushback>   // parsed from grpc-retry-pushback-ms
pub enum Pushback { Delay(Duration), DoNotRetry }
```

Deliberately crate-internal: the `RetryThrottler` bucket, the
policy-decision helper (`Proceed`/`Throttled`/`PushbackRefused`/`Declined`),
throttle accounting (`note_call_outcome`), and `TransportEvidence`.
Resolvers consume `parse_value` plus A21 keep-last-good handling at the
adoption site, not inside the parser.

## 13. Test vectors for FL-07 (invalid config)

Each vector is a document plus the expected `ServiceConfig::parse` outcome.
"Invalid" always means `Err` with `Code::InvalidArgument` whose message
names the offending entry/field; "valid" means `Ok` with the stated value.

### 13.1 Document shape

1. `not json` → invalid (not valid JSON).
2. `[]` → invalid (must be a JSON object).
3. `{"methodConfig": {}}` → invalid (`methodConfig` must be an array).
4. `{"methodConfig": ["x"]}` → invalid (entry must be an object).
5. `{"methodConfig": [{"name": {}}]}` → invalid (`name` must be an array).
6. `{"methodConfig": [{"name": [{"method": "M"}]}]}` → invalid (method
   without service).
7. Two entries both naming `{service: "s", method: "m"}` → invalid
   (duplicate). Two global defaults (`{}`, `{}`) are likewise duplicates.
8. `{"methodConfig": [{"name": [{}], "retryPolicy": {...}, "hedgingPolicy":
   {...}}]}` → invalid (both policies on one entry).
9. `{"methodConfig": [{"name": [{}], "futureField": 1}], "topLevelFuture":
   {}}` → valid; unknown fields ignored, entry otherwise empty.

### 13.2 Retry policy fields

Let `R(f)` be `{"methodConfig": [{"name": [{}], "retryPolicy": f}]}` with a
baseline `f0 = {"maxAttempts": 3, "initialBackoff": "0.1s", "maxBackoff":
"1s", "backoffMultiplier": 2.0, "retryableStatusCodes": ["UNAVAILABLE"]}`:

10. `f0` → valid; `max_attempts == 3`.
11. `maxAttempts` missing / `1` / `0` / `-2` / `2.5` / `"3"` → invalid
    (required, integer, ≥ 2).
12. `maxAttempts: 7` → valid with `max_attempts == 5` (A6 clamp).
13. `initialBackoff` missing → invalid. `"100ms"` (no `s` suffix),
    `"-1s"`, `"1.1234567899s"` (> 9 fraction digits), `"abc"`, `0.1`
    (non-string), `""` → invalid.
14. `maxBackoff` missing → invalid; same malformed set as 13 → invalid.
15. `backoffMultiplier` missing / `0` / `-1.5` / `"2"` / `NaN` / `Infinity`
    → invalid (required, positive, finite). (JSON has no NaN/Infinity
    literals; they arrive via `parse_value` overlays.)
16. `retryableStatusCodes` missing / `[]` / `"UNAVAILABLE"` (non-array) →
    invalid.
17. `retryableStatusCodes: ["UNAVAILABLE", "BOGUS"]` → invalid (unknown
    name). `["14"]` → valid (`Code::from_str` numeric spelling of
    `UNAVAILABLE`); `["17"]` / `["-1"]` → invalid.
18. `perAttemptRecvTimeout: "0.5s"` → valid. `"500ms"` / `"-2s"` →
    invalid like 13. Absent → valid with `None`.

### 13.3 Throttling, LB, precedence

19. `{"retryThrottling": {"maxTokens": 10, "tokenRatio": 0.5}}` → valid.
20. `retryThrottling` non-object / missing either field / `maxTokens: 0` /
    `tokenRatio: -1` / non-finite → invalid.
21. `loadBalancingConfig` with an unknown policy name → valid; selection
    skips it (A24). LB entries never invalidate the retry/throttling parse.
22. Entries `[{service s default with timeout 1s}, {exact s/m with timeout
    2s}, {global with timeout 3s}]` in any document order →
    `method_config("s", "m")` returns the exact entry; `method_config("s",
    "other")` the service default; `method_config("t", "u")` the global
    default; absent global → `None`.
23. The no-config path (`Channel` without `service_config`) resolves no
    entry and behaves exactly as before FL-07 (no policy struct, no
    throttler, no `RetryPolicy` lookup cost beyond a miss).

## 14. Test vectors for FL-08 (side effects and execution)

Vectors use a scripted handler with an atomic execution counter incremented
on handler entry (before any response/failure), plus the seeded
side-effect histories FL-08 already uses. "Counter" is server-side
executions; attempts are client-side sends. Baseline policy unless stated:
`maxAttempts: 3`, `initialBackoff: "0.01s"`, `maxBackoff: "0.05s"`,
`backoffMultiplier: 2.0`, `retryableStatusCodes: ["UNAVAILABLE"]`.

### 14.1 Policy retries re-execute (no exactly-once)

24. Script `[UNAVAILABLE, UNAVAILABLE, OK]` → client `OK`; counter == 3;
    `policy_retries == 2`, `committed_ok == 1`.
25. Script `[UNAVAILABLE × 5]` → client `UNAVAILABLE`; counter == 3;
    `policy_retries == 2`, `exhausted == 1`, `committed_err == 1`. Attempts
    stop at `maxAttempts` even though the failure stays retryable.
26. Script `[INTERNAL]` → client `INTERNAL`; counter == 1; `policy_retries
    == 0`, `exhausted == 0` (fail fast on a non-retryable code).
27. Unconfigured method, script `[UNAVAILABLE]` → client `UNAVAILABLE`;
    counter == 1; `policy_retries == 0`. No silent policy.
28. Seeded history asserting "unknown outcome ≠ success": script drops the
    TCP connection after handler entry with no policy → client
    `UNAVAILABLE`, counter == 1; the test asserts the error is surfaced
    (not `OK`, not a transparent second execution) and the verdict records
    the outcome as unknown.

### 14.2 Commitment and streaming

29. Server-streaming, trailers-only `UNAVAILABLE` before any message, then
    `OK` stream → client receives the full stream; counter == 2.
30. Server-streaming, one message then transport reset → client error;
    counter == 1; `policy_retries == 0` (committed at first message).
31. Server-streaming, response headers then `RST_STREAM` → client error;
    counter == 1; no retry (committed at headers).
32. Client-streaming/bidi with a `retryPolicy` on the method: mid-call
    failure after sends → client error; kernel sends no replayed attempt
    (assert via execution counter and/or wire tap). Policy on these shapes
    is accepted by the parser but inert in the executor.

### 14.3 Budgets

33. Per-attempt timeout: `perAttemptRecvTimeout: "0.05s"`, handler sleeps
    5 s then would reply `OK`; overall timeout 10 s → first attempt times
    out and retries without `DEADLINE_EXCEEDED` in the retryable set;
    `per_attempt_timeouts >= 1`. With a fast second script step, client
    `OK`.
34. Overall deadline wins: overall timeout 0.1 s, handler always sleeps
    5 s, generous `maxAttempts` → client `DEADLINE_EXCEEDED`; every attempt
    carried a shrinking `grpc-timeout`; no attempt starts after expiry;
    backoff sleeps are interrupted (assert elapsed ≪ attempts × sleeps).
35. Oversized request: message exceeding the method/channel encoding cap →
    local `RESOURCE_EXHAUSTED`, zero bytes sent, zero attempts recorded,
    no throttler debit (never ran).
36. Cancel during backoff: script `[UNAVAILABLE, ...]`, cancel the call
    while the first backoff sleeps → client `CANCELLED`; counter == 1; no
    second attempt; byte permit and RPC slot released (quiescence asserts).

## 15. Test vectors for FL-09 (pushback and amplification)

Same harness; outage scripts fail (near-)unanimously.

37. `DoNotRetry`: script `[UNAVAILABLE + pushback -1, ...]` → client
    `UNAVAILABLE`; counter == 1; `pushback_refusals == 1`,
    `policy_retries == 0`. Takes precedence over the retryable set and a
    full bucket.
38. Pushback delay: script `[UNAVAILABLE + pushback 200ms, OK]` → client
    `OK`; counter == 2; elapsed ≥ 200 ms (and ≫ the 10 ms-class computed
    backoff); `pushback_delays == 1`. Cancel during the pushback sleep →
    `CANCELLED`, counter == 1.
39. Malformed pushback (`"soon"`, `"-2"`, `""`) → ignored: retry proceeds
    on the computed backoff as if no trailer were present.
40. Throttle drain: `retryThrottling: {maxTokens: 4, tokenRatio: 0.5}`,
    drive failures until balance ≤ half → next retryable failure does not
    retry (`throttled == 1`, counter increment of 1 for that call);
    successes refund and retries resume. First attempts are never gated.
41. Outage caps (`rpc-bench/scenarios/retry-outage.json`): all backends
    failing with a retryable code under the approved policy — attempt
    count, wire bytes, and concurrent work stay within the configured caps;
    recovery shows no synchronized retry storm (jitter spreads the second
    wave; assert with a distribution bound, not an exact sleep).
42. Hedging is not smuggled in: with a `retryPolicy` (not `hedgingPolicy`),
    concurrent duplicate sends for one call never occur (assert max
    in-flight attempts per call == 1 via the wire tap).

## 16. FL-04 cross-reference (LB interaction)

Each policy attempt re-acquires a connection through the pool/LB in force:

- Discards on the retry path route per-address (`discard_conn` with the
  RR address), so a poisoned endpoint is skipped by the next pick without
  tearing down healthy subchannels.
- Per-address backoff, health gating (where enabled), and graceful drain
  apply to retry re-acquires exactly as to new calls.
- New-call routing stays separate from retry: LB decides *where* an attempt
  goes; §§2/6/7 decide *whether* it goes. External-LB (passthrough/direct)
  paths are unaffected.

## Open questions for maintainer

1. **Clamp vs reject for `maxAttempts > 5`:** A6 says treat values above 5
   as 5; vector 12 follows that. Do we also want a loud diagnostic (log /
   channelz trace event) when clamping, so operators notice the silent
   reduction?
2. **Policy on client-streaming/bidi shapes:** §7 approves parse-but-inert
   (the parser accepts the entry; the executor ignores it). Alternative:
   reject `retryPolicy` for these shapes at lookup time with
   `InvalidArgument` on the call. Which failure mode do we want — silent
   no-retry or loud per-call error?
3. **Streaming throttling accounting:** §8 records the headers-tap
   approximation as known follow-up. Is that acceptable to ship behind the
   existing tests, or should FL-09 block on A6-faithful streaming
   accounting first?
4. **`"0s"` backoff/pushback semantics:** vector 13 accepts `"0s"`. A zero
   initial backoff plus jitter yields ~zero sleeps (hot retry loop until
   throttling or attempts bite). Should `initialBackoff: "0s"` be invalid,
   or is operator's-choice with the attempt cap sufficient?
5. **Throttler scope:** the bucket is channel-lineage-wide. With a shared
   `Channel` serving hot and cold methods, one method's outage throttles
   the other's retries. Acceptable per A6, or do we want a documented
   per-method opt-out later?
