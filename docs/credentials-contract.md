# Call-credential integration contract (EX-03)

Async OAuth2, JWT, and cloud credential providers are planned work. This
contract describes how they must acquire and refresh tokens without blocking
the RPC runtime or sending credentials to the wrong authority. Existing
interceptors can attach tokens already obtained by the application.

**Status:** Proposed decision contract. No implementation. Base SHA
`fd4500c61421b052ab16e2c5d4abd666ba041634`.

**Scope:** How async call credentials (OAuth2 tokens, JWT, GCE/ADC) are
acquired, refreshed, cached, bound to an audience/authority, and restricted
to secure transports on the native `pbrs-grpc` client. Implemented by EX-04
(providers), EX-05 (JWT signing), qualified by EX-17 (official cloud-auth
cases). ALTS is out of scope here; it is decided separately in EX-18.

**Inputs:** EX-03 card in `docs/plan/tasks.json`; RT-03 (absolute deadline;
`pbrs-grpc/src/timeout.rs`, `pbrs-grpc/src/client/call.rs`); OB-03 (default
redaction; closed 2026-09-28); `pbrs-grpc/src/interceptor.rs`
(`ClientInterceptor`); `pbrs-grpc/src/tls.rs` (`ClientTls`);
`tests/interop/cases.json` auth suite.

## 1. Background: why interceptors are not the credential path

`ClientInterceptor::intercept` is synchronous (`&mut Outgoing -> Result<(),
Status>`) and runs once per RPC at call creation, before the stream opens
(`apply_interceptors` in `pbrs-grpc/src/client/call.rs`). Token acquisition
is async (OAuth2 exchange, metadata-server fetch) and can block, fail, and
need refresh. Putting it in a sync interceptor would either block call
creation on I/O or force `block_on` inside the hot path.

**Decision D1:** Credentials get a dedicated async provider path in the
client call pipeline, sequenced *after* the sync `ClientInterceptor` chain
and *before* the stream opens. Sync interceptors keep their current
contract (sync mutation/reject only); they MUST NOT perform credential
I/O. An interceptor may still stamp a caller-supplied static token it
already holds; it MUST NOT fetch, refresh, or cache tokens.

## 2. Provider model

**Decision D2:** Two credential kinds, matching gRPC semantics:

- *Channel credentials* establish transport security. Only the existing
  reviewed `ClientTls` constructors (`webpki`, `native_roots`, `ca`, and
  the `*_mtls` variants) qualify. There is no skip-verification option
  (IO-02), and this contract adds none.
- *Call (per-RPC) credentials* supply per-call metadata (e.g.
  `authorization: Bearer ...`). They are async: acquisition can await
  network I/O and fail independently of the RPC.

**Decision D3:** EX-04 defines one async provider trait (suggested shape:
`get_token(&self, request: &TokenRequest) -> impl Future<Output =
Result<Token, Status>> + Send`, where `TokenRequest` carries audience,
scopes, authority, and the call deadline). Providers are `Send + Sync +
'static` and shared across channels via `Arc`. Concrete providers
(OAuth2 refresh, JWT signer, GCE metadata, ADC chain) are separate types
behind this trait so synthetic fakes can substitute for mechanics tests
without touching the pipeline.

## 3. Deadline, cancellation, single-flight refresh

RT-03 requires one absolute deadline across queueing, connect, backoff,
and replay; credential acquisition is one more stage under that deadline.

**Decision D4 (deadline):** Token acquisition/refresh races the call's
remaining absolute deadline (`remaining_timeout` /
`prefer_deadline`-family semantics). A refresh that outlives the deadline
fails the call with `DEADLINE_EXCEEDED`, never extends the deadline, and
never sends the RPC unauthenticated as a fallback.

**Decision D5 (cancellation):** Call cancellation aborts an in-flight
acquisition promptly (same `cancel_rx` race as the rest of the call path)
and releases any held slot/permit. A cancelled acquisition MUST NOT later
stamp its token onto a different call.

**Decision D6 (single-flight refresh):** The token cache is keyed by
(provider id, audience, scopes). Concurrent calls needing the same
expired/missing entry trigger exactly one refresh; the rest wait on it,
each bounded by its own call deadline. A waiter whose deadline expires
stops waiting without cancelling the shared refresh for the others. A
failed refresh is not cached as negative beyond a small bounded
backoff decided by EX-04; concurrent waiters all observe the failure.

**Decision D7 (expiry margin):** Tokens are treated as expired a fixed
skew margin before their `expires_in` elapses (EX-04 picks the value;
60s is the suggested default). Clock skew MUST NOT turn an expired token
into a successful send.

## 4. Audience, scopes, and authority binding

**Decision D8 (audience binding):** Every token is bound to an audience
at acquisition. Before stamping, the pipeline checks the token's audience
against the call's effective `:authority`. Mismatch fails the call with
`UNAUTHENTICATED` (safe message, no token material); the token is never
sent and never re-keyed to the new authority implicitly.

**Decision D9 (scopes):** Providers declare the OAuth scopes they mint
for; ADC/GCE providers use the documented default cloud-platform scope
set unless the call requests narrower scopes. Scope narrowing is
per-provider configuration, not per-call string concatenation: the
pipeline passes a scope set through, and providers reject scope sets
they cannot mint rather than silently widening.

**Decision D10 (authority changes):** If the effective authority changes
after a token was resolved for the call (override, redirect, retry on a
different subchannel with a different authority), the resolved token is
discarded and re-resolution is required. Tokens are never forwarded to
an authority outside their audience.

## 5. Secure-transport gate

**Decision D11:** Bearer/call credentials are stamped only on approved
secure transports: verified TLS (any `ClientTls` constructor), mTLS, or
ALTS once EX-18/EX-19 approve it. On plaintext h2c, UDS, or `from_io`
without an explicit secure-transport marker, attaching call credentials
fails the call with `UNAUTHENTICATED` (or `FAILED_PRECONDITION` if the
maintainer prefers; see open questions) before the stream opens. There
is no silent plaintext downgrade.

**Decision D12 (test escape hatch):** EX-04 mechanics tests need
credentials without real TLS. The gate may have an explicit,
non-default, test-only insecure override (constructor or flag whose name
contains `insecure`/`test_only`), which MUST be documented as never
valid for interop evidence. It MUST NOT be reachable from any default
configuration.

## 6. Failure semantics and retries

Credential failure is explicit: no silent unauthenticated send, no
silent retry loop, no token material in errors.

**Decision D13 (error mapping):**

- Provider infrastructure failure (metadata server unreachable, OAuth2
  endpoint error, refresh I/O failure): `UNAVAILABLE` with a safe
  message naming the provider and stage only.
- Unusable credentials (no credentials configured, audience mismatch,
  insecure transport, provider rejects scope set, refresh rejected):
  `UNAUTHENTICATED` with a safe message.
- Refresh exceeding the call deadline: `DEADLINE_EXCEEDED` (D4).
- None of these statuses carry token bytes, keys, or full response
  bodies. `UNAVAILABLE` from refresh does not by itself trigger an A6
  retry; it is a local failure before commitment, reported to the
  caller.

**Decision D14 (retry interaction):** One token resolution per RPC
attempt set: the token is resolved once, reused across transparent
retries of the same RPC only while its authority still matches and it
is unexpired (with D7 margin). On a received `UNAUTHENTICATED`, the
cache entry is evicted and exactly one forced re-resolution plus one
retry is permitted, and only when a retry is otherwise allowed by the
retry/commitment policy (RT-01/RT-02). No unbounded refresh-and-retry
loop.

## 7. Redaction

OB-03 (closed) already masks every metadata value in default `Debug`,
gates `safe_debug` on explicit consent, and masks binlog by default.
Credentials inherit all of it plus:

**Decision D15:** Tokens, keys, JWT assertions, and OAuth response
bodies are credential material: they MUST NOT appear in `Display`/
`Debug` default output, status messages, telemetry labels, binlog
entries, interop reports, or retained logs. Observable credential
diagnostics are limited to provider id, audience, scope names, and
expiry bound (e.g. "token expires in N s"), each subject to the
existing `DiagnosticConfig` consent and byte/entry caps.

## 8. Pinned auth-case mapping (accept 2)

All six cases come from the pinned runner
`grpc/grpc@d1487957:tools/run_tests/run_interop_tests.py`, are
`client_to_server` over `http2_tls` in the `full` profile, owned by
EX-17, and currently `blocked_external`. ALTS (`alts_credentials`) is
not one of the six; it belongs to EX-18/EX-19.

| Pinned case | Provider (per D2/D3) | Real prerequisites for official evidence |
|---|---|---|
| `compute_engine_creds` | GCE metadata-server token provider | Run on an approved GCE VM (or approved metadata-service equivalent) with a service account attached; real metadata token endpoint reachable; TLS to the peer |
| `jwt_token_creds` | JWT-assertion provider (EX-05 signer) + OAuth2 token exchange | Approved service-account private key available to the operator only; real Google OAuth2 token endpoint reachable; signed JWT audience = peer |
| `oauth2_auth_token` | OAuth2 access-token provider | Approved OAuth2 access token with the scopes the case requires, minted for the test peer audience; real token valid at run time |
| `per_rpc_creds` | Per-RPC call-credentials provider over secure channel | Same token prerequisites as the underlying provider, plus secure channel credentials (`ClientTls`); case asserts per-call metadata attachment, not channel auth alone |
| `google_default_credentials` | ADC chain provider (env `GOOGLE_APPLICATION_CREDENTIALS` -> gcloud auth -> GCE metadata) | Approved ADC environment configured by the operator (key file path or gcloud auth or GCE); chain order documented in the evidence |
| `compute_engine_channel_credentials` | GCE channel-credentials provider | Approved GCE VM environment with channel-credential support; asserts channel-level GCE credentials rather than per-call tokens |

**Evidence rules for EX-17 (binding on this contract's consumers):**

- E1. Official cloud-auth evidence requires the real prerequisite in
  the table above, executed in an operator-approved environment. Local
  fakes, loopback stubs, self-signed stand-ins, and the D12 test
  override prove mechanics only and MUST be labeled insufficient for
  qualification (same standard as GT-03 self-only labeling).
- E2. Retained evidence stores redacted logs only (D15): no keys,
  tokens, or JWT assertions, even redacted-in-part. Record provider,
  audience, scope names, transport (`http2_tls`), peer identity, and
  pass/fail per case.
- E3. A missing prerequisite (no GCE environment, no approved key, no
  token, no handshaker access) keeps the case `blocked_external` with
  the missing item named. It is never relabeled `not_applicable` or
  `passed`, and never simulated into a pass.
- E4. Authenticated identity/scope MUST be validated against the
  independent peer's observation (peer-reported identity matches the
  expected service account/token subject), not just a successful RPC.

## 9. Out of scope / non-goals

- ALTS record layer and handshaker (EX-18/EX-19).
- Server-side token validation (authz belongs to the `authz` module;
  inbound `authorization` checks remain sync `Interceptor` policy, D1).
- Token-persistence formats and OS keychain integration.
- xDS-provided credentials (a later EX card may extend D3 with an
  xDS-backed provider; the audience/transport gates still apply).

## 10. Open questions for maintainer

1. D11 error code: `UNAUTHENTICATED` vs `FAILED_PRECONDITION` when call
   credentials meet an insecure transport. Recommendation:
   `UNAUTHENTICATED` (matches grpc-go behavior of refusing to send).
2. D7 skew margin value and whether it is a public knob or a fixed
   constant. Recommendation: fixed 60s constant, no knob.
3. D14 single forced re-resolution on `UNAUTHENTICATED`: keep, or fail
   immediately and let the caller retry? Recommendation: keep the
   single retry (standard gRPC behavior), gated on A6 retry permission.
4. Should the ADC chain order (E-table row 5) be fixed to match
   grpc-go exactly, or is a documented subset acceptable? Recommendation:
   match grpc-go order to avoid interop surprises.
5. Does the D12 test-only insecure override need coordinator review of
   its exact API shape in EX-04, or is the "explicit + non-default +
   test-only name" rule sufficient? Recommendation: rule is sufficient;
   EX-04 review checks it.
