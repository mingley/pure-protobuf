//! Bounded QG-20 vectors and independent descriptor bytes; no compiler is invoked.

use pbrs::rt::{
    WIRE_EGROUP, WIRE_SGROUP, WIRE_VARINT, encode_len_field, encode_tag, encode_varint,
};

pub fn unknown_groups(count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..count {
        encode_tag(&mut out, 99, WIRE_SGROUP);
    }
    out.extend_from_slice(&[0x10, 7]);
    for _ in 0..count {
        encode_tag(&mut out, 99, WIRE_EGROUP);
    }
    out
}

pub fn wrap_known(mut out: Vec<u8>, count: u32, child: u32) -> Vec<u8> {
    for _ in 0..count {
        let mut outer = vec![0x10, 7]; // Every known body is nonempty.
        encode_len_field(&mut outer, child, &out);
        out = outer;
    }
    out
}

pub fn message_entry(key: i32, values: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    encode_tag(&mut out, 1, WIRE_VARINT);
    encode_varint(&mut out, u64::from_le_bytes(i64::from(key).to_le_bytes()));
    for value in values {
        encode_len_field(&mut out, 2, value);
    }
    out
}

pub fn map_node(field: u32, entries: &[&[u8]]) -> Vec<u8> {
    let mut out = vec![0x10, 7];
    for entry in entries {
        encode_len_field(&mut out, field, entry);
    }
    out
}

pub fn message_value(known: u32, child: u32, groups: u32) -> Vec<u8> {
    let value = unknown_groups(groups);
    let entry = message_entry(1, &[&value]);
    wrap_known(map_node(5, &[&entry]), known, child)
}

pub fn scalar_entry(known: u32, child: u32) -> Vec<u8> {
    wrap_known(map_node(6, &[&[0x08, 1, 0x10, 7]]), known, child)
}

pub fn empty_entry(known: u32, child: u32, field: u32) -> Vec<u8> {
    wrap_known(map_node(field, &[&[]]), known, child)
}

pub fn malformed_value(known: u32, child: u32, value: &[u8]) -> Vec<u8> {
    let entry = message_entry(1, &[value]);
    wrap_known(map_node(5, &[&entry]), known, child)
}

fn field(name: &str, number: u32, label: u32, kind: u32, type_name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    encode_len_field(&mut out, 1, name.as_bytes());
    for (tag, value) in [(3, number), (4, label), (5, kind)] {
        encode_tag(&mut out, tag, WIRE_VARINT);
        encode_varint(&mut out, u64::from(value));
    }
    if !type_name.is_empty() {
        encode_len_field(&mut out, 6, type_name.as_bytes());
    }
    out
}

fn map_entry(name: &str, value_kind: u32, value_type: &str) -> Vec<u8> {
    let mut out = Vec::new();
    encode_len_field(&mut out, 1, name.as_bytes());
    for f in [
        field("key", 1, 1, 5, ""),
        field("value", 2, 1, value_kind, value_type),
    ] {
        encode_len_field(&mut out, 2, &f);
    }
    encode_len_field(&mut out, 7, &[0x38, 1]); // MessageOptions.map_entry.
    out
}

pub fn descriptor_set() -> Vec<u8> {
    // Mirrors fixtures/generated_map_value_depth.proto; no protoc/frozen FDS changes.
    let mut node = Vec::new();
    encode_len_field(&mut node, 1, b"Node");
    for f in [
        field("child", 1, 1, 11, ".qg20.Node"),
        field("value", 2, 1, 5, ""),
        field("children", 3, 3, 11, ".qg20.Node"),
        field("label", 4, 1, 9, ""),
        field("nodes", 5, 3, 11, ".qg20.Node.NodesEntry"),
        field("scores", 6, 3, 11, ".qg20.Node.ScoresEntry"),
        field("payloads", 7, 3, 11, ".qg20.Node.PayloadsEntry"),
    ] {
        encode_len_field(&mut node, 2, &f);
    }
    for entry in [
        map_entry("NodesEntry", 11, ".qg20.Node"),
        map_entry("ScoresEntry", 5, ""),
        map_entry("PayloadsEntry", 11, ".qg20.Payload"),
    ] {
        encode_len_field(&mut node, 3, &entry);
    }
    let mut payload = Vec::new();
    encode_len_field(&mut payload, 1, b"Payload");
    for f in [field("value", 1, 2, 5, ""), field("label", 2, 1, 9, "")] {
        encode_len_field(&mut payload, 2, &f);
    }
    let mut file = Vec::new();
    encode_len_field(&mut file, 1, b"qg20.proto");
    encode_len_field(&mut file, 2, b"qg20");
    encode_len_field(&mut file, 4, &node);
    encode_len_field(&mut file, 4, &payload);
    encode_len_field(&mut file, 12, b"proto2");
    let mut out = Vec::new();
    encode_len_field(&mut out, 1, &file);
    out
}
