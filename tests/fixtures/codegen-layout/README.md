# Codegen Layout Fixtures & Verification Contract

This directory contains test fixtures and contract definitions for multi-file code generation,
namespace collision avoidance, transitive public re-exports, and external crate path mapping.

These fixtures serve as the authoritative test oracle for:
- **CG-04**: Canonical proto input identity, collision-safe output layout, and public import chaining.
- **CG-05**: External type and runtime crate mappings (`extern_path`, crate renaming).

This README is the prose oracle; [`expected.json`](expected.json) is the
machine-readable oracle (expected files, module paths, field types, and
negative assertions per fixture set).

---

## 1. Fixture Inventory

| File Path | Proto Package | Purpose |
|---|---|---|
| `proto/pkg_a/common.proto` | `pkg.a` | Defines `CommonMsg`, `CommonEnum`, and nested type `NestedA`. Same basename (`common.proto`) as package B. |
| `proto/pkg_b/common.proto` | `pkg.b` | Defines `CommonMsg`, `CommonEnum`, and nested type `NestedB`. Same basename (`common.proto`) as package A, but with different field numbers/types. |
| `proto/pkg_b/service.proto` | `pkg.b` | Imports both `pkg_a/common.proto` and `pkg_b/common.proto`. Defines `ServiceRequest`, `ServiceResponse`, and `EchoService` to verify cross-package references to identical type names. |
| `proto/reexport/grandparent.proto` | `reexport.grandparent` | Root dependency with `GrandparentData` and `GrandparentLevel`. |
| `proto/reexport/parent.proto` | `reexport.parent` | Uses `import public "reexport/grandparent.proto";` and defines `ParentData`. |
| `proto/reexport/child.proto` | `reexport.child` | Uses `import public "reexport/parent.proto";` and defines `ChildData`. Tests multi-hop transitive public re-export without duplicate Rust structs. |
| `proto/external/client.proto` | `consumer.external` | Imports `google/protobuf/timestamp.proto` and `pkg_a/common.proto`. Tests `extern_path` remapping to external crates (`::pbrs::wkt` and `::external_crate`). |
| `proto/external/service.proto` | `consumer.external` | Imports `google/protobuf/timestamp.proto`, `pkg_a/common.proto`, and `external/client.proto`. Tests multi-module external type sharing without duplicate definitions. |

---

## 2. Expected Generated File Structure

When compiling `pkg_a/common.proto`, `pkg_b/common.proto`, and `pkg_b/service.proto` with include root `-I proto`:

### 2.1 File-Mirroring Layout (Default / Multi-File Mode)

Output files mirror canonical relative paths under `OUT_DIR`:

```text
$OUT_DIR/
├── pkg_a/
│   └── common.rs
├── pkg_b/
│   ├── common.rs
│   └── service.rs
└── mod.rs                 <-- Generated root entrypoint
```

**Anti-Collision Invariant**:
- `$OUT_DIR/common.rs` MUST NOT be emitted as a single file overwriting one of the packages.
- If `$OUT_DIR/common.rs` is requested or expected by a consumer, the compiler must fail with an explicit diagnostic naming both `pkg_a/common.proto` and `pkg_b/common.proto`.

**Inclusion Rule**: consumers include `mod.rs` at the crate root
(`include!(concat!(env!("OUT_DIR"), "/mod.rs"));`). Cross-file references are
`crate::`-anchored, so nesting the include under another module does not
compile. Single-file `stem.rs` outputs with no cross-target references remain
includable anywhere.

### 2.2 Package-Centric Layout (Alternative / Flat Package Mode)

When configured for package-file output:

```text
$OUT_DIR/
├── pkg.a.rs               <-- Contains all types in package pkg.a
├── pkg.b.rs               <-- Contains all types in package pkg.b (common + service)
└── mod.rs                 <-- Root module declaring pub mod pkg { pub mod a; pub mod b; }
```

---

## 3. Expected Rust Module Tree & Type Signatures

### 3.1 Module Hierarchy

The generated code exposes a clean module tree matching protobuf packages:

```rust
pub mod pkg {
    pub mod a {
        // From pkg_a/common.proto
        pub struct CommonMsg { ... }
        pub mod common_msg {
            pub struct NestedA { ... }
        }
        pub struct CommonEnum(i32);
    }
    pub mod b {
        // From pkg_b/common.proto
        pub struct CommonMsg { ... }
        pub mod common_msg {
            pub struct NestedB { ... }
        }
        pub struct CommonEnum(i32);

        // From pkg_b/service.proto
        pub struct ServiceRequest { ... }
        pub struct ServiceResponse { ... }
        pub struct EchoServiceClient<C> { ... }
        pub trait EchoService { ... }
    }
}
```

### 3.2 Cross-Package Field Resolution in `pkg::b::ServiceRequest`

In `pkg_b/service.proto`:
```protobuf
message ServiceRequest {
  pkg.a.CommonMsg a_msg = 1;
  pkg.b.CommonMsg b_msg = 2;
  pkg.a.CommonEnum a_status = 3;
  pkg.b.CommonEnum b_status = 4;
  pkg.a.CommonMsg.NestedA nested_a = 5;
  pkg.b.CommonMsg.NestedB nested_b = 6;
}
```

The generated Rust struct in `pkg::b` must reference:
- `a_msg`: `pbrs::rt::LazyMsg<crate::pkg::a::CommonMsg>`
- `b_msg`: `pbrs::rt::LazyMsg<crate::pkg::b::CommonMsg>` (fully qualified even
  for same-package types from another target file; bare idents are only for
  types emitted into the same generated file)
- `a_status`: getter returns `crate::pkg::a::CommonEnum`
- `b_status`: getter returns `crate::pkg::b::CommonEnum`
- `nested_a`: `pbrs::rt::LazyMsg<crate::pkg::a::common_msg::NestedA>`
- `nested_b`: `pbrs::rt::LazyMsg<crate::pkg::b::common_msg::NestedB>`

**Crucial Invariant**:
Neither `CommonMsg` struct should be mangled to `ACommonMsg` or `BCommonMsg`. Their Rust types are both named `CommonMsg`, scoped strictly by module `crate::pkg::a::CommonMsg` and `crate::pkg::b::CommonMsg`.

---

## 4. `import public` Transitive Chain Verification

For the reexport chain:
`reexport/child.proto` -> `reexport/parent.proto` -> `reexport/grandparent.proto`

### 4.1 Expected Output Files

```text
$OUT_DIR/
└── reexport/
    ├── grandparent.rs     <-- Defines GrandparentData & GrandparentLevel
    ├── parent.rs          <-- Defines ParentData; re-exports grandparent
    └── child.rs           <-- Defines ChildData; re-exports parent + grandparent
```

### 4.2 Module Re-export Code

In `parent.rs`:
```rust
// Re-export of public dependency
pub use crate::reexport::grandparent::*;
// Local definitions
pub struct ParentData { ... }
```

In `child.rs`:
```rust
// Transitive re-export: provides both ParentData and GrandparentData
pub use crate::reexport::parent::*;
// Local definitions
pub struct ChildData { ... }
```

### 4.3 Deduplication Assertion

`GrandparentData` and `GrandparentLevel` MUST ONLY be defined once across the crate:
- Struct `GrandparentData` is declared in `reexport::grandparent`.
- `parent.rs` and `child.rs` MUST NOT declare duplicate `pub struct GrandparentData`.
- Downstream consumers can access `GrandparentData` via:
  - `crate::reexport::grandparent::GrandparentData` (direct)
  - `crate::reexport::parent::GrandparentData` (1-hop public re-export)
  - `crate::reexport::child::GrandparentData` (2-hop transitive public re-export)

---

## 5. External Path Mapping Verification (CG-05)

When compiling `proto/external/client.proto`:
```protobuf
package consumer.external;
import "google/protobuf/timestamp.proto";
import "pkg_a/common.proto";

message ExternalPayload {
  string event_id = 1;
  google.protobuf.Timestamp event_time = 2;
  pkg.a.CommonMsg payload = 3;
}
```

Configured with:
```rust
config
    .extern_path(".google.protobuf", "::pbrs::wkt")
    .extern_path(".pkg.a", "::shared_types::pkg::a");
```

### 5.1 Expected Field Types in `ExternalPayload`

- `event_time`: `pbrs::rt::LazyMsg<::pbrs::wkt::Timestamp>`
- `payload`: `pbrs::rt::LazyMsg<::shared_types::pkg::a::CommonMsg>`

### 5.2 External Deduplication Invariant

- Neither `Timestamp` nor `CommonMsg` (from `pkg_a`) is emitted into the generated `client.rs`.
- `client.rs` does not contain `pub struct Timestamp` or `pub struct CommonMsg`.
- Compilation succeeds when linking against the external crate.

### 5.3 Nested Extern Types and Default Ownership

- Nested suffixes below a matched prefix become snake_case module segments:
  `.pkg.a.CommonMsg.NestedA` with `.extern_path(".pkg.a", "::shared_types::pkg::a")`
  resolves to `::shared_types::pkg::a::common_msg::NestedA`.
- Without `extern_path`, each generated file owns private copies of referenced
  WKTs (`Timestamp` emitted locally, referenced as a bare ident), while
  referenced non-WKT types from files outside the target set are referenced
  via `crate::` package paths and require crate-root `mod.rs` inclusion.

---

## 6. Implementation Test Checklist for CG-04 and CG-05

### For CG-04 (Canonical Identity & Collision-Free Layout):

1. **Stem Collision Test**:
   - Compile `pkg_a/common.proto` and `pkg_b/common.proto` together.
   - Assert `OUT_DIR/pkg_a/common.rs` and `OUT_DIR/pkg_b/common.rs` both exist.
   - Assert neither file overwrote the other.
   - Assert a consumer crate including `mod.rs` at the crate root compiles
     with `cargo check` and both `pkg::a::CommonMsg` / `pkg::b::CommonMsg`
     resolve to distinct types.
2. **Ambiguous Stem Request Diagnostic**:
   - Verify that requesting an ambiguous single-file output name `common.rs` fails with an explicit error detailing conflicting files.
3. **Single-File Backwards Compatibility Test**:
   - Compile a single file `proto/person.proto`.
   - Assert `OUT_DIR/person.rs` is emitted.
   - Assert `include!(concat!(env!("OUT_DIR"), "/person.rs"))` compiles with zero warnings or errors.
4. **Public Re-export Transitive Chain Test**:
   - Compile `grandparent.proto`, `parent.proto`, and `child.proto`.
   - In consumer code:
     ```rust
     use reexport::child::{ChildData, GrandparentData, ParentData};
     ```
   - Assert all three symbols resolve from `reexport::child` without ambiguity or duplicate type errors.

### For CG-05 (External Path Mapping & Crate Aliases):

1. **WKT Mapping Test**:
   - Compile `external/client.proto` with `.extern_path(".google.protobuf", "::pbrs::wkt")`.
   - Assert generated struct contains `::pbrs::wkt::Timestamp`.
2. **Custom Crate Remapping Test**:
   - Compile with `.extern_path(".pkg.a", "::my_shared_lib::pkg::a")`.
   - Assert field uses `::my_shared_lib::pkg::a::CommonMsg`.
3. **Renamed Runtime Crate Test**:
   - Compile with `.runtime_crate("::custom_pbrs")`.
   - Assert generated code uses `::custom_pbrs::rt::LazyStr`, `::custom_pbrs::prelude::*`, etc., instead of hardcoded `pbrs::`.
4. **Renamed gRPC Stubs Test**:
   - Compile `pkg_b/service.proto` with `.grpc_crate("::custom_grpc")` or `.tonic_crate("::custom_tonic")`.
   - Assert stubs reference the configured crate names.
