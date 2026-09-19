#!/usr/bin/env bash
# Generate deterministic binary differential fixtures for pure-protobuf.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export FIXTURES_DIR="$ROOT/tests/fixtures/differential"

mkdir -p "$FIXTURES_DIR"

# 1. Compile .proto files into FileDescriptorSet
if command -v protoc >/dev/null 2>&1; then
  echo "Compiling differential proto files to descriptor set..."
  protoc -I "$FIXTURES_DIR" \
    --descriptor_set_out="$FIXTURES_DIR/differential.fds" \
    --include_imports \
    "$FIXTURES_DIR/differential_proto2.proto" \
    "$FIXTURES_DIR/differential_proto3.proto"
else
  echo "Warning: protoc not found on PATH; skipping .fds generation." >&2
fi

# 2. Generate deterministic binary fixtures
python3 - << 'EOF'
import os
import struct

FIXTURES_DIR = os.environ["FIXTURES_DIR"]

def encode_varint(v):
    if v == 0:
        return b'\x00'
    buf = bytearray()
    while v > 0x7f:
        buf.append((v & 0x7f) | 0x80)
        v >>= 7
    buf.append(v & 0x7f)
    return bytes(buf)

def tag(field_number, wire_type):
    return encode_varint((field_number << 3) | wire_type)

def len_delimited(field_number, payload):
    return tag(field_number, 2) + encode_varint(len(payload)) + payload

def write_fixture(filename, data):
    path = os.path.join(FIXTURES_DIR, filename)
    if isinstance(data, str):
        with open(path, "w", encoding="utf-8") as f:
            f.write(data)
        print(f"Wrote {filename} ({len(data)} chars)")
    else:
        with open(path, "wb") as f:
            f.write(data)
        print(f"Wrote {filename} ({len(data)} bytes)")

# 1. Presence vs absent scalar fields (proto2 vs proto3)
write_fixture("presence_proto3_absent.bin", b"")
write_fixture("presence_proto3_default.bin", b"")
write_fixture(
    "presence_proto3_set.bin",
    tag(1, 0) + encode_varint(42) +
    len_delimited(2, b"hello") +
    tag(3, 0) + encode_varint(100)
)
write_fixture("presence_proto2_absent.bin", b"")
write_fixture(
    "presence_proto2_default_set.bin",
    tag(1, 0) + encode_varint(0)
)
write_fixture(
    "presence_proto2_set.bin",
    tag(1, 0) + encode_varint(42) +
    len_delimited(2, b"world")
)

# 2. Oneof field setting and overwriting
write_fixture(
    "oneof_first.bin",
    len_delimited(1, b"alpha")
)
write_fixture(
    "oneof_second.bin",
    tag(2, 0) + encode_varint(456)
)
write_fixture(
    "oneof_overwritten.bin",
    len_delimited(1, b"alpha") + tag(2, 0) + encode_varint(456)
)

# 3. Unknown fields (varint, fixed32, fixed64, length-delimited)
unknown_data = (
    tag(1, 0) + encode_varint(42) +                          # known field 1: int32 = 42
    tag(10, 0) + encode_varint(99999) +                      # unknown varint: 99999
    tag(11, 1) + struct.pack("<Q", 0x0102030405060708) +    # unknown fixed64
    len_delimited(12, b"unknown payload") +                  # unknown length-delimited
    tag(13, 5) + struct.pack("<I", 0xdeadbeef)               # unknown fixed32
)
write_fixture("unknown_fields_all_types.bin", unknown_data)

# 4. Open vs closed enums with known and unknown enum values
write_fixture(
    "enum_open_known.bin",
    tag(1, 0) + encode_varint(2)
)
write_fixture(
    "enum_open_unknown.bin",
    tag(1, 0) + encode_varint(999)
)
write_fixture(
    "enum_closed_known.bin",
    tag(1, 0) + encode_varint(2)
)
write_fixture(
    "enum_closed_unknown.bin",
    tag(1, 0) + encode_varint(999)
)

# 5. Map entries (last-wins key semantics)
map_entry_1 = len_delimited(1, b"key1") + tag(2, 0) + encode_varint(10)
write_fixture("map_single_entry.bin", len_delimited(1, map_entry_1))

map_dup_1 = len_delimited(1, b"dup_key") + tag(2, 0) + encode_varint(100)
map_dup_2 = len_delimited(1, b"dup_key") + tag(2, 0) + encode_varint(200)
write_fixture("map_duplicate_keys.bin", len_delimited(1, map_dup_1) + len_delimited(1, map_dup_2))

# 6. Extensions (proto2 dynamic extensions)
write_fixture(
    "extension_empty.bin",
    tag(1, 0) + encode_varint(1)
)
write_fixture(
    "extension_set.bin",
    tag(1, 0) + encode_varint(1) +
    tag(101, 0) + encode_varint(42) +
    len_delimited(102, b"extended")
)

# 7. Packed vs unpacked repeated fields
packed_payload = bytes([10, 20, 30, 40])
write_fixture(
    "packed_repeated.bin",
    len_delimited(1, packed_payload)
)
unpacked_payload = (
    tag(1, 0) + encode_varint(10) +
    tag(1, 0) + encode_varint(20) +
    tag(1, 0) + encode_varint(30) +
    tag(1, 0) + encode_varint(40)
)
write_fixture("unpacked_repeated.bin", unpacked_payload)

# 8. Truncated varint / truncated length-delimited payload
write_fixture("truncated_varint.bin", bytes([0x08, 0xff]))
write_fixture("truncated_length_delimited.bin", bytes([0x0a, 0x14, 0x01, 0x02, 0x03, 0x04]))

# 9. Recursion depth limit (100 levels)
# RecursiveNode: value = 1, child = 2
def build_nested_node(depth):
    # inner leaf
    node = tag(1, 0) + encode_varint(1)
    for _ in range(depth):
        node = len_delimited(2, node)
    return node

write_fixture("recursion_depth_100.bin", build_nested_node(100))
write_fixture("recursion_depth_101.bin", build_nested_node(101))

# 10. Merge semantics (wire concatenation / field merge)
merge_part1 = (
    tag(1, 0) + encode_varint(1) +
    len_delimited(2, tag(1, 0) + encode_varint(10)) +
    tag(3, 0) + encode_varint(100)
)
write_fixture("merge_part1.bin", merge_part1)

merge_part2 = (
    tag(1, 0) + encode_varint(2) +
    len_delimited(2, len_delimited(2, b"merged")) +
    tag(3, 0) + encode_varint(200)
)
write_fixture("merge_part2.bin", merge_part2)

write_fixture("merge_concatenated.bin", merge_part1 + merge_part2)

# 11. JSON and text format differential fixtures

# 11.1 WKT JSON and text format fixtures
write_fixture("wkt_timestamp.json", '"2026-09-18T15:12:28.123456789Z"')
write_fixture("wkt_timestamp_offset.json", '"2026-09-18T20:12:28.123456789+05:00"')
write_fixture("wkt_timestamp_epoch.json", '"1970-01-01T00:00:00Z"')
write_fixture("wkt_timestamp.textproto", "seconds: 1789744348\nnanos: 123456789\n")

write_fixture("wkt_duration.json", '"123.456789s"')
write_fixture("wkt_duration_neg.json", '"-0.500s"')
write_fixture("wkt_duration_zero.json", '"0s"')
write_fixture("wkt_duration.textproto", "seconds: 123\nnanos: 456789000\n")

write_fixture("wkt_empty.json", "{}")
write_fixture("wkt_empty.textproto", "")

write_fixture("wkt_bool_wrapper.json", "true")
write_fixture("wkt_bool_wrapper.textproto", "value: true\n")
write_fixture("wkt_int32_wrapper.json", "42")
write_fixture("wkt_int32_wrapper.textproto", "value: 42\n")
write_fixture("wkt_int64_wrapper.json", '"1234567890123"')
write_fixture("wkt_int64_wrapper.textproto", "value: 1234567890123\n")
write_fixture("wkt_uint32_wrapper.json", "4294967295")
write_fixture("wkt_uint32_wrapper.textproto", "value: 4294967295\n")
write_fixture("wkt_uint64_wrapper.json", '"18446744073709551615"')
write_fixture("wkt_uint64_wrapper.textproto", "value: 18446744073709551615\n")
write_fixture("wkt_float_wrapper.json", "1.5")
write_fixture("wkt_float_wrapper.textproto", "value: 1.5\n")
write_fixture("wkt_double_wrapper.json", "2.718281828")
write_fixture("wkt_double_wrapper.textproto", "value: 2.718281828\n")
write_fixture("wkt_string_wrapper.json", '"hello pure-protobuf"')
write_fixture("wkt_string_wrapper.textproto", 'value: "hello pure-protobuf"\n')
write_fixture("wkt_bytes_wrapper.json", '"aGVsbG8gd29ybGQ="')
write_fixture("wkt_bytes_wrapper.textproto", 'value: "hello world"\n')

write_fixture("wkt_fieldmask.json", '"fooBar,bazQux.subField"')
write_fixture("wkt_fieldmask.textproto", 'paths: "foo_bar"\npaths: "baz_qux.sub_field"\n')

write_fixture("wkt_struct.json", '{\n  "bool": true,\n  "list": [\n    "item",\n    10.0,\n    false\n  ],\n  "nested": {\n    "key": "value"\n  },\n  "null": null,\n  "num": 42.5,\n  "str": "hello"\n}')
write_fixture("wkt_listvalue.json", '[\n  "item",\n  42.0,\n  true,\n  null\n]')
write_fixture("wkt_value_str.json", '"hello value"')
write_fixture("wkt_value_num.json", "42.5")
write_fixture("wkt_value_bool.json", "true")
write_fixture("wkt_value_null.json", "null")

# 11.2 Enum JSON and text format fixtures
write_fixture("enum_string_name.json", '{"status":"OPEN_TWO"}')
write_fixture("enum_integer_number.json", '{"status":2}')
write_fixture("enum_open_unknown.json", '{"status":999}')
write_fixture("enum_unknown_string.json", '{"status":"UNKNOWN_CUSTOM_ENUM"}')
write_fixture("enum_string_name.textproto", "status: OPEN_TWO\n")
write_fixture("enum_integer_number.textproto", "status: 2\n")
write_fixture("enum_open_unknown.textproto", "status: 999\n")

# 11.3 Numeric edge cases
write_fixture("numeric_signed_zero.json", '{"optionalFloat": -0.0, "optionalDouble": -0.0}')
write_fixture("numeric_signed_zero_str.json", '{"optionalFloat": "-0.0", "optionalDouble": "-0.0"}')
write_fixture("numeric_special_strings.json", '{"optionalFloat": "NaN", "optionalDouble": "Infinity"}')
write_fixture("numeric_neg_infinity.json", '{"optionalFloat": "-Infinity", "optionalDouble": "-Infinity"}')
write_fixture("numeric_subnormals.json", '{"optionalFloat": 1.40129846e-45, "optionalDouble": 4.9406564584124654e-324}')
write_fixture("numeric_subnormals_neg.json", '{"optionalFloat": -1.40129846e-45, "optionalDouble": -4.9406564584124654e-324}')
write_fixture("numeric_large_exponents.json", '{"optionalFloat": "1.5e+2", "optionalDouble": "2.5E-3"}')
write_fixture("numeric_float32_max.json", '{"optionalFloat": 3.402823e+38}')
write_fixture("numeric_float32_overflow.json", '{"optionalFloat": 3.502823e+38}')
write_fixture("numeric_float64_overflow.json", '{"optionalDouble": "1.89769e+308"}')
write_fixture("numeric_signed_zero.textproto", "optional_float: -0\noptional_double: -0\n")
write_fixture("numeric_specials.textproto", "optional_float: nan\noptional_double: inf\n")
write_fixture("numeric_neg_inf.textproto", "optional_float: -inf\noptional_double: -inf\n")
write_fixture("numeric_subnormals.textproto", "optional_float: 1.40129846e-45\noptional_double: 4.9406564584124654e-324\n")

# 11.4 Maps fixtures
write_fixture("map_string_key.json", '{"mapStringString": {"farewell": "world", "greeting": "hello"}}')
write_fixture("map_integer_key.json", '{"mapInt32Int32": {"-1": 200, "0": 0, "42": 100}}')
write_fixture("map_bool_key.json", '{"mapBoolBool": {"false": false, "true": true}}')
write_fixture("map_duplicate_keys.json", '{"mapStringString": {"dupKey": "first", "dupKey": "second"}}')
write_fixture("map_cases.textproto", """map_int32_int32 {
  key: -1
  value: 200
}
map_int32_int32 {
  key: 0
  value: 0
}
map_int32_int32 {
  key: 42
  value: 100
}
map_bool_bool {
  key: false
  value: false
}
map_bool_bool {
  key: true
  value: true
}
map_string_string {
  key: "farewell"
  value: "world"
}
map_string_string {
  key: "greeting"
  value: "hello"
}
""")
write_fixture("map_duplicate_keys.textproto", """map_string_string {
  key: "dupKey"
  value: "first"
}
map_string_string {
  key: "dupKey"
  value: "second"
}
""")

# 11.5 Casing and duplicate fields
write_fixture("casing_camel.json", '{"optionalInt32": 42}')
write_fixture("casing_snake.json", '{"optional_int32": 42}')
write_fixture("casing_duplicate.json", '{"optionalInt32": 42, "optional_int32": 99}')

# 11.6 Full round-trip payload
write_fixture("format_roundtrip.json", """{
  "optionalInt32": 101,
  "optionalInt64": "102",
  "optionalUint32": 103,
  "optionalUint64": "104",
  "optionalSint32": -105,
  "optionalSint64": "-106",
  "optionalFixed32": 107,
  "optionalFixed64": "108",
  "optionalSfixed32": -109,
  "optionalSfixed64": "-110",
  "optionalFloat": 1.5,
  "optionalDouble": -2.5,
  "optionalBool": true,
  "optionalString": "roundtrip-string",
  "optionalBytes": "cm91bmR0cmlwLWJ5dGVz",
  "optionalNestedEnum": "BAR",
  "repeatedString": [
    "alpha",
    "beta"
  ],
  "mapStringString": {
    "k1": "v1",
    "k2": "v2"
  },
  "mapInt32Int32": {
    "1": 10,
    "2": 20
  },
  "mapBoolBool": {
    "false": false,
    "true": true
  }
}""")

write_fixture("format_roundtrip.textproto", """optional_int32: 101
optional_int64: 102
optional_uint32: 103
optional_uint64: 104
optional_sint32: -105
optional_sint64: -106
optional_fixed32: 107
optional_fixed64: 108
optional_sfixed32: -109
optional_sfixed64: -110
optional_float: 1.5
optional_double: -2.5
optional_bool: true
optional_string: "roundtrip-string"
optional_bytes: "roundtrip-bytes"
optional_nested_enum: BAR
repeated_string: "alpha"
repeated_string: "beta"
map_int32_int32 {
  key: 1
  value: 10
}
map_int32_int32 {
  key: 2
  value: 20
}
map_bool_bool {
  key: false
  value: false
}
map_bool_bool {
  key: true
  value: true
}
map_string_string {
  key: "k1"
  value: "v1"
}
map_string_string {
  key: "k2"
  value: "v2"
}
""")

print("All differential fixtures generated successfully.")
EOF

chmod +x "$ROOT/scripts/generate-differential-fixtures.sh"
echo "Fixture generation complete."
