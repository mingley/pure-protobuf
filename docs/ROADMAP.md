# Production Readiness Roadmap

This document outlines the clear, phased ramp to bring all crates in the `pure-protobuf` workspace to production-ready **v1.0.0**.

---

## Production Readiness Criteria by Crate

### 1. `pbrs` (Core Kernel)
- **Current Status**: `v0.1.0` (Production-grade core; passes 100% of official Google conformance tests).
- **Readiness Bar for 1.0**:
  - [ ] **MSRV & API Stability**: Formally guarantee MSRV (currently Rust 1.85+) and semver stability for all `pbrs::prelude::*` public APIs.
  - [ ] **Protobuf Edition 2024**: Implement features and extensions for Protobuf Edition 2024 (from Google Protobuf v29+).
  - [ ] **Optional `no_std` Support**: Provide `default-features = false` with `alloc` support for embedded, kernel, and WASM runtimes.
  - [ ] **Direct Serde Derives**: Optional derive feature for direct `serde::Serialize` and `serde::Deserialize` on generated structs without passing through `DynamicMessage`.
  - [ ] **Specialized WKT Formatters**: Dedicated field-wise JSON and Text formatters for `Any`, `Struct`, `Value`, `ListValue`, and `FieldMask` to eliminate dynamic reflection overhead.

---

### 2. `pbrs-grpc` (Native HTTP/2 gRPC Kernel)
- **Current Status**: `v0.1.0-alpha.1` (Preview / Pre-release).
- **Readiness Bar for 1.0**:
  - [ ] **Service Discovery & Name Resolution**:
    - Built-in asynchronous DNS resolver (`dns:///host:port`).
    - Pluggable `Resolver` trait for custom service discovery (Kubernetes endpoints, Consul, static IP lists).
  - [ ] **Client-Side Load Balancing**:
    - Subchannel management and connection pooling across multiple resolved endpoints.
    - Standard load balancing policies: `pick_first` (default) and `round_robin`.
  - [ ] **Configurable Retries & Hedging**:
    - gRPC Service Config (`methodConfig.retryPolicy`) with exponential backoff, retryable status codes, and max attempts.
    - Concurrent request hedging for latency-critical paths.
  - [ ] **Network & Proxy Traversal**:
    - Outbound HTTP `CONNECT` proxy support (`HTTPS_PROXY` / `HTTP_PROXY`).
    - Platform-resilient loopback binding on macOS and BSD systems.
  - [ ] **Observability**:
    - OpenTelemetry tracing spans and metrics for RPC lifecycle events.
    - Structured logging hooks for connection lifecycle and transport errors.

---

### 3. `protobuf-tonic` (Tonic 0.14+ Codec Adapter)
- **Current Status**: `v0.1.0-alpha.1` (Preview / Pre-release).
- **Readiness Bar for 1.0**:
  - [ ] **Trailers Handling**:
    - Document and provide ergonomic helpers for custom metadata trailers across unary and streaming RPCs.
  - [ ] **Prost Migration Bridge**:
    - Provide optional conversion helpers and documentation for gradual migration from `prost` to `pbrs` in existing Tonic applications.
  - [ ] **Tonic Compatibility Tracking**:
    - Continuous validation and tracking of new upstream Tonic releases (0.14+).

---

## Implementation Phases

| Phase | Focus | Target Milestones |
|---|---|---|
| **Phase 1: Stabilization & Transport Hardening** | Resolve platform quirks, add basic DNS resolution, and expand code generation helpers. | `pbrs 0.1.1`, `pbrs-grpc 0.1.0-alpha.2`, `protobuf-tonic 0.1.0-alpha.2` |
| **Phase 2: Discovery, Load Balancing & Retries** | Subchannel load balancing, DNS resolver, configurable retry policies, and `no_std` core support. | `pbrs 0.2.0`, `pbrs-grpc 0.2.0-beta.1` |
| **Phase 3: Production GA (1.0.0)** | Long-running soak testing, cross-language conformance suites in CI, API freeze, and formal MSRV stability. | `v1.0.0` for all crates |
