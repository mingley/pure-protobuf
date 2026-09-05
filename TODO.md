# TODO: Production Readiness Ramp

Prioritized task tracker for production readiness across the `pure-protobuf` workspace.

---

## High Priority (P0: Required for Beta / Initial Production)

- [ ] **`pbrs-grpc`**: Implement asynchronous DNS name resolution for client channels (`dns:///` resolver).
- [ ] **`pbrs-grpc`**: Implement basic client-side load balancing (`pick_first` and `round_robin` across resolved addresses).
- [ ] **`pbrs-grpc`**: Fix platform-specific loopback test (`127.0.0.2` on macOS/BSD) by binding to ephemeral ports on `127.0.0.1`.
- [ ] **`pbrs`**: Add derive option for `serde::Serialize` / `serde::Deserialize` in `pbrs::codegen`.
- [ ] **`protobuf-tonic`**: Provide prost migration guide and compatibility shim for hybrid services.

---

## Medium Priority (P1: Robustness & Scale)

- [ ] **`pbrs-grpc`**: Implement gRPC Service Config retries (`retryPolicy` with backoff and jitter).
- [ ] **`pbrs-grpc`**: Add outbound HTTP `CONNECT` proxy support (`HTTP_PROXY` / `HTTPS_PROXY`).
- [ ] **`pbrs-grpc`**: Add OpenTelemetry tracing instrumentation to `Server` and `Channel`.
- [ ] **`pbrs`**: Implement Protobuf Edition 2024 features and syntax extensions.
- [ ] **`pbrs`**: Optimize field-wise JSON/Text formatters for `Any`, `Struct`, and `Value`.
- [ ] **CI**: Add automated cross-language interop matrix running continuously against Go and C++ gRPC reference implementations in GitHub Actions.

---

## Low Priority (P2: Enhancements & Extensions)

- [ ] **`pbrs`**: Support `no_std` + `alloc` for embedded and WebAssembly targets.
- [ ] **`pbrs-grpc`**: Support gRPC-Web protocol framing for browser clients.
- [ ] **`pbrs-grpc`**: Add request hedging for p99 latency reduction.
- [ ] **Docs**: Create complete microservice tutorials featuring authentication, TLS, reflection, and health checking.
