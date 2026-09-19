# Codegen Compatibility & Evolution Fixtures

This directory contains test fixtures and frozen versioned generated code for schema evolution,
forward and backward binary compatibility, presence semantics, and gRPC stub evolution across versions.

These fixtures serve as the authoritative test oracle for:
- **CG-11**: Test generated-code and runtime evolution.

---

## 1. Schema Inventory

| Schema Path | Package | Status | Key Features |
|---|---|---|---|
| `v1/compat.proto` | `compat` | Initial (v1) | Scalar fields (`id`, `name`), optional fields (`description`, `priority`), repeated fields (`tags`, `scores`), map field (`properties`), enum (`Status`), oneof (`payload`), legacy field (`legacy_field`), and service `CompatService` with 4 call shapes. |
| `v2/compat.proto` | `compat` | Evolved (v2) | Added scalar/optional/repeated/map fields (`extra_info`, `timestamp`, `categories`, `flags`), reordered field declarations, reserved field numbers & names (retired `legacy_field` -> `reserved 11`), added enum variants (`STATUS_SUSPENDED`, `STATUS_ARCHIVED`), added oneof variant (`boolean_payload`), and new service methods (`NewUnaryCall`, `NewServerStreamCall`). |

---

## 2. Frozen Generated Consumers

| File | Stub Flavour | Runtime Dependencies | Purpose |
|---|---|---|---|
| `v1_generated.rs` | `stubs=none` | `pbrs` | Frozen v1 generated messages & enums. Tests old-generated code against current/new runtime without external dependencies. |
| `v2_generated.rs` | `stubs=none` | `pbrs` | Frozen v2 generated messages & enums. Tests regenerated code with schema extensions against current runtime. |
| `v1_with_stubs.rs` | `stubs=kernel` | `pbrs`, `pbrs-grpc` | Frozen v1 generated code including gRPC client/server stubs for all 4 call shapes. |
| `v2_with_stubs.rs` | `stubs=kernel` | `pbrs`, `pbrs-grpc` | Frozen v2 generated code including extended gRPC client/server stubs with new methods. |

---

## 3. Schema Evolution Invariants Tested

1. **Forward Binary Compatibility (Old Consumer Reading New Wire Data)**:
   - A v1 consumer deserializing bytes encoded by v2 must successfully parse all shared fields.
   - All fields unknown to v1 (tags 12, 13, 14, 15, 16) must be preserved in `v1.unknown`.
   - Re-serializing the v1 message must retain the unknown fields bit-for-bit so that a downstream v2 consumer recovers the full payload without loss.

2. **Backward Binary Compatibility (New Consumer Reading Old Wire Data)**:
   - A v2 consumer deserializing bytes encoded by v1 must populate default values for missing fields:
     - Optional fields (`timestamp`) report `has_timestamp() == false`.
     - Singular scalars (`extra_info`) report empty/zero defaults.
     - Repeated and map fields (`categories`, `flags`) report empty collections.
   - Fields retired and marked `reserved` in v2 (e.g. tag 11 `legacy_field`) must be preserved as unknown fields in v2 and survive roundtrips back to v1.

3. **Presence Semantics & Mutation Invalidation**:
   - Explicit presence (`optional`) fields must maintain accurate `has_*` and `clear_*` state.
   - Modifying any field (scalar, optional, repeated, map, or oneof) must dirty `cached_size` and invalidate serialized size caches.
   - Calling `clear()` must reset all fields and clear cached sizes.

4. **gRPC Service Stub Evolution (All 4 Call Shapes)**:
   - All four gRPC communication shapes are supported across versions:
     - **Unary**: `UnaryCall(Req) -> Resp`
     - **Client Streaming**: `ClientStreamCall(stream Req) -> Resp`
     - **Server Streaming**: `ServerStreamCall(Req) -> stream Resp`
     - **Bidirectional Streaming**: `BidiStreamCall(stream Req) -> stream Resp`
   - A v1 client can invoke all 4 methods against a v2 server.
   - When a client calls a method that the server does not implement (such as a v2 client calling `NewUnaryCall` against a v1 server), the runtime must clearly return `Status::unimplemented()`.
