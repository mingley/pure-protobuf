# Protocol Buffers Edition 2024 Semantic Contract & Feature Specification

`protoc-gen-pbrs` accepts a qualified subset of Protocol Buffers Edition
2024 and advertises maximum edition `1001`. Repeated and map fields with
closed enums are rejected; typed extension accessors remain CG-14b work.
These limits prevent a full Edition 2024 support claim.

This is the semantic reference for contributors implementing descriptors,
generation, or a source frontend. For application setup, use the
[codegen guide](guides/codegen.md). For runnable examples and rejection
oracles, use the [Edition fixtures](../tests/fixtures/edition2024/README.md).

---

## 1. Executive Summary & Architectural Scope

Protocol Buffers Edition 2024 uses:

- `edition = "2024";`
- edition number `1001`

This document is the semantic contract for Edition 2024 in `pure-protobuf` (`pbrs`). It covers defaults, inheritance, visibility, naming, extensions, and language-specific options needed for conformance with upstream Protocol Buffers (`protocolbuffers/protobuf@v35.1` / `v36.1`) without relying on the C++ / upb kernel.

This is **not** Rust language Edition 2024 in `Cargo.toml`. Switching a crate's Rust edition does not enable Protobuf Edition 2024 generation. The plugin's advertised maximum is Edition 2024 (`1001`) for the qualified subset; inputs outside it fail loudly instead of generating code.

### Deliverables & Contract Boundary

1. **Pinned Edition Defaults.** Complete global feature defaults for Edition 2024 compared with Proto2, Proto3, and Edition 2023.
2. **Inheritance & Resolution Rules.** Exact merge order from edition defaults through file, message, field, and enum descriptors.
3. **Symbol Visibility & Naming.** `export` / `local`, `default_symbol_visibility`, and `enforce_naming_style = STYLE2024`.
4. **Extensions Contract.** Wire and descriptor semantics for `extensions <start> to <end>;` and `extend` blocks under Edition 2024.
5. **Language-Specific Options Policy.** How `pbrs` handles `(pb.cpp)`, `(pb.java)`, `(pb.go)`, `(pb.python)`, and `(pb.csharp)` feature extensions.
6. **Implementation Mapping & Task Decomposition.** Every upstream feature mapped to existing code, bounded implementation slices (`CG-13`, `CG-14`), or explicit blockers.
7. **Approved Fixtures & Oracles.** Test schemas, compiled descriptor sets (`.fds`), wire-format vectors (`.bin`), and negative rejection suites in `tests/fixtures/edition2024/`.

> **Constraint invariant (satisfied by `CG-14`):** `maximum_edition` in `src/codegen.rs` stayed frozen at `1000` (`EDITION_2023`) through `CG-13`. It was raised to `1001` (`EDITION_2024`) only after descriptor resolution semantics plus the differential and original shared-test evidence in §8 were complete. Any future exclusion added to the qualified subset must re-freeze or re-qualify the cap; a numeric bump alone never qualifies.

---

## 2. Upstream Edition 2024 Feature Set & Default Matrix

Protocol Buffers Edition 2024 is the second official edition release. It builds on Edition 2023 by standardizing naming style, adding symbol visibility controls, and retiring legacy syntax.

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

- **Values:** `EXPLICIT` (1), `IMPLICIT` (2), `LEGACY_REQUIRED` (3).
- **Edition 2024 default:** `EXPLICIT`.
- **Target scope:** File, Field. Setting it on Message is rejected by `protoc`.
- **Semantics:**
  - Singular scalar fields, such as `int32 int32_field = 1;`, track presence explicitly like proto2 `optional` fields or proto3 `optional` fields.
  - Setting a field to zero (`0`, `""`, `false`) marks it present and emits tag + value on the wire. Example: `[0x08, 0x00]` for `int32_field: 0`.
  - If overridden with `IMPLICIT`, zero values do not set presence and are omitted from the wire, matching proto3 default behavior.
  - If overridden with `LEGACY_REQUIRED`, deserialization fails when the field is missing from the wire, matching proto2 `required` behavior.

#### 2. `enum_type` (Tag 2)

- **Values:** `OPEN` (1), `CLOSED` (2).
- **Edition 2024 default:** `OPEN`.
- **Target scope:** File, Enum. Setting it on Message is rejected by `protoc`.
- **Semantics:**
  - Unrecognized enum integer values, such as `99`, remain valid enum values. Accessors can read them, and serialization preserves them.
  - If overridden with `CLOSED`, as in `option features.enum_type = CLOSED;`, unrecognized wire values move to unknown fields and are not recognized as enum variants, matching proto2 behavior.

#### 3. `repeated_field_encoding` (Tag 3)

- **Values:** `PACKED` (1), `EXPANDED` (2).
- **Edition 2024 default:** `PACKED`.
- **Target scope:** File, Field.
- **Semantics:**
  - Packable repeated scalar types, including varints, fixed32, and fixed64, default to packed wire format (`WIRE_LEN` with length prefix).
  - If overridden with `EXPANDED`, each element serializes as a distinct tag + wire varint/fixed pair.

#### 4. `utf8_validation` (Tag 4)

- **Values:** `VERIFY` (2), `NONE` (3). Value 1 is reserved.
- **Edition 2024 default:** `VERIFY`.
- **Target scope:** File, Field.
- **Semantics:**
  - String fields must contain valid UTF-8. Invalid bytes cause a parse error.
  - If overridden with `NONE`, raw string bytes are accepted without validation, matching proto2 string behavior.

#### 5. `message_encoding` (Tag 5)

- **Values:** `LENGTH_PREFIXED` (1), `DELIMITED` (2).
- **Edition 2024 default:** `LENGTH_PREFIXED`.
- **Target scope:** File, Field.
- **Semantics:**
  - Submessages default to standard length-delimited encoding: `WIRE_LEN` followed by a length varint.
  - If overridden with `DELIMITED`, submessages use wire tags `WIRE_SGROUP` (3, start group) and `WIRE_EGROUP` (4, end group). This replaces legacy `group` syntax.

#### 6. `json_format` (Tag 6)

- **Values:** `ALLOW` (1), `LEGACY_BEST_EFFORT` (2).
- **Edition 2024 default:** `ALLOW`.
- **Target scope:** File, Message, Enum.
- **Semantics:**
  - Canonical Protobuf JSON mapping is allowed.
  - If overridden with `LEGACY_BEST_EFFORT`, runtimes allow non-conformant best-effort mappings, such as unknown enum names as strings.

#### 7. `enforce_naming_style` (Tag 7)

- **Values:** `STYLE2024` (1), `STYLE_LEGACY` (2), `STYLE2026` (3).
- **Edition 2024 default:** `STYLE2024`.
- **Target scope:** File, Message, Field, Oneof, Enum, EnumValue, Service, Method, ExtensionRange.
- **Semantics:**
  - Canonical protobuf casing is enforced:
    - Messages, Enums, Services: `PascalCase`.
    - Fields, Oneofs: `lower_snake_case`.
    - RPC Methods: `TitleCase`.
    - Enum Values: `SCREAMING_SNAKE_CASE`.
  - Non-conformant identifiers, such as `message badName { int32 BadField = 1; }`, are rejected at compile time unless explicitly opted out with `features.enforce_naming_style = STYLE_LEGACY`.

#### 8. `default_symbol_visibility` (Tag 8)

- **Values:** `EXPORT_ALL` (1), `EXPORT_TOP_LEVEL` (2), `LOCAL_ALL` (3), `STRICT` (4).
- **Edition 2024 default:** `EXPORT_TOP_LEVEL`.
- **Target scope:** File.
- **Semantics:**
  - Symbol visibility is checked across `.proto` file import boundaries.
  - Top-level messages and enums default to `EXPORT`, so other files can reference them.
  - Nested messages and enums default to `LOCAL`, so they are private to the defining `.proto` file.
  - Explicit `export` and `local` keywords on `message` and `enum` declarations override the file default.

### 2.3 Additional Edition 2024 Syntax & Lifecycle Changes

| Change | Edition 2024 behavior |
|---|---|
| `import weak` removed | `import weak "dep.proto";` is illegal. `protoc` emits: `weak import is not supported in edition 2024 and above. Consider using option import instead.` |
| `import option` introduced | `import option "opt.proto";` is valid in Edition 2024 and disallowed in Edition 2023. It signals that the import is only for descriptor options/features and has no runtime message dependency. |
| `ctype` option removed | `[ctype = STRING_PIECE]` is illegal. `protoc` emits: `ctype option is not allowed under edition 2024 and beyond. Use the feature string_type = VIEW|CORD|STRING|... instead.` |
| `java_multiple_files` option removed | `option java_multiple_files = true;` is illegal. Multiple files are mandatory by default; overrides move to `features.(pb.java).nest_in_file_class`. |
| `group` keyword removed | `group Foo = 1 { ... }` is rejected. Delimited framing must use `features.message_encoding = DELIMITED`. |
| `optional` and `required` labels removed | `optional` and `required` are illegal. Singular fields default to explicit presence. Required fields must use `[features.field_presence = LEGACY_REQUIRED]`. |

---

## 3. Feature Resolution & Inheritance Rules

### 3.1 Inheritance Hierarchy

Feature resolution follows a lexical tree walk. Child scopes inherit from parent scopes unless they override a value:

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

Upstream `protoc` rejects features set on invalid targets:

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

Inspection of `src/dynamic.rs` records these implementation details and gaps:

1. **`edition_defaults()`.**
   - It groups `edition >= 1000` into Edition 2023 defaults.
   - For Edition 2024 (`edition == 1001`), tags 1 through 5 already align with Edition 2023: `presence=1`, `enum_type=1`, `repeated_encoding=1`, `utf8=2`, `message_encoding=1`.
   - **Gap:** `RawFeatures` does not store `json_format` (tag 6), `enforce_naming_style` (tag 7), or `default_symbol_visibility` (tag 8).
2. **Enum-level features parsing.**
   - `parse_enum_options()` skips tag 7 (`EnumOptions.features`):

     ```rust
     if n == 7 && w == WIRE_LEN { wire::skip_field(bytes, &mut pos, w)?; }
     ```

   - `parse_file()` then applies file-level enum closure to every enum:

     ```rust
     let closed = file.features.enum_type == 2;
     for e in &mut file.enums { e.closed = closed; }
     prefix_enums_in_messages(&mut file.messages, closed);
     ```

   - **Gap:** an enum-level `option features.enum_type = CLOSED;`, such as `ClosedEnum`, is discarded. The enum incorrectly inherits `open` from the file.
3. **Descriptor representation.**
   - `MessageDescriptor` and `EnumDescriptor` do not expose `SymbolVisibility` (`export` / `local` / `unset`).
4. **Feature validation and rejection.**
   - `src/dynamic.rs` silently ignores unknown feature values instead of validating them against known ranges.

---

## 4. Symbol Visibility & Naming Contract

### 4.1 Symbol Visibility Rules

Edition 2024 uses symbol visibility to control cross-file encapsulation.

1. **Declared visibility (`SymbolVisibility`).**
   - `VISIBILITY_UNSET` (0): no explicit keyword.
   - `VISIBILITY_LOCAL` (1): explicitly declared with `local message` or `local enum`.
   - `VISIBILITY_EXPORT` (2): explicitly declared with `export message` or `export enum`.
2. **Effective visibility resolution.**
   - If declared visibility is `VISIBILITY_EXPORT`, effective visibility is **Exported**.
   - If declared visibility is `VISIBILITY_LOCAL`, effective visibility is **Local**.
   - If declared visibility is `VISIBILITY_UNSET`, apply `default_symbol_visibility`:
     - `EXPORT_TOP_LEVEL` (Edition 2024 default): top-level messages/enums are **Exported**; nested messages/enums are **Local**.
     - `EXPORT_ALL`: all symbols are **Exported**.
     - `LOCAL_ALL`: all symbols are **Local**.
3. **Cross-file import boundary.**
   - If file `A.proto` imports `B.proto`, `A.proto` may reference only symbols in `B.proto` whose effective visibility is **Exported**.
   - Referencing a **Local** symbol is a `protoc` compile error:

     ```text
     Symbol "...", defined in "..." is not visible from "...". It is explicitly marked 'local' and cannot be accessed outside its own file
     ```

4. **Rust code generation contract (`src/codegen.rs`).**
   - For `compile_protos`, `protoc` enforces cross-proto import rules before invoking the plugin.
   - Generated Rust emits effective **Exported** types as `pub struct` / `pub enum`.
   - Effective **Local** types remain accessible within their crate/module hierarchy but can be restricted from top-level crate re-exports.

### 4.2 Naming Style Contract (`STYLE2024`)

Under `enforce_naming_style = STYLE2024`, the required casing is:

- Messages, Enums, Services: `PascalCase`.
- Fields, Oneofs: `lower_snake_case`.
- RPC Methods: `TitleCase`.
- Enum Values: `SCREAMING_SNAKE_CASE`.

`DescriptorPool` validates inherited naming options in Edition 2024 descriptors. Generated Edition 2024 consumers are qualified under `CG-14` (§8): the generator advertises Edition 2024 and compiles the approved naming, visibility, and extension consumers, while nonconforming names and unqualified subsets still fail before generation.

The RPC method rule was verified with `libprotoc 36.1` on 2026-09-23: `bad_method` is rejected with a `TitleCase` diagnostic, while `GoodMethod` is accepted. The independently runnable Edition 2023 conformance baseline remains pinned to v35.1.

---

## 5. Extensions Contract

### 5.1 Syntax in Edition 2024

Edition 2024 declares extension ranges inside messages and extends them through `extend` blocks:

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

- `DynamicMessage`
  - `get_extension(tag: u32) -> Option<&Value>`
  - `set_extension(tag: u32, value: Value)`
  - `has_extension(tag: u32) -> bool`
  - `clear_extension(tag: u32)`
- `DescriptorPool`
  - `get_extension(full_name: &str) -> Option<(Arc<MessageDescriptor>, FieldDescriptor)>`
  - `extension_numbers_of(containing_type: &str) -> Vec<u32>`
  - `file_for_extension(containing_type: &str, number: u32) -> Option<&str>`
- Unknown extension wire fields round-trip losslessly in `unknown_fields`.

### 5.3 Generated Code Extension Status

Google's official `rust_upb` implementation has an empty typed extension test stub in `third_party/protobuf/rust/test/shared/extensions_test.rs`; see [docs/upb.md](upb.md).

In `pure-protobuf`:

- Native code generation has an opt-in scalar slice: repeatable
  `typed_extension=fully.qualified.name` plugin parameters (or
  `Config::typed_extension(name)`) select singular Edition 2024 `int32`
  identifiers. The extension and its owned host must be declared in the same
  requested file. Without selections, existing generated output is unchanged.
- Selected hosts receive `get_extension`, `has_extension`, `set_extension`, and
  `clear_extension` methods taking `Extension<Host, i32>` constants from a
  generated `extensions` module. Reads scan the existing unknown bag without
  mutation; the last matching varint wins, with the descriptor default when
  absent. Set/clear affect matching varints only, preserving unrelated records
  and same-number fields of other wire types. Explicit default/zero values are
  present and serialize. Mutations invalidate the encoded-size cache.
- Unsupported selected types, cardinalities, editions, cross-file/extern
  extendees, illegal tags, ambiguous raw declarations, and generated namespace
  collisions fail before output is returned. Selected owner files sharing a
  package are rejected when their `extensions` modules would collide. The
  native bag/message storage layout and ordinary parser are unchanged;
  canonical unknown values and order are retained, without promising arbitrary
  noncanonical original byte fidelity.
- Extension fields parsed on typed messages are retained in the `UnknownFields` bag and can be inspected through dynamic reflection. `CG-14` qualifies this wire-preserving behavior for the approved fixture extensions and every extension kind in the original `rust/test/extensions.proto` schema (see §8).
- This scalar slice is tracked as `GN-15`. Repeated, enum, message, group,
  cross-file and view/mut extension access remain unresolved under `CG-14b`;
  the official upb extension ABI/registry remains separate under `UK-08`.
  Neither full card is completed by the scalar slice. Source and behavioral
  qualification for this slice must be read from its evidence record; an API
  description alone is not a gate result.

---

## 6. Language-Specific Options & Extensions Policy

Protocol Buffers defines language-specific feature extensions in `descriptor.proto`:

| Extension | Feature fields |
|---|---|
| `pb.cpp` (`CppFeatures`, tag 1000) | `string_type`: `VIEW`, `CORD`, `STRING`; `legacy_closed_enum`: `true`, `false` |
| `pb.java` (`JavaFeatures`, tag 1001) | `nest_in_file_class`: `YES`, `NO`; `legacy_closed_enum`: `true`, `false` |
| `pb.go` (`GoFeatures`, tag 1002) | `legacy_unmarshal_json_enum`: `true`, `false` |
| `pb.python` (`PythonFeatures`, tag 1003) | `py_generic_services`: `true`, `false` |
| `pb.csharp` (`CSharpFeatures`, tag 1004) | No additional behavior is defined here. |

Policy for pure-protobuf:

1. **Permissive parsing.** All language-specific feature extensions are valid protobuf option extensions and must be parsed or skipped without failing schema loading.
2. **C++ `string_type`.** `VIEW` and `CORD` map to standard `ProtoString` / `ProtoStr` representations, with `LazyStr` zero-copy borrow from the parse buffer where applicable. See [docs/upb.md](upb.md).
3. **No behavioral coupling.** Java, Go, Python, and C# generator features have zero runtime effect on Rust data structures or wire codecs.
4. **No `.pb.rust` upstream.** Official Protocol Buffers v35.1 does not define a `.pb.rust` feature extension. `pure-protobuf` introduces no proprietary required feature extensions.

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
| `repeated_field_encoding = EXPANDED` | `src/dynamic.rs` | Supported (`repeated_encoding = 2`) | Existing | Verified by `tests/fixtures/edition2024/bin/overrides_expanded_repeated.bin` |
| `utf8_validation = VERIFY` | `src/dynamic.rs` | Supported (`utf8 = 2`) | Existing | UTF-8 verify on parse active |
| `utf8_validation = NONE` | `src/dynamic.rs` | Supported (`utf8 = 3`, sets `utf8_validate = false`) | Existing | Raw string bytes accepted |
| `message_encoding = LENGTH_PREFIXED` | `src/dynamic.rs` | Supported (`message_encoding = 1`) | Existing | Standard length-delimited wire encoding |
| `message_encoding = DELIMITED` | `src/dynamic.rs` | Supported (`message_encoding = 2`, `delimited = true`) | Existing | Verified by `tests/fixtures/edition2024/bin/overrides_delimited_message.bin` |
| `json_format` | `src/dynamic.rs` | Resolved from File/Message/Enum feature options | **CG-13** | Inheritance and fixture oracles in `tests/dynamic.rs` |
| `enforce_naming_style = STYLE2024` | `src/dynamic.rs` | Descriptor names validated with inherited overrides; generated consumers qualified | **CG-13**, **CG-14** | The plugin advertises maximum Edition 2024 for the qualified subset |
| `default_symbol_visibility` | `src/dynamic.rs` | Resolved from file features | **CG-13** | Nested defaults and file overrides checked in `tests/dynamic.rs` |
| `export` / `local` keywords | `src/dynamic.rs` | Parsed on Message/Enum and enforced for cross-file references | **CG-13** | Local imports rejected by fixture tests |
| Extension Ranges & Declarations | `src/dynamic.rs` | Supported (`parse_extension_range`, `collect_raw`) | Existing | Fully parsed into `MessageDescriptor.extension_ranges` |
| Dynamic Extension Access | `src/dynamic.rs` | Supported (`get_extension`, `set_extension`) | Existing | Verified by `tests/json_text_ext.rs` |
| Typed Extension Codegen | `src/codegen.rs` | Not implemented; matches upstream `rust_upb` stub | **CG-14b** | Split task card for typed extension accessors |
| Plugin Supported Edition Range | `src/codegen.rs` | Advertises `EDITION_2024` (`1001`) for the qualified subset | **CG-14** | Raised after differential + original shared evidence; closed collections and `CG-14b` block a full claim |

### 7.2 Task Card Decomposition

Full Edition 2024 coverage spans descriptor resolution, symbol visibility, generated-code qualification, and typed extensions. It is split into bounded review cards:

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
       │ CG-14: Qualify Edition 2024 Generated Consumers (Done)  │
       │ - Raised maximum_edition to 1001 in CodeGeneratorResponse│
       │ - Compiled naming/visibility/extension consumers via plugin│
       │ - Verified typed accessors, visibility & wire preservation│
       │ - Differential + original shared evidence; 2023 baseline kept│
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

All fixtures in `tests/fixtures/edition2024/` are approved and serve as the immutable oracle for `CG-13` and `CG-14`.

### 8.1 Schema & Artifact Inventory

| Fixture | What it covers | Pinned vectors or notes |
|---|---|---|
| `defaults.proto` and `defaults.fds` | `edition = "2024";` with no explicit feature overrides. All fields have explicit presence; enums are open; repeated scalars are packed; strings verify UTF-8; submessages are length-prefixed; symbols default to exported. | `defaults_empty.bin` (0 bytes), `defaults_zero_set.bin` (6 bytes), `defaults_populated.bin` (96 bytes). |
| `overrides.proto` and `overrides.fds` | Explicit overrides for presence (`IMPLICIT`, `LEGACY_REQUIRED`), repeated encoding (`EXPANDED`), string validation (`NONE`), message encoding (`DELIMITED`), JSON format (`LEGACY_BEST_EFFORT`), and enum type (`CLOSED`). | `overrides_implicit_zero.bin` (0 bytes), `overrides_implicit_set.bin` (2 bytes), `overrides_expanded_repeated.bin` (6 bytes), `overrides_delimited_message.bin` (9 bytes). |
| `inheritance.proto` and `inheritance.fds` | File-level overrides (`IMPLICIT`, `EXPANDED`, `NONE`) inherited by `FileDefaultsConsumer` and overridden by `FieldLevelOverrides`; message-level and nested message `json_format` overrides; enum-level `CLOSED` overrides at file scope and inside messages. | Includes `TopLevelEnumClosed` and `MessageWithNestedEnums.NestedClosedEnum`. |
| `visibility.proto` and `visibility.fds` | Explicit `export` and `local` keywords on top-level and nested messages/enums. | Unadorned symbols default to `EXPORT` at top level and `LOCAL` when nested under `EXPORT_TOP_LEVEL`. |
| `extensions.proto` and `extensions.fds` | Extension ranges `100 to 1000;`, file-level `extend` blocks, and nested `extend` blocks. | `extensions_populated.bin` (34 bytes). |
| `rejected/` suite | Negative fixtures for syntax and visibility rejection. | 8 fixtures cover `import weak`, `ctype`, `java_multiple_files`, `group`, `optional`, `required`, non-standard naming, and cross-file `local` symbol imports. |

### 8.2 Checksum & Verification Integrity

All `.fds` and `.bin` artifacts are cryptographically registered in `tests/fixtures/edition2024/expectations.json`. Descriptor parsing or wire encoding regressions fail immediately against these golden oracles.

### 8.3 CG-14 Qualification Evidence (2026-09-29)

The direct descriptor-set generator compiles the five checked Edition 2024 fixtures plus the checked supplemental `fds/cg14_preview.fds` into a strict Rust-language Edition 2024 consumer.

The supplemental set contains:

- retained `STYLE_LEGACY` naming options;
- verified and unverified string maps;
- the already-vendored original extension schema.

The `tests/plugin.rs` consumer runs nine cases: presence, inherited overrides, singular closed enums, delimited framing, visibility, checked wire vectors, verified and unverified map string entries, pinned-vector typed/dynamic agreement, and all-wire-type original extension preservation.

Generated map decoders use the resolved key and value features of the map entry. The outer 2024 map field's `utf8_validate` flag is false.

#### Qualified subset

`CG-14` raised `maximum_edition` to `1001` for this qualified subset, evidenced by `tests/plugin.rs`:

- **Naming consumers.** `legacy_style.proto` compiles with inherited `STYLE_LEGACY`; nonconforming names without the opt-out fail before generation (`edition2024_direct_generation_rejects_unknown_feature_and_bad_name`).
- **Visibility consumers.** Exported and local symbols resolve per `EXPORT_TOP_LEVEL`; cross-file references to local symbols fail (`edition2024_direct_generation_enforces_imported_visibility`).
- **Extension consumers.** Fixture and original `rust/test/extensions.proto` schemas compile; extension wire bytes round-trip through unknown fields on typed messages and resolve field-by-field through dynamic reflection (`pinned_vectors_agree_between_typed_and_dynamic`, `original_extensions_preserve_all_wire_types`).
- **Differential agreement.** Every pinned `.bin` vector survives a typed parse/serialize and a dynamic parse/serialize byte-identically, including packed, expanded, delimited, and extension payloads.
- **Plugin wire contract.** Direct binary `CodeGeneratorRequest` tests prove `protoc-gen-pbrs` reports `maximum_edition = 1001` and emits files for every qualified fixture while returning a descriptive error (no files, no crash) for the fail-closed subset.
- **Live negotiation.** `edition2024_protoc_negotiates_2024_plugin_support` proves a 2024-capable `protoc` sends Edition 2024 files to the plugin end to end (capability-gated; checked-FDS tests cover the semantics hermetically).

#### Original shared-test evidence

The upstream `rust/test/shared/extensions_test.rs` is a license-only stub at the pinned `v35.1` (`35cd01f9fe9afbeea38cc7b979a3b6bfcde82c03`); the vendored copy is byte-identical to `third_party/protobuf` at that SHA. There are no runnable original Edition 2024 extension cases, so the applicable original evidence is the Edition 2024 `rust/test/extensions.proto` schema itself: `edition2024_original_shared_extension_suite_is_empty_at_pin` asserts the stub gained no cases and that all 23 declared extension numbers resolve in the checked preview descriptor set. Compiling that schema and checking unknown-extension wire preservation does not qualify typed extension access; that remains `CG-14b`.

#### Exclusions blocking a full 2024 claim

Repeated and map fields with closed enum values are explicitly rejected until their unknown-value semantics are implemented. Extension fields are not emitted as ordinary typed fields; their wire bytes round-trip through unknown fields until `CG-14b` adds typed accessors.

The repeated/map `CLOSED` guard remains necessary despite new checked `tests/fixtures/edition2024/{proto,fds}/closed_enum.*` and nine pinned, **local C++ v36.1** generated/dynamic wire observations in `tests/fixtures/edition2024/reference/`.

The source, descriptor, replay driver, and outputs have SHA-256 checks and a reproducible opt-in command in the fixture README. Default continuous integration (CI) checks their integrity without requiring an Edition 2024 compiler. The generated and dynamic parsers use the same C++ library, whose local binary was not independently verified as an official release.

The local C++ reference observations are exact but narrow:

- a packed negative `CLOSED` value re-encodes as a ten-byte unknown varint;
- an earlier unpinned Python 6.33.1 probe produced five bytes;
- unknown packed values move after the known packed field;
- invalid enum map entries become whole unknown entries.

These are exact **C++ reference** observations. They are not a universal Rust/upb re-encoding contract and do not prove that `pbrs` handles either collection shape. A reviewed cross-runtime policy and generated Rust tests against the checked vectors must precede removing the guards.

#### Conformance baseline

The pinned `v35.1` conformance runner contains no Edition 2024 cases (its suites top out at `EDITION_2023`), so a 2024 run would be case-identical to the 2023 baseline and cannot serve as 2024 evidence. `scripts/conformance.sh` therefore keeps the `v35.1`/max-2023 baseline as the independently runnable qualification gate and fails loudly if a future pin introduces 2024 cases. 2024-specific conformance qualification is blocked until the runner and the conformance message set grow 2024 coverage.

#### Retained-options note

The supplemental FDS was produced once with `libprotoc 36.1` and `--retain_options`. Mandatory Cargo tests consume only its checked bytes. `protoc 36.1` strips source-retention features such as `enforce_naming_style = STYLE_LEGACY` from ordinary descriptor sets. Without retained options, the resolver rejects nonconforming names instead of silently assuming Edition 2023 semantics.

`Config::compile_protos` does not request `--retain_options` by default.

Source-only rejection cases and byte-for-byte preview-FDS regeneration are explicitly ignored in the default Cargo gate. Run these ignored tests only with a compatible pinned `libprotoc 36.1`:

```bash
cargo test --test plugin edition2024_rejected_source_fixtures_fail_in_protoc -- --ignored --exact
cargo test --test plugin edition2024_preview_descriptor_matches_pinned_source_compiler -- --ignored --exact
```

The default Rust 2024 consumer, closed-enum, and plugin-cap tests do not invoke `protoc`.
