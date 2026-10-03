// Shared, bounded QG-18 descriptor and wire vectors. No compiler is invoked here.

use pbrs::rt::{WIRE_EGROUP, WIRE_SGROUP, WIRE_VARINT, encode_len_field, encode_tag, encode_varint};

pub fn unknown_groups(count: u32, number: u32) -> Vec<u8> {
    let mut wire = Vec::new();
    for _ in 0..count {
        encode_tag(&mut wire, number, WIRE_SGROUP);
    }
    encode_tag(&mut wire, 2, WIRE_VARINT);
    encode_varint(&mut wire, 7);
    for _ in 0..count {
        encode_tag(&mut wire, number, WIRE_EGROUP);
    }
    wire
}

pub fn known_then_unknown(known: u32, child: u32, groups: u32, number: u32) -> Vec<u8> {
    let mut wire = unknown_groups(groups, number);
    for _ in 0..known {
        let mut outer = vec![0x10, 7]; // Nonempty known scalar prevents empty-child shortcuts.
        encode_len_field(&mut outer, child, &wire);
        wire = outer;
    }
    wire
}

pub fn known_group_pairs_then_unknown(pairs: u32, groups: u32) -> Vec<u8> {
    let mut wire = unknown_groups(groups, 99);
    for _ in 0..pairs {
        let mut outer = vec![0x10, 7];
        encode_tag(&mut outer, 4, WIRE_SGROUP);
        encode_len_field(&mut outer, 1, &wire);
        encode_tag(&mut outer, 4, WIRE_EGROUP);
        wire = outer;
    }
    wire
}

fn descriptor_field(name: &str, number: u32, label: u32, kind: u32, type_name: &str) -> Vec<u8> {
    let mut field = Vec::new();
    encode_len_field(&mut field, 1, name.as_bytes());
    for (tag, value) in [(3, number), (4, label), (5, kind)] {
        encode_tag(&mut field, tag, WIRE_VARINT);
        encode_varint(&mut field, u64::from(value));
    }
    if !type_name.is_empty() {
        encode_len_field(&mut field, 6, type_name.as_bytes());
    }
    field
}

pub fn descriptor_set() -> Vec<u8> {
    // Mirrors tests/fixtures/unknown_group_depth.proto; no protoc or frozen FDS is changed.
    let mut node = Vec::new();
    encode_len_field(&mut node, 1, b"Node");
    for field in [
        descriptor_field("child", 1, 1, 11, ".qg18.Node"),
        descriptor_field("value", 2, 1, 5, ""),
        descriptor_field("children", 3, 3, 11, ".qg18.Node"),
        descriptor_field("childgroup", 4, 1, 10, ".qg18.Node.ChildGroup"),
    ] {
        encode_len_field(&mut node, 2, &field);
    }
    let mut group = Vec::new();
    encode_len_field(&mut group, 1, b"ChildGroup");
    encode_len_field(
        &mut group,
        2,
        &descriptor_field("child", 1, 1, 11, ".qg18.Node"),
    );
    encode_len_field(&mut node, 3, &group);
    let mut file = Vec::new();
    encode_len_field(&mut file, 1, b"qg18.proto");
    encode_len_field(&mut file, 2, b"qg18");
    encode_len_field(&mut file, 4, &node);
    encode_len_field(&mut file, 12, b"proto2");
    let mut fds = Vec::new();
    encode_len_field(&mut fds, 1, &file);
    fds
}
