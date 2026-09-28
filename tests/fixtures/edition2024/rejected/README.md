# Disallowed & Rejected Edition 2024 Constructs

This directory contains negative test fixtures demonstrating protobuf constructs that are explicitly
disallowed, removed, or rejected by upstream `protoc` and the Edition 2024 semantic specification.

These serve as authoritative negative oracles for:
- **CG-12**: Freeze the Edition 2024 semantic contract
- **CG-13**: Implement approved Edition 2024 descriptor semantics
- **CG-16**: Select a reviewed Rust proto frontend

---

## Rejection Inventory

| Fixture | Rejected Construct | Upstream Error Pattern | Semantic Reason |
|---|---|---|---|
| `import_weak.proto` | `import weak "...";` | `weak import is not supported in edition 2024 and above. Consider using option import instead.` | Weak imports are retired in Edition 2024. Runtimes must reject weak dependencies. |
| `ctype_option.proto` | `[ctype = STRING_PIECE]` | `ctype option is not allowed under edition 2024 and beyond. Use the feature string_type = VIEW\|CORD\|STRING\|... instead.` | Field-level `ctype` option is replaced by `features.(pb.cpp).string_type`. |
| `java_multiple_files.proto` | `option java_multiple_files = true;` | `google.protobuf.FileOptions.java_multiple_files has been removed in edition 2024: This behavior is enabled by default in editions 2024 and above. To disable it, you can set features.(pb.java).nest_in_file_class = YES on individual messages, enums, or services.` | File-level Java option removed; multiple files is the default in Edition 2024. |
| `group_syntax.proto` | `group Foo = 1 { ... }` | `Group syntax is no longer supported in editions. To get group behavior you can specify features.message_encoding = DELIMITED on a message field.` | Group keyword completely removed. Delimited wire encoding is selected via `features.message_encoding = DELIMITED`. |
| `optional_keyword.proto` | `optional int32 x = 1;` | `Label "optional" is not supported in editions. By default, all singular fields have presence unless features.field_presence is set.` | Label `optional` is redundant and illegal in editions. Singular fields default to explicit presence. |
| `required_keyword.proto` | `required int32 x = 1;` | `Label "required" is not supported in editions, use features.field_presence = LEGACY_REQUIRED.` | Label `required` removed from syntax. Required presence must be declared via `features.field_presence = LEGACY_REQUIRED`. |
| `naming_style.proto` | `message badName { int32 BadField = 1; }` | `Message name badName should begin with a capital letter... Field name BadField should be lower_snake_case...` | `enforce_naming_style` defaults to `STYLE2024` in Edition 2024, enforcing PascalCase types and snake_case fields. |
| `visibility_import_local.proto` | Importing `defs.LocalMsg` from `visibility_defs.proto` | `Symbol "defs.LocalMsg", defined in "visibility_defs.proto" is not visible from "...". It is explicitly marked 'local' and cannot be accessed outside its own file` | Symbols marked `local` cannot be imported across `.proto` source file boundaries. |
