//! Table-driven vs inline parse differential (PK-06).
//!
//! Every binary payload under `tests/fixtures`, `bench/corpora/payloads`,
//! and `fuzz/corpus` is parsed through both the inline engine and the
//! table-driven engine, comparing success/failure plus the resulting
//! messages byte-for-byte (known fields and unknown fields separately).
//! Correct-schema cases pin differential fixtures to their own messages;
//! the sweep runs every payload against small, wide, and synthetic schemas.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    clippy::disallowed_types,
    unreachable_pub,
    reason = "integration tests are sync; generated fixtures live in the test crate"
)]

use pbrs::table::{
    FieldEntry, FieldFlags, FieldKind, Presence, Scalar, TableMerge, build_table, find_entry,
    merge_table, parse_dynamic_table_with, validate_table,
};
use pbrs::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, MessageDescriptor,
    Serialize,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DIFFERENTIAL_FDS: &[u8] = include_bytes!("fixtures/differential/differential.fds");
const CONFORMANCE_FDS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/vendor/google/conformance_fds.bin"
));

fn differential_pool() -> Arc<DescriptorPool> {
    Arc::new(DescriptorPool::from_file_descriptor_set(DIFFERENTIAL_FDS).expect("differential.fds"))
}

fn conformance_pool() -> Arc<DescriptorPool> {
    Arc::new(DescriptorPool::from_file_descriptor_set(CONFORMANCE_FDS).expect("conformance FDS"))
}

/// Parse `data` for `message` through both dynamic engines and require
/// identical outcomes: both fail, or both succeed with byte-identical
/// known fields and identical unknown-field lists.
fn assert_dynamic_agree(pool: &Arc<DescriptorPool>, message: &str, data: &[u8], context: &str) {
    let desc = pool
        .get_message(message)
        .unwrap_or_else(|| panic!("missing descriptor {message}"));
    let inline = DynamicMessage::parse_with_pool(desc.clone(), Some(pool.clone()), data);
    let tabled = parse_dynamic_table_with(desc, Some(pool.clone()), data, 0, true);
    match (inline, tabled) {
        (Ok(inline), Ok(tabled)) => {
            let mut combined = tabled.msg.serialize().expect("table serialize");
            tabled.unknown.encode(&mut combined);
            assert_eq!(
                combined,
                inline.serialize().expect("inline serialize"),
                "bytes diverge for {context} as {message}"
            );
            assert_eq!(
                tabled.unknown,
                *inline.unknown_fields(),
                "unknowns diverge for {context} as {message}"
            );
        }
        (Err(_), Err(_)) => {}
        (inline, tabled) => panic!(
            "engines disagree for {context} as {message}: inline ok={} table ok={}",
            inline.is_ok(),
            tabled.is_ok()
        ),
    }
}

#[test]
fn correct_schema_fixtures_agree() {
    let pool = differential_pool();
    let cases = [
        ("presence_proto3_absent.bin", "differential.Proto3Presence"),
        ("presence_proto3_default.bin", "differential.Proto3Presence"),
        ("presence_proto3_set.bin", "differential.Proto3Presence"),
        ("presence_proto2_absent.bin", "differential.Proto2Presence"),
        (
            "presence_proto2_default_set.bin",
            "differential.Proto2Presence",
        ),
        ("presence_proto2_set.bin", "differential.Proto2Presence"),
        ("oneof_first.bin", "differential.OneofMessage"),
        ("oneof_second.bin", "differential.OneofMessage"),
        ("oneof_overwritten.bin", "differential.OneofMessage"),
        (
            "unknown_fields_all_types.bin",
            "differential.UnknownFieldsHost",
        ),
        ("enum_open_known.bin", "differential.OpenEnumMessage"),
        ("enum_open_unknown.bin", "differential.OpenEnumMessage"),
        ("enum_closed_known.bin", "differential.ClosedEnumMessage"),
        ("enum_closed_unknown.bin", "differential.ClosedEnumMessage"),
        ("map_single_entry.bin", "differential.MapMessage"),
        ("map_duplicate_keys.bin", "differential.MapMessage"),
        ("extension_empty.bin", "differential.Proto2ExtensionsHost"),
        ("extension_set.bin", "differential.Proto2ExtensionsHost"),
        ("packed_repeated.bin", "differential.PackedRepeated"),
        ("packed_repeated.bin", "differential.UnpackedRepeated"),
        ("unpacked_repeated.bin", "differential.PackedRepeated"),
        ("unpacked_repeated.bin", "differential.UnpackedRepeated"),
        ("truncated_varint.bin", "differential.UnknownFieldsHost"),
        (
            "truncated_length_delimited.bin",
            "differential.UnknownFieldsHost",
        ),
        ("recursion_depth_100.bin", "differential.RecursiveNode"),
        ("recursion_depth_101.bin", "differential.RecursiveNode"),
        ("merge_part1.bin", "differential.MergeTarget"),
        ("merge_part2.bin", "differential.MergeTarget"),
        ("merge_concatenated.bin", "differential.MergeTarget"),
    ];
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/differential");
    for (file, message) in cases {
        let bytes = std::fs::read(dir.join(file)).expect("fixture bytes");
        assert_dynamic_agree(&pool, message, &bytes, file);
    }
}

/// Collect every `.bin` payload under the given workspace-relative roots.
fn collect_payloads(roots: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir).expect("payload dir");
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "bin") {
                out.push(path);
            }
        }
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for root in roots {
        walk(&manifest.join(root), &mut out);
    }
    out.sort();
    out
}

/// Wide synthetic schema: mixed scalars plus map, packed, oneof, required,
/// nested, group, and closed-enum coverage in one message.
fn wide_desc() -> Arc<MessageDescriptor> {
    use pbrs::Presence as DynPresence;
    let nested = MessageDescriptor::builder("sweep.Nested")
        .field(FieldDescriptor::new(
            "id",
            1,
            FieldType::Int32,
            Cardinality::Optional,
            DynPresence::Implicit,
        ))
        .field(FieldDescriptor::new(
            "name",
            2,
            FieldType::String,
            Cardinality::Optional,
            DynPresence::Implicit,
        ))
        .build();
    let group = MessageDescriptor::builder("sweep.G")
        .field(FieldDescriptor::new(
            "id",
            1,
            FieldType::Int32,
            Cardinality::Optional,
            DynPresence::Implicit,
        ))
        .build();
    let entry = MessageDescriptor::builder("sweep.ScoresEntry")
        .field(FieldDescriptor::new(
            "key",
            1,
            FieldType::String,
            Cardinality::Optional,
            DynPresence::Implicit,
        ))
        .field(FieldDescriptor::new(
            "value",
            2,
            FieldType::Int32,
            Cardinality::Optional,
            DynPresence::Implicit,
        ))
        .map_entry(true)
        .build();
    let mut values = std::collections::BTreeMap::new();
    values.insert(0, "A".to_string());
    values.insert(1, "B".to_string());
    let closed = pbrs::EnumDescriptor {
        full_name: "sweep.E".to_string(),
        values,
        closed: true,
        ..Default::default()
    };
    let scalar_kinds = [
        FieldType::Int32,
        FieldType::Int64,
        FieldType::Uint32,
        FieldType::Uint64,
        FieldType::Sint32,
        FieldType::Sint64,
        FieldType::Fixed32,
        FieldType::Sfixed32,
        FieldType::Fixed64,
        FieldType::Sfixed64,
        FieldType::Float,
        FieldType::Double,
        FieldType::Bool,
        FieldType::String,
        FieldType::Bytes,
    ];
    let mut builder = MessageDescriptor::builder("sweep.Wide");
    for (index, kind) in scalar_kinds.iter().enumerate() {
        let number = index as u32 + 1;
        builder = builder.field(FieldDescriptor::new(
            format!("f{number}"),
            number,
            *kind,
            Cardinality::Optional,
            DynPresence::Implicit,
        ));
    }
    let mut nested_field = FieldDescriptor::new(
        "child",
        16,
        FieldType::Message,
        Cardinality::Optional,
        DynPresence::Explicit,
    );
    nested_field.message = Some(Arc::new(nested));
    builder = builder.field(nested_field);
    let mut map_field = FieldDescriptor::new(
        "scores",
        17,
        FieldType::Message,
        Cardinality::Repeated,
        DynPresence::Implicit,
    );
    map_field.is_map = true;
    map_field.message = Some(Arc::new(entry));
    builder = builder.field(map_field);
    builder = builder.field(FieldDescriptor::new(
        "nums",
        18,
        FieldType::Int32,
        Cardinality::Repeated,
        DynPresence::Implicit,
    ));
    let mut unpacked = FieldDescriptor::new(
        "plain",
        19,
        FieldType::Uint32,
        Cardinality::Repeated,
        DynPresence::Implicit,
    );
    unpacked.packed = false;
    builder = builder.field(unpacked);
    let mut oneof_a = FieldDescriptor::new(
        "oa",
        20,
        FieldType::Int32,
        Cardinality::Optional,
        DynPresence::Implicit,
    );
    oneof_a.oneof_index = Some(0);
    builder = builder.field(oneof_a);
    let mut oneof_b = FieldDescriptor::new(
        "ob",
        21,
        FieldType::String,
        Cardinality::Optional,
        DynPresence::Implicit,
    );
    oneof_b.oneof_index = Some(0);
    builder = builder.field(oneof_b);
    builder = builder.field(FieldDescriptor::new(
        "req",
        22,
        FieldType::Int32,
        Cardinality::Required,
        DynPresence::Explicit,
    ));
    let mut enum_field = FieldDescriptor::new(
        "state",
        23,
        FieldType::Enum,
        Cardinality::Optional,
        DynPresence::Implicit,
    );
    enum_field.enum_ty = Some(Arc::new(closed));
    builder = builder.field(enum_field);
    let mut group_field = FieldDescriptor::new(
        "g",
        24,
        FieldType::Group,
        Cardinality::Optional,
        DynPresence::Explicit,
    );
    group_field.message = Some(Arc::new(group));
    builder = builder.field(group_field);
    let mut desc = builder.build();
    desc.oneofs = vec![vec![20, 21]];
    Arc::new(desc)
}

#[test]
fn every_payload_sweep_agrees() {
    let diff = differential_pool();
    let conf = conformance_pool();
    let wide = wide_desc();
    let mut wide_pool = DescriptorPool::new();
    wide_pool.register_message((*wide).clone());
    let wide_pool = Arc::new(wide_pool);
    let schemas: &[(&Arc<DescriptorPool>, &str)] = &[
        (&diff, "differential.Proto3Presence"),
        (&conf, "protobuf_test_messages.proto3.TestAllTypesProto3"),
        (&conf, "protobuf_test_messages.proto2.TestAllTypesProto2"),
        (&wide_pool, "sweep.Wide"),
    ];
    let payloads = collect_payloads(&["tests/fixtures", "bench/corpora/payloads", "fuzz/corpus"]);
    assert!(
        payloads.len() >= 50,
        "expected the corpus sweep to find payloads, found {}",
        payloads.len()
    );
    for path in &payloads {
        let bytes = std::fs::read(path).expect("payload bytes");
        let context = path.to_string_lossy().into_owned();
        for (pool, message) in schemas {
            assert_dynamic_agree(pool, message, &bytes, &context);
        }
    }
}

#[test]
fn conformance_tables_are_sorted_and_searchable() {
    let conf = conformance_pool();
    for message in [
        "protobuf_test_messages.proto3.TestAllTypesProto3",
        "protobuf_test_messages.proto2.TestAllTypesProto2",
    ] {
        let desc = conf.get_message(message).expect("TAT descriptor");
        let table = build_table(&desc);
        assert!(table.len() > 50, "{message} table size");
        validate_table(&table).expect("sorted table");
        for field in desc.fields.keys() {
            assert_eq!(
                find_entry(&table, *field).map(|entry| entry.number),
                Some(*field),
                "find {field} in {message}"
            );
        }
        assert!(find_entry(&table, u32::MAX).is_none());
    }
    let diff = differential_pool();
    let desc = diff
        .get_message("differential.Proto3Presence")
        .expect("presence descriptor");
    validate_table(&build_table(&desc)).expect("sorted table");
}

// --- Typed differential -----------------------------------------------------

use pbrs::rt::{
    LazyBytes, LazyStr, PackedI32, WIRE_LEN, WIRE_VARINT, Wire, capture_unknown, decode_tag,
    decode_varint, encode_len_field, encode_tag, encode_varint, read_len_bytes, read_len_span,
};
use pbrs::{ParseError, UnknownFields};

#[derive(Debug, Default, PartialEq)]
struct TSub2 {
    id: i32,
    name: LazyStr,
    unknown: UnknownFields,
}

static T_SUB2_TABLE: &[FieldEntry] = &[
    FieldEntry {
        number: 1,
        expected_wire: WIRE_VARINT as u8,
        kind: FieldKind::Int32,
        storage: 1,
        presence: Presence::Implicit,
        flags: FieldFlags::NONE,
    },
    FieldEntry {
        number: 2,
        expected_wire: WIRE_LEN as u8,
        kind: FieldKind::String,
        storage: 2,
        presence: Presence::Implicit,
        flags: FieldFlags::UTF8_VALIDATE,
    },
];

impl TableMerge for TSub2 {
    fn table() -> &'static [FieldEntry] {
        T_SUB2_TABLE
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }

    fn set_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
        match (entry.storage, value) {
            (1, Scalar::I32(value)) => {
                self.id = value;
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn set_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
        match entry.storage {
            2 => {
                self.name = value;
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }
}

impl TSub2 {
    fn parse_table(data: &[u8]) -> Result<Self, ParseError> {
        let mut msg = Self::default();
        let mut slot = None;
        let mut pos = 0;
        merge_table(&mut msg, data, &mut slot, &mut pos, 0, false, None)?;
        Ok(msg)
    }

    fn parse_inline(data: &[u8]) -> Result<Self, ParseError> {
        let mut msg = Self::default();
        let mut slot: Option<Wire> = None;
        let mut pos = 0;
        while pos < data.len() {
            let (number, wire_type) = decode_tag(data, &mut pos)?;
            match number {
                1 => match wire_type {
                    WIRE_VARINT => msg.id = decode_varint(data, &mut pos)? as i32,
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                2 => match wire_type {
                    WIRE_LEN => {
                        let (start, end) = read_len_span(data, &mut pos)?;
                        msg.name = LazyStr::from_parse_span(&mut slot, data, start, end)?;
                    }
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                _ => msg
                    .unknown
                    .fields
                    .push(capture_unknown(data, &mut pos, number, wire_type)?),
            }
        }
        Ok(msg)
    }

    fn serialize_to(&self, out: &mut Vec<u8>) {
        if self.id != 0 {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, self.id as u64);
        }
        if !self.name.is_empty() {
            encode_len_field(out, 2, self.name.as_bytes());
        }
        self.unknown.encode(out);
    }

    fn serialized(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.serialize_to(&mut out);
        out
    }
}

#[derive(Debug, Default, PartialEq)]
struct TMsg2 {
    a: i32,
    s: LazyStr,
    b: LazyBytes,
    sub: Option<Box<TSub2>>,
    rep: PackedI32,
    unknown: UnknownFields,
}

static T_MSG2_TABLE: &[FieldEntry] = &[
    FieldEntry {
        number: 1,
        expected_wire: WIRE_VARINT as u8,
        kind: FieldKind::Int32,
        storage: 1,
        presence: Presence::Implicit,
        flags: FieldFlags::NONE,
    },
    FieldEntry {
        number: 2,
        expected_wire: WIRE_LEN as u8,
        kind: FieldKind::String,
        storage: 2,
        presence: Presence::Implicit,
        flags: FieldFlags::UTF8_VALIDATE,
    },
    FieldEntry {
        number: 3,
        expected_wire: WIRE_LEN as u8,
        kind: FieldKind::Bytes,
        storage: 3,
        presence: Presence::Implicit,
        flags: FieldFlags::NONE,
    },
    FieldEntry {
        number: 4,
        expected_wire: WIRE_LEN as u8,
        kind: FieldKind::Message,
        storage: 4,
        presence: Presence::Optional,
        flags: FieldFlags::NONE,
    },
    FieldEntry {
        number: 5,
        expected_wire: WIRE_LEN as u8,
        kind: FieldKind::Int32,
        storage: 5,
        presence: Presence::None,
        flags: FieldFlags::PACKED.union(FieldFlags::REPEATED),
    },
];

impl TableMerge for TMsg2 {
    fn table() -> &'static [FieldEntry] {
        T_MSG2_TABLE
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }

    fn set_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
        match (entry.storage, value) {
            (1, Scalar::I32(value)) => {
                self.a = value;
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn push_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
        match (entry.storage, value) {
            (5, Scalar::I32(value)) => {
                self.rep.push(value);
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn set_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
        match entry.storage {
            2 => {
                self.s = value;
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn set_bytes(&mut self, entry: &FieldEntry, value: LazyBytes) -> Result<(), ParseError> {
        match entry.storage {
            3 => {
                self.b = value;
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn append_packed(&mut self, entry: &FieldEntry, payload: Wire) -> Result<(), ParseError> {
        match entry.storage {
            5 => self.rep.append_wire(payload),
            _ => Err(ParseError::new("bad slot")),
        }
    }

    fn merge_message(
        &mut self,
        entry: &FieldEntry,
        data: &[u8],
        start: usize,
        end: usize,
        _wire: &mut Option<Wire>,
        depth: u32,
    ) -> Result<(), ParseError> {
        match entry.storage {
            4 => {
                let payload = &data[start..end];
                if let Some(existing) = self.sub.as_mut() {
                    let mut pos = 0;
                    let mut slot = None;
                    merge_table(
                        existing.as_mut(),
                        payload,
                        &mut slot,
                        &mut pos,
                        depth,
                        false,
                        None,
                    )?;
                } else {
                    let mut inner = TSub2::default();
                    let mut pos = 0;
                    let mut slot = None;
                    merge_table(&mut inner, payload, &mut slot, &mut pos, depth, false, None)?;
                    self.sub = Some(Box::new(inner));
                }
                Ok(())
            }
            _ => Err(ParseError::new("bad slot")),
        }
    }
}

impl TMsg2 {
    fn parse_table(data: &[u8]) -> Result<Self, ParseError> {
        let mut msg = Self::default();
        let mut slot = None;
        let mut pos = 0;
        merge_table(&mut msg, data, &mut slot, &mut pos, 0, false, None)?;
        Ok(msg)
    }

    fn parse_inline(data: &[u8]) -> Result<Self, ParseError> {
        let mut msg = Self::default();
        let mut slot: Option<Wire> = None;
        let mut pos = 0;
        while pos < data.len() {
            let (number, wire_type) = decode_tag(data, &mut pos)?;
            match number {
                1 => match wire_type {
                    WIRE_VARINT => msg.a = decode_varint(data, &mut pos)? as i32,
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                2 => match wire_type {
                    WIRE_LEN => {
                        let (start, end) = read_len_span(data, &mut pos)?;
                        msg.s = LazyStr::from_parse_span(&mut slot, data, start, end)?;
                    }
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                3 => match wire_type {
                    WIRE_LEN => {
                        let (start, end) = read_len_span(data, &mut pos)?;
                        msg.b = LazyBytes::from_parse_span(&mut slot, data, start, end);
                    }
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                4 => match wire_type {
                    WIRE_LEN => {
                        let payload = read_len_bytes(data, &mut pos)?;
                        if let Some(existing) = msg.sub.as_mut() {
                            let mut inner_pos = 0;
                            let mut inner_slot = None;
                            merge_table(
                                existing.as_mut(),
                                payload,
                                &mut inner_slot,
                                &mut inner_pos,
                                1,
                                false,
                                None,
                            )?;
                        } else {
                            let mut inner = TSub2::default();
                            let mut inner_pos = 0;
                            let mut inner_slot = None;
                            merge_table(
                                &mut inner,
                                payload,
                                &mut inner_slot,
                                &mut inner_pos,
                                1,
                                false,
                                None,
                            )?;
                            msg.sub = Some(Box::new(inner));
                        }
                    }
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                5 => match wire_type {
                    WIRE_LEN => {
                        let (start, end) = read_len_span(data, &mut pos)?;
                        msg.rep
                            .append_wire(Wire::ensure(&mut slot, data).window(start, end))?;
                    }
                    WIRE_VARINT => msg.rep.push(decode_varint(data, &mut pos)? as i32),
                    _ => msg
                        .unknown
                        .fields
                        .push(capture_unknown(data, &mut pos, number, wire_type)?),
                },
                _ => msg
                    .unknown
                    .fields
                    .push(capture_unknown(data, &mut pos, number, wire_type)?),
            }
        }
        Ok(msg)
    }

    fn serialize_to(&self, out: &mut Vec<u8>) {
        if self.a != 0 {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, self.a as u64);
        }
        if !self.s.is_empty() {
            encode_len_field(out, 2, self.s.as_bytes());
        }
        if !self.b.is_empty() {
            encode_len_field(out, 3, self.b.as_bytes());
        }
        if let Some(sub) = self.sub.as_deref() {
            encode_len_field(out, 4, &sub.serialized());
        }
        if !self.rep.is_empty() {
            let mut payload = Vec::new();
            for value in self.rep.iter() {
                encode_varint(&mut payload, *value as u64);
            }
            encode_len_field(out, 5, &payload);
        }
        self.unknown.encode(out);
    }

    fn serialized(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.serialize_to(&mut out);
        out
    }
}

#[test]
fn typed_table_matches_hand_inline_on_every_payload() {
    validate_table(T_MSG2_TABLE).expect("sorted table");
    validate_table(T_SUB2_TABLE).expect("sorted table");
    let payloads = collect_payloads(&["tests/fixtures", "bench/corpora/payloads", "fuzz/corpus"]);
    assert!(!payloads.is_empty());
    for path in &payloads {
        let bytes = std::fs::read(path).expect("payload bytes");
        let context = path.to_string_lossy().into_owned();
        let table = TMsg2::parse_table(&bytes);
        let inline = TMsg2::parse_inline(&bytes);
        assert_eq!(
            table.is_ok(),
            inline.is_ok(),
            "typed engines disagree on {context}"
        );
        if let (Ok(table), Ok(inline)) = (table, inline) {
            assert_eq!(table, inline, "typed message differs on {context}");
            assert_eq!(
                table.serialized(),
                inline.serialized(),
                "typed bytes differ on {context}"
            );
        }
        // The nested message through both of its own entry points.
        let table = TSub2::parse_table(&bytes);
        let inline = TSub2::parse_inline(&bytes);
        assert_eq!(
            table.is_ok(),
            inline.is_ok(),
            "nested engines disagree on {context}"
        );
        if let (Ok(table), Ok(inline)) = (table, inline) {
            assert_eq!(table.serialized(), inline.serialized());
        }
    }
    // Generated nested shapes, including packed forms and merge repeats.
    let mut nested = Vec::new();
    encode_tag(&mut nested, 1, WIRE_VARINT);
    encode_varint(&mut nested, 7);
    encode_len_field(&mut nested, 2, b"seven");
    let mut payload = Vec::new();
    encode_tag(&mut payload, 1, WIRE_VARINT);
    encode_varint(&mut payload, 3);
    encode_len_field(&mut payload, 4, &nested);
    let mut second = Vec::new();
    encode_tag(&mut second, 1, WIRE_VARINT);
    encode_varint(&mut second, 8);
    encode_len_field(&mut payload, 4, &second);
    let mut packed = Vec::new();
    encode_varint(&mut packed, 9);
    encode_len_field(&mut payload, 5, &packed);
    encode_tag(&mut payload, 5, WIRE_VARINT);
    encode_varint(&mut payload, 10);
    let table = TMsg2::parse_table(&payload).expect("table nested");
    let inline = TMsg2::parse_inline(&payload).expect("inline nested");
    assert_eq!(table, inline);
    assert_eq!(table.sub.as_deref().unwrap().id, 8);
    assert_eq!(table.sub.as_deref().unwrap().name.as_bytes(), b"seven");
    assert_eq!(table.rep.iter().collect::<Vec<_>>(), [&9, &10]);
    assert_eq!(table.serialized(), inline.serialized());
}
