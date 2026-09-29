//! Table-driven binary parse engine (PK-06).
//!
//! Implements the hybrid design recorded in
//! `docs/decisions/table-driven-parse.md`: small hot messages keep generated
//! inline `match` parsing, while wide/cold messages share this generic loop
//! driven by one static [`FieldEntry`] table per message.
//!
//! This module is safe-only Rust. Field storage is addressed through the
//! [`TableMerge`] trait (slot identifiers, never raw offsets), so no `unsafe`
//! block, `SAFETY` comment, or `docs/unsafe-invariants.md` entry is required.
//! Raw-offset dispatch remains a possible follow-up behind its own card.
//!
//! Two drivers share the loop core:
//!
//! * [`TableMerge`] + [`merge_table`] for typed (generated-style) messages
//!   with lazy string/bytes, packed, and lazy-message storage. Codegen does
//!   not emit table users yet (PK-07); the unit tests below drive
//!   representative typed messages through the loop.
//! * `parse_dynamic_table` (with the `reflect` feature) for dynamic messages,
//!   used by the `tests/table_parse.rs` differential against inline parsing.
//!
//! Semantics mirror generated `merge_inner` (typed path) and dynamic
//! `merge_bytes` (dynamic path): last-wins singular assignment, oneof
//! replacement, proto2 required checks, per-edition UTF-8 rules,
//! unknown-enum retention for closed enums, packed/unpacked compatibility,
//! groups (including truncation/mismatch errors), recursion limits, and
//! unknown-field preservation through the shared `capture_unknown` helper.
//!
//! One intentional typed/dynamic difference is inherited from the inline
//! engines: generated code keeps unknown values of *repeated* closed enums
//! in the field, while dynamic parsing moves them to unknown fields. Each
//! table path mirrors its own inline counterpart.

use crate::error::ParseError;
use crate::lazy::{LazyBytes, LazyStr, Wire};
use crate::wire::{
    UnknownField, UnknownFields, WIRE_EGROUP, WIRE_I32, WIRE_I64, WIRE_LEN, WIRE_SGROUP,
    WIRE_VARINT, capture_unknown, decode_tag, decode_varint, decode_zigzag32, decode_zigzag64,
    read_fixed32, read_fixed64, read_len_bytes, read_len_span, validate_varints,
};

/// Maximum nesting depth for table-driven parse.
///
/// Mirrors the dynamic parser limit; the `const` assertion below keeps the
/// two in sync wherever `reflect` is enabled.
pub const RECURSION_LIMIT: u32 = 100;

#[cfg(feature = "reflect")]
const _: () = assert!(RECURSION_LIMIT == crate::dynamic::RECURSION_LIMIT);

/// Protobuf scalar/message kind of one table entry.
///
/// Variants parallel descriptor field types so [`build_table`] can map them
/// 1:1; `Group` covers group-encoded fields (`delimited`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Double,
    Float,
    Int64,
    Uint64,
    Int32,
    Fixed64,
    Fixed32,
    Bool,
    String,
    Group,
    Message,
    Bytes,
    Uint32,
    Enum,
    Sfixed32,
    Sfixed64,
    Sint32,
    Sint64,
}

impl FieldKind {
    /// Canonical wire type for this kind (groups use start-group).
    #[must_use]
    pub const fn native_wire(self) -> u8 {
        match self {
            Self::Fixed64 | Self::Sfixed64 | Self::Double => WIRE_I64 as u8,
            Self::Fixed32 | Self::Sfixed32 | Self::Float => WIRE_I32 as u8,
            Self::String | Self::Bytes | Self::Message => WIRE_LEN as u8,
            Self::Group => WIRE_SGROUP as u8,
            _ => WIRE_VARINT as u8,
        }
    }

    /// Whether this kind accepts packed wire form when repeated.
    #[must_use]
    pub const fn is_packable(self) -> bool {
        !matches!(
            self,
            Self::String | Self::Bytes | Self::Message | Self::Group
        )
    }

    /// Fixed-width kinds whose packed payload keeps a payload-only copy
    /// (`Wire::from_slice`) instead of windowing the parent frame, mirroring
    /// generated `is_memcpy_packed`.
    #[must_use]
    pub const fn is_memcpy(self) -> bool {
        matches!(
            self,
            Self::Fixed32
                | Self::Fixed64
                | Self::Sfixed32
                | Self::Sfixed64
                | Self::Float
                | Self::Double
        )
    }
}

/// Presence tracking of one table entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Proto3 implicit presence: defaults are omitted on the wire.
    Implicit,
    /// Explicit presence (`optional`, proto2 singular): set bits are kept.
    Optional,
    /// Member of a oneof: setting this entry clears its sibling arms.
    Oneof,
    /// Proto2 required: missing values fail when enforced.
    Required,
    /// No presence tracking (repeated and map entries).
    None,
}

/// Per-field option bits. `const`-constructible so static tables can embed
/// them without runtime initialization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldFlags(u16);

impl FieldFlags {
    pub const NONE: Self = Self(0);
    /// Validate string payloads as UTF-8 (`utf8_validation = VERIFY`).
    pub const UTF8_VALIDATE: Self = Self(1 << 0);
    /// Repeated packable field with `packed = true` storage.
    pub const PACKED: Self = Self(1 << 1);
    /// Repeated field (packed storage or item pushes).
    pub const REPEATED: Self = Self(1 << 2);
    /// Map field (length-delimited entry messages, last-wins keys).
    pub const MAP: Self = Self(1 << 3);
    /// Singular message eligible for lazy parse-then-window storage.
    pub const LAZY: Self = Self(1 << 4);
    /// Closed enum: unknown values move to unknown fields.
    pub const CLOSED_ENUM: Self = Self(1 << 5);

    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One row of a message field table, sorted by `number`.
///
/// `storage` is a message-local slot identifier interpreted by the
/// [`TableMerge`] implementation (generated code maps it to a field); the
/// dynamic driver reuses the field number as the slot. The layout is
/// append-only per PK-05: later size/encode work adds variants and flags
/// without reordering these fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldEntry {
    pub number: u32,
    pub expected_wire: u8,
    pub kind: FieldKind,
    pub storage: u32,
    pub presence: Presence,
    pub flags: FieldFlags,
}

impl FieldEntry {
    #[must_use]
    pub const fn is_repeated(self) -> bool {
        self.flags.0 & FieldFlags::REPEATED.0 != 0
    }

    #[must_use]
    pub const fn is_map(self) -> bool {
        self.flags.0 & FieldFlags::MAP.0 != 0
    }

    #[must_use]
    pub const fn is_packable_repeated(self) -> bool {
        self.is_repeated() && self.kind.is_packable()
    }
}

/// Binary search for `number` in a table sorted by field number.
///
/// Returns `None` for unknown fields; callers preserve those through
/// `capture_unknown`, exactly like the inline `_ =>` match arms.
#[must_use]
pub fn find_entry(table: &[FieldEntry], number: u32) -> Option<&FieldEntry> {
    table
        .binary_search_by_key(&number, |entry| entry.number)
        .ok()
        .map(|index| &table[index])
}

/// Check the table invariants `find_entry` relies on: strictly increasing
/// field numbers. Codegen-time / test-time validation, not per-parse work.
pub fn validate_table(table: &[FieldEntry]) -> Result<(), ParseError> {
    let mut prev = 0u32;
    let mut first = true;
    for entry in table {
        if entry.number == 0 || entry.number > crate::wire::MAX_FIELD_NUMBER {
            return Err(ParseError::new("illegal field number"));
        }
        if !first && entry.number <= prev {
            return Err(ParseError::new("field table not sorted"));
        }
        first = false;
        prev = entry.number;
    }
    Ok(())
}

/// Decoded scalar value handed from the shared loop to typed storage.
///
/// Conversions (zigzag, float bits, integer casts) happen once here so every
/// table user shares them; storage assignment stays message-specific.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scalar {
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    S32(i32),
    S64(i64),
    F32(f32),
    F64(f64),
    Enum(i32),
}

/// Decode one scalar of `kind` from `buf` at `pos`.
///
/// Mirrors the codegen `read_scalar_expr` mapping 1:1, including `as` casts
/// for over-wide varints and `!= 0` for bools.
pub fn decode_scalar(kind: FieldKind, buf: &[u8], pos: &mut usize) -> Result<Scalar, ParseError> {
    match kind {
        FieldKind::Bool => Ok(Scalar::Bool(decode_varint(buf, pos)? != 0)),
        FieldKind::Int32 => Ok(Scalar::I32(decode_varint(buf, pos)? as i32)),
        FieldKind::Int64 => Ok(Scalar::I64(decode_varint(buf, pos)? as i64)),
        FieldKind::Uint32 => Ok(Scalar::U32(decode_varint(buf, pos)? as u32)),
        FieldKind::Uint64 => Ok(Scalar::U64(decode_varint(buf, pos)?)),
        FieldKind::Sint32 => Ok(Scalar::S32(decode_zigzag32(decode_varint(buf, pos)?))),
        FieldKind::Sint64 => Ok(Scalar::S64(decode_zigzag64(decode_varint(buf, pos)?))),
        FieldKind::Enum => Ok(Scalar::Enum(decode_varint(buf, pos)? as i32)),
        FieldKind::Fixed32 => Ok(Scalar::U32(read_fixed32(buf, pos)?)),
        FieldKind::Sfixed32 => Ok(Scalar::I32(read_fixed32(buf, pos)? as i32)),
        FieldKind::Float => Ok(Scalar::F32(f32::from_bits(read_fixed32(buf, pos)?))),
        FieldKind::Fixed64 => Ok(Scalar::U64(read_fixed64(buf, pos)?)),
        FieldKind::Sfixed64 => Ok(Scalar::I64(read_fixed64(buf, pos)? as i64)),
        FieldKind::Double => Ok(Scalar::F64(f64::from_bits(read_fixed64(buf, pos)?))),
        FieldKind::String | FieldKind::Bytes | FieldKind::Message | FieldKind::Group => {
            Err(ParseError::new("scalar decode of length-delimited field"))
        }
    }
}

/// Validate a packed payload without materializing values.
///
/// Mirrors `PackedCodec::validate`: varint kinds share `validate_varints`,
/// fixed kinds require a whole number of elements.
pub fn validate_packed_bytes(kind: FieldKind, bytes: &[u8]) -> Result<(), ParseError> {
    match kind {
        FieldKind::Bool
        | FieldKind::Int32
        | FieldKind::Int64
        | FieldKind::Uint32
        | FieldKind::Uint64
        | FieldKind::Sint32
        | FieldKind::Sint64
        | FieldKind::Enum => validate_varints(bytes),
        FieldKind::Fixed32 | FieldKind::Sfixed32 | FieldKind::Float => {
            if bytes.len() % 4 != 0 {
                return Err(ParseError::new("truncated packed fixed"));
            }
            Ok(())
        }
        FieldKind::Fixed64 | FieldKind::Sfixed64 | FieldKind::Double => {
            if bytes.len() % 8 != 0 {
                return Err(ParseError::new("truncated packed fixed"));
            }
            Ok(())
        }
        FieldKind::String | FieldKind::Bytes | FieldKind::Message | FieldKind::Group => {
            Err(ParseError::new("packed wire for non-packable field"))
        }
    }
}

/// Parse one string span with generated-code semantics.
///
/// `utf8_validate` selects `LazyStr::from_parse_span` (proto3 `VERIFY`) or
/// `from_parse_span_unchecked` (proto2/edition `NONE`), including the inline
/// thresholds that skip the parent `Wire` for small or sparse fields.
pub fn parse_table_string(
    slot: &mut Option<Wire>,
    data: &[u8],
    start: usize,
    end: usize,
    utf8_validate: bool,
) -> Result<LazyStr, ParseError> {
    if start > end || end > data.len() {
        return Err(ParseError::new("string span out of bounds"));
    }
    if utf8_validate {
        LazyStr::from_parse_span(slot, data, start, end)
    } else {
        Ok(LazyStr::from_parse_span_unchecked(slot, data, start, end))
    }
}

/// Parse one bytes span with generated-code semantics.
///
/// Shares the `LazyBytes::from_parse_span` threshold policy: small fields in
/// a shared frame copy instead of pinning it.
pub fn parse_table_bytes(
    slot: &mut Option<Wire>,
    data: &[u8],
    start: usize,
    end: usize,
) -> Result<LazyBytes, ParseError> {
    if start > end || end > data.len() {
        return Err(ParseError::new("bytes span out of bounds"));
    }
    Ok(LazyBytes::from_parse_span(slot, data, start, end))
}

/// Callbacks a table-driven typed message implements.
///
/// The shared [`merge_table`] loop owns framing (tag decode, table lookup,
/// wire checks, unknown capture, span extraction, packed framing, depth and
/// group limits); the message owns storage assignment per
/// [`FieldEntry::storage`] slot plus oneof/required/enum bookkeeping it alone
/// knows. PK-07 codegen will emit these impls; until then the unit tests
/// below implement them by hand for representative messages.
///
/// Every value handler defaults to an error so messages only implement the
/// kinds their tables name; the loop never calls an unimplemented handler
/// for a well-formed table.
pub trait TableMerge {
    /// Static field table, sorted by field number (see [`validate_table`]).
    fn table() -> &'static [FieldEntry];

    /// MessageSet wire format: field 1 carries type-id/payload items.
    const MESSAGE_SET: bool = false;

    /// Unknown-field sink for unlisted fields and wire mismatches.
    fn unknown_mut(&mut self) -> &mut UnknownFields;

    /// Mark cached sizes dirty. Generated messages dirty `cached_size`;
    /// messages without a cache use the default no-op.
    fn dirty(&mut self) {}

    /// Clear oneof arms conflicting with `entry` before it is set. The loop
    /// calls this only for singular (non-repeated, non-map) entries, matching
    /// generated `emit_oneof_clear` placement.
    fn clear_oneof(&mut self, entry: &FieldEntry) {
        let _ = entry;
    }

    /// Proto2 required check at the end of an enforced parse.
    fn check_required(&self) -> Result<(), ParseError> {
        Ok(())
    }

    /// Whether `value` is a known number for the closed enum behind `entry`.
    /// Consulted only when the entry carries [`FieldFlags::CLOSED_ENUM`].
    fn classify_enum(&self, entry: &FieldEntry, value: i32) -> bool {
        let _ = entry;
        let _ = value;
        true
    }

    /// Assign one decoded scalar to a singular field.
    fn set_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no scalar handler for slot"))
    }

    /// Append one decoded scalar to a repeated field.
    fn push_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no scalar handler for slot"))
    }

    /// Assign one parsed string to a singular field.
    fn set_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no string handler for slot"))
    }

    /// Append one parsed string to a repeated field.
    fn push_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no string handler for slot"))
    }

    /// Assign parsed bytes to a singular field.
    fn set_bytes(&mut self, entry: &FieldEntry, value: LazyBytes) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no bytes handler for slot"))
    }

    /// Append parsed bytes to a repeated field.
    fn push_bytes(&mut self, entry: &FieldEntry, value: LazyBytes) -> Result<(), ParseError> {
        let _ = entry;
        let _ = value;
        Err(ParseError::new("table: no bytes handler for slot"))
    }

    /// Append one validated packed payload. The loop windows the parent frame
    /// (or copies payload-only storage for fixed kinds) before calling.
    fn append_packed(&mut self, entry: &FieldEntry, payload: Wire) -> Result<(), ParseError> {
        let _ = entry;
        let _ = payload;
        Err(ParseError::new("table: no packed handler for slot"))
    }

    /// Merge one length-delimited submessage into a singular field.
    ///
    /// `data[start..end]` is the submessage payload. Eager fields parse it
    /// into the existing value or a fresh one; lazy fields validate it and
    /// store `wire.window(start, end)` over the parent frame, exactly like
    /// the generated `merge_inner` arms. `wire` is the parent frame slot.
    fn merge_message(
        &mut self,
        entry: &FieldEntry,
        data: &[u8],
        start: usize,
        end: usize,
        wire: &mut Option<Wire>,
        depth: u32,
    ) -> Result<(), ParseError> {
        let _ = entry;
        let _ = data;
        let _ = start;
        let _ = end;
        let _ = wire;
        let _ = depth;
        Err(ParseError::new("table: no message handler for slot"))
    }

    /// Parse one length-delimited submessage into a fresh repeated element.
    fn push_message(
        &mut self,
        entry: &FieldEntry,
        payload: &[u8],
        depth: u32,
    ) -> Result<(), ParseError> {
        let _ = entry;
        let _ = payload;
        let _ = depth;
        Err(ParseError::new("table: no message handler for slot"))
    }

    /// Merge one group body into a singular field. Implementations recurse
    /// with [`merge_table`] and `until` set to the group number.
    fn merge_group(
        &mut self,
        entry: &FieldEntry,
        data: &[u8],
        wire: &mut Option<Wire>,
        pos: &mut usize,
        depth: u32,
    ) -> Result<(), ParseError> {
        let _ = entry;
        let _ = data;
        let _ = wire;
        let _ = pos;
        let _ = depth;
        Err(ParseError::new("table: no group handler for slot"))
    }

    /// Parse one group body into a fresh repeated element.
    fn push_group(
        &mut self,
        entry: &FieldEntry,
        data: &[u8],
        wire: &mut Option<Wire>,
        pos: &mut usize,
        depth: u32,
    ) -> Result<(), ParseError> {
        let _ = entry;
        let _ = data;
        let _ = wire;
        let _ = pos;
        let _ = depth;
        Err(ParseError::new("table: no group handler for slot"))
    }

    /// Decode one map entry window and insert it (last-wins on keys).
    fn merge_map(&mut self, entry: &FieldEntry, wire: &Wire, depth: u32) -> Result<(), ParseError> {
        let _ = entry;
        let _ = wire;
        let _ = depth;
        Err(ParseError::new("table: no map handler for slot"))
    }

    /// Merge one MessageSet item (field 1). The default preserves the item
    /// as unknown; MessageSet messages override this with type-id dispatch.
    fn merge_message_set(
        &mut self,
        data: &[u8],
        wire: &mut Option<Wire>,
        pos: &mut usize,
        wire_type: u32,
        depth: u32,
    ) -> Result<(), ParseError> {
        let _ = wire;
        let _ = depth;
        let field = capture_unknown(data, pos, 1, wire_type)?;
        self.unknown_mut().fields.push(field);
        Ok(())
    }
}

/// Generic table-driven merge into a typed message.
///
/// Parameters mirror generated `merge_inner`: `data`/`pos` frame the input,
/// `wire` is the parent lazy-backing slot, `depth` counts nesting, `enforce`
/// selects proto2 required checks, and `until` terminates a group body on its
/// end-group tag. Dispatch order matches generated code: tag decode, group
/// terminator, MessageSet item, table lookup, wire check, per-kind handler.
pub fn merge_table<M: TableMerge>(
    msg: &mut M,
    data: &[u8],
    wire: &mut Option<Wire>,
    pos: &mut usize,
    depth: u32,
    enforce: bool,
    until: Option<u32>,
) -> Result<(), ParseError> {
    if depth > RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    msg.dirty();
    let table = M::table();
    while *pos < data.len() {
        let (number, wire_type) = decode_tag(data, pos)?;
        if let Some(group) = until {
            if wire_type == WIRE_EGROUP {
                if number != group {
                    return Err(ParseError::new("mismatched end-group"));
                }
                return Ok(());
            }
        }
        if M::MESSAGE_SET && number == 1 {
            msg.merge_message_set(data, wire, pos, wire_type, depth)?;
            continue;
        }
        let Some(entry) = find_entry(table, number) else {
            msg.unknown_mut()
                .fields
                .push(capture_unknown(data, pos, number, wire_type)?);
            continue;
        };
        // Repeated packable fields accept both packed (LEN) and unpacked
        // (native) wire forms regardless of the PACKED flag, matching
        // generated code and the wire-format spec.
        let packed_wire = entry.is_packable_repeated() && wire_type == WIRE_LEN;
        let unpacked_wire =
            entry.is_packable_repeated() && wire_type == u32::from(entry.kind.native_wire());
        let map_wire = entry.is_map() && wire_type == WIRE_LEN;
        if u32::from(entry.expected_wire) != wire_type
            && !packed_wire
            && !unpacked_wire
            && !map_wire
        {
            msg.unknown_mut()
                .fields
                .push(capture_unknown(data, pos, number, wire_type)?);
            continue;
        }
        dispatch_typed(msg, entry, data, wire, pos, wire_type, depth)?;
    }
    if until.is_some() {
        return Err(ParseError::new("truncated group"));
    }
    if enforce {
        msg.check_required()?;
    }
    Ok(())
}

/// Per-kind handler dispatch for one table entry with accepted wire type.
#[allow(clippy::too_many_arguments, reason = "mirrors merge_table framing")]
fn dispatch_typed<M: TableMerge>(
    msg: &mut M,
    entry: &FieldEntry,
    data: &[u8],
    wire: &mut Option<Wire>,
    pos: &mut usize,
    wire_type: u32,
    depth: u32,
) -> Result<(), ParseError> {
    if entry.is_map() {
        let (start, end) = read_len_span(data, pos)?;
        let window = Wire::ensure(wire, data).window(start, end);
        return msg.merge_map(entry, &window, depth + 1);
    }
    let repeated = entry.is_repeated();
    if entry.is_packable_repeated() && wire_type == WIRE_LEN {
        return dispatch_packed(msg, entry, data, wire, pos);
    }
    match entry.kind {
        FieldKind::String => {
            let (start, end) = read_len_span(data, pos)?;
            let utf8 = entry.flags.contains(FieldFlags::UTF8_VALIDATE);
            let value = parse_table_string(wire, data, start, end, utf8)?;
            if repeated {
                msg.push_string(entry, value)
            } else {
                msg.clear_oneof(entry);
                msg.set_string(entry, value)
            }
        }
        FieldKind::Bytes => {
            let (start, end) = read_len_span(data, pos)?;
            let value = parse_table_bytes(wire, data, start, end)?;
            if repeated {
                msg.push_bytes(entry, value)
            } else {
                msg.clear_oneof(entry);
                msg.set_bytes(entry, value)
            }
        }
        FieldKind::Message => {
            let (start, end) = read_len_span(data, pos)?;
            if repeated {
                msg.push_message(entry, &data[start..end], depth + 1)
            } else {
                msg.clear_oneof(entry);
                msg.merge_message(entry, data, start, end, wire, depth + 1)
            }
        }
        FieldKind::Group => {
            if repeated {
                msg.push_group(entry, data, wire, pos, depth + 1)
            } else {
                msg.clear_oneof(entry);
                msg.merge_group(entry, data, wire, pos, depth + 1)
            }
        }
        FieldKind::Enum if !repeated => {
            let value = decode_varint(data, pos)? as i32;
            if entry.flags.contains(FieldFlags::CLOSED_ENUM) && !msg.classify_enum(entry, value) {
                msg.unknown_mut().fields.push(UnknownField::Varint {
                    number: entry.number,
                    value: value as u64,
                });
                Ok(())
            } else {
                msg.clear_oneof(entry);
                msg.set_scalar(entry, Scalar::Enum(value))
            }
        }
        kind => {
            let value = decode_scalar(kind, data, pos)?;
            if repeated {
                msg.push_scalar(entry, value)
            } else {
                msg.clear_oneof(entry);
                msg.set_scalar(entry, value)
            }
        }
    }
}

/// Packed-wire payload for a repeated packable field.
///
/// `packed = true` fields validate and append the window (payload-only
/// storage for fixed kinds); `packed = false` fields decode items inline.
/// Both directions stay accepted either way, matching generated code.
fn dispatch_packed<M: TableMerge>(
    msg: &mut M,
    entry: &FieldEntry,
    data: &[u8],
    wire: &mut Option<Wire>,
    pos: &mut usize,
) -> Result<(), ParseError> {
    if entry.flags.contains(FieldFlags::PACKED) {
        let (start, end) = read_len_span(data, pos)?;
        validate_packed_bytes(entry.kind, &data[start..end])?;
        let payload = if entry.kind.is_memcpy() {
            Wire::from_slice(&data[start..end])
        } else {
            Wire::ensure(wire, data).window(start, end)
        };
        msg.append_packed(entry, payload)
    } else {
        let payload = read_len_bytes(data, pos)?;
        let mut item = 0;
        while item < payload.len() {
            let value = decode_scalar(entry.kind, payload, &mut item)?;
            msg.push_scalar(entry, value)?;
        }
        Ok(())
    }
}

// --- Dynamic driver ---------------------------------------------------------

#[cfg(feature = "reflect")]
use crate::dynamic::{
    Cardinality, DescriptorPool, DynamicMessage, FieldDescriptor, FieldType, MapKeyValue,
    MessageDescriptor, Value,
};
#[cfg(feature = "reflect")]
use crate::message::{MergeFrom, Serialize};
#[cfg(feature = "reflect")]
use crate::string::{ProtoBytes, ProtoString};
#[cfg(feature = "reflect")]
use crate::wire::{decode_tag as dyn_decode_tag, skip_field as dyn_skip_field};
#[cfg(feature = "reflect")]
use std::sync::Arc;

/// Build the sorted field table for a message descriptor.
///
/// The table drives dispatch (binary search instead of the descriptor
/// `BTreeMap` lookup); nested-type metadata still comes from the descriptor
/// itself, which the driver re-consults by field number after dispatch.
#[cfg(feature = "reflect")]
pub fn build_table(desc: &MessageDescriptor) -> Vec<FieldEntry> {
    desc.fields
        .values()
        .map(|field| {
            let kind = table_kind(field);
            let mut flags = FieldFlags::NONE;
            if field.utf8_validate {
                flags = flags.union(FieldFlags::UTF8_VALIDATE);
            }
            if field.packed {
                flags = flags.union(FieldFlags::PACKED);
            }
            if field.cardinality == Cardinality::Repeated {
                flags = flags.union(FieldFlags::REPEATED);
            }
            if field.is_map {
                flags = flags.union(FieldFlags::MAP);
            }
            if field.cardinality != Cardinality::Repeated
                && !field.is_map
                && field.field_type == FieldType::Message
                && !field.delimited
            {
                flags = flags.union(FieldFlags::LAZY);
            }
            if field.field_type == FieldType::Enum
                && field.enum_ty.as_ref().is_some_and(|ty| ty.closed)
            {
                flags = flags.union(FieldFlags::CLOSED_ENUM);
            }
            let presence = if field.is_map || field.cardinality == Cardinality::Repeated {
                Presence::None
            } else if field.cardinality == Cardinality::Required {
                Presence::Required
            } else if field.oneof_index.is_some() {
                Presence::Oneof
            } else if field.presence == crate::dynamic::Presence::Explicit {
                Presence::Optional
            } else {
                Presence::Implicit
            };
            FieldEntry {
                number: field.number,
                expected_wire: if field.is_map {
                    WIRE_LEN as u8
                } else {
                    kind.native_wire()
                },
                kind,
                storage: field.number,
                presence,
                flags,
            }
        })
        .collect()
}

/// Map a descriptor field to its table kind (group encoding wins).
#[cfg(feature = "reflect")]
fn table_kind(field: &FieldDescriptor) -> FieldKind {
    if field.delimited || field.field_type == FieldType::Group {
        return FieldKind::Group;
    }
    match field.field_type {
        FieldType::Double => FieldKind::Double,
        FieldType::Float => FieldKind::Float,
        FieldType::Int64 => FieldKind::Int64,
        FieldType::Uint64 => FieldKind::Uint64,
        FieldType::Int32 => FieldKind::Int32,
        FieldType::Fixed64 => FieldKind::Fixed64,
        FieldType::Fixed32 => FieldKind::Fixed32,
        FieldType::Bool => FieldKind::Bool,
        FieldType::String => FieldKind::String,
        FieldType::Group => FieldKind::Group,
        FieldType::Message => FieldKind::Message,
        FieldType::Bytes => FieldKind::Bytes,
        FieldType::Uint32 => FieldKind::Uint32,
        FieldType::Enum => FieldKind::Enum,
        FieldType::Sfixed32 => FieldKind::Sfixed32,
        FieldType::Sfixed64 => FieldKind::Sfixed64,
        FieldType::Sint32 => FieldKind::Sint32,
        FieldType::Sint64 => FieldKind::Sint64,
    }
}

/// Dynamic message parsed through the table loop.
///
/// Unknown storage lives in `dynamic.rs`, which PK-06 must not modify, so
/// top-level unknowns are returned alongside the message instead of inside
/// it: `msg.serialize()` bytes followed by `unknown.encode()` bytes equal the
/// inline parse serialization byte-for-byte. Nested values are fused through
/// a serialize/reparse round trip (see [`parse_dynamic_table`]).
#[cfg(feature = "reflect")]
pub struct TabledDynamic {
    pub msg: DynamicMessage,
    pub unknown: UnknownFields,
}

/// Parse `data` for `desc` through the table loop (depth 0, enforced).
#[cfg(feature = "reflect")]
pub fn parse_dynamic_table(
    desc: Arc<MessageDescriptor>,
    pool: Option<Arc<DescriptorPool>>,
    data: &[u8],
) -> Result<TabledDynamic, ParseError> {
    parse_dynamic_table_with(desc, pool, data, 0, true)
}

/// Parse `data` for `desc` through the table loop with explicit depth and
/// required enforcement, mirroring `parse_with_pool_depth`.
#[cfg(feature = "reflect")]
pub fn parse_dynamic_table_with(
    desc: Arc<MessageDescriptor>,
    pool: Option<Arc<DescriptorPool>>,
    data: &[u8],
    depth: u32,
    enforce_required: bool,
) -> Result<TabledDynamic, ParseError> {
    let table = build_table(&desc);
    let mut msg = DynamicMessage::new(desc.clone());
    if let Some(pool) = &pool {
        msg.set_pool(pool.clone());
    }
    let mut unknown = UnknownFields::default();
    merge_dynamic_table(
        &desc,
        pool.as_ref(),
        &table,
        &mut msg,
        &mut unknown,
        data,
        depth,
        enforce_required,
        None,
    )?;
    Ok(TabledDynamic { msg, unknown })
}

/// Table-driven merge into a dynamic message; mirrors `merge_bytes` with
/// `until` added for group bodies.
#[cfg(feature = "reflect")]
#[allow(clippy::too_many_arguments, reason = "mirrors merge_table framing")]
fn merge_dynamic_table(
    desc: &MessageDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
    table: &[FieldEntry],
    msg: &mut DynamicMessage,
    unknown: &mut UnknownFields,
    data: &[u8],
    depth: u32,
    enforce: bool,
    until: Option<u32>,
) -> Result<(), ParseError> {
    if depth > RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    let mut pos = 0;
    while pos < data.len() {
        let (number, wire_type) = dyn_decode_tag(data, &mut pos)?;
        if let Some(group) = until {
            if wire_type == WIRE_EGROUP {
                if number != group {
                    return Err(ParseError::new("mismatched end-group"));
                }
                return Ok(());
            }
        }
        if desc.message_set_wire_format && number == 1 {
            merge_dynamic_message_set(desc, pool, msg, unknown, data, &mut pos, wire_type, depth)?;
            continue;
        }
        let Some(entry) = find_entry(table, number) else {
            unknown
                .fields
                .push(capture_unknown(data, &mut pos, number, wire_type)?);
            continue;
        };
        let Some(field) = desc.field(number).cloned() else {
            return Err(ParseError::new("table descriptor mismatch"));
        };
        merge_dynamic_field(
            &field, entry, pool, msg, unknown, data, &mut pos, wire_type, depth,
        )?;
    }
    if until.is_some() {
        return Err(ParseError::new("truncated group"));
    }
    if enforce {
        for field in desc.fields.values() {
            if field.cardinality == Cardinality::Required && !msg.has(field.number) {
                return Err(ParseError::new("missing required field"));
            }
        }
    }
    Ok(())
}

/// One accepted field occurrence; mirrors `merge_field` including packed
/// acceptance independent of the `packed` flag.
#[cfg(feature = "reflect")]
#[allow(clippy::too_many_arguments, reason = "mirrors merge_field shape")]
fn merge_dynamic_field(
    field: &FieldDescriptor,
    entry: &FieldEntry,
    pool: Option<&Arc<DescriptorPool>>,
    msg: &mut DynamicMessage,
    unknown: &mut UnknownFields,
    data: &[u8],
    pos: &mut usize,
    wire_type: u32,
    depth: u32,
) -> Result<(), ParseError> {
    let expected = u32::from(entry.expected_wire);
    let packed_ok = entry.is_packable_repeated() && wire_type == WIRE_LEN;
    let map_wire = entry.is_map() && wire_type == WIRE_LEN;
    if wire_type != expected && !packed_ok && !map_wire {
        unknown
            .fields
            .push(capture_unknown(data, pos, field.number, wire_type)?);
        return Ok(());
    }
    if field.is_map {
        let payload = read_len_bytes(data, pos)?;
        let entry_desc = table_field_message_desc(field, pool)?;
        let (key, value) = decode_dynamic_map_entry(&entry_desc, payload, pool, depth)?;
        msg.insert_map(field.number, key, value);
        return Ok(());
    }
    if field.cardinality == Cardinality::Repeated {
        if packed_ok {
            let payload = read_len_bytes(data, pos)?;
            let mut item = 0;
            let leaf_wire = u32::from(entry.kind.native_wire());
            while item < payload.len() {
                let value = decode_dynamic_leaf(field, payload, &mut item, leaf_wire, pool, depth)?;
                if let Value::Enum(number) = &value {
                    if is_dynamic_closed_unknown(field, *number) {
                        unknown.fields.push(UnknownField::Varint {
                            number: field.number,
                            value: *number as u64,
                        });
                        continue;
                    }
                }
                msg.push(field.number, value);
            }
            return Ok(());
        }
        let value = decode_dynamic_leaf(field, data, pos, wire_type, pool, depth)?;
        if let Value::Enum(number) = &value {
            if is_dynamic_closed_unknown(field, *number) {
                unknown.fields.push(UnknownField::Varint {
                    number: field.number,
                    value: *number as u64,
                });
                return Ok(());
            }
        }
        msg.push(field.number, value);
        return Ok(());
    }
    let value = decode_dynamic_leaf(field, data, pos, wire_type, pool, depth)?;
    if let Value::Enum(number) = &value {
        if is_dynamic_closed_unknown(field, *number) {
            unknown.fields.push(UnknownField::Varint {
                number: field.number,
                value: *number as u64,
            });
            return Ok(());
        }
    }
    if let Value::Message(incoming) = value {
        if let Some(Value::Message(existing)) = msg.get_singular(field.number).cloned() {
            let mut merged = existing;
            merged.merge_from(incoming);
            msg.set(field.number, Value::Message(merged));
        } else {
            msg.set(field.number, Value::Message(incoming));
        }
        return Ok(());
    }
    msg.set(field.number, value);
    Ok(())
}

/// Decode one field value; mirrors `decode_leaf` including its defensive wire
/// checks and `std::str::from_utf8` UTF-8 path.
#[cfg(feature = "reflect")]
fn decode_dynamic_leaf(
    field: &FieldDescriptor,
    data: &[u8],
    pos: &mut usize,
    wire_type: u32,
    pool: Option<&Arc<DescriptorPool>>,
    depth: u32,
) -> Result<Value, ParseError> {
    if field.delimited || field.field_type == FieldType::Group {
        return decode_dynamic_group(field, data, pos, pool, depth);
    }
    match field.field_type {
        FieldType::Message => {
            if wire_type != WIRE_LEN {
                return Err(ParseError::new("bad wire type for message"));
            }
            let payload = read_len_bytes(data, pos)?;
            let desc = table_field_message_desc(field, pool)?;
            let nested = parse_dynamic_table_with(desc, pool.cloned(), payload, depth + 1, true)?;
            Ok(Value::Message(fuse_dynamic(nested)?))
        }
        FieldType::Group => decode_dynamic_group(field, data, pos, pool, depth),
        FieldType::String => {
            if wire_type != WIRE_LEN {
                return Err(ParseError::new("bad wire type for string"));
            }
            let bytes = read_len_bytes(data, pos)?;
            if field.utf8_validate {
                std::str::from_utf8(bytes).map_err(|_| ParseError::new("invalid utf-8"))?;
            }
            Ok(Value::String(ProtoString::from_bytes(bytes)))
        }
        FieldType::Bytes => {
            if wire_type != WIRE_LEN {
                return Err(ParseError::new("bad wire type for bytes"));
            }
            Ok(Value::Bytes(ProtoBytes::from(read_len_bytes(data, pos)?)))
        }
        FieldType::Double => Ok(Value::Double(f64::from_bits(read_fixed64(data, pos)?))),
        FieldType::Float => Ok(Value::Float(f32::from_bits(read_fixed32(data, pos)?))),
        FieldType::Fixed64 => Ok(Value::Uint64(read_fixed64(data, pos)?)),
        FieldType::Sfixed64 => Ok(Value::Int64(read_fixed64(data, pos)? as i64)),
        FieldType::Fixed32 => Ok(Value::Uint32(read_fixed32(data, pos)?)),
        FieldType::Sfixed32 => Ok(Value::Int32(read_fixed32(data, pos)? as i32)),
        FieldType::Bool => Ok(Value::Bool(decode_varint(data, pos)? != 0)),
        FieldType::Int32 => Ok(Value::Int32(decode_varint(data, pos)? as i32)),
        FieldType::Int64 => Ok(Value::Int64(decode_varint(data, pos)? as i64)),
        FieldType::Uint32 => Ok(Value::Uint32(decode_varint(data, pos)? as u32)),
        FieldType::Uint64 => Ok(Value::Uint64(decode_varint(data, pos)?)),
        FieldType::Sint32 => Ok(Value::Int32(decode_zigzag32(decode_varint(data, pos)?))),
        FieldType::Sint64 => Ok(Value::Int64(decode_zigzag64(decode_varint(data, pos)?))),
        FieldType::Enum => Ok(Value::Enum(decode_varint(data, pos)? as i32)),
    }
}

/// Decode one group body; mirrors `decode_group` by running the table loop
/// over the group descriptor with `until` set to the group number.
#[cfg(feature = "reflect")]
fn decode_dynamic_group(
    field: &FieldDescriptor,
    data: &[u8],
    pos: &mut usize,
    pool: Option<&Arc<DescriptorPool>>,
    depth: u32,
) -> Result<Value, ParseError> {
    if depth + 1 > RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    let desc = table_field_message_desc(field, pool)?;
    let table = build_table(&desc);
    let mut msg = DynamicMessage::new(desc.clone());
    if let Some(pool) = pool {
        msg.set_pool((*pool).clone());
    }
    let mut unknown = UnknownFields::default();
    merge_dynamic_group_body(
        &desc,
        pool,
        &table,
        &mut msg,
        &mut unknown,
        data,
        pos,
        depth + 1,
        field.number,
    )?;
    let nested = TabledDynamic { msg, unknown };
    Ok(Value::Message(fuse_dynamic(nested)?))
}

/// Group-body loop sharing `merge_dynamic_table` dispatch with a caller-owned
/// cursor. Group field errors surface exactly as in `decode_group`.
#[cfg(feature = "reflect")]
#[allow(
    clippy::too_many_arguments,
    reason = "group framing needs the full context"
)]
fn merge_dynamic_group_body(
    desc: &Arc<MessageDescriptor>,
    pool: Option<&Arc<DescriptorPool>>,
    table: &[FieldEntry],
    msg: &mut DynamicMessage,
    unknown: &mut UnknownFields,
    data: &[u8],
    pos: &mut usize,
    depth: u32,
    group_number: u32,
) -> Result<(), ParseError> {
    loop {
        if *pos >= data.len() {
            return Err(ParseError::new("truncated group"));
        }
        let (number, wire_type) = dyn_decode_tag(data, pos)?;
        if wire_type == WIRE_EGROUP {
            if number != group_number {
                return Err(ParseError::new("mismatched end-group"));
            }
            return Ok(());
        }
        if desc.message_set_wire_format && number == 1 {
            merge_dynamic_message_set(desc, pool, msg, unknown, data, pos, wire_type, depth)?;
            continue;
        }
        let Some(entry) = find_entry(table, number) else {
            unknown
                .fields
                .push(capture_unknown(data, pos, number, wire_type)?);
            continue;
        };
        let Some(field) = desc.field(number).cloned() else {
            return Err(ParseError::new("table descriptor mismatch"));
        };
        merge_dynamic_field(
            &field, entry, pool, msg, unknown, data, pos, wire_type, depth,
        )?;
    }
}

/// Fuse a split nested parse into one dynamic message.
///
/// Serializes known fields plus unknowns and reparses with the inline engine,
/// which restores the identical field set and unknown list (the differential
/// test proves this round trip is lossless). Needed only because unknown
/// storage is private to `dynamic.rs`, outside the PK-06 write scope; the
/// typed table path (the PK-07 codegen target) fuses structurally instead.
#[cfg(feature = "reflect")]
fn fuse_dynamic(nested: TabledDynamic) -> Result<DynamicMessage, ParseError> {
    if nested.unknown.fields.is_empty() {
        return Ok(nested.msg);
    }
    let desc = nested.msg.descriptor().clone();
    let pool = nested.msg.pool().cloned();
    let mut combined = nested
        .msg
        .serialize()
        .map_err(|_| ParseError::new("table: nested fuse serialize failed"))?;
    nested.unknown.encode(&mut combined);
    DynamicMessage::parse_with_pool(desc, pool, &combined)
}

/// Decode one map entry; mirrors `decode_map_entry` (entry unknowns are
/// dropped by design in both engines).
#[cfg(feature = "reflect")]
fn decode_dynamic_map_entry(
    entry: &MessageDescriptor,
    payload: &[u8],
    pool: Option<&Arc<DescriptorPool>>,
    depth: u32,
) -> Result<(MapKeyValue, Value), ParseError> {
    let owned_pool = pool.cloned();
    let parsed = parse_dynamic_table_with(
        Arc::new(entry.clone()),
        owned_pool.clone(),
        payload,
        depth + 1,
        false,
    )?;
    let key_field = entry
        .field(1)
        .ok_or_else(|| ParseError::new("map entry missing key"))?;
    let val_field = entry
        .field(2)
        .ok_or_else(|| ParseError::new("map entry missing value"))?;
    let key = match parsed.msg.get_singular(1) {
        Some(value) => table_value_to_map_key(value)?,
        None => table_default_map_key(key_field.field_type)?,
    };
    let value = match parsed.msg.get_singular(2) {
        Some(value) => value.clone(),
        None => table_default_value(val_field, owned_pool.as_ref())?,
    };
    Ok((key, value))
}

/// Mirror of the dynamic `value_to_map_key` helper.
#[cfg(feature = "reflect")]
fn table_value_to_map_key(value: &Value) -> Result<MapKeyValue, ParseError> {
    match value {
        Value::Int32(number) => Ok(MapKeyValue::I32(*number)),
        Value::Int64(number) => Ok(MapKeyValue::I64(*number)),
        Value::Uint32(number) => Ok(MapKeyValue::U32(*number)),
        Value::Uint64(number) => Ok(MapKeyValue::U64(*number)),
        Value::Bool(flag) => Ok(MapKeyValue::Bool(*flag)),
        Value::String(text) => Ok(MapKeyValue::String(text.clone())),
        _ => Err(ParseError::new("invalid map key type")),
    }
}

/// Mirror of the dynamic `default_map_key` helper.
#[cfg(feature = "reflect")]
fn table_default_map_key(ty: FieldType) -> Result<MapKeyValue, ParseError> {
    match ty {
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => Ok(MapKeyValue::I32(0)),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => Ok(MapKeyValue::I64(0)),
        FieldType::Uint32 | FieldType::Fixed32 => Ok(MapKeyValue::U32(0)),
        FieldType::Uint64 | FieldType::Fixed64 => Ok(MapKeyValue::U64(0)),
        FieldType::Bool => Ok(MapKeyValue::Bool(false)),
        FieldType::String => Ok(MapKeyValue::String(ProtoString::new())),
        _ => Err(ParseError::new("invalid map key type")),
    }
}

/// Mirror of the dynamic `default_value` helper.
#[cfg(feature = "reflect")]
fn table_default_value(
    field: &FieldDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
) -> Result<Value, ParseError> {
    match field.field_type {
        FieldType::Double => Ok(Value::Double(0.0)),
        FieldType::Float => Ok(Value::Float(0.0)),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => Ok(Value::Int32(0)),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => Ok(Value::Int64(0)),
        FieldType::Uint32 | FieldType::Fixed32 => Ok(Value::Uint32(0)),
        FieldType::Uint64 | FieldType::Fixed64 => Ok(Value::Uint64(0)),
        FieldType::Bool => Ok(Value::Bool(false)),
        FieldType::String => Ok(Value::String(ProtoString::new())),
        FieldType::Bytes => Ok(Value::Bytes(ProtoBytes::new())),
        FieldType::Enum => Ok(Value::Enum(0)),
        FieldType::Message | FieldType::Group => {
            let desc = table_field_message_desc(field, pool)?;
            Ok(Value::Message(DynamicMessage::new(desc)))
        }
    }
}

/// Mirror of the dynamic `field_message_desc` helper.
#[cfg(feature = "reflect")]
fn table_field_message_desc(
    field: &FieldDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
) -> Result<Arc<MessageDescriptor>, ParseError> {
    if let (Some(name), Some(pool)) = (field.type_name.as_deref(), pool) {
        if let Some(desc) = pool.get_message(name.trim_start_matches('.')) {
            return Ok(desc);
        }
    }
    if let Some(desc) = &field.message {
        return Ok(desc.clone());
    }
    Err(ParseError::new("unresolved message type"))
}

/// Mirror of the dynamic closed-enum check.
#[cfg(feature = "reflect")]
fn is_dynamic_closed_unknown(field: &FieldDescriptor, number: i32) -> bool {
    field
        .enum_ty
        .as_ref()
        .is_some_and(|ty| ty.closed && !ty.values.contains_key(&number))
}

/// Merge one MessageSet item; mirrors `merge_message_set_item`, including
/// its two quirks: length-delimited inner unknowns are dropped while
/// group-form inner unknowns land in the parent unknown list.
#[cfg(feature = "reflect")]
#[allow(clippy::too_many_arguments, reason = "mirrors message-set shape")]
fn merge_dynamic_message_set(
    desc: &MessageDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
    msg: &mut DynamicMessage,
    unknown: &mut UnknownFields,
    data: &[u8],
    pos: &mut usize,
    wire_type: u32,
    depth: u32,
) -> Result<(), ParseError> {
    let mut type_id = 0u32;
    let mut payload = Vec::new();
    if wire_type == WIRE_LEN {
        let inner = read_len_bytes(data, pos)?;
        let mut item = 0;
        while item < inner.len() {
            let (number, wire) = dyn_decode_tag(inner, &mut item)?;
            match (number, wire) {
                (2, WIRE_VARINT) => type_id = decode_varint(inner, &mut item)? as u32,
                (3, WIRE_LEN) => payload = read_len_bytes(inner, &mut item)?.to_vec(),
                _ => dyn_skip_field(inner, &mut item, wire)?,
            }
        }
    } else if wire_type == WIRE_SGROUP {
        loop {
            if *pos >= data.len() {
                return Err(ParseError::new("truncated message set"));
            }
            let (number, wire) = dyn_decode_tag(data, pos)?;
            if wire == WIRE_EGROUP && number == 1 {
                break;
            }
            match (number, wire) {
                (2, WIRE_VARINT) => type_id = decode_varint(data, pos)? as u32,
                (3, WIRE_LEN) => payload = read_len_bytes(data, pos)?.to_vec(),
                _ => unknown
                    .fields
                    .push(capture_unknown(data, pos, number, wire)?),
            }
        }
    } else {
        unknown
            .fields
            .push(capture_unknown(data, pos, 1, wire_type)?);
        return Ok(());
    }
    if type_id == 0 {
        return Ok(());
    }
    if let Some(field) = desc.field(type_id).cloned() {
        let item_desc = table_field_message_desc(&field, pool)?;
        let nested =
            parse_dynamic_table_with(item_desc, pool.cloned(), &payload, depth + 1, false)?;
        msg.set(type_id, Value::Message(fuse_dynamic(nested)?));
    } else {
        unknown.fields.push(UnknownField::Group {
            number: 1,
            fields: {
                let mut group = UnknownFields::default();
                group.fields.push(UnknownField::Varint {
                    number: 2,
                    value: u64::from(type_id),
                });
                group.fields.push(UnknownField::LengthDelimited {
                    number: 3,
                    value: payload,
                });
                group
            },
        });
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "unit-test helpers mirror generated code shapes"
)]
mod tests {
    use super::*;
    use crate::lazy::{LazyMsg, MergeBytes, require_utf8};
    use crate::packed::{PackedFx32, PackedI32};
    use crate::repeated::Repeated;
    use crate::wire::{encode_len_field, encode_tag, encode_varint, encode_zigzag32, skip_field};
    use std::collections::BTreeMap;

    // --- Representative typed messages --------------------------------------

    #[derive(Debug, Default, PartialEq)]
    struct TInner {
        id: i32,
        name: LazyStr,
        unknown: UnknownFields,
    }

    static T_INNER_TABLE: &[FieldEntry] = &[
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

    impl TableMerge for TInner {
        fn table() -> &'static [FieldEntry] {
            T_INNER_TABLE
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
                _ => Err(ParseError::new("table test: bad scalar slot")),
            }
        }

        fn set_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
            match entry.storage {
                2 => {
                    self.name = value;
                    Ok(())
                }
                _ => Err(ParseError::new("table test: bad string slot")),
            }
        }
    }

    impl MergeBytes for TInner {
        fn merge_inner(
            &mut self,
            wire: &Wire,
            pos: &mut usize,
            depth: u32,
            enforce: bool,
            until: Option<u32>,
        ) -> Result<(), ParseError> {
            let data = wire.as_slice();
            let mut slot = Some(wire.clone());
            merge_table(self, data, &mut slot, pos, depth, enforce, until)
        }
    }

    impl TInner {
        fn parse(data: &[u8]) -> Result<Self, ParseError> {
            let mut msg = Self::default();
            let mut slot = None;
            let mut pos = 0;
            merge_table(&mut msg, data, &mut slot, &mut pos, 0, true, None)?;
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

    /// Hand-written inline parser for `TInner`, mirroring the codegen `match`
    /// arms field for field. The differential test below proves the table
    /// loop agrees with it on every input.
    fn parse_inner_inline(data: &[u8]) -> Result<TInner, ParseError> {
        let mut msg = TInner::default();
        let mut slot = None;
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

    #[derive(Debug, Default, PartialEq)]
    struct TMsg {
        id: i32,
        flag: Option<bool>,
        name: LazyStr,
        raw: LazyBytes,
        req: Option<i32>,
        oneof_a: Option<i32>,
        oneof_b: Option<LazyStr>,
        eager: Option<Box<TInner>>,
        lazy: LazyMsg<TInner>,
        group: Option<Box<TInner>>,
        nums: PackedI32,
        fx: PackedFx32,
        plain: Vec<u32>,
        tags: Repeated<LazyStr>,
        kids: Vec<TInner>,
        scores: BTreeMap<String, i32>,
        status: i32,
        open: i32,
        ratio: f32,
        big: f64,
        total: u64,
        delta: i32,
        fixed: u32,
        sfixed: i64,
        child: Option<Box<TMsg>>,
        groups: Vec<TInner>,
        unknown: UnknownFields,
    }

    const UTF8: FieldFlags = FieldFlags::UTF8_VALIDATE;
    const PACKED_REP: FieldFlags = FieldFlags::PACKED;
    const REP: FieldFlags = FieldFlags::REPEATED;

    static T_MSG_TABLE: &[FieldEntry] = &[
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
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Bool,
            storage: 2,
            presence: Presence::Optional,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 3,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::String,
            storage: 3,
            presence: Presence::Implicit,
            flags: UTF8,
        },
        FieldEntry {
            number: 4,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Bytes,
            storage: 4,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 5,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Int32,
            storage: 5,
            presence: Presence::Required,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 6,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Int32,
            storage: 6,
            presence: Presence::Oneof,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 7,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::String,
            storage: 7,
            presence: Presence::Oneof,
            flags: UTF8,
        },
        FieldEntry {
            number: 8,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Message,
            storage: 8,
            presence: Presence::Optional,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 9,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Message,
            storage: 9,
            presence: Presence::Optional,
            flags: FieldFlags::LAZY,
        },
        FieldEntry {
            number: 10,
            expected_wire: WIRE_SGROUP as u8,
            kind: FieldKind::Group,
            storage: 10,
            presence: Presence::Optional,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 11,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Int32,
            storage: 11,
            presence: Presence::None,
            flags: PACKED_REP.union(REP),
        },
        FieldEntry {
            number: 12,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Fixed32,
            storage: 12,
            presence: Presence::None,
            flags: PACKED_REP.union(REP),
        },
        FieldEntry {
            number: 13,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Uint32,
            storage: 13,
            presence: Presence::None,
            flags: REP,
        },
        FieldEntry {
            number: 14,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::String,
            storage: 14,
            presence: Presence::None,
            flags: REP.union(UTF8),
        },
        FieldEntry {
            number: 15,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Message,
            storage: 15,
            presence: Presence::None,
            flags: REP,
        },
        FieldEntry {
            number: 16,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Message,
            storage: 16,
            presence: Presence::None,
            flags: FieldFlags::MAP,
        },
        FieldEntry {
            number: 17,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Enum,
            storage: 17,
            presence: Presence::Implicit,
            flags: FieldFlags::CLOSED_ENUM,
        },
        FieldEntry {
            number: 18,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Enum,
            storage: 18,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 19,
            expected_wire: WIRE_I32 as u8,
            kind: FieldKind::Float,
            storage: 19,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 20,
            expected_wire: WIRE_I64 as u8,
            kind: FieldKind::Double,
            storage: 20,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 21,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Uint64,
            storage: 21,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 22,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Sint32,
            storage: 22,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 23,
            expected_wire: WIRE_I32 as u8,
            kind: FieldKind::Fixed32,
            storage: 23,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 24,
            expected_wire: WIRE_I64 as u8,
            kind: FieldKind::Sfixed64,
            storage: 24,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 25,
            expected_wire: WIRE_LEN as u8,
            kind: FieldKind::Message,
            storage: 25,
            presence: Presence::Optional,
            flags: FieldFlags::NONE,
        },
        FieldEntry {
            number: 26,
            expected_wire: WIRE_SGROUP as u8,
            kind: FieldKind::Group,
            storage: 26,
            presence: Presence::None,
            flags: REP,
        },
    ];

    impl TableMerge for TMsg {
        fn table() -> &'static [FieldEntry] {
            T_MSG_TABLE
        }

        fn unknown_mut(&mut self) -> &mut UnknownFields {
            &mut self.unknown
        }

        fn clear_oneof(&mut self, entry: &FieldEntry) {
            match entry.storage {
                6 => self.oneof_b = None,
                7 => self.oneof_a = None,
                _ => {}
            }
        }

        fn check_required(&self) -> Result<(), ParseError> {
            if self.req.is_none() {
                return Err(ParseError::new("missing required field"));
            }
            Ok(())
        }

        fn classify_enum(&self, entry: &FieldEntry, value: i32) -> bool {
            match entry.storage {
                17 => matches!(value, 0..=2),
                _ => true,
            }
        }

        fn set_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
            match (entry.storage, value) {
                (1, Scalar::I32(value)) => self.id = value,
                (2, Scalar::Bool(value)) => self.flag = Some(value),
                (5, Scalar::I32(value)) => self.req = Some(value),
                (6, Scalar::I32(value)) => self.oneof_a = Some(value),
                (17, Scalar::Enum(value)) => self.status = value,
                (18, Scalar::Enum(value)) => self.open = value,
                (19, Scalar::F32(value)) => self.ratio = value,
                (20, Scalar::F64(value)) => self.big = value,
                (21, Scalar::U64(value)) => self.total = value,
                (22, Scalar::S32(value)) => self.delta = value,
                (23, Scalar::U32(value)) => self.fixed = value,
                (24, Scalar::I64(value)) => self.sfixed = value,
                _ => return Err(ParseError::new("table test: bad scalar slot")),
            }
            Ok(())
        }

        fn push_scalar(&mut self, entry: &FieldEntry, value: Scalar) -> Result<(), ParseError> {
            // Unpacked wire into packed storage pushes items, like the
            // generated unpacked arms on packed fields.
            match (entry.storage, value) {
                (11, Scalar::I32(value)) => self.nums.push(value),
                (12, Scalar::U32(value)) => self.fx.push(value),
                (13, Scalar::U32(value)) => self.plain.push(value),
                _ => return Err(ParseError::new("table test: bad scalar slot")),
            }
            Ok(())
        }

        fn set_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
            match entry.storage {
                3 => self.name = value,
                7 => self.oneof_b = Some(value),
                _ => return Err(ParseError::new("table test: bad string slot")),
            }
            Ok(())
        }

        fn push_string(&mut self, entry: &FieldEntry, value: LazyStr) -> Result<(), ParseError> {
            match entry.storage {
                14 => self.tags.push(value),
                _ => return Err(ParseError::new("table test: bad string slot")),
            }
            Ok(())
        }

        fn set_bytes(&mut self, entry: &FieldEntry, value: LazyBytes) -> Result<(), ParseError> {
            match entry.storage {
                4 => self.raw = value,
                _ => return Err(ParseError::new("table test: bad bytes slot")),
            }
            Ok(())
        }

        fn append_packed(&mut self, entry: &FieldEntry, payload: Wire) -> Result<(), ParseError> {
            match entry.storage {
                11 => self.nums.append_wire(payload),
                12 => self.fx.append_wire(payload),
                _ => Err(ParseError::new("table test: bad packed slot")),
            }
        }

        fn merge_message(
            &mut self,
            entry: &FieldEntry,
            data: &[u8],
            start: usize,
            end: usize,
            wire: &mut Option<Wire>,
            depth: u32,
        ) -> Result<(), ParseError> {
            let payload = &data[start..end];
            match entry.storage {
                8 => {
                    if let Some(existing) = self.eager.as_mut() {
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(
                            existing.as_mut(),
                            payload,
                            &mut slot,
                            &mut pos,
                            depth,
                            true,
                            None,
                        )?;
                    } else {
                        let mut inner = TInner::default();
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(&mut inner, payload, &mut slot, &mut pos, depth, true, None)?;
                        self.eager = Some(Box::new(inner));
                    }
                    Ok(())
                }
                9 => {
                    if self.lazy.is_some() {
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(
                            self.lazy.get_or_insert(),
                            payload,
                            &mut slot,
                            &mut pos,
                            depth,
                            true,
                            None,
                        )?;
                    } else {
                        // Validate through a throwaway parse (error-equivalent
                        // to generated `validate_inner`), then window the
                        // parent frame exactly like generated code.
                        let mut tmp = TInner::default();
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(&mut tmp, payload, &mut slot, &mut pos, depth, true, None)?;
                        self.lazy = LazyMsg::from_wire(Wire::ensure(wire, data).window(start, end));
                    }
                    Ok(())
                }
                25 => {
                    if let Some(existing) = self.child.as_mut() {
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(
                            existing.as_mut(),
                            payload,
                            &mut slot,
                            &mut pos,
                            depth,
                            true,
                            None,
                        )?;
                    } else {
                        let mut inner = TMsg::default();
                        let mut pos = 0;
                        let mut slot = None;
                        merge_table(&mut inner, payload, &mut slot, &mut pos, depth, true, None)?;
                        self.child = Some(Box::new(inner));
                    }
                    Ok(())
                }
                _ => Err(ParseError::new("table test: bad message slot")),
            }
        }

        fn push_message(
            &mut self,
            entry: &FieldEntry,
            payload: &[u8],
            depth: u32,
        ) -> Result<(), ParseError> {
            match entry.storage {
                15 => {
                    let mut inner = TInner::default();
                    let mut pos = 0;
                    let mut slot = None;
                    merge_table(&mut inner, payload, &mut slot, &mut pos, depth, true, None)?;
                    self.kids.push(inner);
                    Ok(())
                }
                _ => Err(ParseError::new("table test: bad message slot")),
            }
        }

        fn merge_group(
            &mut self,
            entry: &FieldEntry,
            data: &[u8],
            wire: &mut Option<Wire>,
            pos: &mut usize,
            depth: u32,
        ) -> Result<(), ParseError> {
            match entry.storage {
                10 => {
                    if self.group.is_none() {
                        self.group = Some(Box::default());
                    }
                    let inner = self.group.as_mut().expect("just inserted");
                    merge_table(inner.as_mut(), data, wire, pos, depth, false, Some(10))
                }
                _ => Err(ParseError::new("table test: bad group slot")),
            }
        }

        fn push_group(
            &mut self,
            entry: &FieldEntry,
            data: &[u8],
            wire: &mut Option<Wire>,
            pos: &mut usize,
            depth: u32,
        ) -> Result<(), ParseError> {
            match entry.storage {
                26 => {
                    let mut inner = TInner::default();
                    merge_table(&mut inner, data, wire, pos, depth, false, Some(26))?;
                    self.groups.push(inner);
                    Ok(())
                }
                _ => Err(ParseError::new("table test: bad group slot")),
            }
        }

        fn merge_map(
            &mut self,
            entry: &FieldEntry,
            wire: &Wire,
            _depth: u32,
        ) -> Result<(), ParseError> {
            match entry.storage {
                16 => {
                    let data = wire.as_slice();
                    let mut key = String::new();
                    let mut value = 0i32;
                    let mut pos = 0;
                    while pos < data.len() {
                        let (number, wire_type) = decode_tag(data, &mut pos)?;
                        match (number, wire_type) {
                            (1, WIRE_LEN) => {
                                let (start, end) = read_len_span(data, &mut pos)?;
                                require_utf8(&data[start..end])?;
                                key = String::from_utf8(data[start..end].to_vec())
                                    .map_err(|_| ParseError::new("invalid utf-8"))?;
                            }
                            (2, WIRE_VARINT) => {
                                value = decode_varint(data, &mut pos)? as i32;
                            }
                            _ => skip_field(data, &mut pos, wire_type)?,
                        }
                    }
                    self.scores.insert(key, value);
                    Ok(())
                }
                _ => Err(ParseError::new("table test: bad map slot")),
            }
        }
    }

    impl TMsg {
        fn parse(data: &[u8]) -> Result<Self, ParseError> {
            Self::parse_enforce(data, true)
        }

        fn parse_enforce(data: &[u8], enforce: bool) -> Result<Self, ParseError> {
            let mut msg = Self::default();
            let mut slot = None;
            let mut pos = 0;
            merge_table(&mut msg, data, &mut slot, &mut pos, 0, enforce, None)?;
            Ok(msg)
        }

        fn serialize_to(&self, out: &mut Vec<u8>) {
            if self.id != 0 {
                encode_tag(out, 1, WIRE_VARINT);
                encode_varint(out, self.id as u64);
            }
            if let Some(flag) = self.flag {
                encode_tag(out, 2, WIRE_VARINT);
                encode_varint(out, u64::from(flag));
            }
            if !self.name.is_empty() {
                encode_len_field(out, 3, self.name.as_bytes());
            }
            if !self.raw.is_empty() {
                encode_len_field(out, 4, self.raw.as_bytes());
            }
            if let Some(value) = self.req {
                encode_tag(out, 5, WIRE_VARINT);
                encode_varint(out, value as u64);
            }
            if let Some(value) = self.oneof_a {
                encode_tag(out, 6, WIRE_VARINT);
                encode_varint(out, value as u64);
            }
            if let Some(value) = self.oneof_b.as_ref() {
                encode_len_field(out, 7, value.as_bytes());
            }
            if let Some(inner) = self.eager.as_deref() {
                let bytes = inner.serialized();
                encode_len_field(out, 8, &bytes);
            }
            if let Some(bytes) = self.lazy.wire_bytes() {
                encode_len_field(out, 9, bytes);
            } else if let Some(inner) = self.lazy.as_deref() {
                let bytes = inner.serialized();
                encode_len_field(out, 9, &bytes);
            }
            if let Some(inner) = self.group.as_deref() {
                encode_tag(out, 10, WIRE_SGROUP);
                inner.serialize_to(out);
                encode_tag(out, 10, WIRE_EGROUP);
            }
            if !self.nums.is_empty() {
                let mut payload = Vec::new();
                for value in self.nums.iter() {
                    encode_varint(&mut payload, *value as u64);
                }
                encode_len_field(out, 11, &payload);
            }
            if !self.fx.is_empty() {
                let mut payload = Vec::new();
                for value in self.fx.iter() {
                    payload.extend_from_slice(&value.to_le_bytes());
                }
                encode_len_field(out, 12, &payload);
            }
            for value in &self.plain {
                encode_tag(out, 13, WIRE_VARINT);
                encode_varint(out, u64::from(*value));
            }
            for tag in self.tags.iter() {
                encode_len_field(out, 14, tag.as_bytes());
            }
            for kid in &self.kids {
                let bytes = kid.serialized();
                encode_len_field(out, 15, &bytes);
            }
            for (key, value) in &self.scores {
                let mut entry = Vec::new();
                encode_len_field(&mut entry, 1, key.as_bytes());
                encode_tag(&mut entry, 2, WIRE_VARINT);
                encode_varint(&mut entry, *value as u64);
                encode_len_field(out, 16, &entry);
            }
            if self.status != 0 {
                encode_tag(out, 17, WIRE_VARINT);
                encode_varint(out, self.status as u64);
            }
            if self.open != 0 {
                encode_tag(out, 18, WIRE_VARINT);
                encode_varint(out, self.open as u64);
            }
            if self.ratio.to_bits() != 0 {
                encode_tag(out, 19, WIRE_I32);
                out.extend_from_slice(&self.ratio.to_bits().to_le_bytes());
            }
            if self.big.to_bits() != 0 {
                encode_tag(out, 20, WIRE_I64);
                out.extend_from_slice(&self.big.to_bits().to_le_bytes());
            }
            if self.total != 0 {
                encode_tag(out, 21, WIRE_VARINT);
                encode_varint(out, self.total);
            }
            if self.delta != 0 {
                encode_tag(out, 22, WIRE_VARINT);
                encode_varint(out, encode_zigzag32(self.delta));
            }
            if self.fixed != 0 {
                encode_tag(out, 23, WIRE_I32);
                out.extend_from_slice(&self.fixed.to_le_bytes());
            }
            if self.sfixed != 0 {
                encode_tag(out, 24, WIRE_I64);
                out.extend_from_slice(&self.sfixed.to_le_bytes());
            }
            if let Some(child) = self.child.as_deref() {
                let bytes = child.serialized();
                encode_len_field(out, 25, &bytes);
            }
            for group in &self.groups {
                encode_tag(out, 26, WIRE_SGROUP);
                group.serialize_to(out);
                encode_tag(out, 26, WIRE_EGROUP);
            }
            self.unknown.encode(out);
        }

        fn serialized(&self) -> Vec<u8> {
            let mut out = Vec::new();
            self.serialize_to(&mut out);
            out
        }
    }

    // --- Mechanics ----------------------------------------------------------

    fn entry(number: u32) -> FieldEntry {
        FieldEntry {
            number,
            expected_wire: WIRE_VARINT as u8,
            kind: FieldKind::Int32,
            storage: number,
            presence: Presence::Implicit,
            flags: FieldFlags::NONE,
        }
    }

    #[test]
    fn find_entry_hits_and_misses() {
        let table = [entry(1), entry(3), entry(7), entry(40), entry(100)];
        assert_eq!(find_entry(&table, 1).unwrap().number, 1);
        assert_eq!(find_entry(&table, 7).unwrap().number, 7);
        assert_eq!(find_entry(&table, 100).unwrap().number, 100);
        assert!(find_entry(&table, 0).is_none());
        assert!(find_entry(&table, 2).is_none());
        assert!(find_entry(&table, 101).is_none());
        assert!(find_entry(&[], 1).is_none());
    }

    #[test]
    fn validate_table_accepts_sorted_rejects_bad() {
        assert!(validate_table(&[]).is_ok());
        assert!(validate_table(&[entry(1), entry(2)]).is_ok());
        assert!(validate_table(&[entry(2), entry(1)]).is_err());
        assert!(validate_table(&[entry(1), entry(1)]).is_err());
        assert!(validate_table(&[entry(0)]).is_err());
        assert!(validate_table(&[entry(crate::wire::MAX_FIELD_NUMBER + 1)]).is_err());
        assert!(validate_table(T_MSG_TABLE).is_ok());
        assert!(validate_table(T_INNER_TABLE).is_ok());
    }

    #[test]
    fn native_wire_matches_codegen_map() {
        use FieldKind::*;
        let cases = [
            (Double, WIRE_I64),
            (Float, WIRE_I32),
            (Fixed64, WIRE_I64),
            (Sfixed64, WIRE_I64),
            (Fixed32, WIRE_I32),
            (Sfixed32, WIRE_I32),
            (String, WIRE_LEN),
            (Bytes, WIRE_LEN),
            (Message, WIRE_LEN),
            (Group, WIRE_SGROUP),
            (Bool, WIRE_VARINT),
            (Int32, WIRE_VARINT),
            (Int64, WIRE_VARINT),
            (Uint32, WIRE_VARINT),
            (Uint64, WIRE_VARINT),
            (Sint32, WIRE_VARINT),
            (Sint64, WIRE_VARINT),
            (Enum, WIRE_VARINT),
        ];
        for (kind, wire) in cases {
            assert_eq!(u32::from(kind.native_wire()), wire, "{kind:?}");
        }
        assert!(!FieldKind::String.is_packable());
        assert!(!FieldKind::Bytes.is_packable());
        assert!(!FieldKind::Message.is_packable());
        assert!(!FieldKind::Group.is_packable());
        assert!(FieldKind::Enum.is_packable());
        assert!(FieldKind::Fixed32.is_memcpy());
        assert!(FieldKind::Double.is_memcpy());
        assert!(!FieldKind::Int32.is_memcpy());
    }

    #[test]
    fn decode_scalar_vectors() {
        fn decode(kind: FieldKind, bytes: &[u8]) -> Scalar {
            let mut pos = 0;
            let value = decode_scalar(kind, bytes, &mut pos).unwrap();
            assert_eq!(pos, bytes.len());
            value
        }
        assert_eq!(decode(FieldKind::Bool, &[0]), Scalar::Bool(false));
        assert_eq!(decode(FieldKind::Bool, &[2]), Scalar::Bool(true));
        assert_eq!(
            decode(FieldKind::Int32, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
            Scalar::I32(-1)
        );
        assert_eq!(decode(FieldKind::Uint64, &[0x96, 0x01]), Scalar::U64(150));
        assert_eq!(decode(FieldKind::Sint32, &[0x01]), Scalar::S32(-1));
        assert_eq!(decode(FieldKind::Sint32, &[0x02]), Scalar::S32(1));
        assert_eq!(decode(FieldKind::Sint64, &[0x01]), Scalar::S64(-1));
        assert_eq!(decode(FieldKind::Enum, &[0xe7, 0x07]), Scalar::Enum(999));
        assert_eq!(
            decode(FieldKind::Float, &1.5f32.to_bits().to_le_bytes()),
            Scalar::F32(1.5)
        );
        assert_eq!(
            decode(FieldKind::Double, &(-2.5f64).to_bits().to_le_bytes()),
            Scalar::F64(-2.5)
        );
        assert_eq!(
            decode(FieldKind::Fixed32, &[0xef, 0xbe, 0xad, 0xde]),
            Scalar::U32(0xdead_beef)
        );
        assert_eq!(
            decode(FieldKind::Sfixed64, &(-4i64).to_le_bytes()),
            Scalar::I64(-4)
        );
        assert!(decode_scalar(FieldKind::String, &[0], &mut 0).is_err());
        let mut pos = 0;
        assert!(decode_scalar(FieldKind::Int32, &[0x80], &mut pos).is_err());
    }

    #[test]
    fn validate_packed_vectors() {
        let mut good = Vec::new();
        encode_varint(&mut good, 1);
        encode_varint(&mut good, 300);
        assert!(validate_packed_bytes(FieldKind::Int32, &good).is_ok());
        assert!(validate_packed_bytes(FieldKind::Bool, &good).is_ok());
        assert!(validate_packed_bytes(FieldKind::Enum, &good).is_ok());
        assert!(validate_packed_bytes(FieldKind::Int32, &[0x80]).is_err());
        assert!(validate_packed_bytes(FieldKind::Fixed32, &[1, 2, 3, 4]).is_ok());
        assert!(validate_packed_bytes(FieldKind::Fixed32, &[1, 2, 3]).is_err());
        assert!(validate_packed_bytes(FieldKind::Double, &[0; 8]).is_ok());
        assert!(validate_packed_bytes(FieldKind::Double, &[0; 7]).is_err());
        assert!(validate_packed_bytes(FieldKind::String, &[]).is_err());
    }

    #[test]
    fn lazy_helpers_match_generated_thresholds() {
        // Inline strings never touch the parent slot.
        let data = b"ada";
        let mut slot = None;
        let value = parse_table_string(&mut slot, data, 0, 3, true).unwrap();
        assert_eq!(value.as_bytes(), b"ada");
        assert!(slot.is_none());
        // Invalid UTF-8 fails only when validated.
        let bad = [0xff, 0xfe];
        let mut slot = None;
        assert!(parse_table_string(&mut slot, &bad, 0, 2, true).is_err());
        let mut slot = None;
        let value = parse_table_string(&mut slot, &bad, 0, 2, false).unwrap();
        assert_eq!(value.as_bytes(), &bad);
        assert!(slot.is_none());
        // Dense medium strings window the parent frame.
        let mut frame = vec![0u8; 163];
        frame[10..34].fill(b'x');
        let mut slot = None;
        let value = parse_table_string(&mut slot, &frame, 10, 34, true).unwrap();
        assert_eq!(value.as_bytes(), &[b'x'; 24]);
        assert!(slot.is_some());
        // Bytes share the threshold policy with generated code.
        let mut slot = None;
        let value = parse_table_bytes(&mut slot, data, 0, 3).unwrap();
        assert_eq!(value.as_bytes(), b"ada");
        let mut slot = None;
        assert!(parse_table_string(&mut slot, data, 2, 5, true).is_err());
        assert!(parse_table_bytes(&mut slot, data, 3, 2).is_err());
    }

    // --- Typed loop ---------------------------------------------------------

    fn inner_bytes(id: i32, name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        if id != 0 {
            encode_tag(&mut out, 1, WIRE_VARINT);
            encode_varint(&mut out, id as u64);
        }
        if !name.is_empty() {
            encode_len_field(&mut out, 2, name.as_bytes());
        }
        out
    }

    fn full_payload() -> Vec<u8> {
        let mut out = Vec::new();
        encode_tag(&mut out, 1, WIRE_VARINT);
        encode_varint(&mut out, 150);
        encode_tag(&mut out, 2, WIRE_VARINT);
        encode_varint(&mut out, 1);
        encode_len_field(&mut out, 3, b"hello");
        encode_len_field(&mut out, 4, &[0, 255]);
        encode_tag(&mut out, 5, WIRE_VARINT);
        encode_varint(&mut out, 7);
        encode_len_field(&mut out, 7, b"solo");
        encode_len_field(&mut out, 8, &inner_bytes(1, "e"));
        encode_len_field(&mut out, 9, &inner_bytes(2, "l"));
        encode_tag(&mut out, 10, WIRE_SGROUP);
        out.extend_from_slice(&inner_bytes(3, ""));
        encode_tag(&mut out, 10, WIRE_EGROUP);
        let mut packed = Vec::new();
        for value in [1i32, 2, 300] {
            encode_varint(&mut packed, value as u64);
        }
        encode_len_field(&mut out, 11, &packed);
        let mut fixed = Vec::new();
        for value in [1u32, 2] {
            fixed.extend_from_slice(&value.to_le_bytes());
        }
        encode_len_field(&mut out, 12, &fixed);
        for value in [7u32, 8] {
            encode_tag(&mut out, 13, WIRE_VARINT);
            encode_varint(&mut out, u64::from(value));
        }
        encode_len_field(&mut out, 14, b"a");
        encode_len_field(&mut out, 14, b"bb");
        encode_len_field(&mut out, 15, &inner_bytes(4, ""));
        let mut entry = Vec::new();
        encode_len_field(&mut entry, 1, b"k");
        encode_tag(&mut entry, 2, WIRE_VARINT);
        encode_varint(&mut entry, 9);
        encode_len_field(&mut out, 16, &entry);
        encode_tag(&mut out, 17, WIRE_VARINT);
        encode_varint(&mut out, 1);
        encode_tag(&mut out, 18, WIRE_VARINT);
        encode_varint(&mut out, 999);
        encode_tag(&mut out, 19, WIRE_I32);
        out.extend_from_slice(&1.5f32.to_bits().to_le_bytes());
        encode_tag(&mut out, 20, WIRE_I64);
        out.extend_from_slice(&(-2.5f64).to_bits().to_le_bytes());
        encode_tag(&mut out, 21, WIRE_VARINT);
        encode_varint(&mut out, u64::MAX);
        encode_tag(&mut out, 22, WIRE_VARINT);
        encode_varint(&mut out, encode_zigzag32(-3));
        encode_tag(&mut out, 23, WIRE_I32);
        out.extend_from_slice(&0xdead_beefu32.to_le_bytes());
        encode_tag(&mut out, 24, WIRE_I64);
        out.extend_from_slice(&(-4i64).to_le_bytes());
        let mut child = Vec::new();
        encode_tag(&mut child, 1, WIRE_VARINT);
        encode_varint(&mut child, 9);
        encode_tag(&mut child, 5, WIRE_VARINT);
        encode_varint(&mut child, 1);
        encode_len_field(&mut out, 25, &child);
        encode_tag(&mut out, 26, WIRE_SGROUP);
        out.extend_from_slice(&inner_bytes(5, ""));
        encode_tag(&mut out, 26, WIRE_EGROUP);
        out
    }

    #[test]
    fn all_kinds_round_trip() {
        let payload = full_payload();
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.id, 150);
        assert_eq!(msg.flag, Some(true));
        assert_eq!(msg.name.as_bytes(), b"hello");
        assert_eq!(msg.raw.as_bytes(), &[0, 255]);
        assert_eq!(msg.req, Some(7));
        assert_eq!(msg.oneof_a, None);
        assert_eq!(msg.oneof_b.as_ref().unwrap().as_bytes(), b"solo");
        assert_eq!(msg.eager.as_deref().unwrap().id, 1);
        assert_eq!(msg.lazy.as_deref().unwrap().id, 2);
        assert_eq!(msg.lazy.wire_bytes().unwrap(), inner_bytes(2, "l"));
        assert_eq!(msg.group.as_deref().unwrap().id, 3);
        assert_eq!(msg.nums.iter().collect::<Vec<_>>(), [&1, &2, &300]);
        assert_eq!(msg.fx.iter().collect::<Vec<_>>(), [&1, &2]);
        assert_eq!(msg.plain, [7, 8]);
        assert_eq!(msg.tags.len(), 2);
        assert_eq!(msg.kids.len(), 1);
        assert_eq!(msg.kids[0].id, 4);
        assert_eq!(msg.scores.get("k"), Some(&9));
        assert_eq!(msg.status, 1);
        assert_eq!(msg.open, 999);
        assert_eq!(msg.ratio, 1.5);
        assert_eq!(msg.big, -2.5);
        assert_eq!(msg.total, u64::MAX);
        assert_eq!(msg.delta, -3);
        assert_eq!(msg.fixed, 0xdead_beef);
        assert_eq!(msg.sfixed, -4);
        assert_eq!(msg.child.as_deref().unwrap().id, 9);
        assert_eq!(msg.groups.len(), 1);
        assert!(msg.unknown.fields.is_empty());
        assert_eq!(msg.serialized(), payload);
    }

    #[test]
    fn last_wins_and_oneof_replacement() {
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 1, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 1, WIRE_VARINT);
        encode_varint(&mut payload, 2);
        encode_len_field(&mut payload, 3, b"a");
        encode_len_field(&mut payload, 3, b"b");
        encode_tag(&mut payload, 6, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_len_field(&mut payload, 7, b"x");
        encode_tag(&mut payload, 6, WIRE_VARINT);
        encode_varint(&mut payload, 3);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.id, 2);
        assert_eq!(msg.name.as_bytes(), b"b");
        assert_eq!(msg.oneof_a, Some(3));
        assert!(msg.oneof_b.is_none());
        let mut expected = Vec::new();
        encode_tag(&mut expected, 1, WIRE_VARINT);
        encode_varint(&mut expected, 2);
        encode_len_field(&mut expected, 3, b"b");
        encode_tag(&mut expected, 5, WIRE_VARINT);
        encode_varint(&mut expected, 1);
        encode_tag(&mut expected, 6, WIRE_VARINT);
        encode_varint(&mut expected, 3);
        assert_eq!(msg.serialized(), expected);
    }

    #[test]
    fn unknown_fields_and_wire_mismatches_preserved() {
        let mut payload = Vec::new();
        encode_tag(&mut payload, 1, WIRE_VARINT);
        encode_varint(&mut payload, 5);
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 99, WIRE_VARINT);
        encode_varint(&mut payload, 5);
        encode_len_field(&mut payload, 1, b"xx");
        encode_tag(&mut payload, 3, WIRE_VARINT);
        encode_varint(&mut payload, 9);
        encode_tag(&mut payload, 50, WIRE_I32);
        payload.extend_from_slice(&7u32.to_le_bytes());
        encode_tag(&mut payload, 51, WIRE_SGROUP);
        encode_tag(&mut payload, 1, WIRE_VARINT);
        encode_varint(&mut payload, 9);
        encode_tag(&mut payload, 51, WIRE_EGROUP);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.id, 5);
        let unknowns = msg.unknown.fields.as_slice();
        assert_eq!(unknowns.len(), 5);
        assert!(matches!(
            unknowns[0],
            UnknownField::Varint {
                number: 99,
                value: 5
            }
        ));
        assert!(matches!(
            &unknowns[1],
            UnknownField::LengthDelimited { number: 1, value } if value == b"xx"
        ));
        assert!(matches!(
            unknowns[2],
            UnknownField::Varint {
                number: 3,
                value: 9
            }
        ));
        assert!(matches!(
            unknowns[3],
            UnknownField::Fixed32 {
                number: 50,
                value: 7
            }
        ));
        assert!(matches!(
            unknowns[4],
            UnknownField::Group { number: 51, .. }
        ));
        let mut expected = Vec::new();
        encode_tag(&mut expected, 1, WIRE_VARINT);
        encode_varint(&mut expected, 5);
        encode_tag(&mut expected, 5, WIRE_VARINT);
        encode_varint(&mut expected, 1);
        expected.extend_from_slice(&payload[4..]);
        assert_eq!(msg.serialized(), expected);
    }

    #[test]
    fn required_enforcement() {
        assert!(TMsg::parse(&[]).is_err());
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 0);
        assert!(TMsg::parse(&payload).is_ok());
        let msg = TMsg::parse_enforce(&[], false).unwrap();
        assert_eq!(msg.req, None);
    }

    #[test]
    fn closed_enum_moves_to_unknown_open_keeps_value() {
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 17, WIRE_VARINT);
        encode_varint(&mut payload, 2);
        encode_tag(&mut payload, 17, WIRE_VARINT);
        encode_varint(&mut payload, 999);
        encode_tag(&mut payload, 18, WIRE_VARINT);
        encode_varint(&mut payload, 999);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.status, 2);
        assert_eq!(msg.open, 999);
        let unknowns = msg.unknown.fields.as_slice();
        assert_eq!(unknowns.len(), 1);
        assert!(matches!(
            unknowns[0],
            UnknownField::Varint {
                number: 17,
                value: 999
            }
        ));
        // Unknown closed values never clear a previous known assignment.
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 17, WIRE_VARINT);
        encode_varint(&mut payload, 999);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.status, 0);
        assert_eq!(msg.unknown.fields.as_slice().len(), 1);
    }

    #[test]
    fn packed_accepted_both_directions() {
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        let mut packed = Vec::new();
        encode_varint(&mut packed, 1);
        encode_varint(&mut packed, 2);
        encode_len_field(&mut payload, 11, &packed);
        encode_tag(&mut payload, 11, WIRE_VARINT);
        encode_varint(&mut payload, 3);
        let mut fixed = Vec::new();
        fixed.extend_from_slice(&10u32.to_le_bytes());
        encode_len_field(&mut payload, 12, &fixed);
        encode_tag(&mut payload, 12, WIRE_I32);
        payload.extend_from_slice(&11u32.to_le_bytes());
        // Packed wire into the unpacked field decodes items inline.
        let mut plain_packed = Vec::new();
        encode_varint(&mut plain_packed, 20);
        encode_varint(&mut plain_packed, 21);
        encode_len_field(&mut payload, 13, &plain_packed);
        encode_tag(&mut payload, 13, WIRE_VARINT);
        encode_varint(&mut payload, 22);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.nums.iter().collect::<Vec<_>>(), [&1, &2, &3]);
        assert_eq!(msg.fx.iter().collect::<Vec<_>>(), [&10, &11]);
        assert_eq!(msg.plain, [20, 21, 22]);

        // Truncated packed payloads fail.
        for (number, bytes) in [(11u32, vec![0x80]), (12u32, vec![1, 2, 3])] {
            let mut bad = Vec::new();
            encode_tag(&mut bad, 5, WIRE_VARINT);
            encode_varint(&mut bad, 1);
            encode_len_field(&mut bad, number, &bytes);
            assert!(TMsg::parse(&bad).is_err(), "field {number}");
        }
    }

    #[test]
    fn map_last_wins_with_defaults() {
        fn entry(key: Option<&str>, value: Option<i32>) -> Vec<u8> {
            let mut out = Vec::new();
            if let Some(key) = key {
                encode_len_field(&mut out, 1, key.as_bytes());
            }
            if let Some(value) = value {
                encode_tag(&mut out, 2, WIRE_VARINT);
                encode_varint(&mut out, value as u64);
            }
            out
        }
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_len_field(&mut payload, 16, &entry(Some("a"), Some(1)));
        encode_len_field(&mut payload, 16, &entry(Some("b"), Some(2)));
        encode_len_field(&mut payload, 16, &entry(Some("a"), Some(3)));
        encode_len_field(&mut payload, 16, &entry(Some("c"), None));
        encode_len_field(&mut payload, 16, &entry(None, Some(4)));
        let mut with_unknown = entry(Some("d"), Some(5));
        encode_tag(&mut with_unknown, 9, WIRE_VARINT);
        encode_varint(&mut with_unknown, 1);
        encode_len_field(&mut payload, 16, &with_unknown);
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.scores.get("a"), Some(&3));
        assert_eq!(msg.scores.get("b"), Some(&2));
        assert_eq!(msg.scores.get("c"), Some(&0));
        assert_eq!(msg.scores.get(""), Some(&4));
        assert_eq!(msg.scores.get("d"), Some(&5));

        // Invalid map-key UTF-8 fails the parse.
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        encode_len_field(&mut bad, 16, &entry(Some("\u{fffd}"), Some(1)));
        assert!(TMsg::parse(&bad).is_ok());
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        let mut invalid = Vec::new();
        encode_len_field(&mut invalid, 1, &[0xff, 0xfe]);
        encode_len_field(&mut bad, 16, &invalid);
        assert!(TMsg::parse(&bad).is_err());
    }

    #[test]
    fn group_shapes() {
        // Well-formed singular + repeated groups with nested unknowns.
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_tag(&mut payload, 10, WIRE_SGROUP);
        encode_tag(&mut payload, 1, WIRE_VARINT);
        encode_varint(&mut payload, 7);
        encode_tag(&mut payload, 77, WIRE_VARINT);
        encode_varint(&mut payload, 8);
        encode_tag(&mut payload, 10, WIRE_EGROUP);
        for id in [1, 2] {
            encode_tag(&mut payload, 26, WIRE_SGROUP);
            encode_tag(&mut payload, 26, WIRE_VARINT);
            encode_varint(&mut payload, id);
            encode_tag(&mut payload, 26, WIRE_EGROUP);
        }
        let msg = TMsg::parse(&payload).unwrap();
        assert_eq!(msg.group.as_deref().unwrap().id, 7);
        assert_eq!(
            msg.group
                .as_deref()
                .unwrap()
                .unknown
                .fields
                .as_slice()
                .len(),
            1
        );
        assert_eq!(msg.groups.len(), 2);
        // Field 26 is unknown inside the group body, so it lands in unknowns.
        assert_eq!(msg.groups[0].unknown.fields.as_slice().len(), 1);

        // Truncated group.
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        encode_tag(&mut bad, 10, WIRE_SGROUP);
        encode_tag(&mut bad, 1, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        assert!(TMsg::parse(&bad).is_err());

        // Mismatched end-group number.
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        encode_tag(&mut bad, 10, WIRE_SGROUP);
        encode_tag(&mut bad, 11, WIRE_EGROUP);
        assert!(TMsg::parse(&bad).is_err());

        // Stray top-level end-group is an error, not an unknown.
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        encode_tag(&mut bad, 10, WIRE_EGROUP);
        assert!(TMsg::parse(&bad).is_err());
    }

    fn nest_payload(depth: u32) -> Vec<u8> {
        let mut inner = Vec::new();
        encode_tag(&mut inner, 5, WIRE_VARINT);
        encode_varint(&mut inner, 1);
        for _ in 0..depth {
            let mut wrapped = Vec::new();
            encode_tag(&mut wrapped, 5, WIRE_VARINT);
            encode_varint(&mut wrapped, 1);
            encode_len_field(&mut wrapped, 25, &inner);
            inner = wrapped;
        }
        inner
    }

    #[test]
    fn depth_limit() {
        // Direct guard: over-limit entry fails before reading input.
        let mut msg = TMsg::default();
        let mut slot = None;
        let mut pos = 0;
        assert!(
            merge_table(
                &mut msg,
                &[],
                &mut slot,
                &mut pos,
                RECURSION_LIMIT + 1,
                false,
                None
            )
            .is_err()
        );
        // Nested messages: depth == limit parses, deeper fails.
        assert!(TMsg::parse(&nest_payload(RECURSION_LIMIT)).is_ok());
        assert!(TMsg::parse(&nest_payload(RECURSION_LIMIT + 1)).is_err());
    }

    #[test]
    fn lazy_field_semantics() {
        // Nested payload with unknowns interleaved before known fields.
        let mut nested = Vec::new();
        encode_tag(&mut nested, 99, WIRE_VARINT);
        encode_varint(&mut nested, 1);
        encode_tag(&mut nested, 1, WIRE_VARINT);
        encode_varint(&mut nested, 3);
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_len_field(&mut payload, 9, &nested);
        let mut msg = TMsg::default();
        let mut slot = None;
        let mut pos = 0;
        merge_table(&mut msg, &payload, &mut slot, &mut pos, 0, true, None).unwrap();
        // Parent frame is windowed exactly like generated code.
        assert!(slot.is_some());
        assert_eq!(msg.lazy.wire_bytes().unwrap(), nested);
        assert_eq!(msg.lazy.as_deref().unwrap().id, 3);
        assert_eq!(
            msg.lazy.as_deref().unwrap().unknown.fields.as_slice().len(),
            1
        );
        // Serialization prefers the stored wire bytes verbatim.
        assert_eq!(msg.serialized(), payload);

        // A second occurrence merges into the materialized value.
        let mut payload2 = payload.clone();
        encode_len_field(&mut payload2, 9, &inner_bytes(0, "x"));
        let msg = TMsg::parse(&payload2).unwrap();
        let lazy = msg.lazy.as_deref().unwrap();
        assert_eq!(lazy.id, 3);
        assert_eq!(lazy.name.as_bytes(), b"x");

        // Invalid nested payloads fail at parse time, like validate_inner.
        let mut bad = Vec::new();
        encode_tag(&mut bad, 5, WIRE_VARINT);
        encode_varint(&mut bad, 1);
        encode_len_field(&mut bad, 9, &[0x12, 0x01, 0xff]);
        assert!(TMsg::parse(&bad).is_err());
    }

    #[test]
    fn eager_message_merges_across_occurrences() {
        let mut payload = Vec::new();
        encode_tag(&mut payload, 5, WIRE_VARINT);
        encode_varint(&mut payload, 1);
        encode_len_field(&mut payload, 8, &inner_bytes(1, ""));
        let mut second = inner_bytes(0, "n");
        encode_tag(&mut second, 60, WIRE_VARINT);
        encode_varint(&mut second, 2);
        encode_len_field(&mut payload, 8, &second);
        let msg = TMsg::parse(&payload).unwrap();
        let eager = msg.eager.as_deref().unwrap();
        assert_eq!(eager.id, 1);
        assert_eq!(eager.name.as_bytes(), b"n");
        assert_eq!(eager.unknown.fields.as_slice().len(), 1);
    }

    #[test]
    fn inner_table_matches_hand_inline() {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        for id in [None, Some(0), Some(1), Some(-1), Some(i32::MAX)] {
            let mut part = Vec::new();
            if let Some(id) = id {
                encode_tag(&mut part, 1, WIRE_VARINT);
                encode_varint(&mut part, id as u64);
            }
            parts.push(part);
        }
        parts.push({
            let mut part = Vec::new();
            encode_len_field(&mut part, 1, b"x");
            part
        });
        let mut names: Vec<Vec<u8>> = Vec::new();
        for name in [
            None,
            Some(""),
            Some("a"),
            Some("eighty-chars-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        ] {
            let mut part = Vec::new();
            if let Some(name) = name {
                encode_len_field(&mut part, 2, name.as_bytes());
            }
            names.push(part);
        }
        names.push({
            let mut part = Vec::new();
            encode_len_field(&mut part, 2, &[0xff, 0xfe]);
            part
        });
        names.push({
            let mut part = Vec::new();
            encode_tag(&mut part, 2, WIRE_VARINT);
            encode_varint(&mut part, 4);
            part
        });
        let mut unknowns: Vec<Vec<u8>> = vec![Vec::new()];
        for extra in [
            vec![0xf8, 0x06, 0x05],
            vec![0xfd, 0x06, 0x01, 0x02, 0x03, 0x04],
            vec![0x89, 0x06, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
            vec![0x92, 0x06, 0x03, b'x', b'y', b'z'],
        ] {
            unknowns.push(extra);
        }
        unknowns.push(vec![0x80]);
        unknowns.push(vec![0x0a, 0xff, 0xff, 0xff, 0xff, 0x0f]);
        let mut count = 0;
        for id in &parts {
            for name in &names {
                for unknown in &unknowns {
                    let mut payload = Vec::new();
                    payload.extend_from_slice(id);
                    payload.extend_from_slice(name);
                    payload.extend_from_slice(unknown);
                    let table = TInner::parse(&payload);
                    let inline = parse_inner_inline(&payload);
                    assert_eq!(
                        table.is_ok(),
                        inline.is_ok(),
                        "engines disagree on {payload:02x?}"
                    );
                    if let (Ok(table), Ok(inline)) = (table, inline) {
                        assert_eq!(table, inline, "message mismatch on {payload:02x?}");
                        assert_eq!(
                            table.serialized(),
                            inline.serialized(),
                            "bytes mismatch on {payload:02x?}"
                        );
                    }
                    count += 1;
                }
            }
        }
        // Truncation sweep over one valid payload.
        let mut valid = Vec::new();
        encode_tag(&mut valid, 1, WIRE_VARINT);
        encode_varint(&mut valid, 300);
        encode_len_field(&mut valid, 2, b"ada");
        for cut in 0..=valid.len() {
            let payload = &valid[..cut];
            let table = TInner::parse(payload);
            let inline = parse_inner_inline(payload);
            assert_eq!(table.is_ok(), inline.is_ok(), "cut at {cut}");
            if let (Ok(table), Ok(inline)) = (table, inline) {
                assert_eq!(table.serialized(), inline.serialized());
            }
            count += 1;
        }
        assert!(count > 200, "differential swept {count} payloads");
    }

    #[derive(Default)]
    struct TMset {
        unknown: UnknownFields,
    }

    impl TableMerge for TMset {
        fn table() -> &'static [FieldEntry] {
            &[]
        }

        const MESSAGE_SET: bool = true;

        fn unknown_mut(&mut self) -> &mut UnknownFields {
            &mut self.unknown
        }
    }

    #[cfg(feature = "reflect")]
    fn assert_dynamic_agree(
        desc: &Arc<MessageDescriptor>,
        pool: Option<Arc<DescriptorPool>>,
        data: &[u8],
    ) {
        let inline = DynamicMessage::parse_with_pool(desc.clone(), pool.clone(), data);
        let tabled = parse_dynamic_table_with(desc.clone(), pool, data, 0, true);
        match (inline, tabled) {
            (Ok(inline), Ok(tabled)) => {
                let mut combined = tabled.msg.serialize().unwrap();
                tabled.unknown.encode(&mut combined);
                assert_eq!(combined, inline.serialize().unwrap(), "bytes diverge");
                assert_eq!(tabled.unknown, *inline.unknown_fields(), "unknowns diverge");
            }
            (Err(_), Err(_)) => {}
            (inline, tabled) => panic!(
                "engines disagree: inline ok={} table ok={}",
                inline.is_ok(),
                tabled.is_ok()
            ),
        }
    }

    #[cfg(feature = "reflect")]
    #[test]
    fn dynamic_table_matches_inline_smoke() {
        use crate::dynamic::{DescriptorPool, EnumDescriptor, Presence as DynPresence};
        use std::collections::BTreeMap;

        let nested = MessageDescriptor::builder("t.Nested")
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
        let entry = MessageDescriptor::builder("t.ScoresEntry")
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
        let group = MessageDescriptor::builder("t.G")
            .field(FieldDescriptor::new(
                "id",
                1,
                FieldType::Int32,
                Cardinality::Optional,
                DynPresence::Implicit,
            ))
            .build();
        let mut values = BTreeMap::new();
        values.insert(0, "A".to_string());
        values.insert(1, "B".to_string());
        let closed = EnumDescriptor {
            full_name: "t.E".to_string(),
            values,
            closed: true,
            ..Default::default()
        };
        let mut utf8_off = FieldDescriptor::new(
            "raw",
            3,
            FieldType::String,
            Cardinality::Optional,
            DynPresence::Implicit,
        );
        utf8_off.utf8_validate = false;
        let mut oneof_a = FieldDescriptor::new(
            "oa",
            6,
            FieldType::Int32,
            Cardinality::Optional,
            DynPresence::Implicit,
        );
        oneof_a.oneof_index = Some(0);
        let mut oneof_b = FieldDescriptor::new(
            "ob",
            7,
            FieldType::String,
            Cardinality::Optional,
            DynPresence::Implicit,
        );
        oneof_b.oneof_index = Some(0);
        let mut nested_field = FieldDescriptor::new(
            "child",
            4,
            FieldType::Message,
            Cardinality::Optional,
            DynPresence::Explicit,
        );
        nested_field.message = Some(Arc::new(nested));
        let mut map_field = FieldDescriptor::new(
            "scores",
            5,
            FieldType::Message,
            Cardinality::Repeated,
            DynPresence::Implicit,
        );
        map_field.is_map = true;
        map_field.message = Some(Arc::new(entry));
        let mut enum_field = FieldDescriptor::new(
            "state",
            9,
            FieldType::Enum,
            Cardinality::Optional,
            DynPresence::Implicit,
        );
        enum_field.enum_ty = Some(Arc::new(closed));
        let mut group_field = FieldDescriptor::new(
            "g",
            10,
            FieldType::Group,
            Cardinality::Optional,
            DynPresence::Explicit,
        );
        group_field.message = Some(Arc::new(group));
        let mut unpacked = FieldDescriptor::new(
            "plain",
            12,
            FieldType::Int32,
            Cardinality::Repeated,
            DynPresence::Implicit,
        );
        unpacked.packed = false;
        let mut desc = MessageDescriptor::builder("t.Outer")
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
                DynPresence::Explicit,
            ))
            .field(utf8_off)
            .field(nested_field)
            .field(map_field)
            .field(oneof_a)
            .field(oneof_b)
            .field(FieldDescriptor::new(
                "req",
                8,
                FieldType::Int32,
                Cardinality::Required,
                DynPresence::Explicit,
            ))
            .field(enum_field)
            .field(group_field)
            .field(FieldDescriptor::new(
                "nums",
                11,
                FieldType::Int32,
                Cardinality::Repeated,
                DynPresence::Implicit,
            ))
            .field(unpacked)
            .build();
        desc.oneofs = vec![vec![6, 7]];
        let desc = Arc::new(desc);
        let pool = Arc::new(DescriptorPool::new());

        let mut full = Vec::new();
        encode_tag(&mut full, 8, WIRE_VARINT);
        encode_varint(&mut full, 1);
        encode_tag(&mut full, 1, WIRE_VARINT);
        encode_varint(&mut full, 42);
        encode_len_field(&mut full, 2, b"ada");
        encode_len_field(&mut full, 3, &[0xff]);
        let mut child = Vec::new();
        encode_tag(&mut child, 1, WIRE_VARINT);
        encode_varint(&mut child, 5);
        encode_tag(&mut child, 90, WIRE_VARINT);
        encode_varint(&mut child, 6);
        encode_len_field(&mut full, 4, &child);
        let mut entry = Vec::new();
        encode_len_field(&mut entry, 1, b"k");
        encode_tag(&mut entry, 2, WIRE_VARINT);
        encode_varint(&mut entry, 3);
        encode_len_field(&mut full, 5, &entry);
        encode_tag(&mut full, 6, WIRE_VARINT);
        encode_varint(&mut full, 1);
        encode_len_field(&mut full, 7, b"wins");
        encode_tag(&mut full, 9, WIRE_VARINT);
        encode_varint(&mut full, 1);
        encode_tag(&mut full, 10, WIRE_SGROUP);
        encode_tag(&mut full, 1, WIRE_VARINT);
        encode_varint(&mut full, 9);
        encode_tag(&mut full, 10, WIRE_EGROUP);
        let mut packed = Vec::new();
        encode_varint(&mut packed, 4);
        encode_len_field(&mut full, 11, &packed);
        encode_tag(&mut full, 12, WIRE_VARINT);
        encode_varint(&mut full, 5);
        assert_dynamic_agree(&desc, Some(pool.clone()), &full);

        // Unknowns, mismatches, closed-unknown enums, unpacked-into-packed.
        let mut odd = Vec::new();
        encode_tag(&mut odd, 8, WIRE_VARINT);
        encode_varint(&mut odd, 1);
        encode_tag(&mut odd, 99, WIRE_VARINT);
        encode_varint(&mut odd, 1);
        encode_len_field(&mut odd, 1, b"nope");
        encode_tag(&mut odd, 9, WIRE_VARINT);
        encode_varint(&mut odd, 777);
        encode_tag(&mut odd, 11, WIRE_VARINT);
        encode_varint(&mut odd, 6);
        let mut plain_packed = Vec::new();
        encode_varint(&mut plain_packed, 7);
        encode_len_field(&mut odd, 12, &plain_packed);
        assert_dynamic_agree(&desc, Some(pool.clone()), &odd);

        // Error agreement: bad UTF-8 where validated, truncation, missing required.
        let mut bad_utf8 = Vec::new();
        encode_tag(&mut bad_utf8, 8, WIRE_VARINT);
        encode_varint(&mut bad_utf8, 1);
        encode_len_field(&mut bad_utf8, 2, &[0xff]);
        assert_dynamic_agree(&desc, Some(pool.clone()), &bad_utf8);
        assert_dynamic_agree(&desc, Some(pool.clone()), &full[..full.len() - 1]);
        assert_dynamic_agree(&desc, Some(pool.clone()), &[]);
        assert_dynamic_agree(&desc, Some(pool.clone()), &[0x08, 0x80]);
        assert_dynamic_agree(&desc, Some(pool), &[0x0a, 0xff, 0x01]);
    }

    #[test]
    fn messageset_default_preserves_items() {
        // Length-delimited MessageSet item for an unlisted type id.
        let mut inner = Vec::new();
        encode_tag(&mut inner, 2, WIRE_VARINT);
        encode_varint(&mut inner, 42);
        encode_len_field(&mut inner, 3, b"payload");
        let mut payload = Vec::new();
        encode_len_field(&mut payload, 1, &inner);
        let mut msg = TMset::default();
        let mut slot = None;
        let mut pos = 0;
        merge_table(&mut msg, &payload, &mut slot, &mut pos, 0, false, None).unwrap();
        assert_eq!(msg.unknown.fields.as_slice().len(), 1);
        let mut round = Vec::new();
        msg.unknown.encode(&mut round);
        assert_eq!(round, payload);
        // Group-form item round-trips through the group unknown shape.
        let mut payload = Vec::new();
        encode_tag(&mut payload, 1, WIRE_SGROUP);
        encode_tag(&mut payload, 2, WIRE_VARINT);
        encode_varint(&mut payload, 42);
        encode_len_field(&mut payload, 3, b"payload");
        encode_tag(&mut payload, 1, WIRE_EGROUP);
        let mut msg = TMset::default();
        let mut slot = None;
        let mut pos = 0;
        merge_table(&mut msg, &payload, &mut slot, &mut pos, 0, false, None).unwrap();
        let mut round = Vec::new();
        msg.unknown.encode(&mut round);
        assert_eq!(round, payload);
    }
}
