//! Helpers for plugin-generated typed wrappers around [`DynamicMessage`].

use crate::dynamic::{DynamicMessage, MapKeyValue, Value};
use crate::map::{Map, MapKey, MapValue};
use crate::repeated::Repeated;
use crate::string::{ProtoBytes, ProtoString};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct SyncPtr(*const ());
unsafe impl Send for SyncPtr {}
unsafe impl Sync for SyncPtr {}

/// Process-lifetime default instance (Google `View::default()`).
pub fn default_instance_of<T: Default + Send + Sync + 'static>() -> &'static T {
    static MAP: OnceLock<Mutex<HashMap<TypeId, SyncPtr>>> = OnceLock::new();
    let map = MAP.get_or_init(|| Mutex::new(HashMap::new()));
    let id = TypeId::of::<T>();
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    let ptr = guard
        .entry(id)
        .or_insert_with(|| {
            let leaked: &'static T = Box::leak(Box::new(T::default()));
            SyncPtr(leaked as *const T as *const ())
        })
        .0;
    // SAFETY: pointer was created from &'static T of this TypeId.
    unsafe { &*(ptr as *const T) }
}

pub trait IntoMapKey {
    fn into_map_key(self) -> MapKeyValue;
}

impl IntoMapKey for i32 {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::I32(self)
    }
}
impl IntoMapKey for i64 {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::I64(self)
    }
}
impl IntoMapKey for u32 {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::U32(self)
    }
}
impl IntoMapKey for u64 {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::U64(self)
    }
}
impl IntoMapKey for bool {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::Bool(self)
    }
}
impl IntoMapKey for ProtoString {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::String(self)
    }
}
impl IntoMapKey for &str {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::String(ProtoString::from(self))
    }
}
impl IntoMapKey for String {
    fn into_map_key(self) -> MapKeyValue {
        MapKeyValue::String(ProtoString::from(self.as_str()))
    }
}

pub trait IntoFieldValue {
    fn into_field_value(self) -> Value;
}

impl IntoFieldValue for i32 {
    fn into_field_value(self) -> Value {
        Value::Int32(self)
    }
}
impl IntoFieldValue for i64 {
    fn into_field_value(self) -> Value {
        Value::Int64(self)
    }
}
impl IntoFieldValue for u32 {
    fn into_field_value(self) -> Value {
        Value::Uint32(self)
    }
}
impl IntoFieldValue for u64 {
    fn into_field_value(self) -> Value {
        Value::Uint64(self)
    }
}
impl IntoFieldValue for bool {
    fn into_field_value(self) -> Value {
        Value::Bool(self)
    }
}
impl IntoFieldValue for f32 {
    fn into_field_value(self) -> Value {
        Value::Float(self)
    }
}
impl IntoFieldValue for f64 {
    fn into_field_value(self) -> Value {
        Value::Double(self)
    }
}
impl IntoFieldValue for ProtoString {
    fn into_field_value(self) -> Value {
        Value::String(self)
    }
}
impl IntoFieldValue for ProtoBytes {
    fn into_field_value(self) -> Value {
        Value::Bytes(self)
    }
}
impl IntoFieldValue for DynamicMessage {
    fn into_field_value(self) -> Value {
        Value::Message(self)
    }
}
impl IntoFieldValue for &str {
    fn into_field_value(self) -> Value {
        Value::String(ProtoString::from(self))
    }
}

pub struct DynMapMut<'a, K, V> {
    pub msg: &'a mut DynamicMessage,
    pub number: u32,
    pub _k: std::marker::PhantomData<K>,
    pub _v: std::marker::PhantomData<V>,
}

impl<K: IntoMapKey, V: IntoFieldValue> DynMapMut<'_, K, V> {
    pub fn insert(&mut self, k: K, v: V) {
        self.msg
            .insert_map(self.number, k.into_map_key(), v.into_field_value());
    }
}

pub struct DynRepeatedMut<'a, V> {
    pub msg: &'a mut DynamicMessage,
    pub number: u32,
    pub _v: std::marker::PhantomData<V>,
}

impl<V: IntoFieldValue> DynRepeatedMut<'_, V> {
    pub fn push(&mut self, v: V) {
        self.msg.push(self.number, v.into_field_value());
    }
}

pub fn i32_from(v: Option<&Value>) -> i32 {
    match v {
        Some(Value::Int32(n) | Value::Enum(n)) => *n,
        Some(Value::Int64(n)) => *n as i32,
        Some(Value::Uint32(n)) => *n as i32,
        _ => 0,
    }
}

pub fn i64_from(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Int64(n)) => *n,
        Some(Value::Int32(n) | Value::Enum(n)) => i64::from(*n),
        Some(Value::Uint64(n)) => *n as i64,
        _ => 0,
    }
}

pub fn u32_from(v: Option<&Value>) -> u32 {
    match v {
        Some(Value::Uint32(n)) => *n,
        Some(Value::Int32(n)) => *n as u32,
        _ => 0,
    }
}

pub fn u64_from(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Uint64(n)) => *n,
        Some(Value::Int64(n)) => *n as u64,
        Some(Value::Uint32(n)) => u64::from(*n),
        _ => 0,
    }
}

pub fn bool_from(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}

pub fn f32_from(v: Option<&Value>) -> f32 {
    match v {
        Some(Value::Float(n)) => *n,
        Some(Value::Double(n)) => *n as f32,
        _ => 0.0,
    }
}

pub fn f64_from(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Double(n)) => *n,
        Some(Value::Float(n)) => f64::from(*n),
        _ => 0.0,
    }
}

pub fn empty_str() -> &'static crate::ProtoStr {
    crate::ProtoStr::from_bytes(b"")
}

pub fn str_from(v: Option<&Value>) -> &crate::ProtoStr {
    match v {
        Some(Value::String(s)) => s.as_view(),
        _ => empty_str(),
    }
}

pub fn bytes_from(v: Option<&Value>) -> &[u8] {
    match v {
        Some(Value::Bytes(b)) => b.as_bytes(),
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"",
    }
}

pub trait FromFieldValue: Sized {
    fn from_field_value(v: &Value) -> Option<Self>;
}

pub trait FromMapKey: Sized {
    fn from_map_key(k: &MapKeyValue) -> Option<Self>;
}

impl FromFieldValue for i32 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(i32_from(Some(v)))
    }
}
impl FromFieldValue for i64 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(i64_from(Some(v)))
    }
}
impl FromFieldValue for u32 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(u32_from(Some(v)))
    }
}
impl FromFieldValue for u64 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(u64_from(Some(v)))
    }
}
impl FromFieldValue for bool {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(bool_from(Some(v)))
    }
}
impl FromFieldValue for f32 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(f32_from(Some(v)))
    }
}
impl FromFieldValue for f64 {
    fn from_field_value(v: &Value) -> Option<Self> {
        Some(f64_from(Some(v)))
    }
}
impl FromFieldValue for ProtoString {
    fn from_field_value(v: &Value) -> Option<Self> {
        match v {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}
impl FromFieldValue for ProtoBytes {
    fn from_field_value(v: &Value) -> Option<Self> {
        match v {
            Value::Bytes(b) => Some(b.clone()),
            Value::String(s) => Some(ProtoBytes::from(s.as_bytes())),
            _ => None,
        }
    }
}

impl FromMapKey for i32 {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::I32(n) => Some(*n),
            _ => None,
        }
    }
}
impl FromMapKey for i64 {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::I64(n) => Some(*n),
            MapKeyValue::I32(n) => Some(i64::from(*n)),
            _ => None,
        }
    }
}
impl FromMapKey for u32 {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::U32(n) => Some(*n),
            _ => None,
        }
    }
}
impl FromMapKey for u64 {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::U64(n) => Some(*n),
            MapKeyValue::U32(n) => Some(u64::from(*n)),
            _ => None,
        }
    }
}
impl FromMapKey for bool {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::Bool(b) => Some(*b),
            _ => None,
        }
    }
}
impl FromMapKey for ProtoString {
    fn from_map_key(k: &MapKeyValue) -> Option<Self> {
        match k {
            MapKeyValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

pub fn map_from<K, V>(src: Option<&std::collections::BTreeMap<MapKeyValue, Value>>) -> Map<K, V>
where
    K: FromMapKey + MapKey,
    V: FromFieldValue + MapValue,
{
    let mut m = Map::new();
    if let Some(src) = src {
        for (k, v) in src {
            if let (Some(k), Some(v)) = (K::from_map_key(k), V::from_field_value(v)) {
                m.insert(k, v);
            }
        }
    }
    m
}

pub fn repeated_from<T: FromFieldValue>(src: Option<&[Value]>) -> Repeated<T> {
    let mut r = Repeated::new();
    if let Some(items) = src {
        for v in items {
            if let Some(t) = T::from_field_value(v) {
                r.push(t);
            }
        }
    }
    r
}

pub fn map_string_i32(
    src: Option<&std::collections::BTreeMap<MapKeyValue, Value>>,
) -> Map<ProtoString, i32> {
    map_from(src)
}

/// Typed message wrapping a `DynamicMessage`. Used by plugin gencode and
/// the TestAllTypes wrappers the conformance program drives.
#[macro_export]
macro_rules! impl_generated_message {
    ($Owned:ident, $View:ident, $Mut:ident, $full:expr, $pool:expr) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $Owned {
            inner: $crate::DynamicMessage,
        }
        #[derive(Clone, Copy, Debug)]
        pub struct $View<'msg>(pub &'msg $Owned);
        pub struct $Mut<'msg>(pub &'msg mut $Owned);
        impl std::ops::Deref for $View<'_> {
            type Target = $Owned;
            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
        impl std::ops::Deref for $Mut<'_> {
            type Target = $Owned;
            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
        impl std::ops::DerefMut for $Mut<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.0
            }
        }

        impl $Owned {
            pub const FULL_NAME: &'static str = $full;
            pub fn new() -> Self {
                Self::default()
            }
            pub fn from_dynamic(inner: $crate::DynamicMessage) -> Self {
                Self { inner }
            }
            pub fn as_dynamic(&self) -> &$crate::DynamicMessage {
                &self.inner
            }
            pub fn as_dynamic_mut(&mut self) -> &mut $crate::DynamicMessage {
                &mut self.inner
            }
            pub fn into_dynamic(self) -> $crate::DynamicMessage {
                self.inner
            }
            pub fn to_json(&self) -> Result<String, $crate::SerializeError> {
                self.inner.to_json()
            }
            pub fn from_json(json: &str) -> Result<Self, $crate::ParseError> {
                Self::from_json_ignore(json, false)
            }
            pub fn from_json_ignore(
                json: &str,
                ignore_unknown: bool,
            ) -> Result<Self, $crate::ParseError> {
                let pool = $pool;
                let desc = pool.get_message($full).ok_or_else(|| {
                    $crate::ParseError::owned(format!("missing descriptor {}", $full))
                })?;
                Ok(Self {
                    inner: $crate::DynamicMessage::from_json_with_pool(
                        desc,
                        Some(pool),
                        json,
                        ignore_unknown,
                    )?,
                })
            }
            pub fn to_text(&self) -> Result<String, $crate::SerializeError> {
                self.inner.to_text()
            }
            pub fn to_text_with_unknown(&self) -> Result<String, $crate::SerializeError> {
                self.inner.to_text_with_unknown()
            }
            pub fn from_text(text: &str) -> Result<Self, $crate::ParseError> {
                let pool = $pool;
                let desc = pool.get_message($full).ok_or_else(|| {
                    $crate::ParseError::owned(format!("missing descriptor {}", $full))
                })?;
                Ok(Self {
                    inner: $crate::DynamicMessage::from_text_with_pool(desc, Some(pool), text)?,
                })
            }
        }

        impl Default for $Owned {
            fn default() -> Self {
                let pool = $pool;
                let desc = pool
                    .get_message($full)
                    .unwrap_or_else(|| panic!("missing descriptor {}", $full));
                let mut inner = $crate::DynamicMessage::new(desc);
                inner.set_pool(pool);
                Self { inner }
            }
        }

        impl $crate::__internal::SealedInternal for $Owned {}
        impl $crate::MessageType for $Owned {}
        impl $crate::Proxied for $Owned {
            type View<'msg> = $View<'msg>;
        }
        impl $crate::MutProxied for $Owned {
            type Mut<'msg> = $Mut<'msg>;
        }
        impl $crate::AsView for $Owned {
            type Proxied = Self;
            fn as_view(&self) -> $View<'_> {
                $View(self)
            }
        }
        impl $crate::AsMut for $Owned {
            type MutProxied = Self;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self)
            }
        }
        impl $crate::__internal::SealedInternal for $View<'_> {}
        impl $crate::AsView for $View<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                *self
            }
        }
        impl<'msg> $crate::IntoView<'msg> for $View<'msg> {
            fn into_view<'s>(self) -> $View<'s>
            where
                'msg: 's,
            {
                $View(self.0)
            }
        }
        impl $crate::__internal::SealedInternal for $Mut<'_> {}
        impl $crate::AsView for $Mut<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                $View(self.0)
            }
        }
        impl $crate::AsMut for $Mut<'_> {
            type MutProxied = $Owned;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self.0)
            }
        }
        impl<'msg> $crate::IntoView<'msg> for $Mut<'msg> {
            fn into_view<'s>(self) -> $View<'s>
            where
                'msg: 's,
            {
                $View(self.0)
            }
        }
        impl<'msg> $crate::IntoMut<'msg> for $Mut<'msg> {
            fn into_mut<'s>(self) -> $Mut<'s>
            where
                'msg: 's,
            {
                $Mut(self.0)
            }
        }
        impl $crate::Serialize for $Owned {
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                $crate::Serialize::serialize(&self.inner)
            }
            fn serialized_len(&self) -> usize {
                $crate::Serialize::serialized_len(&self.inner)
            }
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                $crate::Serialize::encode(&self.inner, out)
            }
        }
        impl $crate::Serialize for $View<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                $crate::Serialize::serialize(self.0)
            }
            fn serialized_len(&self) -> usize {
                $crate::Serialize::serialized_len(self.0)
            }
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                $crate::Serialize::encode(self.0, out)
            }
        }
        impl $crate::Serialize for $Mut<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                $crate::Serialize::serialize(self.0)
            }
            fn serialized_len(&self) -> usize {
                $crate::Serialize::serialized_len(self.0)
            }
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                $crate::Serialize::encode(self.0, out)
            }
        }
        impl $crate::Clear for $Owned {
            fn clear(&mut self) {
                $crate::Clear::clear(&mut self.inner);
            }
        }
        impl $crate::Clear for $Mut<'_> {
            fn clear(&mut self) {
                $crate::Clear::clear(&mut self.0.inner);
            }
        }
        impl $crate::ClearAndParse for $Owned {
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::clear_and_parse(&mut self.inner, data)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::clear_and_parse_dont_enforce_required(&mut self.inner, data)
            }
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::merge_from_bytes(&mut self.inner, data)
            }
            fn merge_from_bytes_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::merge_from_bytes_dont_enforce_required(&mut self.inner, data)
            }
        }
        impl $crate::ClearAndParse for $Mut<'_> {
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::clear_and_parse(self.0, data)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::clear_and_parse_dont_enforce_required(self.0, data)
            }
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                $crate::ClearAndParse::merge_from_bytes(self.0, data)
            }
        }
        impl $crate::CopyFrom for $Owned {
            fn copy_from(&mut self, src: impl $crate::AsView<Proxied = Self>) {
                self.inner = src.as_view().0.inner.clone();
            }
        }
        impl $crate::CopyFrom for $Mut<'_> {
            fn copy_from(&mut self, src: impl $crate::AsView<Proxied = $Owned>) {
                self.0.inner = src.as_view().0.inner.clone();
            }
        }
        impl $crate::TakeFrom for $Owned {
            fn take_from(&mut self, mut src: impl $crate::AsMut<MutProxied = Self>) {
                self.inner = std::mem::take(&mut src.as_mut().0.inner);
            }
        }
        impl $crate::TakeFrom for $Mut<'_> {
            fn take_from(&mut self, mut src: impl $crate::AsMut<MutProxied = $Owned>) {
                self.0.inner = std::mem::take(&mut src.as_mut().0.inner);
            }
        }
        impl $crate::MergeFrom for $Owned {
            fn merge_from(&mut self, src: impl $crate::AsView<Proxied = Self>) {
                $crate::MergeFrom::merge_from(&mut self.inner, src.as_view().0.inner.clone());
            }
        }
        impl $crate::MergeFrom for $Mut<'_> {
            fn merge_from(&mut self, src: impl $crate::AsView<Proxied = $Owned>) {
                $crate::MergeFrom::merge_from(self.0, src);
            }
        }
        impl $crate::Message for $Owned {
            type MessageView<'msg> = $View<'msg>;
            type MessageMut<'msg> = $Mut<'msg>;
        }
        impl<'msg> $crate::MessageView<'msg> for $View<'msg> {
            type Message = $Owned;
        }
        impl<'msg> $crate::MessageMut<'msg> for $Mut<'msg> {
            type Message = $Owned;
        }
        impl std::fmt::Debug for $Mut<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self.0, f)
            }
        }
        impl $crate::gen_support::IntoFieldValue for $Owned {
            fn into_field_value(self) -> $crate::Value {
                $crate::Value::Message(self.inner)
            }
        }
        impl $crate::gen_support::FromFieldValue for $Owned {
            fn from_field_value(v: &$crate::Value) -> Option<Self> {
                match v {
                    $crate::Value::Message(m) => Some(Self::from_dynamic(m.clone())),
                    _ => None,
                }
            }
        }
    };
}

/// Trait impls for field-wise generated messages (`merge_bytes` / `write_to` / `compute_size`).
#[macro_export]
macro_rules! impl_typed_message {
    ($Owned:ident, $View:ident, $Mut:ident) => {
        #[derive(Clone, Copy, Debug)]
        pub struct $View<'msg>(pub &'msg $Owned);
        pub struct $Mut<'msg>(pub &'msg mut $Owned);
        impl std::ops::Deref for $View<'_> {
            type Target = $Owned;
            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
        impl std::ops::Deref for $Mut<'_> {
            type Target = $Owned;
            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
        impl std::ops::DerefMut for $Mut<'_> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.0
            }
        }
        impl $crate::__internal::SealedInternal for $Owned {}
        impl $crate::MessageType for $Owned {}
        impl $crate::Proxied for $Owned {
            type View<'msg> = $View<'msg>;
        }
        impl $crate::MutProxied for $Owned {
            type Mut<'msg> = $Mut<'msg>;
        }
        impl $crate::AsView for $Owned {
            type Proxied = Self;
            fn as_view(&self) -> $View<'_> {
                $View(self)
            }
        }
        impl Default for $View<'_> {
            fn default() -> Self {
                $View($crate::gen_support::default_instance_of::<$Owned>())
            }
        }
        impl $crate::rt::MergeBytes for $Owned {
            fn merge_inner(
                &mut self,
                wire: &$crate::rt::Wire,
                pos: &mut usize,
                depth: u32,
                enforce: bool,
                until: Option<u32>,
            ) -> Result<(), $crate::ParseError> {
                let data = wire.as_slice();
                let mut w = Some(wire.clone());
                $Owned::merge_inner(self, data, &mut w, pos, depth, enforce, until)
            }
        }
        impl $crate::AsMut for $Owned {
            type MutProxied = Self;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self)
            }
        }
        impl $crate::__internal::SealedInternal for $View<'_> {}
        impl $crate::AsView for $View<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                *self
            }
        }
        impl<'msg> $crate::IntoView<'msg> for $View<'msg> {
            fn into_view<'s>(self) -> $View<'s>
            where
                'msg: 's,
            {
                $View(self.0)
            }
        }
        impl $crate::__internal::SealedInternal for $Mut<'_> {}
        impl $crate::AsView for $Mut<'_> {
            type Proxied = $Owned;
            fn as_view(&self) -> $View<'_> {
                $View(self.0)
            }
        }
        impl $crate::AsMut for $Mut<'_> {
            type MutProxied = $Owned;
            fn as_mut(&mut self) -> $Mut<'_> {
                $Mut(self.0)
            }
        }
        impl<'msg> $crate::IntoView<'msg> for $Mut<'msg> {
            fn into_view<'s>(self) -> $View<'s>
            where
                'msg: 's,
            {
                $View(self.0)
            }
        }
        impl<'msg> $crate::IntoMut<'msg> for $Mut<'msg> {
            fn into_mut<'s>(self) -> $Mut<'s>
            where
                'msg: 's,
            {
                $Mut(self.0)
            }
        }
        impl $crate::Serialize for $Owned {
            #[inline]
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                let mut out = Vec::with_capacity(self.compute_size() as usize);
                self.write_to(&mut out);
                $crate::rt::check_size(out.len() as u64)?;
                Ok(out)
            }
            fn serialized_len(&self) -> usize {
                self.compute_size() as usize
            }
            #[inline]
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                $crate::rt::check_size(self.compute_size())?;
                self.write_to(out);
                Ok(())
            }
        }
        impl $crate::Serialize for $View<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                self.0.serialize()
            }
            fn serialized_len(&self) -> usize {
                self.0.serialized_len()
            }
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                self.0.encode(out)
            }
        }
        impl $crate::Serialize for $Mut<'_> {
            fn serialize(&self) -> Result<Vec<u8>, $crate::SerializeError> {
                self.0.serialize()
            }
            fn serialized_len(&self) -> usize {
                self.0.serialized_len()
            }
            fn encode(
                &self,
                out: &mut impl $crate::rt::WireOut,
            ) -> Result<(), $crate::SerializeError> {
                self.0.encode(out)
            }
        }
        impl $crate::Clear for $Owned {
            fn clear(&mut self) {
                *self = Self::default();
            }
        }
        impl $crate::Clear for $Mut<'_> {
            fn clear(&mut self) {
                *self.0 = $Owned::default();
            }
        }
        impl $crate::ClearAndParse for $Owned {
            const EMPTY_PARSE_OK: bool = $Owned::EMPTY_PARSE_OK;
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                $crate::Clear::clear(self);
                self.merge_bytes(data, 0)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                $crate::Clear::clear(self);
                self.merge_bytes_dont_enforce(data, 0)
            }
            #[inline(always)]
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                if data.is_empty() {
                    if $Owned::EMPTY_PARSE_OK {
                        return Ok(());
                    }
                    return self.check_required();
                }
                self.merge_bytes(data, 0)
            }
            fn merge_from_bytes_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                if data.is_empty() {
                    return Ok(());
                }
                self.merge_bytes_dont_enforce(data, 0)
            }
        }
        impl $crate::ClearAndParse for $Mut<'_> {
            fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                self.0.clear_and_parse(data)
            }
            fn clear_and_parse_dont_enforce_required(
                &mut self,
                data: &[u8],
            ) -> Result<(), $crate::ParseError> {
                self.0.clear_and_parse_dont_enforce_required(data)
            }
            fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), $crate::ParseError> {
                self.0.merge_from_bytes(data)
            }
        }
        impl $crate::CopyFrom for $Owned {
            fn copy_from(&mut self, src: impl $crate::AsView<Proxied = Self>) {
                *self = src.as_view().0.clone();
            }
        }
        impl $crate::CopyFrom for $Mut<'_> {
            fn copy_from(&mut self, src: impl $crate::AsView<Proxied = $Owned>) {
                *self.0 = src.as_view().0.clone();
            }
        }
        impl $crate::TakeFrom for $Owned {
            fn take_from(&mut self, mut src: impl $crate::AsMut<MutProxied = Self>) {
                *self = std::mem::take(src.as_mut().0);
            }
        }
        impl $crate::TakeFrom for $Mut<'_> {
            fn take_from(&mut self, mut src: impl $crate::AsMut<MutProxied = $Owned>) {
                *self.0 = std::mem::take(src.as_mut().0);
            }
        }
        impl $crate::MergeFrom for $Owned {
            fn merge_from(&mut self, src: impl $crate::AsView<Proxied = Self>) {
                let b = $crate::Serialize::serialize(src.as_view().0).unwrap_or_default();
                let _ = self.merge_bytes(&b, 0);
            }
        }
        impl $crate::MergeFrom for $Mut<'_> {
            fn merge_from(&mut self, src: impl $crate::AsView<Proxied = $Owned>) {
                $crate::MergeFrom::merge_from(self.0, src);
            }
        }
        impl $crate::Message for $Owned {
            type MessageView<'msg> = $View<'msg>;
            type MessageMut<'msg> = $Mut<'msg>;
        }
        impl<'msg> $crate::MessageView<'msg> for $View<'msg> {
            type Message = $Owned;
        }
        impl<'msg> $crate::MessageMut<'msg> for $Mut<'msg> {
            type Message = $Owned;
        }
        impl std::fmt::Debug for $Mut<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Debug::fmt(self.0, f)
            }
        }
    };
}
