# Protocol Buffers Edition 2024 Semantic Contract & Feature Specification

## 1. Executive Summary & Architectural Scope

This document specifies the authoritative semantic contract for **Protocol Buffers Edition 2024** (`edition = "2024";`, edition number `1001`) in `pure-protobuf` (`pbrs`). It establishes the exact defaults, inheritance rules, visibility mechanics, naming constraints, extension behaviors, and language-option policies required to achieve complete conformance with upstream Protocol Buffers (`protocolbuffers/protobuf@v35.1` / `v36.1`) without relying on the C++ / upb kernel.

This is distinct from the Rust language's Edition 2024 in `Cargo.toml`.
Switching a crate's Rust edition does **not** enable Protobuf Edition 2024
generation; the plugin's advertised maximum remains Edition 2023.

### Deliverables & Contract Boundary

1. **Pinned Edition Defaults**: Complete specification of all global features for Edition 2024 compared to Proto2, Proto3, and Edition 2023.
2. **Inheritance & Resolution Rules**: Exact merging hierarchy from edition defaults through file, message, field, and enum descriptors.
3. **Symbol Visibility & Naming**: Formalization of `export` / `local` modifiers, `default_symbol_visibility`, and `enforce_naming_style = STYLE2024`.
4. **Extensions Contract**: Wire and descriptor semantics for `extensions <start> to <end>;` and `extend` blocks under Edition 2024.
5. **Language-Specific Options Policy**: Unambiguous handling of `(pb.cpp)`, `(pb.java)`, `(pb.go)`, `(pb.python)`, and `(pb.csharp)` feature extensions.
6. **Implementation Mapping & Task Decomposition**: Mapping of every upstream feature to existing code, bounded implementation slices (`CG-13`, `CG-14`), or explicit blockers.
7. **Approved Fixtures & Oracles**: Test schemas, compiled descriptor sets (`.fds`), wire-format vectors (`.bin`), and negative rejection suites in `tests/fixtures/edition2024/`.

> **Constraint Invariant**: `maximum_edition` in `src/codegen.rs` remains frozen at `1000` (`EDITION_2023`) through `CG-13`. Raising it to `1001` is strictly reserved for `CG-14`, only after descriptor resolution semantics and differential test evidence are complete.

---

## 2. Upstream Edition 2024 Feature Set & Default Matrix

Protocol Buffers Edition 2024 represents the second official edition release. It builds upon Edition 2023 by standardizing naming styles, introducing symbol visibility controls, and retiring legacy syntax constructs.

### 2.1 The Core Feature Matrix

| Feature | FeatureSet Field | Targets | Proto2 (`998`) | Proto3 (`999`) | Edition 2023 (`1000`) | Edition 2024 (`1001`) |
|---|---|---|---|---|---|---|
| `field_presence` | Tag 1 | File, Field | `EXPLICIT` (1) | `IMPLICIT` (2) | `EXPLICIT` (1) | `EXPLICIT` (1) |
| `enum_type` | Tag 2 | File, Enum | `CLOSED` (2) | `OPEN` (1) | `OPEN` (1) | `OPEN` (1) |
| `repeated_field_encoding` | Tag 3 | File, Field | `EXPANDED` (2) | `PACKED` (1) | `PACKED` (1) | `PACKED` (1) |
| `utf8_validation` | Tag 4 | File, Field | `NONE` (3) | `VERIFY` (2) | `VERIFY` (2) | `VERIFY` (2) |
| `message_encoding` | Tag 5 | File, Field | `LENGTH_PREFIXED` (1) | `LENGTH_PREFIXED` (1) | `LENGTH_PREFIXED` (1) | `LENGTH_PREFIXED` (1) |
| `json_format` | Tag 6 | File, Message, Enum | `LEGACY_BEST_EFFORT` (2) | `ALLOW` (1) | `ALLOW` (1) | `ALLOW` (1) |
| `enforce_naming_style` | Tag 7 | File, Message, Field, Oneof, Enum, EnumValue, Service, Method | `STYLE_LEGACY` (2) | `STYLE_LEGACY` (2) | `STYLE_LEGACY` (2) | **`STYLE2024` (1)** |
| `default_symbol_visibility` | Tag 8 | File | `EXPORT_ALL` (1) | `EXPORT_ALL` (1) | `EXPORT_ALL` (1) | **`EXPORT_TOP_LEVEL` (2)** |

### 2.2 Detailed Feature Semantics

#### 1. `field_presence` (Tag 1)
- **Values**: `EXPLICIT` (1), `IMPLICIT` (2), `LEGACY_REQUIRED` (3).
- **Edition 2024 Default**: `EXPLICIT`.
- **Target Scope**: File, Field. (*Note: Setting on Message is rejected by protoc*).
- **Semantics**:
  - Singular scalar fields (e.g. `int32 int32_field = 1;`) track presence explicitly, exactly like proto2 `optional` fields or proto3 `optional` fields.
  - Setting a field to zero (`0`, `""`, `false`) marks it present and emits tag + value on the wire (e.g., `[0x08, 0x00]` for `int32_field: 0`).
  - If overridden with `IMPLICIT`, zero values do not set presence and are omitted from the wire (proto3 default behavior).
  - If overridden with `LEGACY_REQUIRED`, deserialization fails if the field is missing from the wire (proto2 `required` behavior).

#### 2. `enum_type` (Tag 2)
- **Values**: `OPEN` (1), `CLOSED` (2).
- **Edition 2024 Default**: `OPEN`.
- **Target Scope**: File, Enum. (*Note: Setting on Message is rejected by protoc*).
- **Semantics**:
  - Unrecognized enum integer values (e.g., `99`) are retained as valid enum values, accessible via accessors, and preserved during serialization.
  - If overridden with `CLOSED` (e.g. `option features.enum_type = CLOSED;` on the enum), unrecognized wire values are moved to unknown fields and not recognized as enum variants (proto2 behavior).

#### 3. `repeated_field_encoding` (Tag 3)
- **Values**: `PACKED` (1), `EXPANDED` (2).
- **Edition 2024 Default**: `PACKED`.
- **Target Scope**: File, Field.
- **Semantics**:
  - All packable repeated scalar types (varints, fixed32, fixed64) default to packed wire format (`WIRE_LEN` with length prefix).
  - If overridden with `EXPANDED`, each element is serialized as a distinct tag + wire varint/fixed pair.

#### 4. `utf8_validation` (Tag 4)
- **Values**: `VERIFY` (2), `NONE` (3). (*Value 1 is reserved*).
- **Edition 2024 Default**: `VERIFY`.
- **Target Scope**: File, Field.
- **Semantics**:
  - String fields must contain valid UTF-8 sequences. Invalid byte sequences cause a parse error.
  - If overridden with `NONE`, raw string bytes are accepted without validation (proto2 string behavior).

#### 5. `message_encoding` (Tag 5)
- **Values**: `LENGTH_PREFIXED` (1), `DELIMITED` (2).
- **Edition 2024 Default**: `LENGTH_PREFIXED`.
- **Target Scope**: File, Field.
- **Semantics**:
  - Submessages default to standard length-delimited encoding (`WIRE_LEN` followed by length varint).
  - If overridden with `DELIMITED`, submessages are framed using wire tags `WIRE_SGROUP` (3, start group) and `WIRE_EGROUP` (4, end group). This replaces legacy `group` syntax.

#### 6. `json_format` (Tag 6)
- **Values**: `ALLOW` (1), `LEGACY_BEST_EFFORT` (2).
- **Edition 2024 Default**: `ALLOW`.
- **Target Scope**: File, Message, Enum.
- **Semantics**:
  - Canonical Protobuf JSON mapping is allowed.
  - If overridden with `LEGACY_BEST_EFFORT`, runtimes allow non-conformant best-effort mappings (e.g. unknown enum names as strings).

#### 7. `enforce_naming_style` (Tag 7)
- **Values**: `STYLE2024` (1), `STYLE_LEGACY` (2), `STYLE2026` (3).
- **Edition 2024 Default**: `STYLE2024`.
- **Target Scope**: File, Message, Field, Oneof, Enum, EnumValue, Service, Method, ExtensionRange.
- **Semantics**:
  - Enforces strict canonical protobuf casing:
    - Messages, Enums, Services: `PascalCase`.
    - Fields, Oneofs: `lower_snake_case`.
    - RPC Methods: `TitleCase`.
    - Enum Values: `SCREAMING_SNAKE_CASE`.
  - Non-conformant identifiers (e.g. `message badName { int32 BadField = 1; }`) are rejected at compile time unless explicitly opted out with `features.enforce_naming_style = STYLE_LEGACY`.

#### 8. `default_symbol_visibility` (Tag 8)
- **Values**: `EXPORT_ALL` (1), `EXPORT_TOP_LEVEL` (2), `LOCAL_ALL` (3), `STRICT` (4).
- **Edition 2024 Default**: `EXPORT_TOP_LEVEL`.
- **Target Scope**: File.
- **Semantics**:
  - Governs symbol visibility across `.proto` file import boundaries:
    - Top-level messages and enums default to `EXPORT` (visible to other files).
    - Nested messages and enums default to `LOCAL` (private to the defining `.proto` file).
  - Explicit keywords `export` and `local` on `message` and `enum` declarations override the file default.

### 2.3 Additional Edition 2024 Syntax & Lifecycle Changes

1. **`import weak` Removed**:
   - `import weak "dep.proto";` is illegal in Edition 2024. `protoc` emits:
     `weak import is not supported in edition 2024 and above. Consider using option import instead.`
2. **`import option` Introduced**:
   - `import option "opt.proto";` is valid in Edition 2024 (disallowed in Edition 2023). It signals that the import is solely for descriptor options/features and carries no runtime message dependency.
3. **`ctype` Option Removed**:
   - `[ctype = STRING_PIECE]` is illegal in Edition 2024. `protoc` emits:
     `ctype option is not allowed under edition 2024 and beyond. Use the feature string_type = VIEW|CORD|STRING|... instead.`
4. **`java_multiple_files` Option Removed**:
   - `option java_multiple_files = true;` is illegal in Edition 2024. Multiple files is the mandatory default; overriding is moved to `features.(pb.java).nest_in_file_class`.
5. **`group` Keyword Removed**:
   - Group syntax (`group Foo = 1 { ... }`) is rejected. Delimited framing must be specified via `features.message_encoding = DELIMITED`.
6. **`optional` and `required` Field Labels Removed**:
   - Keywords `optional` and `required` are illegal in Edition 2024. Singular fields default to explicit presence; required fields must be annotated with `[features.field_presence = LEGACY_REQUIRED]`.

---

## 3. Feature Resolution & Inheritance Rules

### 3.1 Inheritance Hierarchy

Feature resolution follows a strict lexical tree walk where child scopes inherit from parent scopes unless overridden:

```text
Edition Defaults (File Edition: 1001)
       │
       ▼
File Options (file.options.features)
       │
       ├─────────────────────────────────┬───────────────────────────────┐
       ▼                                 ▼                               ▼
Message Options (msg.options.features) Top-Level Enum (enum.features) Top-Level Extensions
       │                                                                 │
       ├─────────────────┬──────────────┐                                ▼
       ▼                 ▼              ▼                       Extension Field Features
Field Options     Nested Message  Nested Enum
(field.features)  (nested.features) (nested_enum.features)
                         │
                         ▼
                  Nested Field Features
```

### 3.2 Target Constraint Matrix

Upstream `protoc` enforces strict target validation. Features set on invalid targets produce compile errors:

| Feature | Target File | Target Message | Target Field | Target Enum |
|---|:---:|:---:|:---:|:---:|
| `field_presence` | **Yes** | No | **Yes** | No |
| `enum_type` | **Yes** | No | No | **Yes** |
| `repeated_field_encoding` | **Yes** | No | **Yes** | No |
| `utf8_validation` | **Yes** | No | **Yes** | No |
| `message_encoding` | **Yes** | No | **Yes** | No |
| `json_format` | **Yes** | **Yes** | No | **Yes** |
| `enforce_naming_style` | **Yes** | **Yes** | **Yes** | **Yes** |
| `default_symbol_visibility` | **Yes** | No | No | No |

### 3.3 Audit of Current `src/dynamic.rs` Implementation

Inspection of `src/dynamic.rs` reveals the following current state and concrete implementation gaps:

1. **`edition_defaults()`**:
   - Currently groups `edition >= 1000` into Edition 2023 defaults.
   - For Edition 2024 (`edition == 1001`), the values for tags 1..5 (`presence=1`, `enum_type=1`, `repeated_encoding=1`, `utf8=2`, `message_encoding=1`) happen to align with Edition 2023.
   - **Gap**: `RawFeatures` does not store `json_format` (tag 6), `enforce_naming_style` (tag 7), or `default_symbol_visibility` (tag 8).
2. **Enum-Level Features Parsing**:
   - `parse_enum_options()` currently skips tag 7 (`EnumOptions.features`):
     ```rust
     if n == 7 && w == WIRE_LEN { wire::skip_field(bytes, &mut pos, w)?; }
     ```
   - `parse_file()` unconditionally sets:
     ```rust
     let closed = file.features.enum_type == 2;
     for e in &mut file.enums { e.closed = closed; }
     prefix_enums_in_messages(&mut file.messages, closed);
     ```
   - **Gap**: When an enum sets `option features.enum_type = CLOSED;` (as in `ClosedEnum`), the enum-level feature is completely discarded. The enum erroneously inherits `open` from the file.
3. **Descriptor Representation**:
   - `MessageDescriptor` and `EnumDescriptor` do not expose `SymbolVisibility` (`export` / `local` / `unset`).
4. **Feature Validation & Rejection**:
   - `src/dynamic.rs` silently ignores unknown feature values instead of validating against known ranges.

---

## 4. Symbol Visibility & Naming Contract

### 4.1 Symbol Visibility Rules

In Edition 2024, symbol visibility controls cross-file encapsulation.

1. **Declared Visibility (`SymbolVisibility`)**:
   - `VISIBILITY_UNSET` (0): Symbol has no explicit keyword.
   - `VISIBILITY_LOCAL` (1): Symbol explicitly declared with `local message` or `local enum`.
   - `VISIBILITY_EXPORT` (2): Symbol explicitly declared with `export message` or `export enum`.
2. **Effective Visibility Resolution**:
   - If declared visibility is `VISIBILITY_EXPORT`, effective visibility is **Exported**.
   - If declared visibility is `VISIBILITY_LOCAL`, effective visibility is **Local**.
   - If declared visibility is `VISIBILITY_UNSET`:
     - Under `default_symbol_visibility = EXPORT_TOP_LEVEL` (Edition 2024 default):
       - Top-level messages and enums resolve to **Exported**.
       - Nested messages and enums resolve to **Local**.
     - Under `default_symbol_visibility = EXPORT_ALL`: All symbols resolve to **Exported**.
     - Under `default_symbol_visibility = LOCAL_ALL`: All symbols resolve to **Local**.
3. **Cross-File Import Boundary**:
   - When file `A.proto` imports file `B.proto`, `A.proto` may reference only symbols in `B.proto` whose effective visibility is **Exported**.
   - Referencing a symbol with effective visibility **Local** is a compile error in `protoc`:
     `Symbol "...", defined in "..." is not visible from "...". It is explicitly marked 'local' and cannot be accessed outside its own file`
4. **Rust Code Generation Contract (`src/codegen.rs`)**:
   - For `compile_protos` (build-time `protoc` execution), `protoc` enforces cross-proto import rules prior to invoking the plugin.
   - Within generated Rust code, types with effective visibility `Exported` are emitted as `pub struct` / `pub enum`.
   - Types with effective visibility `Local` remain accessible within their crate/module hierarchy but can be restricted from top-level crate re-exports.

### 4.2 Naming Style Contract (`STYLE2024`)

Under `enforce_naming_style = STYLE2024`:
- Messages, Enums, Services: `PascalCase`.
- Fields, Oneofs: `lower_snake_case`.
- RPC Methods: `TitleCase`.
- Enum Values: `SCREAMING_SNAKE_CASE`.
- `DescriptorPool` validates inherited naming options in Edition 2024
  descriptors. The generator still advertises Edition 2023 as its maximum;
  generated Edition 2024 consumers remain a separate `CG-14` qualification.

The RPC method rule was verified with `libprotoc 36.1` on 2026-09-23:
`bad_method` is rejected with a `TitleCase` diagnostic, while `GoodMethod`
is accepted. The independently runnable Edition 2023 conformance baseline
remains pinned to v35.1.

---

## 5. Extensions Contract

### 5.1 Syntax in Edition 2024

In Edition 2024, extensions are declared inside messages using `extensions` ranges and extended via `extend` blocks:

```protobuf
message ExtendableMessage {
  int32 base_field = 1;
  extensions 100 to 1000;

  extend ExtendableMessage {
    int32 nested_scoped_extension = 150;
  }
}

extend ExtendableMessage {
  int32 ext_int32 = 101;
  string ext_string = 102;
  repeated int32 ext_repeated_int32 = 103;
  ExtensionSubmessage ext_submessage = 104;
  ExtensionClosedEnum ext_closed_enum = 105;
  int32 ext_int32_with_default = 106 [default = 42];
}
```

### 5.2 Dynamic Message Support

`pure-protobuf` currently provides full dynamic extension support:
- `DynamicMessage`:
  - `get_extension(tag: u32) -> Option<&Value>`
  - `set_extension(tag: u32, value: Value)`
  - `has_extension(tag: u32) -> bool`
  - `clear_extension(tag: u32)`
- `DescriptorPool`:
  - `get_extension(full_name: &str) -> Option<(Arc<MessageDescriptor>, FieldDescriptor)>`
  - `extension_numbers_of(containing_type: &str) -> Vec<u32>`
  - `file_for_extension(containing_type: &str, number: u32) -> Option<&str>`
- Unknown extension wire fields round-trip losslessly in `unknown_fields`.

### 5.3 Generated Code Extension Status

In Google's official `rust_upb` implementation (`third_party/protobuf/rust/test/shared/extensions_test.rs`), typed extension tests are an empty stub (`docs/upb.md`).

In `pure-protobuf`:
- `src/codegen.rs` does not currently emit typed extension constants or extension accessors on generated structs.
- Extension fields parsed on typed messages are retained in the `UnknownFields` bag and can be inspected via dynamic reflection.
- Generating typed extension accessors (e.g. `msg.get(&ext_int32)`) is an independent codegen enhancement that will be scoped as task `CG-14b`.

---

## 6. Language-Specific Options & Extensions Policy

Protocol Buffers defines several language-specific feature extensions in `descriptor.proto`:
- `pb.cpp` (`CppFeatures`, tag 1000):
  - `string_type`: `VIEW`, `CORD`, `STRING`.
  - `legacy_closed_enum`: `true`, `false`.
- `pb.java` (`JavaFeatures`, tag 1001):
  - `nest_in_file_class`: `YES`, `NO`.
  - `legacy_closed_enum`: `true`, `false`.
- `pb.go` (`GoFeatures`, tag 1002):
  - `legacy_unmarshal_json_enum`: `true`, `false`.
- `pb.python` (`PythonFeatures`, tag 1003):
  - `py_generic_services`: `true`, `false`.
- `pb.csharp` (`CSharpFeatures`, tag 1004).

### Policy for Pure-Protobuf

1. **Permissive Parsing**: All language-specific feature extensions are valid protobuf option extensions and must be parsed or skipped without failing schema loading.
2. **C++ `string_type`**: In pure-Rust, `VIEW` and `CORD` map to standard `ProtoString` / `ProtoStr` representations (with `LazyStr` zero-copy borrow from parse buffer where applicable), as documented in `docs/upb.md`.
3. **No Behavioral Coupling**: Java, Go, Python, and C# generator features have zero runtime effect on Rust data structures or wire codecs.
4. **No `.pb.rust` Upstream**: Official Protocol Buffers v35.1 does not define a `.pb.rust` feature extension; `pure-protobuf` introduces no proprietary required feature extensions.

---

## 7. Implementation Mapping & Task Decomposition

### 7.1 Feature Implementation Mapping

| Upstream Feature | Component | Current Pure-Protobuf Status | Target Task | Owner / Notes |
|---|---|---|---|---|
| `field_presence = EXPLICIT` | `src/dynamic.rs` | Supported in `edition_defaults()` (`presence = 1`) | Existing | Verified by `tests/fixtures/edition2024/bin/defaults_zero_set.bin` |
| `field_presence = IMPLICIT` | `src/dynamic.rs` | Supported in `parse_field_options()` (`presence = 2`) | Existing | Verified by `tests/fixtures/edition2024/bin/overrides_implicit_zero.bin` |
| `field_presence = LEGACY_REQUIRED` | `src/dynamic.rs` | Supported (`cardinality = Required`, `presence = Explicit`) | Existing | Verified by `tests/fixtures/edition2024/proto/overrides.proto` |
| `enum_type = OPEN` | `src/dynamic.rs` | Supported in `edition_defaults()` (`enum_type = 1`) | Existing | Open enum integer round-trip verified |
| `enum_type = CLOSED` (Enum-level) | `src/dynamic.rs` | Resolved from `EnumOptions.features`; unknown integers stay in unknown fields | **CG-13** | Checked descriptor and wire fixture tests in `tests/dynamic.rs` |
| `repeated_field_encoding = PACKED` | `src/dynamic.rs` | Supported (`repeated_encoding = 1`) | Existing | Verified by `tests/fixtures/edition2024/bin/defaults_populated.bin` |
| `repeated_field_encoding = EXPANDED`| `src/dynamic.rs` | Supported (`repeated_encoding = 2`) | Existing | Verified by `tests/fixtures/edition2024/bin/overrides_expanded_repeated.bin` |
| `utf8_validation = VERIFY` | `src/dynamic.rs` | Supported (`utf8 = 2`) | Existing | UTF-8 verify on parse active |
| `utf8_validation = NONE` | `src/dynamic.rs` | Supported (`utf8 = 3`, sets `utf8_validate = false`) | Existing | Raw string bytes accepted |
| `message_encoding = LENGTH_PREFIXED`| `src/dynamic.rs` | Supported (`message_encoding = 1`) | Existing | Standard length-delimited wire encoding |
| `message_encoding = DELIMITED` | `src/dynamic.rs` | Supported (`message_encoding = 2`, `delimited = true`)| Existing | Verified by `tests/fixtures/edition2024/bin/overrides_delimited_message.bin` |
| `json_format` | `src/dynamic.rs` | Resolved from File/Message/Enum feature options | **CG-13** | Inheritance and fixture oracles in `tests/dynamic.rs` |
| `enforce_naming_style = STYLE2024` | `src/dynamic.rs` | Descriptor names validated with inherited overrides; generated consumers pending | **CG-13**, **CG-14** | The plugin still advertises maximum Edition 2023 |
| `default_symbol_visibility` | `src/dynamic.rs` | Resolved from file features | **CG-13** | Nested defaults and file overrides checked in `tests/dynamic.rs` |
| `export` / `local` keywords | `src/dynamic.rs` | Parsed on Message/Enum and enforced for cross-file references | **CG-13** | Local imports rejected by fixture tests |
| Extension Ranges & Declarations | `src/dynamic.rs` | Supported (`parse_extension_range`, `collect_raw`) | Existing | Fully parsed into `MessageDescriptor.extension_ranges` |
| Dynamic Extension Access | `src/dynamic.rs` | Supported (`get_extension`, `set_extension`) | Existing | Verified by `tests/json_text_ext.rs` |
| Typed Extension Codegen | `src/codegen.rs` | Not implemented (matches upstream rust_upb stub) | **CG-14b** | Split task card for typed extension accessors |
| Plugin Supported Edition Range | `src/codegen.rs` | Pinned to `EDITION_2023` (`1000`) | **CG-14** | Bump `maximum_edition = 1001` upon qualification |

### 7.2 Task Card Decomposition

Because full Edition 2024 coverage spans descriptor resolution, symbol visibility, codegen qualification, and typed extensions, the implementation inventory is decomposed into bounded, reviewed task cards:

```text
       ┌────────────────────────────────────────────────────────┐
       │ CG-12: Freeze Edition 2024 Semantic Contract (Current)  │
       └───────────────────────────┬────────────────────────────┘
                                   │
                                   ▼
       ┌────────────────────────────────────────────────────────┐
       │ CG-13: Implement Approved Edition 2024 Descriptors      │
       │ - Parse EnumOptions.features (tag 7) & fix closed enums │
       │ - Parse json_format (tag 6), naming (tag 7), vis (tag 8)│
       │ - Capture SymbolVisibility on Message & Enum           │
       │ - Reject unknown/unresolved features explicitly        │
       │ - Pass tests/fixtures/edition2024/ fds & bin suites     │
       └───────────────────────────┬────────────────────────────┘
                                   │
                                   ▼
       ┌────────────────────────────────────────────────────────┐
       │ CG-14: Qualify Edition 2024 Generated Consumers         │
       │ - Raise maximum_edition to 1001 in CodeGeneratorResponse│
       │ - Compile tests/fixtures/edition2024/proto/ via plugin  │
       │ - Verify typed message accessors & visibility          │
       │ - Run full conformance & shared test gates              │
       └───────────────────────────┬────────────────────────────┘
                                   │
                                   ▼
       ┌────────────────────────────────────────────────────────┐
       │ CG-14b (Split Card): Typed Extension Code Generation   │
       │ - Emit typed Extension identifiers for extend blocks   │
       │ - Implement typed get/set extension accessors on structs│
       │ - Qualify vendor/google/rust/test/extensions.proto     │
       └────────────────────────────────────────────────────────┘
```

---

## 8. Approved Feature-Specific Fixture Expectations & Test Oracles

All fixtures in `tests/fixtures/edition2024/` are approved and serve as the immutable oracle for `CG-13` and `CG-14`:

### 8.1 Schema & Artifact Inventory

1. **`defaults.proto` & `defaults.fds`**:
   - `edition = "2024";` with no explicit feature overrides.
   - Oracle: All fields have explicit presence; enums are open; repeated scalars are packed; strings verify UTF-8; submessages are length-prefixed; symbols default to exported.
   - Pinned wire vectors: `defaults_empty.bin` (0 bytes), `defaults_zero_set.bin` (6 bytes), `defaults_populated.bin` (96 bytes).
2. **`overrides.proto` & `overrides.fds`**:
   - Exercises explicit feature overrides for presence (`IMPLICIT`, `LEGACY_REQUIRED`), repeated encoding (`EXPANDED`), string validation (`NONE`), message encoding (`DELIMITED`), JSON format (`LEGACY_BEST_EFFORT`), and enum type (`CLOSED`).
   - Pinned wire vectors: `overrides_implicit_zero.bin` (0 bytes), `overrides_implicit_set.bin` (2 bytes), `overrides_expanded_repeated.bin` (6 bytes), `overrides_delimited_message.bin` (9 bytes).
3. **`inheritance.proto` & `inheritance.fds`**:
   - File-level overrides (`IMPLICIT`, `EXPANDED`, `NONE`) inherited by `FileDefaultsConsumer` and overridden by `FieldLevelOverrides`.
   - Message-level and nested message `json_format` overrides.
   - Enum-level `CLOSED` overrides at file scope (`TopLevelEnumClosed`) and inside messages (`MessageWithNestedEnums.NestedClosedEnum`).
4. **`visibility.proto` & `visibility.fds`**:
   - Explicit `export` and `local` keywords on top-level and nested messages/enums.
   - Unadorned symbols default to `EXPORT` (top-level) and `LOCAL` (nested) under `EXPORT_TOP_LEVEL`.
5. **`extensions.proto` & `extensions.fds`**:
   - Extension ranges `100 to 1000;`, file-level and nested `extend` blocks.
   - Pinned wire vector: `extensions_populated.bin` (34 bytes).
6. **`rejected/` Suite**:
   - Contains 8 negative test fixtures verifying that `protoc` and downstream pure-Rust frontends reject `import weak`, `ctype`, `java_multiple_files`, `group`, `optional`, `required`, non-standard naming, and cross-file `local` symbol imports.

### 8.2 Checksum & Verification Integrity

All `.fds` and `.bin` artifacts are cryptographically registered in `tests/fixtures/edition2024/expectations.json`. Any regression in descriptor parsing or wire encoding will produce an immediate failure against these golden oracles.

### 8.3 Bounded Generated-Consumer Preview (CG-14 Pending)

The direct descriptor-set generator compiles the five checked Edition 2024
fixtures plus the checked supplemental `fds/cg14_preview.fds` into a strict
Rust-language Edition 2024 consumer. The preview set contains retained
`STYLE_LEGACY` naming options, verified/unverified string maps, and the
already-vendored original extension schema. The focused `tests/plugin.rs`
consumer checks presence, inherited overrides, singular
closed enums, delimited framing, visibility, checked wire vectors, and both
verified and unverified map string entries. Generated map decoders use the
resolved key and value features of the map entry; the outer 2024 map field's
`utf8_validate` flag is false. Repeated and map fields with closed enum values
are explicitly rejected until their unknown-value semantics are implemented.
Extension fields are not emitted as ordinary typed fields: their wire bytes
round-trip through unknown fields until `CG-14b` adds typed accessors.

The repeated-CLOSED guard remains necessary after a bounded 2026-09-24
investigation. The checked `overrides.fds` has only a **singular** `ClosedEnum`;
none of the checked Edition 2024 descriptors and wire vectors cover a repeated
CLOSED enum. The local synthetic `tests/dynamic.rs` packed test verifies one
known typed value and one unknown field, but does not pin reserialization of
mixed packed/unpacked inputs, expanded output, or negative/out-of-range
unknown values. Local `protoc 36.1` text decoding of the vendored Edition 2023
closed-enum schema identifies unknown values; it does not provide a canonical
Edition 2024 re-encode oracle. An unretained local Python protobuf 6.33.1
probe, with a repeated field added to the checked descriptor in memory,
suggested different negative-unknown normalization for packed and unpacked
input. No checked Edition 2024 repeated-enum reference vector establishes the
required wire contract, so removing the fail-closed guard risks wire loss.
A future slice needs a checked
Edition 2024 repeated-CLOSED descriptor and independently pinned output
vectors for packed, unpacked, and mixed known/unknown values (including
negative and out-of-range numbers). Closed enum map values remain a separate
unsupported case.

The preview FDS was produced once with `libprotoc 36.1` and `--retain_options`;
mandatory Cargo tests consume only its checked bytes. `protoc 36.1` strips
source-retention features such as `enforce_naming_style = STYLE_LEGACY` from
ordinary descriptor sets. Without retained options, the resolver rejects
nonconforming names instead of silently assuming Edition 2023 semantics.
`Config::compile_protos` does not request `--retain_options` by default.
Source-only rejection cases and byte-for-byte preview-FDS regeneration are
explicitly ignored in the default Cargo gate:
run `cargo test --test plugin edition2024_rejected_source_fixtures_fail_in_protoc -- --ignored --exact`
and `cargo test --test plugin edition2024_preview_descriptor_matches_pinned_source_compiler -- --ignored --exact`
only with a compatible pinned `libprotoc 36.1`. The default Rust 2024
consumer, closed-enum, and plugin-cap tests do not invoke `protoc`.

These are descriptor-set consumer proofs, **not** advertised Edition 2024
plugin support. A direct binary `CodeGeneratorRequest` test verifies that
`protoc-gen-pbrs` still reports `maximum_edition = 1000`; a compatible
`protoc` refuses its Edition 2024 output when run with a compatible compiler.
The upstream
`rust/test/shared/extensions_test.rs` is empty; compiling its Edition 2024
schema and checking unknown-extension wire preservation do not qualify typed
extension access. Full original shared-consumer and pinned differential/
conformance evidence, including the independent v35.1/max-2023 baseline,
remain required before `CG-14` can raise the cap. The local warm target does
not contain the pinned v35.1 compiler or conformance runner; the original
shared and full conformance suites were not run in this bounded preview.
