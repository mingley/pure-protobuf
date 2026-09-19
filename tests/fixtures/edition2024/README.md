# Edition 2024 Test Fixtures & Oracles

This directory contains versioned test fixtures, compiled descriptor sets, wire-format test vectors,
and expected semantic oracles for Protocol Buffers **Edition 2024** (`edition = "2024";`, enum `1001`).

These fixtures serve as the authoritative test oracle for:
- **CG-12**: Freeze the Edition 2024 semantic contract
- **CG-13**: Implement approved Edition 2024 descriptor semantics (`src/dynamic.rs`)
- **CG-14**: Qualify Edition 2024 generated consumers (`src/codegen.rs`)
- **CG-16**: Select a reviewed Rust proto frontend

---

## 1. Directory Structure

```text
tests/fixtures/edition2024/
├── README.md               # This document: fixture inventory, contracts, and oracles
├── expectations.json       # Consolidated machine-readable test oracle & checksums
├── proto/                  # Canonical Edition 2024 .proto schemas
│   ├── defaults.proto      # Baseline schema exercising all Edition 2024 defaults
│   ├── overrides.proto     # Schema exercising explicit overrides for every feature
│   ├── inheritance.proto   # Schema exercising file -> message -> field / enum inheritance
│   ├── visibility.proto    # Schema exercising export/local visibility keywords and defaults
│   └── extensions.proto    # Schema exercising Edition 2024 extension syntax and ranges
├── fds/                    # Deterministic FileDescriptorSet binaries compiled via protoc 36.1
│   ├── defaults.fds
│   ├── overrides.fds
│   ├── inheritance.fds
│   ├── visibility.fds
│   └── extensions.fds
├── bin/                    # Canonical wire-encoded binary payloads (.bin)
│   ├── defaults_empty.bin
│   ├── defaults_zero_set.bin
│   ├── defaults_populated.bin
│   ├── overrides_implicit_zero.bin
│   ├── overrides_implicit_set.bin
│   ├── overrides_expanded_repeated.bin
│   ├── overrides_delimited_message.bin
│   └── extensions_populated.bin
├── expectations/           # Per-schema resolved descriptor expectation JSONs
│   ├── defaults.json
│   ├── overrides.json
│   ├── inheritance.json
│   ├── visibility.json
│   └── extensions.json
└── rejected/               # Schemas rejected by upstream protoc & Edition 2024 specification
    ├── README.md
    ├── import_weak.proto
    ├── ctype_option.proto
    ├── java_multiple_files.proto
    ├── group_syntax.proto
    ├── optional_keyword.proto
    ├── required_keyword.proto
    ├── naming_style.proto
    ├── visibility_defs.proto
    └── visibility_import_local.proto
```

---

## 2. Schema Catalog

| Schema Path | Package | Key Behaviors Exercised |
|---|---|---|
| `proto/defaults.proto` | `edition2024.defaults` | Exercises default Edition 2024 behaviors: explicit presence on singular scalars without `optional`, open enum preserving unknown variants, packed repeated scalars, verified UTF-8 strings, length-prefixed submessages, allowed JSON format, STYLE2024 naming, and EXPORT_TOP_LEVEL symbol visibility. |
| `proto/overrides.proto` | `edition2024.overrides` | Exercises explicit feature overrides: `features.field_presence = IMPLICIT`, `features.field_presence = LEGACY_REQUIRED`, `features.repeated_field_encoding = EXPANDED`, `features.utf8_validation = NONE`, `features.message_encoding = DELIMITED`, `features.json_format = LEGACY_BEST_EFFORT`, and `features.enum_type = CLOSED`. |
| `proto/inheritance.proto` | `edition2024.inheritance` | Exercises feature inheritance rules: file-level defaults (`IMPLICIT`, `EXPANDED`, `NONE`), field-level overrides reverting to (`EXPLICIT`, `PACKED`, `VERIFY`), message-level `json_format` overrides with nested message overrides, and enum-level `CLOSED` overrides both at file scope and nested inside messages. |
| `proto/visibility.proto` | `edition2024.visibility` | Exercises symbol visibility: `export` and `local` keywords on top-level and nested messages/enums, and confirms unadorned nested messages/enums default to local under `EXPORT_TOP_LEVEL`. |
| `proto/extensions.proto` | `edition2024.extensions` | Exercises Edition 2024 extensions: `extensions 100 to 1000;` declaration, file-level `extend` block, nested message-scoped `extend` block, scalar, string, repeated, submessage, enum extensions, default values (`[default = 42]`), and extension feature overrides (`features.enum_type = CLOSED`). |

---

## 3. Pinned Edition 2024 Defaults Matrix

| Feature | FeatureSet Tag | Edition 2024 Default | Enum Name & Value | Semantic Meaning |
|---|---|---|---|---|
| `field_presence` | 1 | `EXPLICIT` | `FieldPresence::EXPLICIT = 1` | Singular scalar fields have presence tracking. Setting a field to `0` or `""` serializes it on the wire. |
| `enum_type` | 2 | `OPEN` | `EnumType::OPEN = 1` | Unknown enum integers are preserved as valid enum numbers and round-trip without corruption. |
| `repeated_field_encoding` | 3 | `PACKED` | `RepeatedFieldEncoding::PACKED = 1` | Packable repeated scalar fields are packed into length-delimited byte chunks on the wire. |
| `utf8_validation` | 4 | `VERIFY` | `Utf8Validation::VERIFY = 2` | String fields must contain valid UTF-8 bytes; invalid sequences trigger deserialization error. |
| `message_encoding` | 5 | `LENGTH_PREFIXED` | `MessageEncoding::LENGTH_PREFIXED = 1` | Submessages are encoded with wire type 2 (`WIRE_LEN`) followed by length varint and message payload. |
| `json_format` | 6 | `ALLOW` | `JsonFormat::ALLOW = 1` | Canonical Protobuf JSON serialization and deserialization is supported and allowed. |
| `enforce_naming_style` | 7 | `STYLE2024` | `EnforceNamingStyle::STYLE2024 = 1` | Enforces PascalCase for types and lower_snake_case for fields/methods during compilation. |
| `default_symbol_visibility`| 8 | `EXPORT_TOP_LEVEL` | `VisibilityFeature::EXPORT_TOP_LEVEL = 2` | Top-level messages/enums default to exported; nested messages/enums default to local. |

---

## 4. Wire-Format Vectors (`.bin`)

The `bin/` directory contains exact binary payloads encoded with `protoc --encode` to serve as golden wire-format oracles:

1. **`defaults_empty.bin` (0 bytes)**:
   An unpopulated `DefaultMessage`. Serializes to empty slice.

2. **`defaults_zero_set.bin` (6 bytes: `08 00 28 00 32 00`)**:
   `DefaultMessage` with `int32_field: 0`, `bool_field: false`, `string_field: ""`.
   - `08 00`: Tag 1 (`(1 << 3) | 0`), varint 0
   - `28 00`: Tag 5 (`(5 << 3) | 0`), varint 0
   - `32 00`: Tag 6 (`(6 << 3) | 2`), length 0
   *Demonstrates that in Edition 2024, default values for fields with EXPLICIT presence are emitted on the wire.*

3. **`defaults_populated.bin` (96 bytes)**:
   `DefaultMessage` populated with all standard types: scalars, enum, repeated int32 (packed: `52 03 01 02 03`), repeated string (unpacked strings: `5a 05 ... 5a 04 ...`), submessage, and map entry.

4. **`overrides_implicit_zero.bin` (0 bytes)**:
   `OverridesMessage` with `implicit_int32: 0`.
   *Demonstrates that with `features.field_presence = IMPLICIT`, setting a field to zero suppresses wire emission.*

5. **`overrides_implicit_set.bin` (2 bytes: `08 2a`)**:
   `OverridesMessage` with `implicit_int32: 42`. Emits tag 1 varint 42.

6. **`overrides_expanded_repeated.bin` (6 bytes: `18 0a 18 14 18 1e`)**:
   `OverridesMessage` with `expanded_int32: [10, 20, 30]`.
   - `18 0a`: Tag 3 (`(3 << 3) | 0`), varint 10
   - `18 14`: Tag 3, varint 20
   - `18 1e`: Tag 3, varint 30
   *Demonstrates `features.repeated_field_encoding = EXPANDED` emitting individual repeated varint tags rather than a packed length-delimited blob.*

7. **`overrides_delimited_message.bin` (9 bytes: `2b 0a 05 68 65 6c 6c 6f 2c`)**:
   `OverridesMessage` with `delimited_message: { text: "hello" }`.
   - `2b`: Tag 5, wire type 3 (`WIRE_SGROUP`, `(5 << 3) | 3`)
   - `0a 05 68 65 6c 6c 6f`: Submessage field tag 1, len 5, `"hello"`
   - `2c`: Tag 5, wire type 4 (`WIRE_EGROUP`, `(5 << 3) | 4`)
   *Demonstrates group-like delimited encoding selected via `features.message_encoding = DELIMITED`.*

8. **`extensions_populated.bin` (34 bytes)**:
   `ExtendableMessage` with base field 1 and extensions 101, 102, 103, 104, 105, 106.
   Includes extension range verification and default value handling.

---

## 5. Negative Oracle Summary (`rejected/`)

All negative fixtures in `rejected/` are verified against `protoc 36.1` to confirm exact rejection:

1. `import_weak.proto`: Weak imports are removed in Edition 2024.
2. `ctype_option.proto`: `[ctype = ...]` option is removed in Edition 2024.
3. `java_multiple_files.proto`: `option java_multiple_files = ...` is removed in Edition 2024.
4. `group_syntax.proto`: `group` keyword syntax is removed in editions.
5. `optional_keyword.proto`: `optional` keyword is removed in editions.
6. `required_keyword.proto`: `required` keyword is removed in editions.
7. `naming_style.proto`: Non-standard naming rejected under `STYLE2024`.
8. `visibility_import_local.proto`: Importing symbols marked `local` is rejected across file boundaries.

---

## 6. Downstream Task Consumption Contract

- **CG-13 (`src/dynamic.rs`)**:
  - Must parse `fds/*.fds` without panic or error.
  - Must verify resolved features on each `FieldDescriptor`, `MessageDescriptor`, and `EnumDescriptor` against `expectations/*.json`.
  - Must correctly identify `enum_type = CLOSED` on enums overriding the default (e.g. `ClosedEnum`).
  - Must decode `bin/*.bin` into `DynamicMessage` and match field presence, packed repeated, and delimited submessages.
- **CG-14 (`src/codegen.rs`)**:
  - Must compile `proto/*.proto` after raising `maximum_edition = 1001`.
  - Must generate typed accessors matching presence and encoding contracts.
