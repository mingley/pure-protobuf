//! Runtime descriptors and `DynamicMessage`.

use crate::error::{ParseError, SerializeError};
use crate::internal::SealedInternal;
use crate::message::{
    Clear, ClearAndParse, CopyFrom, MergeFrom, Message, MessageMut, MessageType, MessageView,
    Serialize, TakeFrom,
};
use crate::proxied::{AsMut, AsView, IntoMut, IntoView, MutProxied, Proxied};
use crate::string::{ProtoBytes, ProtoString};
use crate::wire::{
    self, decode_tag, decode_varint, encode_len_field, encode_tag, encode_varint, encode_zigzag32,
    encode_zigzag64, key_len_value_len, read_fixed32, read_fixed64, read_len_bytes, tag_len,
    varint_len, UnknownField, UnknownFields, WIRE_EGROUP, WIRE_I32, WIRE_I64, WIRE_LEN,
    WIRE_SGROUP, WIRE_VARINT,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

/// Maximum nesting depth for binary, JSON, and text parse.
/// A payload nested more than this many messages returns [`ParseError`].
pub const RECURSION_LIMIT: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Double = 1,
    Float = 2,
    Int64 = 3,
    Uint64 = 4,
    Int32 = 5,
    Fixed64 = 6,
    Fixed32 = 7,
    Bool = 8,
    String = 9,
    Group = 10,
    Message = 11,
    Bytes = 12,
    Uint32 = 13,
    Enum = 14,
    Sfixed32 = 15,
    Sfixed64 = 16,
    Sint32 = 17,
    Sint64 = 18,
}

impl FieldType {
    fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            1 => Self::Double,
            2 => Self::Float,
            3 => Self::Int64,
            4 => Self::Uint64,
            5 => Self::Int32,
            6 => Self::Fixed64,
            7 => Self::Fixed32,
            8 => Self::Bool,
            9 => Self::String,
            10 => Self::Group,
            11 => Self::Message,
            12 => Self::Bytes,
            13 => Self::Uint32,
            14 => Self::Enum,
            15 => Self::Sfixed32,
            16 => Self::Sfixed64,
            17 => Self::Sint32,
            18 => Self::Sint64,
            _ => return None,
        })
    }

    pub(crate) fn is_packable(self) -> bool {
        !matches!(
            self,
            Self::String | Self::Bytes | Self::Message | Self::Group
        )
    }

    fn default_wire(self) -> u32 {
        match self {
            Self::Fixed64 | Self::Sfixed64 | Self::Double => WIRE_I64,
            Self::Fixed32 | Self::Sfixed32 | Self::Float => WIRE_I32,
            Self::String | Self::Bytes | Self::Message | Self::Group => WIRE_LEN,
            _ => WIRE_VARINT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinality {
    Optional,
    Required,
    Repeated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Implicit,
    Explicit,
}

#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    pub name: String,
    pub number: u32,
    pub field_type: FieldType,
    pub cardinality: Cardinality,
    pub presence: Presence,
    pub packed: bool,
    pub type_name: Option<String>,
    pub json_name: String,
    pub is_map: bool,
    pub message: Option<Arc<MessageDescriptor>>,
    pub enum_ty: Option<Arc<EnumDescriptor>>,
    pub oneof_index: Option<u32>,
    pub utf8_validate: bool,
    pub default: Option<String>,
    pub extendee: Option<String>,
    pub delimited: bool,
    pub extension_name: Option<String>,
    /// Unrecognized `FieldOptions` tags (custom options). Payload is the option
    /// body: length-delimited bytes, or the varint/fixed encoding.
    pub options: Vec<DescriptorOption>,
}

impl FieldDescriptor {
    pub fn new(
        name: impl Into<String>,
        number: u32,
        field_type: FieldType,
        cardinality: Cardinality,
        presence: Presence,
    ) -> Self {
        let name = name.into();
        Self {
            json_name: name.clone(),
            name,
            number,
            field_type,
            cardinality,
            presence,
            packed: cardinality == Cardinality::Repeated && field_type.is_packable(),
            type_name: None,
            is_map: false,
            message: None,
            enum_ty: None,
            oneof_index: None,
            utf8_validate: true,
            default: None,
            extendee: None,
            delimited: field_type == FieldType::Group,
            extension_name: None,
            options: Vec::new(),
        }
    }

    /// Custom `FieldOptions` tag payload, if present.
    pub fn custom_option(&self, number: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|o| o.number == number)
            .map(|o| o.value.as_slice())
    }
}

#[derive(Clone, Debug)]
pub struct MessageDescriptor {
    pub name: String,
    pub full_name: String,
    pub fields: BTreeMap<u32, FieldDescriptor>,
    pub fields_by_name: BTreeMap<String, u32>,
    pub is_map_entry: bool,
    pub oneofs: Vec<Vec<u32>>,
    pub fields_by_json_name: BTreeMap<String, u32>,
    pub extension_ranges: Vec<(u32, u32)>,
    pub reserved_names: BTreeSet<String>,
    pub file_name: String,
    pub message_set_wire_format: bool,
    /// Unrecognized `MessageOptions` tags (custom options).
    pub options: Vec<DescriptorOption>,
}

impl MessageDescriptor {
    pub fn builder(full_name: impl Into<String>) -> MessageDescriptorBuilder {
        let full_name = full_name.into();
        let name = full_name
            .rsplit('.')
            .next()
            .unwrap_or(full_name.as_str())
            .to_string();
        MessageDescriptorBuilder {
            desc: MessageDescriptor {
                name,
                full_name,
                fields: BTreeMap::new(),
                fields_by_name: BTreeMap::new(),
                is_map_entry: false,
                oneofs: Vec::new(),
                fields_by_json_name: BTreeMap::new(),
                extension_ranges: Vec::new(),
                reserved_names: BTreeSet::new(),
                file_name: String::new(),
                message_set_wire_format: false,
                options: Vec::new(),
            },
        }
    }

    pub fn field(&self, number: u32) -> Option<&FieldDescriptor> {
        self.fields.get(&number)
    }

    pub fn field_by_name(&self, name: &str) -> Option<&FieldDescriptor> {
        let name = name
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(name);
        self.fields_by_name
            .get(name)
            .or_else(|| self.fields_by_json_name.get(name))
            .or_else(|| self.fields_by_name.get(&name.to_ascii_lowercase()))
            .or_else(|| {
                name.rsplit('.').next().and_then(|short| {
                    self.fields_by_name
                        .get(short)
                        .or_else(|| self.fields_by_name.get(&short.to_ascii_lowercase()))
                })
            })
            .and_then(|n| self.fields.get(n))
    }

    pub fn is_reserved_name(&self, name: &str) -> bool {
        self.reserved_names.contains(name)
    }

    /// Custom `MessageOptions` tag payload, if present.
    pub fn custom_option(&self, number: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|o| o.number == number)
            .map(|o| o.value.as_slice())
    }
}

/// One custom option on a file, message, field, enum, or method descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DescriptorOption {
    pub number: u32,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct EnumDescriptor {
    pub name: String,
    pub full_name: String,
    pub file_name: String,
    pub values: BTreeMap<i32, String>,
    pub names: BTreeMap<String, i32>,
    /// Proto-order (number, original name). First entry is the default.
    pub listed: Vec<(i32, String)>,
    pub closed: bool,
    /// Unrecognized `EnumOptions` tags (custom options).
    pub options: Vec<DescriptorOption>,
}

impl EnumDescriptor {
    /// Custom `EnumOptions` tag payload, if present.
    pub fn custom_option(&self, number: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|o| o.number == number)
            .map(|o| o.value.as_slice())
    }
}

pub struct MessageDescriptorBuilder {
    desc: MessageDescriptor,
}

impl MessageDescriptorBuilder {
    pub fn field(mut self, field: FieldDescriptor) -> Self {
        self.desc
            .fields_by_name
            .insert(field.name.clone(), field.number);
        self.desc
            .fields_by_json_name
            .insert(field.json_name.clone(), field.number);
        self.desc.fields.insert(field.number, field);
        self
    }

    pub fn map_entry(mut self, yes: bool) -> Self {
        self.desc.is_map_entry = yes;
        self
    }

    pub fn build(self) -> MessageDescriptor {
        self.desc
    }
}

#[derive(Clone, Debug, Default)]
pub struct MethodDescriptor {
    pub name: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    /// Unrecognized `MethodOptions` tags (custom options).
    pub options: Vec<DescriptorOption>,
}

impl MethodDescriptor {
    /// Custom `MethodOptions` tag payload, if present.
    pub fn custom_option(&self, number: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|o| o.number == number)
            .map(|o| o.value.as_slice())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ServiceDescriptor {
    pub name: String,
    pub full_name: String,
    pub file_name: String,
    pub methods: Vec<MethodDescriptor>,
}

#[derive(Clone, Debug, Default)]
pub struct FileDescriptor {
    pub name: String,
    pub package: String,
    /// Unrecognized `FileOptions` tags (custom options).
    pub options: Vec<DescriptorOption>,
}

impl FileDescriptor {
    /// Custom `FileOptions` tag payload, if present.
    pub fn custom_option(&self, number: u32) -> Option<&[u8]> {
        self.options
            .iter()
            .find(|o| o.number == number)
            .map(|o| o.value.as_slice())
    }
}

#[derive(Clone, Debug, Default)]
pub struct DescriptorPool {
    messages: BTreeMap<String, Arc<MessageDescriptor>>,
    enums: BTreeMap<String, Arc<EnumDescriptor>>,
    extensions_by_name: BTreeMap<String, (String, u32)>,
    services: BTreeMap<String, Arc<ServiceDescriptor>>,
    files: BTreeMap<String, Arc<FileDescriptor>>,
    /// file name -> public import file names
    public_imports: BTreeMap<String, Vec<String>>,
}

impl DescriptorPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn collect_names(&self) -> Vec<String> {
        self.messages.keys().cloned().collect()
    }

    pub fn get_message(&self, full_name: &str) -> Option<Arc<MessageDescriptor>> {
        self.messages
            .get(full_name.trim_start_matches('.'))
            .cloned()
    }

    pub fn get_enum(&self, full_name: &str) -> Option<Arc<EnumDescriptor>> {
        self.enums.get(full_name.trim_start_matches('.')).cloned()
    }

    pub(crate) fn collect_enum_names(&self) -> Vec<String> {
        self.enums.keys().cloned().collect()
    }

    pub(crate) fn public_import_files(&self, targets: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for t in targets {
            for (file, pubs) in &self.public_imports {
                if file_name_matches(t, file) {
                    for p in pubs {
                        if !out.contains(p) {
                            out.push(p.clone());
                        }
                    }
                }
            }
        }
        out
    }

    /// Look up an extension by its full name (`package.field`).
    pub fn get_extension(
        &self,
        full_name: &str,
    ) -> Option<(Arc<MessageDescriptor>, FieldDescriptor)> {
        let key = full_name.trim_start_matches('.');
        let (extendee, number) = self.extensions_by_name.get(key)?;
        let desc = self.get_message(extendee)?;
        let field = desc.field(*number)?.clone();
        Some((desc, field))
    }

    pub fn register_message(&mut self, desc: MessageDescriptor) -> Arc<MessageDescriptor> {
        let key = desc.full_name.clone();
        let arc = Arc::new(desc);
        self.messages.insert(key, arc.clone());
        arc
    }

    pub fn get_service(&self, full_name: &str) -> Option<Arc<ServiceDescriptor>> {
        self.services
            .get(full_name.trim_start_matches('.'))
            .cloned()
    }

    pub fn get_file(&self, name: &str) -> Option<Arc<FileDescriptor>> {
        self.files.get(name).cloned().or_else(|| {
            self.files
                .values()
                .find(|f| file_name_matches(name, &f.name))
                .cloned()
        })
    }

    pub fn collect_services(&self) -> Vec<Arc<ServiceDescriptor>> {
        self.services.values().cloned().collect()
    }

    pub fn register_enum(&mut self, desc: EnumDescriptor) -> Arc<EnumDescriptor> {
        let key = desc.full_name.clone();
        let arc = Arc::new(desc);
        self.enums.insert(key, arc.clone());
        arc
    }

    /// Parse a serialized `google.protobuf.FileDescriptorSet`.
    pub fn from_file_descriptor_set(bytes: &[u8]) -> Result<Self, ParseError> {
        let files = parse_file_descriptor_set(bytes)?;
        let mut raw: BTreeMap<String, RawMessage> = BTreeMap::new();
        let mut raw_enums: BTreeMap<String, RawEnum> = BTreeMap::new();
        let mut extensions = Vec::new();
        for file in &files {
            collect_raw(file, &mut raw, &mut raw_enums, &mut extensions);
        }
        let mut pool = resolve_pool(raw, raw_enums, extensions);
        for file in &files {
            pool.files.insert(
                file.name.clone(),
                Arc::new(FileDescriptor {
                    name: file.name.clone(),
                    package: file.package.clone(),
                    options: file.options.clone(),
                }),
            );
            let pubs: Vec<String> = file
                .public_dependency
                .iter()
                .filter_map(|&i| file.dependencies.get(i as usize).cloned())
                .collect();
            if !pubs.is_empty() {
                pool.public_imports.insert(file.name.clone(), pubs);
            }
            for svc in &file.services {
                let desc = Arc::new(ServiceDescriptor {
                    name: svc.name.clone(),
                    full_name: svc.full_name.clone(),
                    file_name: file.name.clone(),
                    methods: svc.methods.clone(),
                });
                pool.services.insert(desc.full_name.clone(), desc);
            }
        }
        Ok(pool)
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Double(f64),
    Float(f32),
    Int32(i32),
    Int64(i64),
    Uint32(u32),
    Uint64(u64),
    Bool(bool),
    String(ProtoString),
    Bytes(ProtoBytes),
    Enum(i32),
    Message(DynamicMessage),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Int32(a), Self::Int32(b)) => a == b,
            (Self::Int64(a), Self::Int64(b)) => a == b,
            (Self::Uint32(a), Self::Uint32(b)) => a == b,
            (Self::Uint64(a), Self::Uint64(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Enum(a), Self::Enum(b)) => a == b,
            (Self::Message(a), Self::Message(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub(crate) fn is_implicit_default(&self) -> bool {
        match self {
            Self::Double(v) => v.to_bits() == 0f64.to_bits(),
            Self::Float(v) => v.to_bits() == 0f32.to_bits(),
            Self::Int32(v) | Self::Enum(v) => *v == 0,
            Self::Int64(v) => *v == 0,
            Self::Uint32(v) => *v == 0,
            Self::Uint64(v) => *v == 0,
            Self::Bool(v) => !*v,
            Self::String(v) => v.is_empty(),
            Self::Bytes(v) => v.is_empty(),
            Self::Message(_) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValue {
    Singular(Value),
    Repeated(Vec<Value>),
    Map(BTreeMap<MapKeyValue, Value>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MapKeyValue {
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    Bool(bool),
    String(ProtoString),
}

#[derive(Clone)]
pub struct DynamicMessage {
    desc: Arc<MessageDescriptor>,
    pool: Option<Arc<DescriptorPool>>,
    fields: BTreeMap<u32, FieldValue>,
    unknown: UnknownFields,
}

impl PartialEq for DynamicMessage {
    fn eq(&self, other: &Self) -> bool {
        self.desc.full_name == other.desc.full_name
            && self.fields == other.fields
            && self.unknown == other.unknown
    }
}

impl fmt::Debug for DynamicMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicMessage")
            .field("type", &self.desc.full_name)
            .field("fields", &self.fields)
            .finish()
    }
}

impl DynamicMessage {
    pub fn new(desc: Arc<MessageDescriptor>) -> Self {
        Self {
            desc,
            pool: None,
            fields: BTreeMap::new(),
            unknown: UnknownFields::default(),
        }
    }

    pub fn set_pool(&mut self, pool: Arc<DescriptorPool>) {
        self.pool = Some(pool);
    }

    pub fn pool(&self) -> Option<&Arc<DescriptorPool>> {
        self.pool.as_ref()
    }

    pub fn descriptor(&self) -> &Arc<MessageDescriptor> {
        &self.desc
    }

    pub(crate) fn raw_fields(&self) -> &BTreeMap<u32, FieldValue> {
        &self.fields
    }

    pub fn parse_with(desc: Arc<MessageDescriptor>, data: &[u8]) -> Result<Self, ParseError> {
        Self::parse_with_pool(desc, None, data)
    }

    pub fn parse_with_pool(
        desc: Arc<MessageDescriptor>,
        pool: Option<Arc<DescriptorPool>>,
        data: &[u8],
    ) -> Result<Self, ParseError> {
        Self::parse_with_pool_depth(desc, pool, data, 0, true)
    }

    pub(crate) fn parse_with_pool_depth(
        desc: Arc<MessageDescriptor>,
        pool: Option<Arc<DescriptorPool>>,
        data: &[u8],
        depth: u32,
        enforce_required: bool,
    ) -> Result<Self, ParseError> {
        let mut msg = Self::new(desc);
        msg.pool = pool;
        msg.merge_bytes(data, enforce_required, depth)?;
        Ok(msg)
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }

    pub fn has(&self, number: u32) -> bool {
        self.fields.contains_key(&number)
    }

    pub fn get_singular(&self, number: u32) -> Option<&Value> {
        match self.fields.get(&number) {
            Some(FieldValue::Singular(v)) => Some(v),
            _ => None,
        }
    }

    pub fn get_repeated(&self, number: u32) -> Option<&[Value]> {
        match self.fields.get(&number) {
            Some(FieldValue::Repeated(v)) => Some(v),
            _ => None,
        }
    }

    pub fn get_map(&self, number: u32) -> Option<&BTreeMap<MapKeyValue, Value>> {
        match self.fields.get(&number) {
            Some(FieldValue::Map(v)) => Some(v),
            _ => None,
        }
    }

    pub fn set(&mut self, number: u32, value: Value) {
        if let Some(field) = self.desc.field(number) {
            if let Some(idx) = field.oneof_index {
                if let Some(members) = self.desc.oneofs.get(idx as usize) {
                    for n in members.clone() {
                        if n != number {
                            self.fields.remove(&n);
                        }
                    }
                }
            }
        }
        self.fields.insert(number, FieldValue::Singular(value));
    }

    pub fn set_extension(&mut self, number: u32, value: Value) {
        self.set(number, value);
    }

    pub fn get_extension(&self, number: u32) -> Option<&Value> {
        self.get_singular(number)
    }

    pub fn has_extension(&self, number: u32) -> bool {
        self.has(number)
    }

    pub fn clear_extension(&mut self, number: u32) {
        self.clear_field(number);
    }

    pub fn to_json(&self) -> Result<String, SerializeError> {
        crate::json::encode(self)
    }

    pub fn from_json(desc: Arc<MessageDescriptor>, json: &str) -> Result<Self, ParseError> {
        crate::json::decode(desc, json, false, None)
    }

    pub fn from_json_ignore_unknown(
        desc: Arc<MessageDescriptor>,
        json: &str,
    ) -> Result<Self, ParseError> {
        crate::json::decode(desc, json, true, None)
    }

    pub fn from_json_with_pool(
        desc: Arc<MessageDescriptor>,
        pool: Option<Arc<DescriptorPool>>,
        json: &str,
        ignore_unknown: bool,
    ) -> Result<Self, ParseError> {
        crate::json::decode(desc, json, ignore_unknown, pool)
    }

    pub fn to_text(&self) -> Result<String, SerializeError> {
        crate::text::encode(self)
    }

    pub fn to_text_with_unknown(&self) -> Result<String, SerializeError> {
        crate::text::encode_with_unknown(self)
    }

    pub fn from_text(desc: Arc<MessageDescriptor>, text: &str) -> Result<Self, ParseError> {
        crate::text::decode(desc, text)
    }

    pub fn from_text_with_pool(
        desc: Arc<MessageDescriptor>,
        pool: Option<Arc<DescriptorPool>>,
        text: &str,
    ) -> Result<Self, ParseError> {
        crate::text::decode_with_pool(desc, text, pool)
    }

    pub fn push(&mut self, number: u32, value: Value) {
        match self.fields.entry(number) {
            std::collections::btree_map::Entry::Occupied(mut e) => match e.get_mut() {
                FieldValue::Repeated(v) => v.push(value),
                other => *other = FieldValue::Repeated(vec![value]),
            },
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(FieldValue::Repeated(vec![value]));
            }
        }
    }

    pub fn insert_map(&mut self, number: u32, key: MapKeyValue, value: Value) {
        match self.fields.entry(number) {
            std::collections::btree_map::Entry::Occupied(mut e) => match e.get_mut() {
                FieldValue::Map(m) => {
                    m.insert(key, value);
                }
                other => {
                    let mut m = BTreeMap::new();
                    m.insert(key, value);
                    *other = FieldValue::Map(m);
                }
            },
            std::collections::btree_map::Entry::Vacant(e) => {
                let mut m = BTreeMap::new();
                m.insert(key, value);
                e.insert(FieldValue::Map(m));
            }
        }
    }

    pub fn clear_field(&mut self, number: u32) {
        self.fields.remove(&number);
    }

    fn merge_bytes(
        &mut self,
        data: &[u8],
        enforce_required: bool,
        depth: u32,
    ) -> Result<(), ParseError> {
        if depth > RECURSION_LIMIT {
            return Err(ParseError::new("recursion limit exceeded"));
        }
        let mut pos = 0;
        while pos < data.len() {
            let (number, wire) = decode_tag(data, &mut pos)?;
            if self.desc.message_set_wire_format && number == 1 {
                self.merge_message_set_item(data, &mut pos, wire, depth)?;
                continue;
            }
            match self.desc.field(number).cloned() {
                None => {
                    self.unknown
                        .fields
                        .push(wire::capture_unknown(data, &mut pos, number, wire)?);
                }
                Some(field) => self.merge_field(&field, data, &mut pos, wire, depth)?,
            }
        }
        if enforce_required {
            for f in self.desc.fields.values() {
                if f.cardinality == Cardinality::Required && !self.fields.contains_key(&f.number) {
                    return Err(ParseError::new("missing required field"));
                }
            }
        }
        Ok(())
    }

    fn merge_field(
        &mut self,
        field: &FieldDescriptor,
        data: &[u8],
        pos: &mut usize,
        wire: u32,
        depth: u32,
    ) -> Result<(), ParseError> {
        let expected = if field.delimited {
            WIRE_SGROUP
        } else {
            field.field_type.default_wire()
        };
        let packed_ok = field.cardinality == Cardinality::Repeated
            && field.field_type.is_packable()
            && wire == WIRE_LEN;
        if wire != expected && !packed_ok && !(field.is_map && wire == WIRE_LEN) {
            self.unknown
                .fields
                .push(wire::capture_unknown(data, pos, field.number, wire)?);
            return Ok(());
        }
        if field.is_map {
            let payload = read_len_bytes(data, pos)?;
            let entry = field_message_desc(field, self.pool.as_ref())?;
            let (k, v) = decode_map_entry(&entry, payload, self.pool.clone(), depth)?;
            self.insert_map(field.number, k, v);
            return Ok(());
        }
        if field.cardinality == Cardinality::Repeated {
            if packed_ok {
                let payload = read_len_bytes(data, pos)?;
                let mut p = 0;
                let leaf_wire = field.field_type.default_wire();
                while p < payload.len() {
                    let v =
                        decode_leaf(field, payload, &mut p, leaf_wire, self.pool.clone(), depth)?;
                    self.push(field.number, v);
                }
                return Ok(());
            }
            let v = decode_leaf(field, data, pos, wire, self.pool.clone(), depth)?;
            if let Value::Enum(n) = v {
                if is_closed_unknown(field, n) {
                    self.unknown.fields.push(UnknownField::Varint {
                        number: field.number,
                        value: n as u64,
                    });
                    return Ok(());
                }
            }
            self.push(field.number, v);
            return Ok(());
        }
        let v = decode_leaf(field, data, pos, wire, self.pool.clone(), depth)?;
        if let Value::Enum(n) = &v {
            if is_closed_unknown(field, *n) {
                self.unknown.fields.push(UnknownField::Varint {
                    number: field.number,
                    value: *n as u64,
                });
                return Ok(());
            }
        }
        if let Value::Message(incoming) = v {
            match self.fields.get_mut(&field.number) {
                Some(FieldValue::Singular(Value::Message(existing))) => {
                    existing.merge_from_dyn(&incoming);
                }
                _ => {
                    self.set(field.number, Value::Message(incoming));
                }
            }
            return Ok(());
        }
        self.set(field.number, v);
        Ok(())
    }

    pub(crate) fn merge_from_dyn(&mut self, src: &DynamicMessage) {
        for (n, val) in &src.fields {
            match val {
                FieldValue::Singular(Value::Message(child)) => match self.fields.get_mut(n) {
                    Some(FieldValue::Singular(Value::Message(dst))) => dst.merge_from_dyn(child),
                    _ => {
                        self.fields
                            .insert(*n, FieldValue::Singular(Value::Message(child.clone())));
                    }
                },
                FieldValue::Repeated(items) => match self.fields.get_mut(n) {
                    Some(FieldValue::Repeated(dst)) => dst.extend(items.iter().cloned()),
                    _ => {
                        self.fields.insert(*n, FieldValue::Repeated(items.clone()));
                    }
                },
                FieldValue::Map(items) => match self.fields.get_mut(n) {
                    Some(FieldValue::Map(dst)) => {
                        dst.extend(items.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    _ => {
                        self.fields.insert(*n, FieldValue::Map(items.clone()));
                    }
                },
                other => {
                    self.fields.insert(*n, other.clone());
                }
            }
        }
        self.unknown
            .fields
            .extend(src.unknown.fields.iter().cloned());
    }

    fn compute_size(&self) -> u64 {
        let mut n = 0u64;
        for (number, val) in &self.fields {
            if let Some(field) = self.desc.field(*number) {
                n += field_value_size(field, val);
            } else {
                n += untyped_size(*number, val);
            }
        }
        n + self.unknown.encoded_len()
    }

    fn write_to(&self, out: &mut impl crate::wire::WireOut) {
        if self.desc.message_set_wire_format {
            self.write_message_set(out);
            self.unknown.encode(out);
            return;
        }
        for (number, val) in &self.fields {
            if let Some(field) = self.desc.field(*number) {
                write_field_value(field, val, out);
            } else {
                write_untyped(*number, val, out);
            }
        }
        self.unknown.encode(out);
    }

    fn merge_message_set_item(
        &mut self,
        data: &[u8],
        pos: &mut usize,
        wire: u32,
        depth: u32,
    ) -> Result<(), ParseError> {
        let mut type_id = 0u32;
        let mut payload = Vec::new();
        if wire == WIRE_LEN {
            let inner = read_len_bytes(data, pos)?;
            let mut p = 0;
            while p < inner.len() {
                let (n, w) = decode_tag(inner, &mut p)?;
                match (n, w) {
                    (2, WIRE_VARINT) => type_id = decode_varint(inner, &mut p)? as u32,
                    (3, WIRE_LEN) => payload = read_len_bytes(inner, &mut p)?.to_vec(),
                    _ => wire::skip_field(inner, &mut p, w)?,
                }
            }
        } else if wire == WIRE_SGROUP {
            loop {
                if *pos >= data.len() {
                    return Err(ParseError::new("truncated message set"));
                }
                let (n, w) = decode_tag(data, pos)?;
                if w == WIRE_EGROUP && n == 1 {
                    break;
                }
                match (n, w) {
                    (2, WIRE_VARINT) => type_id = decode_varint(data, pos)? as u32,
                    (3, WIRE_LEN) => payload = read_len_bytes(data, pos)?.to_vec(),
                    _ => self
                        .unknown
                        .fields
                        .push(wire::capture_unknown(data, pos, n, w)?),
                }
            }
        } else {
            self.unknown
                .fields
                .push(wire::capture_unknown(data, pos, 1, wire)?);
            return Ok(());
        }
        if type_id == 0 {
            return Ok(());
        }
        if let Some(field) = self.desc.field(type_id).cloned() {
            let mut inner = DynamicMessage::new(field_message_desc(&field, self.pool.as_ref())?);
            if let Some(p) = self.pool.clone() {
                inner.set_pool(p);
            }
            inner.merge_bytes(&payload, false, depth + 1)?;
            self.set(type_id, Value::Message(inner));
        } else {
            self.unknown.fields.push(UnknownField::Group {
                number: 1,
                fields: {
                    let mut u = UnknownFields::default();
                    u.fields.push(UnknownField::Varint {
                        number: 2,
                        value: u64::from(type_id),
                    });
                    u.fields.push(UnknownField::LengthDelimited {
                        number: 3,
                        value: payload,
                    });
                    u
                },
            });
        }
        Ok(())
    }

    fn write_message_set(&self, out: &mut impl crate::wire::WireOut) {
        for (number, val) in &self.fields {
            let FieldValue::Singular(Value::Message(m)) = val else {
                continue;
            };
            let mut inner = Vec::new();
            m.write_to(&mut inner);
            encode_tag(out, 1, WIRE_SGROUP);
            encode_tag(out, 2, WIRE_VARINT);
            encode_varint(out, u64::from(*number));
            encode_len_field(out, 3, &inner);
            encode_tag(out, 1, WIRE_EGROUP);
        }
    }
}

fn is_closed_unknown(field: &FieldDescriptor, n: i32) -> bool {
    field
        .enum_ty
        .as_ref()
        .is_some_and(|e| e.closed && !e.values.contains_key(&n))
}

fn field_message_desc(
    field: &FieldDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
) -> Result<Arc<MessageDescriptor>, ParseError> {
    if let (Some(tn), Some(pool)) = (field.type_name.as_deref(), pool) {
        if let Some(m) = pool.get_message(tn.trim_start_matches('.')) {
            return Ok(m);
        }
    }
    if let Some(m) = &field.message {
        return Ok(m.clone());
    }
    Err(ParseError::new("unresolved message type"))
}

fn decode_leaf(
    field: &FieldDescriptor,
    data: &[u8],
    pos: &mut usize,
    wire: u32,
    pool: Option<Arc<DescriptorPool>>,
    depth: u32,
) -> Result<Value, ParseError> {
    if field.delimited || field.field_type == FieldType::Group {
        return decode_group(field, data, pos, pool, depth);
    }
    match field.field_type {
        FieldType::Message => {
            if wire != WIRE_LEN {
                return Err(ParseError::new("bad wire type for message"));
            }
            let payload = read_len_bytes(data, pos)?;
            let desc = field_message_desc(field, pool.as_ref())?;
            Ok(Value::Message(DynamicMessage::parse_with_pool_depth(
                desc,
                pool,
                payload,
                depth + 1,
                true,
            )?))
        }
        FieldType::Group => decode_group(field, data, pos, pool, depth),
        FieldType::String => {
            if wire != WIRE_LEN {
                return Err(ParseError::new("bad wire type for string"));
            }
            let bytes = read_len_bytes(data, pos)?;
            if field.utf8_validate {
                std::str::from_utf8(bytes).map_err(|_| ParseError::new("invalid utf-8"))?;
            }
            Ok(Value::String(ProtoString::from_bytes(bytes)))
        }
        FieldType::Bytes => {
            if wire != WIRE_LEN {
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
        FieldType::Sint32 => Ok(Value::Int32(wire::decode_zigzag32(decode_varint(
            data, pos,
        )?))),
        FieldType::Sint64 => Ok(Value::Int64(wire::decode_zigzag64(decode_varint(
            data, pos,
        )?))),
        FieldType::Enum => Ok(Value::Enum(decode_varint(data, pos)? as i32)),
    }
}

fn decode_group(
    field: &FieldDescriptor,
    data: &[u8],
    pos: &mut usize,
    pool: Option<Arc<DescriptorPool>>,
    depth: u32,
) -> Result<Value, ParseError> {
    if depth + 1 > RECURSION_LIMIT {
        return Err(ParseError::new("recursion limit exceeded"));
    }
    let desc = field_message_desc(field, pool.as_ref())?;
    let mut msg = DynamicMessage::new(desc);
    msg.pool = pool;
    loop {
        if *pos >= data.len() {
            return Err(ParseError::new("truncated group"));
        }
        let (n, w) = decode_tag(data, pos)?;
        if w == WIRE_EGROUP {
            if n != field.number {
                return Err(ParseError::new("mismatched end-group"));
            }
            break;
        }
        match msg.desc.field(n).cloned() {
            None => msg
                .unknown
                .fields
                .push(wire::capture_unknown(data, pos, n, w)?),
            Some(f) => msg.merge_field(&f, data, pos, w, depth + 1)?,
        }
    }
    Ok(Value::Message(msg))
}

fn decode_map_entry(
    entry: &MessageDescriptor,
    payload: &[u8],
    pool: Option<Arc<DescriptorPool>>,
    depth: u32,
) -> Result<(MapKeyValue, Value), ParseError> {
    let msg = DynamicMessage::parse_with_pool_depth(
        Arc::new(entry.clone()),
        pool.clone(),
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
    let key = match msg.get_singular(1) {
        Some(v) => value_to_map_key(v)?,
        None => default_map_key(key_field.field_type)?,
    };
    let value = match msg.get_singular(2) {
        Some(v) => v.clone(),
        None => default_value(val_field, pool.as_ref())?,
    };
    Ok((key, value))
}

fn value_to_map_key(v: &Value) -> Result<MapKeyValue, ParseError> {
    Ok(match v {
        Value::Int32(n) => MapKeyValue::I32(*n),
        Value::Int64(n) => MapKeyValue::I64(*n),
        Value::Uint32(n) => MapKeyValue::U32(*n),
        Value::Uint64(n) => MapKeyValue::U64(*n),
        Value::Bool(n) => MapKeyValue::Bool(*n),
        Value::String(s) => MapKeyValue::String(s.clone()),
        _ => return Err(ParseError::new("invalid map key type")),
    })
}

fn default_map_key(ty: FieldType) -> Result<MapKeyValue, ParseError> {
    Ok(match ty {
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => MapKeyValue::I32(0),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => MapKeyValue::I64(0),
        FieldType::Uint32 | FieldType::Fixed32 => MapKeyValue::U32(0),
        FieldType::Uint64 | FieldType::Fixed64 => MapKeyValue::U64(0),
        FieldType::Bool => MapKeyValue::Bool(false),
        FieldType::String => MapKeyValue::String(ProtoString::new()),
        _ => return Err(ParseError::new("invalid map key type")),
    })
}

fn default_value(
    field: &FieldDescriptor,
    pool: Option<&Arc<DescriptorPool>>,
) -> Result<Value, ParseError> {
    Ok(match field.field_type {
        FieldType::Double => Value::Double(0.0),
        FieldType::Float => Value::Float(0.0),
        FieldType::Int32 | FieldType::Sint32 | FieldType::Sfixed32 => Value::Int32(0),
        FieldType::Int64 | FieldType::Sint64 | FieldType::Sfixed64 => Value::Int64(0),
        FieldType::Uint32 | FieldType::Fixed32 => Value::Uint32(0),
        FieldType::Uint64 | FieldType::Fixed64 => Value::Uint64(0),
        FieldType::Bool => Value::Bool(false),
        FieldType::String => Value::String(ProtoString::new()),
        FieldType::Bytes => Value::Bytes(ProtoBytes::new()),
        FieldType::Enum => Value::Enum(0),
        FieldType::Message | FieldType::Group => {
            let desc = field_message_desc(field, pool)?;
            Value::Message(DynamicMessage::new(desc))
        }
    })
}

fn synthetic(number: u32, v: &Value) -> FieldDescriptor {
    let ty = match v {
        Value::Double(_) => FieldType::Double,
        Value::Float(_) => FieldType::Float,
        Value::Int32(_) => FieldType::Int32,
        Value::Int64(_) => FieldType::Int64,
        Value::Uint32(_) => FieldType::Uint32,
        Value::Uint64(_) => FieldType::Uint64,
        Value::Bool(_) => FieldType::Bool,
        Value::String(_) => FieldType::String,
        Value::Bytes(_) => FieldType::Bytes,
        Value::Enum(_) => FieldType::Enum,
        Value::Message(_) => FieldType::Message,
    };
    let mut f = FieldDescriptor::new("ext", number, ty, Cardinality::Optional, Presence::Explicit);
    if let Value::Message(m) = v {
        f.message = Some(m.descriptor().clone());
    }
    f
}

fn untyped_size(number: u32, val: &FieldValue) -> u64 {
    match val {
        FieldValue::Singular(v) => field_value_size(&synthetic(number, v), val),
        FieldValue::Repeated(items) => items
            .iter()
            .map(|v| field_value_size(&synthetic(number, v), &FieldValue::Singular(v.clone())))
            .sum(),
        FieldValue::Map(_) => 0,
    }
}

fn write_untyped(number: u32, val: &FieldValue, out: &mut impl crate::wire::WireOut) {
    match val {
        FieldValue::Singular(v) => write_field_value(&synthetic(number, v), val, out),
        FieldValue::Repeated(items) => {
            for v in items {
                write_field_value(&synthetic(number, v), &FieldValue::Singular(v.clone()), out);
            }
        }
        FieldValue::Map(_) => {}
    }
}

fn field_value_size(field: &FieldDescriptor, val: &FieldValue) -> u64 {
    match val {
        FieldValue::Singular(v) => {
            if field.presence == Presence::Implicit && v.is_implicit_default() {
                0
            } else {
                scalar_size(field, v)
            }
        }
        FieldValue::Repeated(items) => {
            if field.packed && field.field_type.is_packable() {
                let payload: u64 = items
                    .iter()
                    .map(|v| packed_scalar_len(field.field_type, v))
                    .sum();
                if payload == 0 {
                    0
                } else {
                    key_len_value_len(field.number, payload)
                }
            } else {
                items.iter().map(|v| scalar_size(field, v)).sum()
            }
        }
        FieldValue::Map(items) => items
            .iter()
            .map(|(k, v)| key_len_value_len(field.number, map_entry_len(field, k, v)))
            .sum(),
    }
}

fn write_field_value(
    field: &FieldDescriptor,
    val: &FieldValue,
    out: &mut impl crate::wire::WireOut,
) {
    match val {
        FieldValue::Singular(v) => {
            if field.presence == Presence::Implicit && v.is_implicit_default() {
                return;
            }
            write_scalar(field, v, out);
        }
        FieldValue::Repeated(items) => {
            if field.packed && field.field_type.is_packable() {
                let mut payload = Vec::new();
                for v in items {
                    write_packed_scalar(field.field_type, v, &mut payload);
                }
                if !payload.is_empty() {
                    encode_len_field(out, field.number, &payload);
                }
            } else {
                for v in items {
                    write_scalar(field, v, out);
                }
            }
        }
        FieldValue::Map(items) => {
            for (k, v) in items {
                let mut payload = Vec::new();
                write_map_entry(field, k, v, &mut payload);
                encode_len_field(out, field.number, &payload);
            }
        }
    }
}

fn map_entry_len(field: &FieldDescriptor, key: &MapKeyValue, value: &Value) -> u64 {
    let entry = match field.message.as_ref() {
        Some(e) => e,
        None => return 0,
    };
    let mut n = 0u64;
    if let Some(kf) = entry.field(1) {
        n += scalar_size(kf, &map_key_to_value(key));
    }
    if let Some(vf) = entry.field(2) {
        n += scalar_size(vf, value);
    }
    n
}

fn write_map_entry(
    field: &FieldDescriptor,
    key: &MapKeyValue,
    value: &Value,
    out: &mut impl crate::wire::WireOut,
) {
    let kv = map_key_to_value(key);
    let mut kf = field
        .message
        .as_ref()
        .and_then(|e| e.field(1).cloned())
        .unwrap_or_else(|| synthetic(1, &kv));
    kf.delimited = false;
    write_scalar(&kf, &kv, out);
    let mut vf = field
        .message
        .as_ref()
        .and_then(|e| e.field(2).cloned())
        .unwrap_or_else(|| synthetic(2, value));
    vf.delimited = false;
    write_scalar(&vf, value, out);
}

fn map_key_to_value(key: &MapKeyValue) -> Value {
    match key {
        MapKeyValue::I32(n) => Value::Int32(*n),
        MapKeyValue::I64(n) => Value::Int64(*n),
        MapKeyValue::U32(n) => Value::Uint32(*n),
        MapKeyValue::U64(n) => Value::Uint64(*n),
        MapKeyValue::Bool(n) => Value::Bool(*n),
        MapKeyValue::String(s) => Value::String(s.clone()),
    }
}

fn scalar_size(field: &FieldDescriptor, v: &Value) -> u64 {
    match v {
        Value::Message(m) if field.delimited || field.field_type == FieldType::Group => {
            tag_len(field.number, WIRE_SGROUP)
                + m.compute_size()
                + tag_len(field.number, WIRE_EGROUP)
        }
        Value::Message(m) => key_len_value_len(field.number, m.compute_size()),
        Value::String(s) => key_len_value_len(field.number, s.as_bytes().len() as u64),
        Value::Bytes(b) => key_len_value_len(field.number, b.as_bytes().len() as u64),
        other => {
            tag_len(field.number, field.field_type.default_wire())
                + packed_scalar_len(field.field_type, other)
        }
    }
}

fn packed_scalar_len(ty: FieldType, v: &Value) -> u64 {
    match (ty, v) {
        (FieldType::Double | FieldType::Fixed64 | FieldType::Sfixed64, _) => 8,
        (FieldType::Float | FieldType::Fixed32 | FieldType::Sfixed32, _) => 4,
        (FieldType::Bool, Value::Bool(b)) => varint_len(u64::from(*b)),
        (FieldType::Int32, Value::Int32(n)) => varint_len(*n as u64),
        (FieldType::Int64, Value::Int64(n)) => varint_len(*n as u64),
        (FieldType::Uint32, Value::Uint32(n)) => varint_len(u64::from(*n)),
        (FieldType::Uint64, Value::Uint64(n)) => varint_len(*n),
        (FieldType::Sint32, Value::Int32(n)) => varint_len(encode_zigzag32(*n)),
        (FieldType::Sint64, Value::Int64(n)) => varint_len(encode_zigzag64(*n)),
        (FieldType::Enum, Value::Enum(n)) => varint_len(*n as u64),
        (FieldType::String, Value::String(s)) => {
            varint_len(s.as_bytes().len() as u64) + s.as_bytes().len() as u64
        }
        (FieldType::Bytes, Value::Bytes(b)) => {
            varint_len(b.as_bytes().len() as u64) + b.as_bytes().len() as u64
        }
        _ => 0,
    }
}

fn write_scalar(field: &FieldDescriptor, v: &Value, out: &mut impl crate::wire::WireOut) {
    match v {
        Value::Message(m) if field.delimited || field.field_type == FieldType::Group => {
            encode_tag(out, field.number, WIRE_SGROUP);
            m.write_to(out);
            encode_tag(out, field.number, WIRE_EGROUP);
        }
        Value::Message(m) => {
            let mut inner = Vec::new();
            m.write_to(&mut inner);
            encode_len_field(out, field.number, &inner);
        }
        Value::String(s) => encode_len_field(out, field.number, s.as_bytes()),
        Value::Bytes(b) => encode_len_field(out, field.number, b.as_bytes()),
        _ => {
            encode_tag(out, field.number, field.field_type.default_wire());
            write_packed_scalar(field.field_type, v, out);
        }
    }
}

fn write_packed_scalar(ty: FieldType, v: &Value, out: &mut impl crate::wire::WireOut) {
    match (ty, v) {
        (FieldType::Double, Value::Double(n)) => out.extend_from_slice(&n.to_bits().to_le_bytes()),
        (FieldType::Float, Value::Float(n)) => out.extend_from_slice(&n.to_bits().to_le_bytes()),
        (FieldType::Fixed64, Value::Uint64(n)) => out.extend_from_slice(&n.to_le_bytes()),
        (FieldType::Sfixed64, Value::Int64(n)) => out.extend_from_slice(&(*n as u64).to_le_bytes()),
        (FieldType::Fixed32, Value::Uint32(n)) => out.extend_from_slice(&n.to_le_bytes()),
        (FieldType::Sfixed32, Value::Int32(n)) => out.extend_from_slice(&(*n as u32).to_le_bytes()),
        (FieldType::Bool, Value::Bool(b)) => encode_varint(out, u64::from(*b)),
        (FieldType::Int32, Value::Int32(n)) => encode_varint(out, *n as u64),
        (FieldType::Int64, Value::Int64(n)) => encode_varint(out, *n as u64),
        (FieldType::Uint32, Value::Uint32(n)) => encode_varint(out, u64::from(*n)),
        (FieldType::Uint64, Value::Uint64(n)) => encode_varint(out, *n),
        (FieldType::Sint32, Value::Int32(n)) => encode_varint(out, encode_zigzag32(*n)),
        (FieldType::Sint64, Value::Int64(n)) => encode_varint(out, encode_zigzag64(*n)),
        (FieldType::Enum, Value::Enum(n)) => encode_varint(out, *n as u64),
        (FieldType::String, Value::String(s)) => {
            encode_varint(out, s.as_bytes().len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        (FieldType::Bytes, Value::Bytes(b)) => {
            encode_varint(out, b.as_bytes().len() as u64);
            out.extend_from_slice(b.as_bytes());
        }
        _ => {}
    }
}

// --- Message trait impls ----------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DynamicMessageView<'msg> {
    inner: &'msg DynamicMessage,
}

impl Default for DynamicMessageView<'_> {
    fn default() -> Self {
        Self {
            inner: crate::gen_support::default_instance_of::<DynamicMessage>(),
        }
    }
}

pub struct DynamicMessageMut<'msg> {
    inner: &'msg mut DynamicMessage,
}

impl SealedInternal for DynamicMessage {}
impl MessageType for DynamicMessage {}
impl Proxied for DynamicMessage {
    type View<'msg> = DynamicMessageView<'msg>;
}
impl MutProxied for DynamicMessage {
    type Mut<'msg> = DynamicMessageMut<'msg>;
}
impl AsView for DynamicMessage {
    type Proxied = Self;
    fn as_view(&self) -> DynamicMessageView<'_> {
        DynamicMessageView { inner: self }
    }
}
impl AsMut for DynamicMessage {
    type MutProxied = Self;
    fn as_mut(&mut self) -> DynamicMessageMut<'_> {
        DynamicMessageMut { inner: self }
    }
}

impl SealedInternal for DynamicMessageView<'_> {}
impl AsView for DynamicMessageView<'_> {
    type Proxied = DynamicMessage;
    fn as_view(&self) -> DynamicMessageView<'_> {
        *self
    }
}
impl<'msg> IntoView<'msg> for DynamicMessageView<'msg> {
    fn into_view<'shorter>(self) -> DynamicMessageView<'shorter>
    where
        'msg: 'shorter,
    {
        DynamicMessageView { inner: self.inner }
    }
}

impl SealedInternal for DynamicMessageMut<'_> {}
impl AsView for DynamicMessageMut<'_> {
    type Proxied = DynamicMessage;
    fn as_view(&self) -> DynamicMessageView<'_> {
        DynamicMessageView { inner: self.inner }
    }
}
impl AsMut for DynamicMessageMut<'_> {
    type MutProxied = DynamicMessage;
    fn as_mut(&mut self) -> DynamicMessageMut<'_> {
        DynamicMessageMut { inner: self.inner }
    }
}
impl<'msg> IntoView<'msg> for DynamicMessageMut<'msg> {
    fn into_view<'shorter>(self) -> DynamicMessageView<'shorter>
    where
        'msg: 'shorter,
    {
        DynamicMessageView { inner: self.inner }
    }
}
impl<'msg> IntoMut<'msg> for DynamicMessageMut<'msg> {
    fn into_mut<'shorter>(self) -> DynamicMessageMut<'shorter>
    where
        'msg: 'shorter,
    {
        DynamicMessageMut { inner: self.inner }
    }
}

impl Serialize for DynamicMessage {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut out = Vec::new();
        self.write_to(&mut out);
        wire::check_size(out.len() as u64)?;
        Ok(out)
    }
    fn serialized_len(&self) -> usize {
        self.compute_size() as usize
    }
    fn encode(&self, out: &mut impl crate::wire::WireOut) -> Result<(), SerializeError> {
        wire::check_size(self.compute_size())?;
        self.write_to(out);
        Ok(())
    }
}

impl Serialize for DynamicMessageView<'_> {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        self.inner.serialize()
    }
    fn serialized_len(&self) -> usize {
        self.inner.serialized_len()
    }
    fn encode(&self, out: &mut impl crate::wire::WireOut) -> Result<(), SerializeError> {
        self.inner.encode(out)
    }
}

impl Serialize for DynamicMessageMut<'_> {
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        self.inner.serialize()
    }
    fn serialized_len(&self) -> usize {
        self.inner.serialized_len()
    }
    fn encode(&self, out: &mut impl crate::wire::WireOut) -> Result<(), SerializeError> {
        self.inner.encode(out)
    }
}

impl Clear for DynamicMessage {
    fn clear(&mut self) {
        self.fields.clear();
        self.unknown.clear();
    }
}
impl Clear for DynamicMessageMut<'_> {
    fn clear(&mut self) {
        self.inner.clear();
    }
}

impl ClearAndParse for DynamicMessage {
    fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.clear();
        self.merge_bytes(data, true, 0)
    }
    fn clear_and_parse_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.clear();
        self.merge_bytes(data, false, 0)
    }
    fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.merge_bytes(data, true, 0)
    }
    fn merge_from_bytes_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.merge_bytes(data, false, 0)
    }
}
impl ClearAndParse for DynamicMessageMut<'_> {
    fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.inner.clear_and_parse(data)
    }
    fn clear_and_parse_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.inner.clear_and_parse_dont_enforce_required(data)
    }
    fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        self.inner.merge_from_bytes(data)
    }
}

impl CopyFrom for DynamicMessage {
    fn copy_from(&mut self, src: impl AsView<Proxied = Self>) {
        let src = src.as_view();
        *self = src.inner.clone();
    }
}
impl CopyFrom for DynamicMessageMut<'_> {
    fn copy_from(&mut self, src: impl AsView<Proxied = DynamicMessage>) {
        let src = src.as_view();
        *self.inner = src.inner.clone();
    }
}

impl TakeFrom for DynamicMessage {
    fn take_from(&mut self, mut src: impl AsMut<MutProxied = Self>) {
        let src = src.as_mut();
        *self = std::mem::take(src.inner);
        // std::mem::take needs Default. Provide it.
    }
}

impl Default for DynamicMessage {
    fn default() -> Self {
        let desc = Arc::new(MessageDescriptor::builder("_Unset").build());
        Self::new(desc)
    }
}

impl TakeFrom for DynamicMessageMut<'_> {
    fn take_from(&mut self, mut src: impl AsMut<MutProxied = DynamicMessage>) {
        let src = src.as_mut();
        *self.inner = std::mem::take(src.inner);
    }
}

impl MergeFrom for DynamicMessage {
    fn merge_from(&mut self, src: impl AsView<Proxied = Self>) {
        let src = src.as_view();
        self.merge_from_dyn(src.inner);
    }
}
impl MergeFrom for DynamicMessageMut<'_> {
    fn merge_from(&mut self, src: impl AsView<Proxied = DynamicMessage>) {
        let src = src.as_view();
        self.inner.merge_from_dyn(src.inner);
    }
}

impl Message for DynamicMessage {
    type MessageView<'msg> = DynamicMessageView<'msg>;
    type MessageMut<'msg> = DynamicMessageMut<'msg>;
}
impl<'msg> MessageView<'msg> for DynamicMessageView<'msg> {
    type Message = DynamicMessage;
}
impl<'msg> MessageMut<'msg> for DynamicMessageMut<'msg> {
    type Message = DynamicMessage;
}

impl fmt::Debug for DynamicMessageMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.inner, f)
    }
}

// --- FileDescriptorSet bootstrap -------------------------------------------

#[derive(Clone, Copy, Default)]
struct RawFeatures {
    presence: u32,
    enum_type: u32,
    repeated_encoding: u32,
    utf8: u32,
    message_encoding: u32,
}

impl RawFeatures {
    fn merge(self, over: Self) -> Self {
        Self {
            presence: if over.presence != 0 {
                over.presence
            } else {
                self.presence
            },
            enum_type: if over.enum_type != 0 {
                over.enum_type
            } else {
                self.enum_type
            },
            repeated_encoding: if over.repeated_encoding != 0 {
                over.repeated_encoding
            } else {
                self.repeated_encoding
            },
            utf8: if over.utf8 != 0 { over.utf8 } else { self.utf8 },
            message_encoding: if over.message_encoding != 0 {
                over.message_encoding
            } else {
                self.message_encoding
            },
        }
    }
}

fn edition_defaults(syntax: &str, edition: i32) -> RawFeatures {
    if syntax == "proto3" || edition == 999 {
        RawFeatures {
            presence: 2,
            enum_type: 1,
            repeated_encoding: 1,
            utf8: 2,
            message_encoding: 1,
        }
    } else if syntax == "editions" || edition >= 1000 {
        RawFeatures {
            presence: 1,
            enum_type: 1,
            repeated_encoding: 1,
            utf8: 2,
            message_encoding: 1,
        }
    } else {
        RawFeatures {
            presence: 1,
            enum_type: 2,
            repeated_encoding: 2,
            utf8: 3,
            message_encoding: 1,
        }
    }
}

#[derive(Default)]
struct RawFile {
    name: String,
    package: String,
    syntax: String,
    edition: i32,
    features: RawFeatures,
    messages: Vec<RawMessage>,
    enums: Vec<RawEnum>,
    extensions: Vec<RawField>,
    services: Vec<RawService>,
    dependencies: Vec<String>,
    public_dependency: Vec<i32>,
    options: Vec<DescriptorOption>,
}

#[derive(Default, Clone)]
struct RawService {
    name: String,
    full_name: String,
    methods: Vec<MethodDescriptor>,
}

#[derive(Default, Clone)]
struct RawField {
    name: String,
    number: u32,
    label: i32,
    ty: i32,
    type_name: String,
    json_name: String,
    proto3_optional: bool,
    packed: Option<bool>,
    oneof_index: Option<u32>,
    default_value: Option<String>,
    extendee: String,
    features: RawFeatures,
    full_ext_name: String,
    options: Vec<DescriptorOption>,
}

#[derive(Default, Clone)]
struct RawMessage {
    name: String,
    full_name: String,
    fields: Vec<RawField>,
    nested: Vec<RawMessage>,
    is_map_entry: bool,
    syntax_proto3: bool,
    oneof_count: u32,
    enums: Vec<RawEnum>,
    features: RawFeatures,
    extensions: Vec<RawField>,
    extension_ranges: Vec<(u32, u32)>,
    reserved_names: Vec<String>,
    file_name: String,
    message_set_wire_format: bool,
    options: Vec<DescriptorOption>,
}

#[derive(Default, Clone)]
struct RawEnum {
    name: String,
    full_name: String,
    file_name: String,
    values: Vec<(i32, String)>,
    closed: bool,
    options: Vec<DescriptorOption>,
}

fn file_name_matches(wanted: &str, file_name: &str) -> bool {
    if wanted == file_name {
        return true;
    }
    let w = std::path::Path::new(wanted);
    let f = std::path::Path::new(file_name);
    w.file_name() == f.file_name() || wanted.ends_with(file_name) || file_name.ends_with(wanted)
}

fn parse_file_descriptor_set(bytes: &[u8]) -> Result<Vec<RawFile>, ParseError> {
    let mut files = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if n == 1 && w == WIRE_LEN {
            let payload = read_len_bytes(bytes, &mut pos)?;
            files.push(parse_file(payload)?);
        } else {
            wire::skip_field(bytes, &mut pos, w)?;
        }
    }
    Ok(files)
}

fn parse_file(bytes: &[u8]) -> Result<RawFile, ParseError> {
    let mut file = RawFile::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => file.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => file.package = read_string(bytes, &mut pos)?,
            (3, WIRE_LEN) => file.dependencies.push(read_string(bytes, &mut pos)?),
            (10, WIRE_VARINT) => file
                .public_dependency
                .push(decode_varint(bytes, &mut pos)? as i32),
            (4, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                file.messages.push(parse_descriptor(payload, "")?);
            }
            (5, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                file.enums.push(parse_enum(payload, false)?);
            }
            (6, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                file.services.push(parse_service(payload)?);
            }
            (7, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                file.extensions.push(parse_field(payload)?);
            }
            (8, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                let (features, options) = parse_file_options(payload)?;
                file.features = features;
                file.options = options;
            }
            (12, WIRE_LEN) => file.syntax = read_string(bytes, &mut pos)?,
            (14, WIRE_VARINT) => file.edition = decode_varint(bytes, &mut pos)? as i32,
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    let defaults = edition_defaults(&file.syntax, file.edition);
    file.features = defaults.merge(file.features);
    let proto3 = file.syntax == "proto3" || file.edition == 999;
    let closed = file.features.enum_type == 2;
    mark_syntax(&mut file.messages, proto3, file.features);
    prefix_names(&mut file.messages, &file.package);
    for e in &mut file.enums {
        e.closed = closed;
        e.file_name = file.name.clone();
        e.full_name = if file.package.is_empty() {
            e.name.clone()
        } else {
            format!("{}.{}", file.package, e.name)
        };
    }
    prefix_enums_in_messages(&mut file.messages, closed);
    for ext in &mut file.extensions {
        if ext.extendee.starts_with('.') {
            ext.extendee = ext.extendee.trim_start_matches('.').to_string();
        } else if !file.package.is_empty() && !ext.extendee.contains('.') {
            ext.extendee = format!("{}.{}", file.package, ext.extendee);
        }
        ext.full_ext_name = if file.package.is_empty() {
            ext.name.clone()
        } else {
            format!("{}.{}", file.package, ext.name)
        };
    }
    stamp_file(&mut file.messages, &file.name);
    for svc in &mut file.services {
        svc.full_name = if file.package.is_empty() {
            svc.name.clone()
        } else {
            format!("{}.{}", file.package, svc.name)
        };
        for m in &mut svc.methods {
            m.input_type = m.input_type.trim_start_matches('.').to_string();
            m.output_type = m.output_type.trim_start_matches('.').to_string();
        }
    }
    Ok(file)
}

fn parse_service(bytes: &[u8]) -> Result<RawService, ParseError> {
    let mut svc = RawService::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => svc.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                svc.methods.push(parse_method(payload)?);
            }
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(svc)
}

fn parse_method(bytes: &[u8]) -> Result<MethodDescriptor, ParseError> {
    let mut m = MethodDescriptor::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => m.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => m.input_type = read_string(bytes, &mut pos)?,
            (3, WIRE_LEN) => m.output_type = read_string(bytes, &mut pos)?,
            (5, WIRE_VARINT) => m.client_streaming = decode_varint(bytes, &mut pos)? != 0,
            (6, WIRE_VARINT) => m.server_streaming = decode_varint(bytes, &mut pos)? != 0,
            (4, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                m.options = parse_method_options(payload)?;
            }
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(m)
}

fn stamp_file(msgs: &mut [RawMessage], file_name: &str) {
    for m in msgs {
        m.file_name = file_name.to_string();
        stamp_file(&mut m.nested, file_name);
        for e in &mut m.enums {
            e.file_name = file_name.to_string();
        }
        for ext in &mut m.extensions {
            ext.full_ext_name = format!("{}.{}", m.full_name, ext.name);
        }
    }
}

fn prefix_enums_in_messages(msgs: &mut [RawMessage], closed: bool) {
    for m in msgs {
        for e in &mut m.enums {
            e.closed = closed;
            e.full_name = format!("{}.{}", m.full_name, e.name);
        }
        prefix_enums_in_messages(&mut m.nested, closed);
    }
}

fn parse_descriptor(bytes: &[u8], _parent: &str) -> Result<RawMessage, ParseError> {
    let mut msg = RawMessage::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => msg.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                msg.fields.push(parse_field(payload)?);
            }
            (3, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                msg.nested.push(parse_descriptor(payload, "")?);
            }
            (4, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                msg.enums.push(parse_enum(payload, false)?);
            }
            (5, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                if let Some(range) = parse_extension_range(payload)? {
                    msg.extension_ranges.push(range);
                }
            }
            (6, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                msg.extensions.push(parse_field(payload)?);
            }
            (8, WIRE_LEN) => {
                msg.oneof_count += 1;
                wire::skip_field(bytes, &mut pos, w)?;
            }
            (7, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                let (map_entry, message_set, features, options) = parse_message_options(payload)?;
                msg.is_map_entry = map_entry;
                msg.message_set_wire_format = message_set;
                msg.features = features;
                msg.options = options;
            }
            (10, WIRE_LEN) => msg.reserved_names.push(read_string(bytes, &mut pos)?),
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(msg)
}

fn parse_field(bytes: &[u8]) -> Result<RawField, ParseError> {
    let mut f = RawField::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => f.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => f.extendee = read_string(bytes, &mut pos)?,
            (3, WIRE_VARINT) => f.number = decode_varint(bytes, &mut pos)? as u32,
            (4, WIRE_VARINT) => f.label = decode_varint(bytes, &mut pos)? as i32,
            (5, WIRE_VARINT) => f.ty = decode_varint(bytes, &mut pos)? as i32,
            (6, WIRE_LEN) => f.type_name = read_string(bytes, &mut pos)?,
            (7, WIRE_LEN) => f.default_value = Some(read_string(bytes, &mut pos)?),
            (9, WIRE_VARINT) => f.oneof_index = Some(decode_varint(bytes, &mut pos)? as u32),
            (10, WIRE_LEN) => f.json_name = read_string(bytes, &mut pos)?,
            (8, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                let (packed, features, options) = parse_field_options(payload)?;
                f.packed = packed;
                f.features = features;
                f.options = options;
            }
            (17, WIRE_VARINT) => f.proto3_optional = decode_varint(bytes, &mut pos)? != 0,
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(f)
}

fn parse_message_options(
    bytes: &[u8],
) -> Result<(bool, bool, RawFeatures, Vec<DescriptorOption>), ParseError> {
    let mut map_entry = false;
    let mut message_set = false;
    let mut features = RawFeatures::default();
    let mut options = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_VARINT) => message_set = decode_varint(bytes, &mut pos)? != 0,
            (7, WIRE_VARINT) => map_entry = decode_varint(bytes, &mut pos)? != 0,
            (12, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                features = parse_features(payload)?;
            }
            _ => options.push(DescriptorOption {
                number: n,
                value: capture_option_value(bytes, &mut pos, w)?,
            }),
        }
    }
    Ok((map_entry, message_set, features, options))
}

fn parse_field_options(
    bytes: &[u8],
) -> Result<(Option<bool>, RawFeatures, Vec<DescriptorOption>), ParseError> {
    let mut packed = None;
    let mut features = RawFeatures::default();
    let mut options = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (2, WIRE_VARINT) => packed = Some(decode_varint(bytes, &mut pos)? != 0),
            (21, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                features = parse_features(payload)?;
            }
            _ => options.push(DescriptorOption {
                number: n,
                value: capture_option_value(bytes, &mut pos, w)?,
            }),
        }
    }
    Ok((packed, features, options))
}

fn capture_option_value(bytes: &[u8], pos: &mut usize, w: u32) -> Result<Vec<u8>, ParseError> {
    match w {
        WIRE_VARINT => {
            let v = decode_varint(bytes, pos)?;
            let mut out = Vec::new();
            encode_varint(&mut out, v);
            Ok(out)
        }
        WIRE_LEN => Ok(read_len_bytes(bytes, pos)?.to_vec()),
        WIRE_I32 => {
            let v = read_fixed32(bytes, pos)?;
            Ok(v.to_le_bytes().to_vec())
        }
        WIRE_I64 => {
            let v = read_fixed64(bytes, pos)?;
            Ok(v.to_le_bytes().to_vec())
        }
        _ => {
            wire::skip_field(bytes, pos, w)?;
            Ok(Vec::new())
        }
    }
}

fn parse_file_options(bytes: &[u8]) -> Result<(RawFeatures, Vec<DescriptorOption>), ParseError> {
    let mut features = RawFeatures::default();
    let mut options = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if n == 50 && w == WIRE_LEN {
            let payload = read_len_bytes(bytes, &mut pos)?;
            features = parse_features(payload)?;
        } else {
            options.push(DescriptorOption {
                number: n,
                value: capture_option_value(bytes, &mut pos, w)?,
            });
        }
    }
    Ok((features, options))
}

fn parse_enum_options(bytes: &[u8]) -> Result<Vec<DescriptorOption>, ParseError> {
    let mut options = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if n == 7 && w == WIRE_LEN {
            wire::skip_field(bytes, &mut pos, w)?;
        } else {
            options.push(DescriptorOption {
                number: n,
                value: capture_option_value(bytes, &mut pos, w)?,
            });
        }
    }
    Ok(options)
}

fn parse_method_options(bytes: &[u8]) -> Result<Vec<DescriptorOption>, ParseError> {
    let mut options = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if n == 35 && w == WIRE_LEN {
            wire::skip_field(bytes, &mut pos, w)?;
        } else {
            options.push(DescriptorOption {
                number: n,
                value: capture_option_value(bytes, &mut pos, w)?,
            });
        }
    }
    Ok(options)
}

fn parse_features(bytes: &[u8]) -> Result<RawFeatures, ParseError> {
    let mut f = RawFeatures::default();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if w == WIRE_VARINT {
            let v = decode_varint(bytes, &mut pos)? as u32;
            match n {
                1 => f.presence = v,
                2 => f.enum_type = v,
                3 => f.repeated_encoding = v,
                4 => f.utf8 = v,
                5 => f.message_encoding = v,
                _ => {}
            }
        } else {
            wire::skip_field(bytes, &mut pos, w)?;
        }
    }
    Ok(f)
}

fn parse_extension_range(bytes: &[u8]) -> Result<Option<(u32, u32)>, ParseError> {
    let mut start = 0u32;
    let mut end = 0u32;
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_VARINT) => start = decode_varint(bytes, &mut pos)? as u32,
            (2, WIRE_VARINT) => end = decode_varint(bytes, &mut pos)? as u32,
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(if end > start {
        Some((start, end))
    } else {
        None
    })
}

fn parse_enum(bytes: &[u8], closed: bool) -> Result<RawEnum, ParseError> {
    let mut e = RawEnum {
        closed,
        ..RawEnum::default()
    };
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => e.name = read_string(bytes, &mut pos)?,
            (2, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                e.values.push(parse_enum_value(payload)?);
            }
            (3, WIRE_LEN) => {
                let payload = read_len_bytes(bytes, &mut pos)?;
                e.options = parse_enum_options(payload)?;
            }
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok(e)
}

fn parse_enum_value(bytes: &[u8]) -> Result<(i32, String), ParseError> {
    let mut name = String::new();
    let mut number = 0i32;
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => name = read_string(bytes, &mut pos)?,
            (2, WIRE_VARINT) => number = decode_varint(bytes, &mut pos)? as i32,
            _ => wire::skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok((number, name))
}

fn read_string(bytes: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    let b = read_len_bytes(bytes, pos)?;
    Ok(String::from_utf8_lossy(b).into_owned())
}

fn mark_syntax(msgs: &mut [RawMessage], proto3: bool, features: RawFeatures) {
    for m in msgs {
        m.syntax_proto3 = proto3;
        m.features = features.merge(m.features);
        mark_syntax(&mut m.nested, proto3, m.features);
    }
}

fn prefix_names(msgs: &mut [RawMessage], package: &str) {
    for m in msgs {
        m.full_name = if package.is_empty() {
            m.name.clone()
        } else {
            format!("{package}.{}", m.name)
        };
        prefix_names(&mut m.nested, &m.full_name);
    }
}

fn collect_raw(
    file: &RawFile,
    out: &mut BTreeMap<String, RawMessage>,
    enums: &mut BTreeMap<String, RawEnum>,
    extensions: &mut Vec<(RawFeatures, RawField)>,
) {
    fn walk(
        m: &RawMessage,
        out: &mut BTreeMap<String, RawMessage>,
        enums: &mut BTreeMap<String, RawEnum>,
        extensions: &mut Vec<(RawFeatures, RawField)>,
    ) {
        out.insert(m.full_name.clone(), m.clone());
        for e in &m.enums {
            enums.insert(e.full_name.clone(), e.clone());
        }
        for ext in &m.extensions {
            extensions.push((m.features, ext.clone()));
        }
        for n in &m.nested {
            walk(n, out, enums, extensions);
        }
    }
    for e in &file.enums {
        enums.insert(e.full_name.clone(), e.clone());
    }
    for ext in &file.extensions {
        extensions.push((file.features, ext.clone()));
    }
    for m in &file.messages {
        walk(m, out, enums, extensions);
    }
}

fn resolve_pool(
    raw: BTreeMap<String, RawMessage>,
    raw_enums: BTreeMap<String, RawEnum>,
    extensions: Vec<(RawFeatures, RawField)>,
) -> DescriptorPool {
    // First pass: skeleton descriptors (no nested message arcs).
    let mut skeletons: BTreeMap<String, MessageDescriptor> = BTreeMap::new();
    for (name, raw_msg) in &raw {
        let mut b = MessageDescriptor::builder(name.clone()).map_entry(raw_msg.is_map_entry);
        let mut oneofs: Vec<Vec<u32>> = vec![Vec::new(); raw_msg.oneof_count as usize];
        for f in &raw_msg.fields {
            let fd = raw_field_to_desc(f, raw_msg.features);
            if let Some(idx) = fd.oneof_index {
                if let Some(slot) = oneofs.get_mut(idx as usize) {
                    slot.push(fd.number);
                }
            }
            b = b.field(fd);
        }
        let mut built = b.build();
        built.oneofs = oneofs;
        built.extension_ranges = raw_msg.extension_ranges.clone();
        built.reserved_names = raw_msg.reserved_names.iter().cloned().collect();
        built.message_set_wire_format = raw_msg.message_set_wire_format;
        built.file_name = raw_msg.file_name.clone();
        built.options = raw_msg.options.clone();
        skeletons.insert(name.clone(), built);
    }
    let mut enum_arcs: BTreeMap<String, Arc<EnumDescriptor>> = BTreeMap::new();
    for (name, raw_e) in &raw_enums {
        let mut ed = EnumDescriptor {
            name: raw_e.name.clone(),
            full_name: name.clone(),
            file_name: raw_e.file_name.clone(),
            values: BTreeMap::new(),
            names: BTreeMap::new(),
            listed: raw_e.values.clone(),
            closed: raw_e.closed,
            options: raw_e.options.clone(),
        };
        for (num, n) in &raw_e.values {
            ed.values.entry(*num).or_insert_with(|| n.clone());
            ed.names.insert(n.clone(), *num);
        }
        enum_arcs.insert(name.clone(), Arc::new(ed));
    }
    let mut ext_index: BTreeMap<String, (String, u32)> = BTreeMap::new();
    for (parent_feat, ext) in &extensions {
        let fd = raw_field_to_desc(ext, *parent_feat);
        let extendee = fd
            .extendee
            .as_deref()
            .unwrap_or("")
            .trim_start_matches('.')
            .to_string();
        if let Some(desc) = skeletons.get_mut(&extendee) {
            desc.fields_by_name
                .entry(fd.name.clone())
                .or_insert(fd.number);
            if !fd.json_name.is_empty() {
                desc.fields_by_json_name
                    .entry(fd.json_name.clone())
                    .or_insert(fd.number);
            }
            let full = format!("{extendee}.{}", fd.name);
            desc.fields_by_name.insert(full, fd.number);
            if !ext.full_ext_name.is_empty() {
                desc.fields_by_name
                    .insert(ext.full_ext_name.clone(), fd.number);
                ext_index.insert(ext.full_ext_name.clone(), (extendee.clone(), fd.number));
            }
            desc.fields.insert(fd.number, fd);
        }
    }
    let lookup: BTreeMap<String, Arc<MessageDescriptor>> = skeletons
        .iter()
        .map(|(k, v)| (k.clone(), Arc::new(v.clone())))
        .collect();
    let mut resolved = BTreeMap::new();
    for (name, mut d) in skeletons {
        for field in d.fields.values_mut() {
            if let Some(tn) = &field.type_name {
                let key = tn.trim_start_matches('.');
                if let Some(target) = lookup.get(key) {
                    field.message = Some(target.clone());
                    if target.is_map_entry {
                        field.is_map = true;
                        field.cardinality = Cardinality::Repeated;
                        field.packed = false;
                        field.delimited = false;
                    }
                }
                if field.field_type == FieldType::Enum || field.message.is_none() {
                    if let Some(en) = enum_arcs.get(key) {
                        field.enum_ty = Some(en.clone());
                        field.field_type = FieldType::Enum;
                    }
                }
            }
        }
        if d.is_map_entry {
            for field in d.fields.values_mut() {
                field.delimited = false;
            }
        }
        let mut alts = Vec::new();
        for field in d.fields.values() {
            if field.extension_name.is_some() {
                continue;
            }
            if field.field_type == FieldType::Group || field.delimited {
                if let Some(tn) = &field.type_name {
                    let short = tn.rsplit('.').next().unwrap_or(tn);
                    alts.push((short.to_string(), field.number));
                    alts.push((short.to_ascii_lowercase(), field.number));
                }
            }
        }
        for (alt, n) in alts {
            d.fields_by_name.entry(alt).or_insert(n);
        }
        resolved.insert(name, Arc::new(d));
    }
    DescriptorPool {
        messages: resolved,
        enums: enum_arcs,
        extensions_by_name: ext_index,
        services: BTreeMap::new(),
        files: BTreeMap::new(),
        public_imports: BTreeMap::new(),
    }
}

fn raw_field_to_desc(f: &RawField, parent: RawFeatures) -> FieldDescriptor {
    let feat = parent.merge(f.features);
    let mut field_type = FieldType::from_i32(f.ty).unwrap_or(FieldType::Message);
    if f.ty == 0 && !f.type_name.is_empty() {
        field_type = FieldType::Message;
    }
    let cardinality = match f.label {
        3 => Cardinality::Repeated,
        2 => Cardinality::Required,
        _ if feat.presence == 3 => Cardinality::Required,
        _ => Cardinality::Optional,
    };
    let presence = if cardinality == Cardinality::Repeated
        || field_type == FieldType::Message
        || field_type == FieldType::Group
        || f.proto3_optional
        || f.oneof_index.is_some()
        || feat.presence == 1
        || feat.presence == 3
    {
        Presence::Explicit
    } else {
        Presence::Implicit
    };
    let packed = f.packed.unwrap_or(
        cardinality == Cardinality::Repeated
            && field_type.is_packable()
            && feat.repeated_encoding == 1,
    );
    FieldDescriptor {
        name: f.name.clone(),
        number: f.number,
        field_type,
        cardinality,
        presence,
        packed,
        type_name: if f.type_name.is_empty() {
            None
        } else {
            Some(f.type_name.clone())
        },
        json_name: if f.json_name.is_empty() {
            f.name.clone()
        } else {
            f.json_name.clone()
        },
        is_map: false,
        message: None,
        enum_ty: None,
        oneof_index: f.oneof_index,
        utf8_validate: feat.utf8 == 2,
        default: f.default_value.clone(),
        extendee: if f.extendee.is_empty() {
            None
        } else {
            Some(f.extendee.clone())
        },
        delimited: field_type == FieldType::Group
            || (feat.message_encoding == 2 && field_type == FieldType::Message),
        extension_name: if f.full_ext_name.is_empty() {
            None
        } else {
            Some(f.full_ext_name.clone())
        },
        options: f.options.clone(),
    }
}
