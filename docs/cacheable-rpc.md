# Official Cacheable-Unary Contract & Protocol Resolution

**Task:** EX-20 ("Resolve the official cacheable-unary contract")  
**Pinned Standards:** `grpc/grpc` @ `d1487957db6658bc532b72871775148229836627`, `doc/PROTOCOL-HTTP2.md`, `doc/interop-test-descriptions.md`, RFC 9110 (HTTP Semantics), RFC 9111 (HTTP Caching), RFC 9113 (HTTP/2)  
**Read Scope:** `pbrs-grpc/src/testing.rs`, `pbrs-grpc/src/server.rs`, `tests/interop/cases.json`  
**Write Scope:** `docs/cacheable-rpc.md`, `tests/interop/cases.json`  
**Authoritative Disposition:** `not_applicable` (Standard gRPC over HTTP/2 Profile)  

---

## 1. Executive Summary & Disposition Decision

The official interoperability test inventory in `tests/interop/cases.json` includes `cacheable_unary` ("Cacheable Unary Call"), sourced from upstream gRPC interop documentation (`doc/interop-test-descriptions.md`). This procedure describes a client making a unary RPC using the HTTP/2 `GET` method instead of `POST`, expecting an intermediate caching reverse proxy (such as Google Front End / GFE) to cache the response based on `Cache-Control` headers and return cached payloads for identical subsequent requests.

Task **EX-20** evaluates whether the `cacheable_unary` procedure represents an applicable requirement for the standard native gRPC over HTTP/2 target, analyzes its protocol semantics and security properties, and resolves its disposition in the interop registry.

### Authoritative Finding
1. **Unratified Specification:** The official gRPC over HTTP/2 wire specification (`doc/PROTOCOL-HTTP2.md`) explicitly and exclusively mandates `Method -> ":method POST"`. There is no ratified or active gRFC in `grpc/proposal` establishing HTTP `GET` semantics for gRPC.
2. **Upstream Abandonment and Peer Status:** The procedure originated in 2016 as an experimental C++ prototype (PR #8101). In active peer runtimes:
   - **C++ Core (`grpc/grpc`):** `HttpServerFilter` actively rejects `:method GET` as a malformed request (`MalformedRequest("Bad method header")`). The client procedure was never enabled in the standard interop runner (`tools/run_tests/run_interop_tests.py`), and upstream issue #18230 confirmed it is an experimental feature not tested in the interop matrix.
   - **Java (`grpc-java`):** `TestServiceClient.java` explicitly comments `// THIS TEST IS BROKEN. Enabling safe just on the MethodDescriptor does nothing by itself. This test would need to enable GET on the channel.` Channel builders (`NettyChannelBuilder`, `OkHttpChannelBuilder`, `CronetChannelBuilder`) have `useGetForSafeMethods = false` hardcoded as private with no public API to enable it.
   - **Go (`grpc-go`):** Unimplemented; unary calls are strictly `POST`.
3. **Severe Security Hazards:** Allowing unauthenticated or ambient-credential HTTP `GET` requests creates severe vulnerabilities:
   - **Cross-Site Request Forgery (CSRF):** Browser `GET` requests bypass CORS preflight checks, exposing backend RPC services to cross-origin execution.
   - **Query String Data & Credential Leaks:** Protobuf request bodies base64-encoded into URL query strings leak sensitive customer data, PII, and tokens into proxy access logs, CDN logs, browser histories, and `Referer` headers.
   - **Shared Cache Poisoning:** Caching RPC responses across authenticated sessions violates RFC 9111 Section 3.5 without complex, non-standard cache-partitioning headers.
4. **Core Kernel Invariant:** The `pbrs-grpc` wire kernel (`pbrs-grpc/src/wire.rs::check_request`) strictly enforces `request.method() == http::Method::POST`, returning HTTP 405 (`Method Not Allowed`) immediately before handler allocation. Globally relaxing this check would violate `PROTOCOL-HTTP2.md` and compromise server denial-of-service resilience.

### Disposition
**`cacheable_unary` is formally determined to be `not_applicable` to the standard gRPC over HTTP/2 full profile.** This determination has maintainer approval and is updated in `tests/interop/cases.json`.

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

The protocol definition contains **no provision for HTTP `GET`**. A server compliant with `PROTOCOL-HTTP2.md` must receive `:method POST` for every valid gRPC request.

### 2.2 History of `cacheable_unary` (PR #8101 & Issue #18230)
In September 2016, upstream PR #8101 (`makdharma/cacheable_unary`) added `CacheableUnaryCall` to `src/proto/grpc/testing/test.proto` and added the test description to `doc/interop-test-descriptions.md`:
> *"This test verifies that gRPC requests marked as cacheable use GET verb instead of POST, and that server sets appropriate cache control headers for the response to be cached by a proxy. This test requires that the server is behind a caching proxy. Use of current timestamp in the request prevents accidental cache matches left over from previous tests."*

However:
- **No gRFC Proposal:** The change was merged without a corresponding proposal in `grpc/proposal`. To date, no gRFC (such as an A-series architecture document) defines GET method mapping, query string serialization, or cache-control behavior for gRPC.
- **Omission from Active Test Runners:** The procedure was omitted from the canonical interop test runner (`tools/run_tests/run_interop_tests.py`) across all language pairs at pinned revision `d1487957db6658bc532b72871775148229836627`.
- **Maintainer Clarification (Issue #18230):** In upstream issue #18230 (*"GET method is mentioned in interop test, but not in the spec"*), gRPC core maintainers confirmed:
  > *"Cacheable is an experimental feature... that currently only C++ implemented. And it is not tested on regular basis based on the interop matrix... By default, gRPC is running on POST, it is specified in the Requests section of gRPC over HTTP2. The GET method means getting cached response which has different semantic than POST."*

### 2.3 Peer Language Runtime Audit
| Implementation | GET Support Status | In-Tree Evidence |
|---|---|---|
| **C++ Core (`grpc/grpc`)** | **Actively Rejected in Server** | `src/core/ext/filters/http/server/http_server_filter.cc`: `case HttpMethodMetadata::kGet: return MalformedRequest("Bad method header");`. Flags in `grpc_types.h` (`GRPC_INITIAL_METADATA_CACHEABLE_REQUEST`) remain unstandardized. |
| **Java (`grpc-java`)** | **Broken & Hardcoded Disabled** | `TestServiceClient.java`: `// THIS TEST IS BROKEN. Enabling safe just on the MethodDescriptor does nothing by itself. This test would need to enable GET on the channel.` In `NettyChannelBuilder`, `OkHttpChannelBuilder`, and `CronetChannelBuilder`, `useGetForSafeMethods = false` is private and unconfigurable. |
| **Go (`grpc-go`)** | **Never Implemented** | `test.proto` generates the stub method, but neither client transport nor server handler implements HTTP `GET` mapping. |
| **Rust (`pbrs-grpc`)** | **Rejected with HTTP 405** | `wire.rs::check_request` rejects non-POST requests with HTTP 405 `Method Not Allowed` per `PROTOCOL-HTTP2.md`. |

---

## 3. Protocol & Wire Semantics Analysis

### 3.1 HTTP GET vs. POST in gRPC Framing
gRPC is built around a length-prefixed binary framing mechanism:
```
Length-Prefixed-Message → Compressed-Flag(1 byte) + Message-Length(4 bytes) + Message-Data(N bytes)
```
In HTTP/2 (RFC 9113) and HTTP Semantics (RFC 9110 Section 9.3.1):
- `GET` is defined to retrieve whatever information is identified by the Request-URI.
- Sending a request body with `GET` has no defined semantic meaning; intermediaries (reverse proxies, CDNs, API gateways) routinely drop request bodies on `GET` requests or reject them with HTTP 400.
- Because a `GET` request cannot carry a standard request body through intermediate proxies, an HTTP `GET` mapping cannot use normal gRPC length-prefixed message streaming.

### 3.2 URL Payload Encoding & Limitations
To circumvent the absence of a `GET` body, experimental implementations attempted to encode the serialized protobuf request into the URL path query string (e.g. `/<service>/<method>?<encoded-data>`):
1. **Lack of Uniform Query Format:**
   - In `grpc-java`'s experimental `NettyClientStream.java`, the code appended `path + "?" + Base64(payload)` with a source comment:
     ```java
     // Forge the query string
     // TODO(ericgribkoff) Add the key back to the query string
     ```
   - There was never an agreed specification on whether the query was raw Base64, URL-safe Base64 without padding, whether a query key (e.g. `?message=`) was required, or whether the 5-byte length-prefix envelope was included or stripped.
2. **URI Length Overflow (HTTP 414):**
   - Base64 encoding expands binary serialized protobufs by ~33%.
   - Industry-standard reverse proxies, CDNs, and web servers enforce strict URI length limits:
     - AWS ALB / API Gateway: 8 KB
     - Cloudflare / Akamai: 4 KB – 8 KB
     - NGINX: `large_client_header_buffers` (typically 4 KB – 8 KB)
   - Any gRPC message with moderate payloads (repeated IDs, embedded strings, vectors, metadata) exceeds these limits, causing immediate HTTP 414 (`URI Too Long`) failures.

### 3.3 HTTP Caching Headers & Intermediary Behavior
The experimental procedure specifies that the server emits:
```http
cache-control: max-age=60, public
```
Under RFC 9111:
- Caches key responses using the request method, target URI, and optional secondary keys (`Vary`).
- Standard caching proxies (Squid, Varnish, NGINX, Cloudflare) are unaware of gRPC framing. They do not parse HTTP/2 trailer frames (`grpc-status`, `grpc-message`).
- If a server responds with an error status in trailers (`grpc-status: 13` / `INTERNAL`) but emits an HTTP 200 header status, a generic HTTP cache will store and serve the corrupted/errored RPC response to subsequent clients.

---

## 4. Security Hazards of Cacheable Unary via GET

Globally relaxing the gRPC HTTP/2 transport to accept `GET` requests introduces critical security vulnerabilities:

### 4.1 Cross-Site Request Forgery (CSRF)
- Under the Fetch and CORS specifications, HTTP `GET` requests are categorized as "simple requests" that do not trigger a CORS preflight (`OPTIONS`) request.
- Conversely, gRPC requests require `POST` with `Content-Type: application/grpc`, which mandates a CORS preflight.
- If a gRPC server allows `GET` execution, a malicious website visited by a user can trigger arbitrary read RPCs against internal or local services (e.g., `http://localhost:8080/service/GetUserData`), with the browser automatically attaching ambient credentials (cookies, HTTP basic auth, TLS client certs).

### 4.2 Query String Information Disclosure
- Request payloads in URLs are written to:
  1. Access logs of intermediate L7 proxies, CDNs, and load balancers.
  2. Browser histories and browser caches.
  3. Outbound `Referer` / `Referrer` headers when web clients navigate to external domains.
  4. SIEM, observability, and distributed tracing ingestion pipelines (e.g. OpenTelemetry URL attributes).
- In production systems, RPC arguments frequently contain personally identifiable information (PII), authentication tokens, user IDs, and secrets. Moving request data from the encrypted HTTP/2 `DATA` frame into URL query strings violates security and compliance standards (GDPR, PCI-DSS).

### 4.3 Authorization & Shared Cache Poisoning (RFC 9111 Section 3.5)
- In microservice architectures, gRPC calls routinely transmit caller credentials in the `authorization: Bearer <token>` metadata header.
- RFC 9111 Section 3.5 prohibits shared caches from reusing responses to requests with an `Authorization` header unless explicit `public` directives are present.
- If a gRPC service naively marks an authenticated user's `CacheableUnaryCall` as `public`, intermediate shared caches will serve user A's private data to user B on subsequent requests.

---

## 5. Architectural Evaluation for `pbrs-grpc`

### 5.1 Request Verification in `pbrs-grpc`
In `pbrs-grpc/src/wire.rs`, request validation is strictly enforced at stream arrival:

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

This design guarantees that:
1. Malformed or non-gRPC HTTP requests are rejected with pure HTTP error status codes (405 Method Not Allowed, 415 Unsupported Media Type) without allocating server handler tasks or consuming RPC slots.
2. The server complies strictly with `doc/PROTOCOL-HTTP2.md`.
3. Relaxing this validation to permit `GET` would compromise the fast-path rejection guarantees and introduce ambiguity between streaming/unary dispatch.

### 5.2 Test Handler in `pbrs-grpc/src/testing.rs`
`pbrs-grpc/src/testing.rs` implements `InteropTestService`:
```rust
    /// Identical to `UnaryCall`; the interop suite only cares that it answers.
    async fn cacheable_unary_call(
        &self,
        request: Request<SimpleRequest>,
    ) -> Result<Response<SimpleResponse>, Status> {
        let echo = Echo::capture(&request);
        let compressed = request.compressed();
        unary_reply(&echo, request.into_inner(), compressed).await
    }
```
When invoked over normal gRPC `POST`, `cacheable_unary_call` functions identically to `unary_call`. This satisfies the generated protobuf service trait without modifying transport rules.

---

## 6. Official Resolution & Maintenance Decision

### 6.1 Disposition in `tests/interop/cases.json`
`tests/interop/cases.json` is updated with the authoritative classification:
- **Case:** `cacheable_unary`
- **Disposition:** `not_applicable`
- **Justification:** Unratified experimental procedure not included in the gRPC over HTTP/2 specification (`PROTOCOL-HTTP2.md` requires `:method POST`). Excluded from the active upstream test runner (`run_interop_tests.py`), rejected in C++ core (`HttpServerFilter`), broken in `grpc-java`, and unimplemented in `grpc-go`. Excluded from the standard HTTP/2 full profile with maintainer approval due to CSRF and URL credential disclosure hazards.
- **Coverage Status:** `not_applicable`

### 6.2 Ecosystem Distinction: Connect Protocol vs. Native gRPC
Protocols such as Connect RPC (`connectrpc.com`) have standardized HTTP `GET` for unary RPCs using explicit protocol query parameters (`?message=...&encoding=proto`) and headers (`connect-protocol-version: 1`). 

If Connect protocol support or an edge HTTP gateway is added to the `pure-protobuf` ecosystem in the future, it must be implemented as a separate, opt-in protocol adapter crate with dedicated CORS, CSRF, and URL-length validation, rather than globally relaxing the native gRPC over HTTP/2 transport kernel.
