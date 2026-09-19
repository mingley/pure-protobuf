# Binary Differential Schema Fixtures

This directory contains deterministic, pinned-reference binary protobuf fixtures (.bin files), proto schema definitions, and a compiled descriptor set (`differential.fds`) used by `tests/differential_binary.rs`.

## Fixture Categories

1. **Scalar Presence (Proto2 vs Proto3)**
   - `presence_proto3_absent.bin`: Empty payload (0 bytes) representing absent/default proto3 fields.
   - `presence_proto3_default.bin`: Empty payload (0 bytes) confirming that default scalar values are omitted on wire in proto3.
   - `presence_proto3_set.bin`: Non-default values for implicit int32, implicit string, and explicit optional int32.
   - `presence_proto2_absent.bin`: Empty payload (0 bytes) for proto2 message with all fields unset (`has_*` false).
   - `presence_proto2_default_set.bin`: Tag 1 varint 0 (`0x08, 0x00`) verifying proto2 explicit presence tracking for 0 (`has_*` true).
   - `presence_proto2_set.bin`: Explicitly set non-default fields.

2. **Oneof Field Setting and Overwriting**
   - `oneof_first.bin`: Oneof with field 1 (`name = "alpha"`).
   - `oneof_second.bin`: Oneof with field 2 (`count = 456`).
   - `oneof_overwritten.bin`: Wire payload containing field 1 followed by field 2. Protobuf semantics require the last field in the oneof to overwrite earlier fields.

3. **Unknown Fields Preservation**
   - `unknown_fields_all_types.bin`: Known field 1 plus all 4 unknown field wire types:
     - Varint (wire 0): field 10 = 99999
     - Fixed64 (wire 1): field 11 = 0x0102030405060708
     - Length-delimited (wire 2): field 12 = "unknown payload"
     - Fixed32 (wire 5): field 13 = 0xdeadbeef
     Verifies byte-for-byte round-trip preservation.

4. **Open vs Closed Enums**
   - `enum_open_known.bin`: Proto3 open enum with known value (2).
   - `enum_open_unknown.bin`: Proto3 open enum with unknown value (999), retained in the field value.
   - `enum_closed_known.bin`: Proto2 closed enum with known value (2).
   - `enum_closed_unknown.bin`: Proto2 closed enum with unknown value (999), moved to unknown fields.

5. **Map Entries (Last-Wins Key Semantics)**
   - `map_single_entry.bin`: Single map entry `key1 -> 10`.
   - `map_duplicate_keys.bin`: Two map entries for `dup_key`: `100` then `200`. Verifies last-wins key semantics on decode.

6. **Extensions (Proto2 Dynamic Extensions)**
   - `extension_empty.bin`: Base message with id = 1 and no extensions.
   - `extension_set.bin`: Base message with id = 1, `ext_int32` (tag 101) = 42, and `ext_string` (tag 102) = "extended".

7. **Packed vs Unpacked Repeated Fields**
   - `packed_repeated.bin`: Repeated int32 encoded as length-delimited packed varints.
   - `unpacked_repeated.bin`: Repeated int32 encoded as separate varint tags.
   Verifies that packed and unpacked formats decode interchangeably into packed or unpacked message definitions.

8. **Truncated Payloads**
   - `truncated_varint.bin`: Incomplete varint tag/value (`0x08, 0xff`).
   - `truncated_length_delimited.bin`: Length-delimited header with truncated body (`0x0a, 0x14, 0x01, 0x02, 0x03, 0x04`).

9. **Recursion Depth Limit (100 Levels)**
   - `recursion_depth_100.bin`: Exactly 100 levels of nested submessages (succeeds at `RECURSION_LIMIT`).
   - `recursion_depth_101.bin`: 101 levels of nested submessages (fails with recursion limit exceeded).

10. **Wire Merge Semantics**
    - `merge_part1.bin`: Scalar field 1 = 1, nested message item_id = 10, repeated_vals = [100].
    - `merge_part2.bin`: Scalar field 1 = 2, nested message item_name = "merged", repeated_vals = [200].
    - `merge_concatenated.bin`: Concatenation of part 1 and part 2 demonstrating scalar overwrite, submessage merge, and repeated append.

11. **WKT Format Fixtures (JSON and Text)**
    - `wkt_timestamp*.json`, `wkt_timestamp.textproto`: RFC 3339 UTC string with fractional seconds and offsets.
    - `wkt_duration*.json`, `wkt_duration.textproto`: Duration format with `s` suffix, negative and fractional durations.
    - `wkt_empty.json`, `wkt_empty.textproto`: Empty `{}` object format.
    - `wkt_*_wrapper.json`, `wkt_*_wrapper.textproto`: Direct value formatting for proto3 wrapper types.
    - `wkt_fieldmask.json`, `wkt_fieldmask.textproto`: Comma-separated camelCase paths in JSON vs snake_case in proto/text.
    - `wkt_struct.json`, `wkt_listvalue.json`, `wkt_value_*.json`: Struct/Value/ListValue JSON and textproto representations.

12. **Enum Format Fixtures**
    - `enum_string_name.*`: Enum serialized/deserialized by name string.
    - `enum_integer_number.*`: Enum deserialized from numeric value.
    - `enum_open_unknown.*`: Open enum preserving unknown numeric values.
    - `enum_unknown_string.json`: Unknown enum string for testing strict rejection vs skip on ignore_unknown.

13. **Numeric Format Edge Cases**
    - `numeric_signed_zero.*`: Signed negative zero (`-0.0` in JSON, `-0` in text) preserving sign bit.
    - `numeric_special_strings.*`, `numeric_specials.textproto`: `NaN` and `Infinity` / `-Infinity`.
    - `numeric_subnormals.*`: Float32 and Float64 minimum positive subnormals.
    - `numeric_large_exponents.json`: Exponent notation strings and numbers.
    - `numeric_float32_max.json`, `numeric_float32_overflow.json`: Boundary rounding and out-of-range rejection.

14. **Map Format Fixtures**
    - `map_string_key.json`, `map_integer_key.json`, `map_bool_key.json`: Map keys of string, integer, and boolean types.
    - `map_cases.textproto`: Text format map entries.
    - `map_duplicate_keys.json`: JSON duplicate map keys (must be rejected).
    - `map_duplicate_keys.textproto`: Text format duplicate map entries (last-wins semantics).

15. **Casing and Duplicate Fields**
    - `casing_camel.json`: Canonical camelCase field name.
    - `casing_snake.json`: Proto field name (snake_case).
    - `casing_duplicate.json`: Both camelCase and snake_case for the same field in one JSON object (must be rejected).

16. **Format Round-Trip Fixtures**
    - `format_roundtrip.json`, `format_roundtrip.textproto`: Comprehensive payloads for parse -> print -> verify equality.

## Regeneration

Run:
```bash
./scripts/generate-differential-fixtures.sh
```
