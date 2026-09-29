# Official Cacheable-Unary Contract & Protocol Resolution

This page explains why `cacheable_unary` is not part of the standard native
gRPC over HTTP/2 profile for `pbrs-grpc`. Bottom line: standard gRPC requires
HTTP/2 `POST`; `cacheable_unary` is an abandoned experimental `GET` procedure
with security risks, so its recommended disposition is `not_applicable`
(pending maintainer approval).

**Task:** EX-20 ("Resolve the official cacheable-unary contract")  
**Pinned Standards:** `grpc/grpc` @ `d1487957db6658bc532b72871775148229836627`, `doc/PROTOCOL-HTTP2.md`, `doc/interop-test-descriptions.md`, RFC 9110 (HTTP Semantics), RFC 9111 (HTTP Caching), RFC 9113 (HTTP/2)  
**Read Scope:** `pbrs-grpc/src/testing.rs`, `pbrs-grpc/src/server.rs`, `tests/interop/cases.json`  
**Write Scope:** `docs/cacheable-rpc.md`, `tests/interop/cases.json`  
**Recommended Disposition:** `not_applicable` (Standard gRPC over HTTP/2 Profile) — **pending maintainer approval (gate, see §9)**  
**Verification:** 2026-09-29, base `fd4500c6`; pinned upstream refs re-verified read-only (see §8). No `third_party/grpc` in this worktree.

---

## 1. Executive Summary & Disposition Decision

The official interoperability inventory in `tests/interop/cases.json` includes
`cacheable_unary` ("Cacheable Unary Call"). It comes from upstream gRPC interop
documentation (`doc/interop-test-descriptions.md`).

The procedure describes a unary RPC sent with HTTP/2 `GET` instead of `POST`.
It expects an intermediate caching reverse proxy, such as Google Front End
(GFE), to cache the response based on `Cache-Control` headers and return cached
payloads for identical later requests.

Task **EX-20** evaluates whether that procedure applies to the standard native
gRPC over HTTP/2 target. It also records the protocol, security, and interop
reasons for the final registry disposition.

### Finding
1. **Unratified Specification:** The official gRPC over HTTP/2 wire
   specification (`doc/PROTOCOL-HTTP2.md`) explicitly and exclusively mandates
   `Method -> ":method POST"`. No ratified or active gRPC Request for
   Comments (gRFC) in `grpc/proposal` establishing HTTP `GET` semantics for
   gRPC is cited by any pinned source (exhaustive absence not proven; see §8).
2. **Upstream Abandonment and Peer Status:** The procedure originated in 2016 as an experimental C++ prototype (PR #8101, opened 2016-09-14, merged 2016-10-12). In active peer runtimes (all pinned refs re-verified 2026-09-29 unless noted):
   - **C++ Core (`grpc/grpc@d1487957`):** `HttpServerFilter` actively rejects
     `:method GET` as a malformed request
     (`MalformedRequest("Bad method header")`). The case is absent from the
     standard interop runner (`tools/run_tests/run_interop_tests.py`: zero
     matches in the full 60,739-byte pinned file), and upstream issue #18230
     (maintainer comment, 2019-09-05) confirmed it is experimental and not
     tested in the interop matrix.
   - **Java (`grpc-java`, unpinned — supporting evidence only):** `TestServiceClient.java` on master explicitly comments
     `// THIS TEST IS BROKEN. Enabling safe just on the MethodDescriptor does nothing by itself. This test would need to enable GET on the channel.`
     (verified on master 2026-09-29; this repo pins no grpc-java revision, and
     channel-builder internals were not re-verified).
   - **Go (`grpc-go@dd51b1c9`):** Unimplemented; the pinned interop client
     contains zero `cacheable` references and unary calls are strictly `POST`.
3. **Severe Security Hazards:** Allowing unauthenticated or ambient-credential HTTP `GET` requests creates severe vulnerabilities:
   - **Cross-Site Request Forgery (CSRF):** Browser `GET` requests bypass
     Cross-Origin Resource Sharing (CORS) preflight checks, exposing backend
     RPC services to cross-origin execution.
   - **Query String Data & Credential Leaks:** Base64-encoded protobuf request
     bodies in URL query strings leak sensitive customer data, personally
     identifiable information (PII), and tokens into proxy access logs, content
     delivery network (CDN) logs, browser histories, and `Referer` headers.
   - **Shared Cache Poisoning:** Caching RPC responses across authenticated
     sessions violates RFC 9111 Section 3.5 without complex, non-standard
     cache-partitioning headers.
4. **Core Kernel Invariant:** The `pbrs-grpc` wire kernel
   (`pbrs-grpc/src/wire/headers.rs::check_request`, invoked from
   `pbrs-grpc/src/server/connection.rs`) strictly enforces
   `request.method() == http::Method::POST`, returning HTTP 405
   (`Method Not Allowed`) immediately before handler allocation. Globally
   relaxing this check would violate `PROTOCOL-HTTP2.md` and compromise server
   denial-of-service resilience.

### Disposition
**`cacheable_unary` is recommended as `not_applicable` to the
standard gRPC over HTTP/2 full profile.** The registry row in
`tests/interop/cases.json` records that recommended disposition, but the
verdict is **not final until the maintainer approves it** (gate; see §9).
No POST, authorization, or caching rule is relaxed by this card.

---

## 2. Upstream Specification & Implementation Status

### 2.1 The Official gRPC over HTTP/2 Wire Specification
The normative reference for gRPC wire transport is `doc/PROTOCOL-HTTP2.md` in `grpc/grpc`. In the **Requests** grammar:

```abnf
Request → Request-Headers *Length-Prefixed-Message EOS
Request-Headers → Call-Definition *Custom-Metadata
Call-Definition → Method Scheme Path [Authority] TE [Timeout] Content-Type [Message-Type] [Message-Encoding] [Message-Accept-Encoding] [User-Agent]
Method → ":method POST"
```

The protocol definition contains **no provision for HTTP `GET`**. A server
that complies with `PROTOCOL-HTTP2.md` must receive `:method POST` for every
valid gRPC request.

### 2.2 History of `cacheable_unary` (PR #8101 & Issue #18230)
Upstream PR #8101 ("Add interop test for Cacheable Unary Calls", opened
2016-09-14 by `makdharma`, merged 2016-10-12) added `CacheableUnaryCall` to
`src/proto/grpc/testing/test.proto` and added the test description to
`doc/interop-test-descriptions.md`:
> *"This test verifies that gRPC requests marked as cacheable use GET verb instead of POST, and that server sets appropriate cache control headers for the response to be cached by a proxy. This test requires that the server is behind a caching proxy. Use of current timestamp in the request prevents accidental cache matches left over from previous tests."*

The pinned description further requires a caching proxy in front of the
server, an `x-user-ip: 1.2.3.4` header (since GFE will not cache
localhost requests), and a client-side cacheable flag, noting that
*"longer term this should be driven by the method option specified in the
proto file itself"* — i.e. even the procedure text admits the signaling is
unsettled.

However:

- **No gRFC Proposal:** The change was merged without a corresponding proposal
  in `grpc/proposal`. To date, no gRFC cited by any pinned source, such as an
  A-series architecture document, defines GET method mapping, query string
  serialization, or cache-control behavior for gRPC.
- **Omission from Active Test Runners:** The procedure is absent from the
  canonical interop test runner (`tools/run_tests/run_interop_tests.py`) —
  zero `cacheable` matches in the full 60,739-byte pinned file at
  `d1487957db6658bc532b72871775148229836627` — so it runs in no language
  pair of the active matrix.
- **Maintainer Clarification (Issue #18230):** In upstream issue #18230 (*"GET method is mentioned in interop test, but not in the spec"*), a gRPC maintainer (lidizheng, 2019-09-05) confirmed:
  > *"Cacheable is an experimental feature... that currently only C++ implemented. And it is not tested on regular basis based on the interop matrix... By default, gRPC is running on POST, it is specified in the Requests section of gRPC over HTTP2. The GET method means getting cached response which has different semantic than POST."*

### 2.3 Peer Language Runtime Audit
| Implementation | GET Support Status | In-Tree Evidence |
|---|---|---|
| **C++ Core (`grpc/grpc@d1487957`, pinned)** | **Actively Rejected in Server** | `src/core/ext/filters/http/server/http_server_filter.cc`: `case HttpMethodMetadata::kGet: return MalformedRequest("Bad method header");`. Flags in `grpc_types.h` (`GRPC_INITIAL_METADATA_CACHEABLE_REQUEST`) remain unstandardized. |
| **Java (`grpc-java`, unpinned)** | **Self-Declared Broken** | `TestServiceClient.java` (master): `// THIS TEST IS BROKEN. Enabling safe just on the MethodDescriptor does nothing by itself. This test would need to enable GET on the channel.` Supporting evidence only; no grpc-java pin in `tests/interop/cases.json`. |
| **Go (`grpc-go@dd51b1c9`, pinned)** | **Never Implemented** | Pinned `interop/client/client.go` has zero `cacheable` references; unary calls are strictly `POST`. |
| **Rust (`pbrs-grpc`)** | **Rejected with HTTP 405** | `wire/headers.rs::check_request` rejects non-POST requests with HTTP 405 `Method Not Allowed` per `PROTOCOL-HTTP2.md`. |

---

## 3. Protocol & Wire Semantics Analysis

### 3.1 HTTP GET vs. POST in gRPC Framing
gRPC is built around a length-prefixed binary framing mechanism:

```
Length-Prefixed-Message → Compressed-Flag(1 byte) + Message-Length(4 bytes) + Message-Data(N bytes)
```

In HTTP/2 (RFC 9113) and HTTP Semantics (RFC 9110 Section 9.3.1):

- `GET` is defined to retrieve whatever information is identified by the Request-URI.
- Sending a request body with `GET` has no defined semantic meaning.
  Intermediaries such as reverse proxies, CDNs, and API gateways routinely drop
  request bodies on `GET` requests or reject them with HTTP 400.
- Because a `GET` request cannot carry a standard request body through
  intermediate proxies, an HTTP `GET` mapping cannot use normal gRPC
  length-prefixed message streaming.

### 3.2 URL Payload Encoding & Limitations
To work around the missing `GET` body, experimental implementations attempted
to encode the serialized protobuf request into the URL path query string, for
example `/<service>/<method>?<encoded-data>`:

1. **Lack of Uniform Query Format:**
   - In `grpc-java`'s experimental `NettyClientStream.java`, the code appended `path + "?" + Base64(payload)` with a source comment:
     ```java
     // Forge the query string
     // TODO(ericgribkoff) Add the key back to the query string
     ```
   - There was never an agreed specification for whether the query was raw
     Base64, URL-safe Base64 without padding, whether a query key such as
     `?message=` was required, or whether the 5-byte length-prefix envelope was
     included or stripped.
2. **URI Length Overflow (HTTP 414):**
   - Base64 encoding expands binary serialized protobufs by ~33%.
   - Industry-standard reverse proxies, CDNs, and web servers enforce strict URI length limits:
     - AWS ALB / API Gateway: 8 KB
     - Cloudflare / Akamai: 4 KB – 8 KB
     - NGINX: `large_client_header_buffers` (typically 4 KB – 8 KB)
   - Any gRPC message with moderate payloads, such as repeated IDs, embedded
     strings, vectors, or metadata, exceeds these limits and causes immediate
     HTTP 414 (`URI Too Long`) failures.

### 3.3 HTTP Caching Headers & Intermediary Behavior
The experimental procedure specifies that the server emits:

```http
cache-control: max-age=60, public
```

Under RFC 9111:

- Caches key responses using the request method, target URI, and optional secondary keys (`Vary`).
- Standard caching proxies, including Squid, Varnish, NGINX, and Cloudflare,
  are unaware of gRPC framing. They do not parse HTTP/2 trailer frames
  (`grpc-status`, `grpc-message`).
- If a server responds with an error status in trailers (`grpc-status: 13` /
  `INTERNAL`) but emits HTTP 200 in the headers, a generic HTTP cache can store
  and serve the corrupted or errored RPC response to later clients.

---

## 4. Security Hazards of Cacheable Unary via GET

Globally relaxing the gRPC HTTP/2 transport to accept `GET` requests introduces
critical security vulnerabilities.

### 4.1 Cross-Site Request Forgery (CSRF)
- Under the Fetch and CORS specifications, HTTP `GET` requests are "simple
  requests" and do not trigger a CORS preflight (`OPTIONS`) request.
- gRPC requests require `POST` with `Content-Type: application/grpc`, which
  mandates a CORS preflight.
- If a gRPC server allows `GET` execution, a malicious website visited by a
  user can trigger arbitrary read RPCs against internal or local services, such
  as `http://localhost:8080/service/GetUserData`. The browser automatically
  attaches ambient credentials: cookies, HTTP basic auth, and TLS client
  certificates.

### 4.2 Query String Information Disclosure
- Request payloads in URLs are written to:
  1. Access logs of intermediate L7 proxies, CDNs, and load balancers.
  2. Browser histories and browser caches.
  3. Outbound `Referer` / `Referrer` headers when web clients navigate to external domains.
  4. SIEM, observability, and distributed tracing ingestion pipelines (e.g. OpenTelemetry URL attributes).
- In production systems, RPC arguments frequently contain personally
  identifiable information (PII), authentication tokens, user IDs, and secrets.
  Moving request data from the encrypted HTTP/2 `DATA` frame into URL query
  strings violates security and compliance standards such as GDPR and PCI-DSS.

### 4.3 Authorization & Shared Cache Poisoning (RFC 9111 Section 3.5)
- In microservice architectures, gRPC calls routinely transmit caller
  credentials in `authorization` metadata.
- RFC 9111 Section 3.5 prohibits shared caches from reusing responses to
  requests with an `Authorization` header unless explicit `public` directives
  are present.
- If a gRPC service naively marks an authenticated user's `CacheableUnaryCall`
  as `public`, intermediate shared caches will serve user A's private data to
  user B on subsequent requests.

---

## 5. Architectural Evaluation for `pbrs-grpc`

### 5.1 Request Verification in `pbrs-grpc`
In `pbrs-grpc/src/wire/headers.rs` (`check_request`, called from
`pbrs-grpc/src/server/connection.rs` before any handler is spawned), request
validation is strictly enforced at stream arrival:

```rust
pub(crate) fn check_request(
    request: &Request<RecvStream>,
    accept_gzip: bool,
) -> Result<(), RequestReject> {
    if request.method() != http::Method::POST {
        return Err(RequestReject::Http(StatusCode::METHOD_NOT_ALLOWED));
    }
    let Some(ct) = request.headers().get(http::header::CONTENT_TYPE) else {
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    };
    let Ok(ct) = ct.to_str() else {
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    };
    if !grpc_content_type(ct) {
        return Err(RequestReject::Http(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    }
    // ...
    Ok(())
}
```

This design guarantees three things:

1. Malformed or non-gRPC HTTP requests are rejected with pure HTTP error status
   codes (405 Method Not Allowed, 415 Unsupported Media Type) without
   allocating server handler tasks or consuming RPC slots.
2. The server complies strictly with `doc/PROTOCOL-HTTP2.md`.
3. Relaxing this validation to permit `GET` would compromise fast-path
   rejection and introduce ambiguity between streaming and unary dispatch.

### 5.2 Test Handler in `pbrs-grpc/src/testing.rs`
`pbrs-grpc/src/testing.rs` implements `InteropTestService` (and the
`SizedInteropTestService` variant identically apart from the response cap):
```rust
    /// Identical to `UnaryCall`; the interop suite only cares that it answers.
    async fn cacheable_unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        unary_call_impl(MAX_INTEROP_RESPONSE_BODY_SIZE, request).await
    }
```
When invoked over normal gRPC `POST`, `cacheable_unary_call` behaves exactly
like `unary_call`. That satisfies the generated protobuf service trait
(`proto/grpc/testing/test.proto` declares
`rpc CacheableUnaryCall(SimpleRequest) returns (SimpleResponse)`) without
changing transport rules. No GET client procedure exists in
`pbrs-grpc/src/interop_cases.rs` or the interop CLI binaries, and the
handler emits no `cache-control` headers — deliberately, per §4.

---

## 6. Official Resolution & Maintenance Decision

### 6.1 Disposition in `tests/interop/cases.json`
`tests/interop/cases.json` records the recommended classification:

- **Case:** `cacheable_unary`
- **Procedure source:** The pinned `doc/interop-test-descriptions.md` description, not `run_interop_tests.py`, which omits the case.
- **Disposition:** `not_applicable` (recommended; final only with maintainer approval — see §9)
- **Justification:** Unratified experimental procedure not included in the
  gRPC over HTTP/2 specification (`PROTOCOL-HTTP2.md` requires
  `:method POST`). Absent from the active upstream test runner
  (`run_interop_tests.py`), rejected in C++ core (`HttpServerFilter`),
  self-declared broken in `grpc-java`, and unimplemented in `grpc-go`.
  Recommended for exclusion from the standard HTTP/2 full profile pending
  maintainer approval, due to CSRF and URL credential disclosure hazards.
- **Coverage Status:** `not_applicable` (same gate)

### 6.2 Ecosystem Distinction: Connect Protocol vs. Native gRPC
Protocols such as Connect RPC (`connectrpc.com`) have standardized HTTP `GET`
for unary RPCs using explicit protocol query parameters
(`?message=...&encoding=proto`) and headers
(`connect-protocol-version: 1`).

If Connect protocol support or an edge HTTP gateway is added to the
`pure-protobuf` ecosystem in the future, it must be a separate, opt-in protocol
adapter crate. It needs dedicated CORS, CSRF, and URL-length validation. It
must not globally relax the native gRPC over HTTP/2 transport kernel.

---

## 7. Conditional Follow-ups (Only If the Maintainer Rules the Case Applicable)

The EX-20 accept condition requires separate opt-in fixture cards if
`cacheable_unary` is applicable. The recommendation here is
`not_applicable`, so **no fixture card is created by this card** and no GET
behavior is implemented. If the maintainer instead rules the case applicable,
the coordinator must add bounded cards (with independent proof each) **before**
the case leaves `unsupported`, for example:

1. **Opt-in GET gateway adapter (protocol card):** a separate, disabled-by-default
   adapter that maps one declared-safe unary method to HTTP `GET` with an
   explicit query-serialization format, URL-length cap, and CORS/CSRF rules —
   without touching the native kernel's POST-only `check_request`.
2. **Caching-proxy fixture (client/server card):** a hermetic caching reverse
   proxy plus `cache-control` emission/observation assertions reproducing the
   pinned two-call procedure (`x-user-ip`, timestamp payload, cache-hit
   discrimination).
3. **Credential/cache-partitioning audit (security card):** proof that
   authenticated responses are never served across sessions from the shared
   cache (RFC 9111 §3.5), and that no PII/tokens leak into URLs, logs, or
   `Referer` headers.

Until all such cards land with independent proof, the case must stay
`unsupported`, never `passed`.

---

## 8. Verification Record (2026-09-29, base `fd4500c6`)

Method: read-only raw-file and API reads of the exact pinned revisions
(`third_party/grpc` is absent from this worktree, so nothing was
fetched or cloned). Each item below was re-verified on 2026-09-29:

| # | Claim | Result |
|---|---|---|
| 1 | Pinned `doc/interop-test-descriptions.md@d1487957` describes `cacheable_unary` (GET, caching proxy, `x-user-ip`, cacheable flag, unsettled signaling) | ✅ Verified verbatim |
| 2 | Pinned `doc/PROTOCOL-HTTP2.md@d1487957` mandates `Method → ":method POST"` with no GET provision | ✅ Verified verbatim |
| 3 | Pinned `tools/run_tests/run_interop_tests.py@d1487957` omits the case (no language pair runs it) | ✅ Verified: 0 `cacheable` matches in the complete 60,739-byte file; `_TEST_CASES` enumerated |
| 4 | Pinned C++ `http_server_filter.cc@d1487957` rejects GET | ✅ Verified: `case HttpMethodMetadata::kGet: return MalformedRequest("Bad method header")` |
| 5 | Pinned `grpc-go@dd51b1c9` interop client omits the case | ✅ Verified: 0 `cacheable` matches in `interop/client/client.go` |
| 6 | Upstream issue #18230 maintainer statement (experimental, untested, POST is the spec) | ✅ Verified verbatim via API (lidizheng, CONTRIBUTOR, 2019-09-05) |
| 7 | PR #8101 provenance (experimental C++ origin, 2016) | ✅ Verified via API: "Add interop test for Cacheable Unary Calls", makdharma, opened 2016-09-14, merged 2016-10-12 |
| 8 | `grpc-java` "THIS TEST IS BROKEN" | ⚠️ Verified on `grpc-java` master only — this repo pins no grpc-java revision, so this is supporting evidence, not pinned proof. Channel-builder internals not re-verified. |
| 9 | No GET-mapping gRFC in `grpc/proposal` | ⚠️ No such proposal is cited by any pinned source; exhaustive absence over the whole proposal repo was not proven. |
| 10 | `pbrs-grpc` POST-only kernel + POST-only `cacheable_unary_call` handler | ✅ Verified in-tree: `wire/headers.rs::check_request` → 405, called from `server/connection.rs`; `testing.rs` delegates to `unary_call_impl`; no GET procedure in `interop_cases.rs` or CLI binaries |

Limitations: upstream reads went over the network to GitHub raw/API rather
than a local pinned checkout; SHAs in the URLs above are the control against
drift. The `grpc-java` row and the gRFC-absence statement are explicitly
weaker than the pinned rows and are not load-bearing for the verdict: rows
1–7 plus 10 suffice.

---

## 9. Open Questions for Maintainer

The `not_applicable` verdict is gated on maintainer approval. Please rule on:

1. **Verdict approval:** Do you approve `not_applicable` for `cacheable_unary`
   in the standard gRPC over HTTP/2 full profile on the cited upstream
   status (spec mandates POST; case absent from the pinned runner; C++ core
   rejects GET; Go omits it; upstream maintainer calls it experimental and
   untested)? If yes, this card's approval evidence closes the EX-20 gate.
2. **Registry wording:** Is the recommended `justification`/`notes` text in
   `tests/interop/cases.json` (which now says "pending maintainer approval")
   acceptable as the final record once you approve, or do you want it
   reworded?
3. **Applicability alternative:** If you rule the case applicable instead,
   do you want the three conditional follow-up cards in §7 filed as written,
   or rescoped (e.g. a single Connect-adapter track instead of a native GET
   path)?
4. **grpc-java pin:** Should a future GT-06 drift pass add a `grpc-java` pin
   so the "broken" peer evidence becomes pinned proof, or is the current
   pinned C++/Go/runner/spec evidence sufficient permanently?
