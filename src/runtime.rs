//! Pure-Rust stand-in for Google protobuf `__internal::runtime` (upb kernel ABI).
//! Official `protoc --rust_out` links against these names. No C/upb.

use crate::error::{ParseError, SerializeError};
use crate::internal::{Private, SealedInternal};
use crate::message::{
    Clear, ClearAndParse, CopyFrom, MergeFrom, Message, MessageMut, MessageView, Serialize,
};
use crate::proxied::{AsView, IntoProxied, View};
use crate::string::{ProtoBytes, ProtoStr, ProtoString};
use crate::wire::{
    decode_tag, decode_varint, decode_zigzag32, decode_zigzag64, encode_len_field, encode_tag,
    encode_varint, read_fixed32, read_fixed64, read_len_bytes, skip_field, UnknownFields, WIRE_I32,
    WIRE_I64, WIRE_LEN, WIRE_VARINT,
};
use std::cell::RefCell;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use std::slice;
use std::sync::OnceLock;

pub type MiniTableEnumPtr = *const ();
pub type MiniTableExtensionPtr = *const ();

#[derive(Clone, Copy, Debug)]
pub struct MiniTableEnumInitPtr(pub MiniTableEnumPtr);
unsafe impl Send for MiniTableEnumInitPtr {}
unsafe impl Sync for MiniTableEnumInitPtr {}

impl MiniTableEnumInitPtr {
    pub const fn dangling() -> Self {
        Self(std::ptr::null())
    }
}
pub type ExtensionRegistryPtr = *const ();
pub type RawMessage = NonNull<MsgData>;
pub type RawRepeatedField = *const RawArrayInner;
pub type RawMap = *const RawMapInner;
pub type PtrAndLen = StringView;

#[derive(Clone, Copy, Debug)]
pub struct StringView {
    ptr: *const u8,
    len: usize,
}

impl StringView {
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null(),
            len: 0,
        }
    }

    pub unsafe fn as_ref<'a>(self) -> &'a [u8] {
        if self.len == 0 || self.ptr.is_null() {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.ptr, self.len) }
        }
    }
}

impl From<&[u8]> for StringView {
    fn from(s: &[u8]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }
}

impl<const N: usize> From<&[u8; N]> for StringView {
    fn from(s: &[u8; N]) -> Self {
        Self::from(s.as_slice())
    }
}

impl From<&ProtoStr> for StringView {
    fn from(s: &ProtoStr) -> Self {
        Self::from(s.as_bytes())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MiniTableInitPtr(pub MiniTablePtr);
unsafe impl Send for MiniTableInitPtr {}
unsafe impl Sync for MiniTableInitPtr {}

#[derive(Clone, Copy, Debug)]
pub struct MiniTablePtr(pub *const MiniTable);
unsafe impl Send for MiniTablePtr {}
unsafe impl Sync for MiniTablePtr {}

impl MiniTablePtr {
    pub const fn dangling() -> Self {
        Self(std::ptr::null())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Double,
    Float,
    Int32,
    Int64,
    UInt32,
    UInt64,
    SInt32,
    SInt64,
    Fixed32,
    Fixed64,
    SFixed32,
    SFixed64,
    Bool,
    String,
    Bytes,
    Message,
    Group,
    Enum,
}

#[derive(Clone, Copy, Debug)]
pub struct MiniField {
    pub number: u32,
    pub ty: FieldType,
    pub repeated: bool,
    pub packed: bool,
    pub proto3_singular: bool,
    pub required: bool,
    pub is_map: bool,
    pub sub: MiniTablePtr,
    pub oneof_group: u32,
}

#[derive(Debug)]
pub struct MiniTable {
    pub fields: Vec<MiniField>,
    pub is_map: bool,
    pub enforce_utf8: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum FieldKind {
    Empty,
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Bytes(*const Vec<u8>),
    Msg(*mut MsgData),
    Repeated(*const RawArrayInner),
    Map(*const RawMapInner),
}

#[derive(Debug)]
pub struct RawArrayInner {
    pub items: RefCell<Vec<FieldKind>>,
    pub strs: RefCell<Vec<Vec<u8>>>,
}

#[derive(Debug)]
pub struct RawMapInner {
    pub entries: RefCell<Vec<(Vec<u8>, FieldKind)>>,
    pub strs: RefCell<Vec<Vec<u8>>>,
}

#[derive(Debug)]
pub struct MsgData {
    pub slots: Vec<FieldKind>,
    pub has: Vec<bool>,
    pub strs: Vec<Vec<u8>>,
    pub unknown: UnknownFields,
    pub mt: MiniTablePtr,
}

#[derive(Debug)]
pub struct ArenaInner {
    // Boxed so pointers handed to rust_out stay valid across Vec growth.
    #[allow(clippy::vec_box)]
    msgs: Vec<Box<MsgData>>,
    #[allow(clippy::vec_box)]
    arrays: Vec<Box<RawArrayInner>>,
    #[allow(clippy::vec_box)]
    maps: Vec<Box<RawMapInner>>,
}

#[derive(Clone, Debug)]
pub struct Arena {
    inner: Rc<RefCell<ArenaInner>>,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ArenaInner {
                msgs: Vec::new(),
                arrays: Vec::new(),
                maps: Vec::new(),
            })),
        }
    }

    pub fn fuse(&self, other: &Arena) {
        if Rc::ptr_eq(&self.inner, &other.inner) {
            return;
        }
        let mut o = other.inner.borrow_mut();
        let mut s = self.inner.borrow_mut();
        s.msgs.append(&mut o.msgs);
        s.arrays.append(&mut o.arrays);
        s.maps.append(&mut o.maps);
    }

    pub(crate) fn alloc_msg(&self, mt: MiniTablePtr) -> *mut MsgData {
        let n = unsafe { mt.0.as_ref().map(|t| t.fields.len()).unwrap_or(0) };
        let mut b = Box::new(MsgData {
            slots: vec![FieldKind::Empty; n],
            has: vec![false; n],
            strs: Vec::new(),
            unknown: UnknownFields::default(),
            mt,
        });
        let p = &mut *b as *mut MsgData;
        self.inner.borrow_mut().msgs.push(b);
        p
    }

    pub(crate) fn alloc_array(&self) -> *const RawArrayInner {
        let b = Box::new(RawArrayInner {
            items: RefCell::new(Vec::new()),
            strs: RefCell::new(Vec::new()),
        });
        let p = &*b as *const RawArrayInner;
        self.inner.borrow_mut().arrays.push(b);
        p
    }

    pub(crate) fn alloc_map(&self) -> *const RawMapInner {
        let b = Box::new(RawMapInner {
            entries: RefCell::new(Vec::new()),
            strs: RefCell::new(Vec::new()),
        });
        let p = &*b as *const RawMapInner;
        self.inner.borrow_mut().maps.push(b);
        p
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct MessagePtr<T> {
    raw: *mut MsgData,
    _phantom: PhantomData<T>,
}

impl<T> Copy for MessagePtr<T> {}
impl<T> Clone for MessagePtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> MessagePtr<T> {
    pub fn raw(&self) -> RawMessage {
        NonNull::new(self.raw).expect("message ptr")
    }

    pub unsafe fn wrap(raw: RawMessage) -> Self {
        Self {
            raw: raw.as_ptr(),
            _phantom: PhantomData,
        }
    }

    fn data(&self) -> &MsgData {
        unsafe { &*self.raw }
    }

    #[allow(clippy::mut_from_ref)]
    fn data_mut(&self) -> &mut MsgData {
        unsafe { &mut *self.raw }
    }

    pub fn new(arena: &Arena) -> Option<MessagePtr<T>>
    where
        T: AssociatedMiniTable,
    {
        let raw = arena.alloc_msg(T::mini_table());
        Some(MessagePtr {
            raw,
            _phantom: PhantomData,
        })
    }

    pub unsafe fn clear(self) {
        let d = self.data_mut();
        d.slots.fill(FieldKind::Empty);
        d.has.fill(false);
        d.strs.clear();
        d.unknown.clear();
    }

    pub unsafe fn deep_copy(self, src: Self, arena: &Arena) -> bool {
        copy_msg(self.raw, src.raw, arena);
        true
    }

    pub unsafe fn which_oneof_field_number_by_index(self, oneof_index: u32) -> u32 {
        let d = self.data();
        let Some(mt) = (unsafe { d.mt.0.as_ref() }) else {
            return 0;
        };
        let group = mt
            .fields
            .get(oneof_index as usize)
            .map(|f| f.oneof_group)
            .unwrap_or(0);
        if group == 0 {
            if d.has.get(oneof_index as usize).copied().unwrap_or(false) {
                return mt
                    .fields
                    .get(oneof_index as usize)
                    .map(|f| f.number)
                    .unwrap_or(0);
            }
            return 0;
        }
        for (i, f) in mt.fields.iter().enumerate() {
            if f.oneof_group == group && d.has.get(i).copied().unwrap_or(false) {
                return f.number;
            }
        }
        0
    }

    pub unsafe fn get_i32_at_index(self, index: u32, default_value: i32) -> i32 {
        match self.slot(index) {
            FieldKind::I32(v) => v,
            FieldKind::U32(v) => v as i32,
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_i32_at_index(self, index: u32, value: i32) {
        self.set_slot(index, FieldKind::I32(value), true);
    }
    pub unsafe fn get_i64_at_index(self, index: u32, default_value: i64) -> i64 {
        match self.slot(index) {
            FieldKind::I64(v) => v,
            FieldKind::U64(v) => v as i64,
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_i64_at_index(self, index: u32, value: i64) {
        self.set_slot(index, FieldKind::I64(value), true);
    }
    pub unsafe fn get_u32_at_index(self, index: u32, default_value: u32) -> u32 {
        match self.slot(index) {
            FieldKind::U32(v) => v,
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_u32_at_index(self, index: u32, value: u32) {
        self.set_slot(index, FieldKind::U32(value), true);
    }
    pub unsafe fn get_u64_at_index(self, index: u32, default_value: u64) -> u64 {
        match self.slot(index) {
            FieldKind::U64(v) => v,
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_u64_at_index(self, index: u32, value: u64) {
        self.set_slot(index, FieldKind::U64(value), true);
    }
    pub unsafe fn get_bool_at_index(self, index: u32, default_value: bool) -> bool {
        match self.slot(index) {
            FieldKind::Bool(v) => v,
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_bool_at_index(self, index: u32, value: bool) {
        self.set_slot(index, FieldKind::Bool(value), true);
    }
    pub unsafe fn get_f32_at_index(self, index: u32, default_value: f32) -> f32 {
        match self.slot(index) {
            FieldKind::F32(v) => v,
            FieldKind::U32(v) => f32::from_bits(v),
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_f32_at_index(self, index: u32, value: f32) {
        self.set_slot(index, FieldKind::F32(value), true);
    }
    pub unsafe fn get_f64_at_index(self, index: u32, default_value: f64) -> f64 {
        match self.slot(index) {
            FieldKind::F64(v) => v,
            FieldKind::U64(v) => f64::from_bits(v),
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_f64_at_index(self, index: u32, value: f64) {
        self.set_slot(index, FieldKind::F64(value), true);
    }
    pub unsafe fn get_string_at_index(self, index: u32, default_value: StringView) -> StringView {
        match self.slot(index) {
            FieldKind::Bytes(p) if !p.is_null() => unsafe { (&*p).as_slice().into() },
            _ => default_value,
        }
    }
    pub unsafe fn set_base_field_string_at_index(self, index: u32, value: StringView) {
        let bytes = unsafe { value.as_ref() }.to_vec();
        let d = self.data_mut();
        d.strs.push(bytes);
        let p = d.strs.last().unwrap() as *const Vec<u8>;
        self.set_slot(index, FieldKind::Bytes(p), true);
    }

    pub unsafe fn has_field_at_index(self, index: u32) -> bool {
        self.data()
            .has
            .get(index as usize)
            .copied()
            .unwrap_or(false)
    }

    pub unsafe fn clear_field_at_index(self, index: u32) {
        self.set_slot(index, FieldKind::Empty, false);
    }

    pub unsafe fn get_message_at_index<ChildT>(self, index: u32) -> Option<MessagePtr<ChildT>> {
        match self.slot(index) {
            FieldKind::Msg(p) if !p.is_null() => Some(MessagePtr {
                raw: p,
                _phantom: PhantomData,
            }),
            _ => None,
        }
    }

    pub unsafe fn set_base_field_message_at_index<ChildT>(
        self,
        index: u32,
        value: MessagePtr<ChildT>,
    ) {
        self.set_slot(index, FieldKind::Msg(value.raw), true);
    }

    pub unsafe fn get_or_create_mutable_message_at_index<ChildT>(
        self,
        index: u32,
        arena: &Arena,
    ) -> Option<MessagePtr<ChildT>>
    where
        ChildT: AssociatedMiniTable,
    {
        if let Some(p) = unsafe { self.get_message_at_index::<ChildT>(index) } {
            return Some(p);
        }
        let child = MessagePtr::<ChildT>::new(arena)?;
        unsafe { self.set_base_field_message_at_index(index, child) };
        Some(child)
    }

    pub unsafe fn get_array_at_index(self, index: u32) -> Option<RawRepeatedField> {
        match self.slot(index) {
            FieldKind::Repeated(p) if !p.is_null() => Some(p),
            _ => None,
        }
    }

    pub unsafe fn set_array_at_index(self, index: u32, value: RawRepeatedField) {
        self.set_slot(index, FieldKind::Repeated(value), true);
    }

    pub unsafe fn get_or_create_mutable_array_at_index(
        self,
        index: u32,
        arena: &Arena,
    ) -> Option<RawRepeatedField> {
        if let Some(p) = unsafe { self.get_array_at_index(index) } {
            return Some(p);
        }
        let p = arena.alloc_array();
        unsafe { self.set_array_at_index(index, p) };
        Some(p)
    }

    pub unsafe fn get_map_at_index(self, index: u32) -> Option<RawMap> {
        match self.slot(index) {
            FieldKind::Map(p) if !p.is_null() => Some(p),
            _ => None,
        }
    }

    pub unsafe fn set_map_at_index(self, index: u32, value: RawMap) {
        self.set_slot(index, FieldKind::Map(value), true);
    }

    pub unsafe fn get_or_create_mutable_map_at_index(
        self,
        index: u32,
        arena: &Arena,
    ) -> Option<RawMap> {
        if let Some(p) = unsafe { self.get_map_at_index(index) } {
            return Some(p);
        }
        let p = arena.alloc_map();
        unsafe { self.set_map_at_index(index, p) };
        Some(p)
    }

    fn slot(self, index: u32) -> FieldKind {
        self.data()
            .slots
            .get(index as usize)
            .copied()
            .unwrap_or(FieldKind::Empty)
    }

    fn set_slot(self, index: u32, v: FieldKind, has: bool) {
        let d = self.data_mut();
        let i = index as usize;
        if i >= d.slots.len() {
            d.slots.resize(i + 1, FieldKind::Empty);
            d.has.resize(i + 1, false);
        }
        if has {
            if let Some(mt) = unsafe { d.mt.0.as_ref() } {
                let group = mt.fields.get(i).map(|f| f.oneof_group).unwrap_or(0);
                if group != 0 {
                    for (j, f) in mt.fields.iter().enumerate() {
                        if f.oneof_group == group && j != i && j < d.has.len() {
                            d.has[j] = false;
                            d.slots[j] = FieldKind::Empty;
                        }
                    }
                }
            }
        }
        d.slots[i] = v;
        d.has[i] = has;
    }
}

fn clone_field_kind(fk: FieldKind, arena: &Arena) -> FieldKind {
    match fk {
        FieldKind::Bytes(p) if !p.is_null() => unsafe {
            let leaked = Box::leak(Box::new((*p).clone()));
            FieldKind::Bytes(leaked as *const Vec<u8>)
        },
        FieldKind::Msg(p) if !p.is_null() => FieldKind::Msg(kernel_clone_msg(p, arena)),
        FieldKind::Repeated(p) if !p.is_null() => {
            let np = arena.alloc_array();
            unsafe {
                let items = (*p)
                    .items
                    .borrow()
                    .iter()
                    .map(|x| clone_field_kind(*x, arena))
                    .collect();
                *(*np).items.borrow_mut() = items;
            }
            FieldKind::Repeated(np)
        }
        FieldKind::Map(p) if !p.is_null() => {
            let np = arena.alloc_map();
            unsafe {
                let entries = (*p)
                    .entries
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), clone_field_kind(*v, arena)))
                    .collect();
                *(*np).entries.borrow_mut() = entries;
            }
            FieldKind::Map(np)
        }
        other => other,
    }
}

fn copy_msg(dst: *mut MsgData, src: *mut MsgData, arena: &Arena) {
    unsafe {
        let s = &*src;
        let d = &mut *dst;
        d.mt = s.mt;
        d.has = s.has.clone();
        d.unknown = s.unknown.clone();
        d.strs = s.strs.clone();
        d.slots = s.slots.clone();
        for slot in &mut d.slots {
            match *slot {
                FieldKind::Bytes(p) if !p.is_null() => {
                    d.strs.push((*p).clone());
                    *slot = FieldKind::Bytes(d.strs.last().unwrap() as *const Vec<u8>);
                }
                FieldKind::Msg(p) if !p.is_null() => {
                    let child = arena.alloc_msg((*p).mt);
                    copy_msg(child, p, arena);
                    *slot = FieldKind::Msg(child);
                }
                FieldKind::Repeated(p) if !p.is_null() => {
                    *slot = clone_field_kind(FieldKind::Repeated(p), arena);
                }
                FieldKind::Map(p) if !p.is_null() => {
                    *slot = clone_field_kind(FieldKind::Map(p), arena);
                }
                _ => {}
            }
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct OwnedMessageInner<T> {
    ptr: MessagePtr<T>,
    arena: Arena,
}

impl<T: AssociatedMiniTable> Default for OwnedMessageInner<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AssociatedMiniTable> OwnedMessageInner<T> {
    pub fn new() -> Self {
        let arena = Arena::new();
        let ptr = MessagePtr::new(&arena).expect("alloc");
        Self { ptr, arena }
    }

    pub fn ptr_mut(&mut self) -> MessagePtr<T> {
        self.ptr
    }
    pub fn ptr(&self) -> MessagePtr<T> {
        self.ptr
    }
    pub fn raw(&self) -> RawMessage {
        self.ptr.raw()
    }
    pub fn arena(&mut self) -> &Arena {
        &self.arena
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct MessageMutInner<'msg, T> {
    pub ptr: MessagePtr<T>,
    pub arena: &'msg Arena,
}

impl<'msg, T> MessageMutInner<'msg, T> {
    pub fn mut_of_owned(msg: &'msg mut OwnedMessageInner<T>) -> Self {
        Self {
            ptr: msg.ptr,
            arena: &msg.arena,
        }
    }
    pub fn from_parent<ParentT>(
        parent_msg: MessageMutInner<'msg, ParentT>,
        ptr: MessagePtr<T>,
    ) -> Self {
        Self {
            ptr,
            arena: parent_msg.arena,
        }
    }
    pub fn ptr_mut(&mut self) -> MessagePtr<T> {
        self.ptr
    }
    pub fn ptr(&self) -> MessagePtr<T> {
        self.ptr
    }
    pub fn raw(&self) -> RawMessage {
        self.ptr.raw()
    }
    pub fn arena(&self) -> &Arena {
        self.arena
    }
    pub fn as_view(&self) -> MessageViewInner<'msg, T> {
        MessageViewInner {
            ptr: self.ptr,
            _phantom: PhantomData,
        }
    }
    pub fn reborrow<'shorter>(&mut self) -> MessageMutInner<'shorter, T>
    where
        'msg: 'shorter,
    {
        Self {
            ptr: self.ptr,
            arena: self.arena,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct MessageViewInner<'msg, T> {
    ptr: MessagePtr<T>,
    _phantom: PhantomData<&'msg ()>,
}

impl<T> Copy for MessageViewInner<'_, T> {}
impl<T> Clone for MessageViewInner<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'msg, T> MessageViewInner<'msg, T> {
    pub unsafe fn wrap(ptr: MessagePtr<T>) -> Self {
        Self {
            ptr,
            _phantom: PhantomData,
        }
    }
    pub fn view_of_owned(owned: &'msg OwnedMessageInner<T>) -> Self {
        Self {
            ptr: owned.ptr,
            _phantom: PhantomData,
        }
    }
    pub fn ptr(&self) -> MessagePtr<T> {
        self.ptr
    }
    pub fn raw(&self) -> RawMessage {
        self.ptr.raw()
    }
}

struct EmptyMsg(MsgData);
unsafe impl Sync for EmptyMsg {}
unsafe impl Send for EmptyMsg {}

impl<T: AssociatedMiniTable> Default for MessageViewInner<'static, T> {
    fn default() -> Self {
        static EMPTY: OnceLock<EmptyMsg> = OnceLock::new();
        let raw = EMPTY.get_or_init(|| {
            EmptyMsg(MsgData {
                slots: Vec::new(),
                has: Vec::new(),
                strs: Vec::new(),
                unknown: UnknownFields::default(),
                mt: MiniTablePtr::dangling(),
            })
        });
        MessageViewInner {
            ptr: MessagePtr {
                raw: (&raw.0 as *const MsgData as *mut MsgData),
                _phantom: PhantomData,
            },
            _phantom: PhantomData,
        }
    }
}

/// MiniTable associated with a generated message.
///
/// # Safety
/// `mini_table()` must return the table rust_out linked for this type.
pub unsafe trait AssociatedMiniTable {
    fn mini_table() -> MiniTablePtr;
}

/// MiniTable associated with a generated enum.
///
/// # Safety
/// Only generated enums implement this.
pub unsafe trait AssociatedMiniTableEnum {
    fn mini_table() -> MiniTableEnumPtr {
        std::ptr::null()
    }
}

/// Access the arena that owns a message.
///
/// # Safety
/// The returned arena must outlive the message pointer.
pub unsafe trait UpbGetArena {
    fn get_arena(&mut self, _private: Private) -> &Arena;
}

/// Read pointer to the kernel message.
///
/// # Safety
/// The pointer must be valid for the lifetime of `self`.
pub unsafe trait UpbGetMessagePtr {
    type Msg;
    fn get_ptr(&self, _private: Private) -> MessagePtr<Self::Msg>;
}

/// Mutable pointer to the kernel message.
///
/// # Safety
/// The pointer must be valid for exclusive mutation through `self`.
pub unsafe trait UpbGetMessagePtrMut {
    type Msg;
    fn get_ptr_mut(&mut self, _private: Private) -> MessagePtr<Self::Msg>;
}

pub trait OwnedMessageInterop: SealedInternal {}
impl<T: Message> OwnedMessageInterop for T {}

pub trait MessageViewInterop<'msg>: SealedInternal {
    fn __unstable_as_raw_message(&self) -> *const std::ffi::c_void {
        std::ptr::null()
    }
    unsafe fn __unstable_wrap_raw_message(_raw: &'msg *const std::ffi::c_void) -> Self
    where
        Self: Sized,
    {
        unimplemented!("raw wrap")
    }
    unsafe fn __unstable_wrap_raw_message_unchecked_lifetime(_raw: *const std::ffi::c_void) -> Self
    where
        Self: Sized,
    {
        unimplemented!("raw wrap")
    }
}

pub trait MessageMutInterop<'msg>: SealedInternal {}
impl<'a, T: MessageMut<'a>> MessageMutInterop<'a> for T {}

impl<'a, T> MessageViewInterop<'a> for T
where
    Self: MessageView<'a> + From<MessageViewInner<'a, <Self as MessageView<'a>>::Message>>,
{
    fn __unstable_as_raw_message(&self) -> *const std::ffi::c_void {
        std::ptr::null()
    }
}

pub trait KernelMessage:
    AssociatedMiniTable + UpbGetArena + UpbGetMessagePtr + UpbGetMessagePtrMut + OwnedMessageInterop
{
}
impl<T> KernelMessage for T where
    T: AssociatedMiniTable
        + UpbGetArena
        + UpbGetMessagePtr
        + UpbGetMessagePtrMut
        + OwnedMessageInterop
{
}

pub trait KernelMessageView<'msg>:
    UpbGetMessagePtr + From<MessageViewInner<'msg, Self::KMessage>>
{
    type KMessage;
}
impl<'msg, T> KernelMessageView<'msg> for T
where
    T: UpbGetMessagePtr + From<MessageViewInner<'msg, T::Msg>>,
{
    type KMessage = T::Msg;
}

pub trait KernelMessageMut<'msg>:
    UpbGetMessagePtr + UpbGetMessagePtrMut + UpbGetArena + From<MessageMutInner<'msg, Self::KMessage>>
{
    type KMessage;
}
impl<'msg, T> KernelMessageMut<'msg> for T
where
    T: UpbGetMessagePtr
        + UpbGetMessagePtrMut
        + UpbGetArena
        + From<MessageMutInner<'msg, <T as UpbGetMessagePtr>::Msg>>,
{
    type KMessage = <T as UpbGetMessagePtr>::Msg;
}

pub fn debug_string<T: UpbGetMessagePtr>(_msg: &T) -> String {
    String::from("<msg>")
}

pub struct InnerProtoString(Vec<u8>, Arena);

impl InnerProtoString {
    pub fn into_raw_parts(self) -> (StringView, Arena) {
        let bytes = self.0;
        let arena = self.1;
        let leaked = Box::leak(bytes.into_boxed_slice());
        (StringView::from(&leaked[..]), arena)
    }
}

impl From<&[u8]> for InnerProtoString {
    fn from(v: &[u8]) -> Self {
        Self(v.to_vec(), Arena::new())
    }
}

impl ProtoString {
    #[doc(hidden)]
    pub fn into_inner(self, _private: Private) -> InnerProtoString {
        InnerProtoString(self.as_bytes().to_vec(), Arena::new())
    }
    #[doc(hidden)]
    pub fn from_inner(_private: Private, inner: InnerProtoString) -> ProtoString {
        ProtoString::from_bytes(&inner.0)
    }
}

impl ProtoBytes {
    #[doc(hidden)]
    pub fn into_inner(self, _private: Private) -> InnerProtoString {
        InnerProtoString(self.as_bytes().to_vec(), Arena::new())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InnerRepeatedMut<'msg> {
    pub raw: RawRepeatedField,
    pub arena: &'msg Arena,
}

impl<'msg> InnerRepeatedMut<'msg> {
    pub fn new(raw: RawRepeatedField, arena: &'msg Arena) -> Self {
        Self { raw, arena }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InnerMapMut<'msg> {
    pub raw: RawMap,
    pub arena: &'msg Arena,
}

impl<'msg> InnerMapMut<'msg> {
    pub fn new(raw: RawMap, arena: &'msg Arena) -> Self {
        Self { raw, arena }
    }
}

pub fn empty_array<T>() -> crate::repeated::RepeatedView<'static, T> {
    crate::repeated::RepeatedView::from_slice(&[])
}

pub fn empty_map<K: crate::map::MapKey, V: crate::map::MapValue>(
) -> crate::map::MapView<'static, K, V> {
    crate::map::MapView::from_slice(&[])
}

pub(crate) fn kernel_array_push<T: 'static>(
    raw: RawRepeatedField,
    value: T,
    arena: Option<&Arena>,
) {
    use std::any::TypeId;
    unsafe {
        let arr = &*raw;
        if TypeId::of::<T>() == TypeId::of::<ProtoString>() {
            let s = std::ptr::read(&value as *const T as *const ProtoString);
            std::mem::forget(value);
            let leaked = Box::leak(Box::new(s.as_bytes().to_vec()));
            arr.items
                .borrow_mut()
                .push(FieldKind::Bytes(leaked as *const Vec<u8>));
        } else if TypeId::of::<T>() == TypeId::of::<i32>() {
            let v = std::ptr::read(&value as *const T as *const i32);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::I32(v));
        } else if TypeId::of::<T>() == TypeId::of::<i64>() {
            let v = std::ptr::read(&value as *const T as *const i64);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::I64(v));
        } else if TypeId::of::<T>() == TypeId::of::<bool>() {
            let v = std::ptr::read(&value as *const T as *const bool);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::Bool(v));
        } else if TypeId::of::<T>() == TypeId::of::<ProtoBytes>() {
            let s = std::ptr::read(&value as *const T as *const ProtoBytes);
            std::mem::forget(value);
            let leaked = Box::leak(Box::new(s.as_bytes().to_vec()));
            arr.items
                .borrow_mut()
                .push(FieldKind::Bytes(leaked as *const Vec<u8>));
        } else if TypeId::of::<T>() == TypeId::of::<u32>() {
            let v = std::ptr::read(&value as *const T as *const u32);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::U32(v));
        } else if TypeId::of::<T>() == TypeId::of::<u64>() {
            let v = std::ptr::read(&value as *const T as *const u64);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::U64(v));
        } else if TypeId::of::<T>() == TypeId::of::<f32>() {
            let v = std::ptr::read(&value as *const T as *const f32);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::F32(v));
        } else if TypeId::of::<T>() == TypeId::of::<f64>() {
            let v = std::ptr::read(&value as *const T as *const f64);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::F64(v));
        } else if std::mem::size_of::<T>() == 4 && std::mem::align_of::<T>() == 4 {
            let v = std::ptr::read(&value as *const T as *const i32);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::I32(v));
        } else {
            arr.items.borrow_mut().push(adopt_owned_msg(value, arena));
        }
    }
}

pub(crate) unsafe fn kernel_repeated_get<'msg, T: crate::proxied::Proxied + 'static>(
    raw: RawRepeatedField,
    index: usize,
) -> Option<crate::proxied::View<'msg, T>> {
    let items = unsafe { (*raw).items.borrow() };
    let fk = *items.get(index)?;
    unsafe { kernel_fieldkind_to_view::<'msg, T>(fk) }
}

pub(crate) unsafe fn kernel_repeated_set<T: 'static>(
    raw: RawRepeatedField,
    index: usize,
    value: T,
) {
    let arr = unsafe { &*raw };
    if index >= arr.items.borrow().len() {
        std::mem::forget(value);
        return;
    }
    arr.items.borrow_mut().remove(index);
    kernel_array_push(raw, value, None);
    let mut items = arr.items.borrow_mut();
    let last = items.pop().unwrap();
    items.insert(index, last);
}

pub(crate) unsafe fn kernel_fieldkind_to_view<'msg, T: crate::proxied::Proxied + 'static>(
    fk: FieldKind,
) -> Option<crate::proxied::View<'msg, T>> {
    use crate::proxied::View;
    use std::any::TypeId;
    unsafe {
        if TypeId::of::<T>() == TypeId::of::<i32>() {
            let v = match fk {
                FieldKind::I32(v) => v,
                FieldKind::U32(v) => v as i32,
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<i64>() {
            let v = match fk {
                FieldKind::I64(v) => v,
                FieldKind::U64(v) => v as i64,
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<u32>() {
            let v = match fk {
                FieldKind::U32(v) => v,
                FieldKind::I32(v) => v as u32,
                FieldKind::F32(v) => v.to_bits(),
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<u64>() {
            let v = match fk {
                FieldKind::U64(v) => v,
                FieldKind::I64(v) => v as u64,
                FieldKind::F64(v) => v.to_bits(),
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<bool>() {
            let FieldKind::Bool(v) = fk else { return None };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<f32>() {
            let v = match fk {
                FieldKind::F32(v) => v,
                FieldKind::U32(v) => f32::from_bits(v),
                FieldKind::I32(v) => f32::from_bits(v as u32),
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<f64>() {
            let v = match fk {
                FieldKind::F64(v) => v,
                FieldKind::U64(v) => f64::from_bits(v),
                FieldKind::I64(v) => f64::from_bits(v as u64),
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<T>() == TypeId::of::<ProtoString>() {
            let FieldKind::Bytes(p) = fk else { return None };
            if p.is_null() {
                return None;
            }
            let s = ProtoStr::from_bytes((*p).as_slice());
            return Some(std::mem::transmute_copy(&s));
        }
        if TypeId::of::<T>() == TypeId::of::<ProtoBytes>() {
            let FieldKind::Bytes(p) = fk else { return None };
            if p.is_null() {
                return None;
            }
            let s: &[u8] = (*p).as_slice();
            return Some(std::mem::transmute_copy(&s));
        }
        if let FieldKind::Msg(p) = fk {
            if p.is_null() {
                return None;
            }
            return kernel_msg_ptr_to_view::<'msg, T>(p);
        }
        if std::mem::size_of::<View<'msg, T>>() == 4 {
            let v = match fk {
                FieldKind::I32(x) => x,
                FieldKind::U32(x) => x as i32,
                FieldKind::F32(x) => x.to_bits() as i32,
                _ => return None,
            };
            return Some(std::mem::transmute_copy(&v));
        }
        let _ = fk;
        None
    }
}

unsafe fn kernel_msg_ptr_to_view<'msg, T: crate::proxied::Proxied + 'static>(
    p: *mut MsgData,
) -> Option<crate::proxied::View<'msg, T>> {
    use crate::proxied::View;
    unsafe {
        let inner = MessageViewInner::<'msg, ()> {
            ptr: MessagePtr {
                raw: p,
                _phantom: PhantomData,
            },
            _phantom: PhantomData,
        };
        let sz = std::mem::size_of::<View<'msg, T>>();
        if sz == std::mem::size_of::<MessageViewInner<'msg, ()>>()
            || sz == std::mem::size_of::<*mut MsgData>()
        {
            return Some(std::mem::transmute_copy(&inner));
        }
        None
    }
}

pub(crate) unsafe fn kernel_msg_ptr_to_mut<'msg, T: crate::proxied::MutProxied + 'static>(
    p: *mut MsgData,
    arena: &'msg Arena,
) -> Option<crate::proxied::Mut<'msg, T>> {
    use crate::proxied::Mut;
    unsafe {
        let sz = std::mem::size_of::<Mut<'msg, T>>();
        let inner = MessageMutInner::<'msg, ()> {
            ptr: MessagePtr {
                raw: p,
                _phantom: PhantomData,
            },
            arena,
        };
        if sz == std::mem::size_of::<MessageMutInner<'msg, ()>>() {
            return Some(std::mem::transmute_copy(&inner));
        }
        None
    }
}

pub(crate) fn kernel_clone_msg(src: *mut MsgData, arena: &Arena) -> *mut MsgData {
    if src.is_null() {
        return src;
    }
    unsafe {
        let dst = arena.alloc_msg((*src).mt);
        copy_msg(dst, src, arena);
        dst
    }
}

/// rust_out owned messages are `{ inner: OwnedMessageInner<T> }` with `ptr` first.
#[repr(C)]
struct OwnedMsgHead {
    raw: *mut MsgData,
    arena: Arena,
}

fn adopt_owned_msg<T>(value: T, parent: Option<&Arena>) -> FieldKind {
    if std::mem::size_of::<T>() < std::mem::size_of::<OwnedMsgHead>() {
        std::mem::forget(value);
        return FieldKind::Empty;
    }
    let head = unsafe { std::ptr::read(&value as *const T as *const OwnedMsgHead) };
    std::mem::forget(value);
    if head.raw.is_null() {
        return FieldKind::Empty;
    }
    if let Some(parent) = parent {
        parent.fuse(&head.arena);
        FieldKind::Msg(head.raw)
    } else {
        let _ = Box::leak(Box::new(head.arena));
        FieldKind::Msg(head.raw)
    }
}

pub(crate) unsafe fn kernel_bytes_to_view<'msg, K: crate::proxied::Proxied + 'static>(
    bytes: &'msg [u8],
) -> Option<crate::proxied::View<'msg, K>> {
    use std::any::TypeId;
    unsafe {
        if TypeId::of::<K>() == TypeId::of::<i32>() && bytes.len() >= 4 {
            let v = i32::from_le_bytes(bytes[..4].try_into().ok()?);
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<K>() == TypeId::of::<i64>() && bytes.len() >= 8 {
            let v = i64::from_le_bytes(bytes[..8].try_into().ok()?);
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<K>() == TypeId::of::<u32>() && bytes.len() >= 4 {
            let v = u32::from_le_bytes(bytes[..4].try_into().ok()?);
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<K>() == TypeId::of::<u64>() && bytes.len() >= 8 {
            let v = u64::from_le_bytes(bytes[..8].try_into().ok()?);
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<K>() == TypeId::of::<bool>() && !bytes.is_empty() {
            let v = bytes[0] != 0;
            return Some(std::mem::transmute_copy(&v));
        }
        if TypeId::of::<K>() == TypeId::of::<ProtoString>() {
            let s = ProtoStr::from_bytes(bytes);
            return Some(std::mem::transmute_copy(&s));
        }
        None
    }
}

pub(crate) unsafe fn kernel_repeated_get_mut<'msg, T: crate::proxied::MutProxied + 'static>(
    raw: RawRepeatedField,
    index: usize,
    arena: &'msg Arena,
) -> Option<crate::proxied::Mut<'msg, T>> {
    let items = unsafe { (*raw).items.borrow() };
    let FieldKind::Msg(p) = *items.get(index)? else {
        return None;
    };
    if p.is_null() {
        return None;
    }
    unsafe { kernel_msg_ptr_to_mut::<T>(p, arena) }
}

pub(crate) fn kernel_key_bytes<K: 'static>(key: K) -> Vec<u8> {
    use std::any::TypeId;
    unsafe {
        if TypeId::of::<K>() == TypeId::of::<ProtoString>() {
            let s = std::ptr::read(&key as *const K as *const ProtoString);
            std::mem::forget(key);
            s.as_bytes().to_vec()
        } else if TypeId::of::<K>() == TypeId::of::<i32>() {
            let v = std::ptr::read(&key as *const K as *const i32);
            std::mem::forget(key);
            v.to_le_bytes().to_vec()
        } else if TypeId::of::<K>() == TypeId::of::<i64>() {
            let v = std::ptr::read(&key as *const K as *const i64);
            std::mem::forget(key);
            v.to_le_bytes().to_vec()
        } else if TypeId::of::<K>() == TypeId::of::<u32>() {
            let v = std::ptr::read(&key as *const K as *const u32);
            std::mem::forget(key);
            v.to_le_bytes().to_vec()
        } else if TypeId::of::<K>() == TypeId::of::<u64>() {
            let v = std::ptr::read(&key as *const K as *const u64);
            std::mem::forget(key);
            v.to_le_bytes().to_vec()
        } else if TypeId::of::<K>() == TypeId::of::<bool>() {
            let v = std::ptr::read(&key as *const K as *const bool);
            std::mem::forget(key);
            vec![v as u8]
        } else {
            std::mem::forget(key);
            Vec::new()
        }
    }
}

fn kernel_value_kind<V: 'static>(value: V, arena: Option<&Arena>) -> FieldKind {
    use std::any::TypeId;
    unsafe {
        if TypeId::of::<V>() == TypeId::of::<i32>() {
            let v = std::ptr::read(&value as *const V as *const i32);
            std::mem::forget(value);
            FieldKind::I32(v)
        } else if TypeId::of::<V>() == TypeId::of::<i64>() {
            let v = std::ptr::read(&value as *const V as *const i64);
            std::mem::forget(value);
            FieldKind::I64(v)
        } else if TypeId::of::<V>() == TypeId::of::<u32>() {
            let v = std::ptr::read(&value as *const V as *const u32);
            std::mem::forget(value);
            FieldKind::U32(v)
        } else if TypeId::of::<V>() == TypeId::of::<u64>() {
            let v = std::ptr::read(&value as *const V as *const u64);
            std::mem::forget(value);
            FieldKind::U64(v)
        } else if TypeId::of::<V>() == TypeId::of::<bool>() {
            let v = std::ptr::read(&value as *const V as *const bool);
            std::mem::forget(value);
            FieldKind::Bool(v)
        } else if TypeId::of::<V>() == TypeId::of::<f32>() {
            let v = std::ptr::read(&value as *const V as *const f32);
            std::mem::forget(value);
            FieldKind::F32(v)
        } else if TypeId::of::<V>() == TypeId::of::<f64>() {
            let v = std::ptr::read(&value as *const V as *const f64);
            std::mem::forget(value);
            FieldKind::F64(v)
        } else if TypeId::of::<V>() == TypeId::of::<ProtoString>()
            || TypeId::of::<V>() == TypeId::of::<ProtoBytes>()
        {
            let s = if TypeId::of::<V>() == TypeId::of::<ProtoString>() {
                let p = std::ptr::read(&value as *const V as *const ProtoString);
                std::mem::forget(value);
                p.as_bytes().to_vec()
            } else {
                let p = std::ptr::read(&value as *const V as *const ProtoBytes);
                std::mem::forget(value);
                p.as_bytes().to_vec()
            };
            let leaked = Box::leak(Box::new(s));
            FieldKind::Bytes(leaked as *const Vec<u8>)
        } else if std::mem::size_of::<V>() == 4 {
            let v = std::ptr::read(&value as *const V as *const i32);
            std::mem::forget(value);
            FieldKind::I32(v)
        } else {
            adopt_owned_msg(value, arena)
        }
    }
}

pub(crate) fn kernel_map_len(raw: RawMap) -> usize {
    unsafe {
        let e = (*raw).entries.borrow();
        let mut n = 0;
        for (i, (k, _)) in e.iter().enumerate() {
            if e[i + 1..].iter().all(|(k2, _)| k2 != k) {
                n += 1;
            }
        }
        n
    }
}

pub(crate) fn kernel_map_insert<K: 'static, V: 'static>(
    raw: RawMap,
    key: K,
    value: V,
    arena: Option<&Arena>,
) -> bool {
    let kb = kernel_key_bytes(key);
    let fk = kernel_value_kind(value, arena);
    unsafe {
        let mut entries = (*raw).entries.borrow_mut();
        if let Some(e) = entries.iter_mut().rev().find(|(k, _)| *k == kb) {
            e.1 = fk;
            false
        } else {
            entries.push((kb, fk));
            true
        }
    }
}

pub(crate) unsafe fn kernel_map_get_bytes<'msg, V>(
    raw: RawMap,
    kb: &[u8],
) -> Option<crate::proxied::View<'msg, V>>
where
    V: crate::proxied::Proxied + 'static,
{
    unsafe {
        let entries = (*raw).entries.borrow();
        let fk = entries
            .iter()
            .rev()
            .find(|(k, _)| k == kb)
            .map(|(_, v)| *v)?;
        kernel_fieldkind_to_view::<'msg, V>(fk)
    }
}

pub unsafe fn message_set_string_field<'msg, P: Message + AssociatedMiniTable>(
    parent: MessageMutInner<'msg, P>,
    index: u32,
    val: impl IntoProxied<ProtoString>,
) {
    let s = val.into_proxied(Private);
    unsafe {
        parent
            .ptr
            .set_base_field_string_at_index(index, StringView::from(s.as_bytes()));
    }
}

pub unsafe fn message_set_bytes_field<'msg, P: Message + AssociatedMiniTable>(
    parent: MessageMutInner<'msg, P>,
    index: u32,
    val: impl IntoProxied<ProtoBytes>,
) {
    let s = val.into_proxied(Private);
    unsafe {
        parent
            .ptr
            .set_base_field_string_at_index(index, StringView::from(s.as_bytes()));
    }
}

pub unsafe fn message_set_sub_message<
    'msg,
    P: Message + AssociatedMiniTable,
    T: Message + UpbGetMessagePtrMut + UpbGetArena,
>(
    parent: MessageMutInner<'msg, P>,
    index: u32,
    val: impl IntoProxied<T>,
) {
    let mut child = val.into_proxied(Private);
    parent.arena.fuse(child.get_arena(Private));
    let child_ptr = child.get_ptr_mut(Private);
    unsafe {
        parent.ptr.set_base_field_message_at_index(index, child_ptr);
    }
}

pub unsafe fn message_set_repeated_field<
    'msg,
    P: Message + AssociatedMiniTable,
    T: Clone + 'static,
>(
    parent: MessageMutInner<'msg, P>,
    index: u32,
    val: impl IntoProxied<crate::repeated::Repeated<T>>,
) {
    let child = val.into_proxied(Private);
    let arr = parent.arena.alloc_array();
    for item in child.as_slice() {
        kernel_array_push(arr, item.clone(), Some(parent.arena));
    }
    unsafe {
        parent.ptr.set_array_at_index(index, arr);
    }
}

pub unsafe fn message_set_map_field<
    'msg,
    P: Message + AssociatedMiniTable,
    K: crate::map::MapKey,
    V: crate::map::MapValue,
>(
    parent: MessageMutInner<'msg, P>,
    index: u32,
    val: impl IntoProxied<crate::map::Map<K, V>>,
) {
    let child = val.into_proxied(Private);
    let m = parent.arena.alloc_map();
    for (k, v) in child.iter() {
        kernel_map_insert(m, k.clone(), v.clone(), Some(parent.arena));
    }
    unsafe {
        parent.ptr.set_map_at_index(index, m);
    }
}

pub mod __unstable {
    pub struct DescriptorInfo {
        pub descriptor: &'static [u8],
        pub deps: &'static [&'static DescriptorInfo],
    }
}

const FROM92: [i8; 95] = [
    0, 1, -1, 2, 3, 4, 5, -1, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
    48, 49, 50, 51, 52, 53, 54, 55, 56, 57, -1, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70,
    71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91,
];

fn from92(ch: u8) -> i8 {
    if !(b' '..=b'~').contains(&ch) {
        return -1;
    }
    FROM92[(ch - b' ') as usize]
}

fn encoded_type(ty: i8) -> FieldType {
    match ty {
        0 => FieldType::Double,
        1 => FieldType::Float,
        2 => FieldType::Fixed32,
        3 => FieldType::Fixed64,
        4 => FieldType::SFixed32,
        5 => FieldType::SFixed64,
        6 => FieldType::Int32,
        7 => FieldType::UInt32,
        8 => FieldType::SInt32,
        9 => FieldType::Int64,
        10 => FieldType::UInt64,
        11 => FieldType::SInt64,
        12 => FieldType::Enum,
        13 => FieldType::Bool,
        14 => FieldType::Bytes,
        15 => FieldType::String,
        16 => FieldType::Group,
        17 => FieldType::Message,
        18 => FieldType::Enum,
        _ => FieldType::Int32,
    }
}

fn decode_base92_varint(s: &[u8], i: &mut usize, first: u8, min: u8, max: u8) -> u32 {
    let mut val = 0u32;
    let mut shift = 0u32;
    let bits = {
        let span = (from92(max) - from92(min)) as u32;
        32 - span.leading_zeros()
    };
    let mut ch = first;
    loop {
        let bits_val = (from92(ch) - from92(min)) as u32;
        val |= bits_val << shift;
        if *i >= s.len() || s[*i] < min || s[*i] > max {
            return val;
        }
        ch = s[*i];
        *i += 1;
        shift += bits;
        if shift >= 32 {
            return val;
        }
    }
}

pub unsafe fn build_mini_table(mini_descriptor: &'static str) -> MiniTablePtr {
    let mt = decode_mini_table(mini_descriptor.as_bytes());
    MiniTablePtr(Box::into_raw(Box::new(mt)))
}

pub unsafe fn build_enum_mini_table(_mini_descriptor: &'static str) -> MiniTableEnumPtr {
    std::ptr::null()
}

pub unsafe fn link_mini_table(
    mini_table: MiniTablePtr,
    submessages: &[MiniTablePtr],
    _subenums: &[MiniTableEnumPtr],
) {
    if mini_table.0.is_null() {
        return;
    }
    let mt = unsafe { &mut *mini_table.0.cast_mut() };
    let mut si = 0usize;
    for f in &mut mt.fields {
        if (f.ty == FieldType::Message || f.ty == FieldType::Group || f.is_map)
            && si < submessages.len()
        {
            f.sub = submessages[si];
            si += 1;
        }
    }
    for f in &mut mt.fields {
        if !f.sub.0.is_null() {
            if let Some(sub) = unsafe { f.sub.0.as_ref() } {
                if sub.is_map {
                    f.is_map = true;
                }
            }
        }
    }
}

fn decode_mini_table(bytes: &[u8]) -> MiniTable {
    if bytes.is_empty() {
        return MiniTable {
            fields: Vec::new(),
            is_map: false,
            enforce_utf8: false,
        };
    }
    let mut i = 0usize;
    let ver = bytes[i];
    i += 1;
    let is_map = ver == b'%';
    let mut fields = Vec::new();
    let mut last_num = 0u32;
    let mut enforce_utf8 = false;
    while i < bytes.len() {
        let ch = bytes[i];
        i += 1;
        if ch <= b'I' {
            last_num += 1;
            let mut tyv = from92(ch);
            let mut repeated = false;
            if tyv >= 20 {
                tyv -= 20;
                repeated = true;
            }
            let ty = encoded_type(tyv);
            let packable = !matches!(
                ty,
                FieldType::String | FieldType::Bytes | FieldType::Message | FieldType::Group
            );
            fields.push(MiniField {
                number: last_num,
                ty,
                repeated,
                packed: repeated && packable,
                proto3_singular: false,
                required: false,
                is_map: false,
                sub: MiniTablePtr::dangling(),
                oneof_group: 0,
            });
        } else if (b'L'..=b'[').contains(&ch) {
            let modv = decode_base92_varint(bytes, &mut i, ch, b'L', b'[');
            if let Some(f) = fields.last_mut() {
                if modv & 1 != 0 {
                    f.packed = !f.packed;
                }
                if modv & 2 != 0 {
                    f.required = true;
                }
                if modv & 4 != 0 {
                    f.proto3_singular = true;
                }
                if modv & 8 != 0 {
                    f.proto3_singular = !f.proto3_singular;
                }
            } else if modv & 1 != 0 {
                enforce_utf8 = true;
            }
        } else if (b'_'..=b'~').contains(&ch) && ch != b'~' {
            let skip = decode_base92_varint(bytes, &mut i, ch, b'_', b'~');
            last_num = last_num.saturating_add(skip).saturating_sub(1);
        } else if ch == b'^' {
            let mut group = 1u32;
            while i < bytes.len() {
                let och = bytes[i];
                i += 1;
                if och == b'~' {
                    group += 1;
                    continue;
                }
                if och == b'|' {
                    continue;
                }
                let num = decode_base92_varint(bytes, &mut i, och, b' ', b'b');
                if let Some(f) = fields.iter_mut().find(|f| f.number == num) {
                    f.oneof_group = group;
                }
            }
            break;
        }
    }
    if is_map {
        for f in &mut fields {
            f.is_map = false;
        }
    }
    MiniTable {
        fields,
        is_map,
        enforce_utf8,
    }
}

impl MiniTable {
    fn field_by_number(&self, n: u32) -> Option<(usize, MiniField)> {
        self.fields
            .iter()
            .copied()
            .enumerate()
            .find(|(_, f)| f.number == n)
    }
}

fn parse_into(
    data: *mut MsgData,
    buf: &[u8],
    arena: &Arena,
    enforce_required: bool,
) -> Result<(), ParseError> {
    let mt = unsafe { (*data).mt };
    if mt.0.is_null() {
        return Ok(());
    }
    let table = unsafe { &*mt.0 };
    let mut pos = 0usize;
    while pos < buf.len() {
        let (num, wire) = decode_tag(buf, &mut pos)?;
        let Some((idx, f)) = table.field_by_number(num) else {
            skip_field(buf, &mut pos, wire)?;
            continue;
        };
        decode_field(data, idx, f, buf, &mut pos, wire, arena)?;
    }
    if enforce_required {
        unsafe {
            let has = &(*data).has;
            for (i, f) in table.fields.iter().enumerate() {
                if f.required && !has.get(i).copied().unwrap_or(false) {
                    return Err(ParseError::new("required"));
                }
            }
        }
    }
    Ok(())
}

fn decode_field(
    data: *mut MsgData,
    idx: usize,
    f: MiniField,
    buf: &[u8],
    pos: &mut usize,
    wire: u32,
    arena: &Arena,
) -> Result<(), ParseError> {
    let ptr = MessagePtr::<()> {
        raw: data,
        _phantom: PhantomData,
    };
    if f.is_map {
        if wire != WIRE_LEN {
            skip_field(buf, pos, wire)?;
            return Ok(());
        }
        let payload = read_len_bytes(buf, pos)?;
        let map = unsafe { ptr.get_or_create_mutable_map_at_index(idx as u32, arena) }
            .ok_or_else(|| ParseError::new("map alloc"))?;
        let (k, v) = decode_map_entry(f.sub, payload, arena)?;
        unsafe {
            (*map).entries.borrow_mut().push((k, v));
        }
        return Ok(());
    }
    if f.repeated && !f.is_map {
        let arr = unsafe { ptr.get_or_create_mutable_array_at_index(idx as u32, arena) }
            .ok_or_else(|| ParseError::new("array alloc"))?;
        if f.packed && wire == WIRE_LEN {
            let payload = read_len_bytes(buf, pos)?;
            let mut p = 0usize;
            while p < payload.len() {
                let v = decode_packed_item(f.ty, payload, &mut p)?;
                unsafe {
                    (*arr).items.borrow_mut().push(v);
                }
            }
            return Ok(());
        }
        let v = decode_one(f, buf, pos, wire, arena)?;
        unsafe {
            (*arr).items.borrow_mut().push(v);
        }
        ptr.set_slot(idx as u32, FieldKind::Repeated(arr), true);
        return Ok(());
    }
    if f.ty == FieldType::Message && !f.repeated {
        let payload = read_len_bytes(buf, pos)?;
        let child = arena.alloc_msg(f.sub);
        parse_into(child, payload, arena, true)?;
        ptr.set_slot(idx as u32, FieldKind::Msg(child), true);
        return Ok(());
    }
    if matches!(f.ty, FieldType::String | FieldType::Bytes) {
        let payload = read_len_bytes(buf, pos)?;
        if f.ty == FieldType::String {
            let enforce = unsafe {
                (*data)
                    .mt
                    .0
                    .as_ref()
                    .map(|t| t.enforce_utf8)
                    .unwrap_or(false)
            };
            if enforce && std::str::from_utf8(payload).is_err() {
                return Err(ParseError::new("utf8"));
            }
        }
        unsafe {
            ptr.set_base_field_string_at_index(idx as u32, StringView::from(payload));
        }
        return Ok(());
    }
    let v = decode_one(f, buf, pos, wire, arena)?;
    ptr.set_slot(idx as u32, v, true);
    Ok(())
}

fn decode_map_entry(
    sub: MiniTablePtr,
    buf: &[u8],
    arena: &Arena,
) -> Result<(Vec<u8>, FieldKind), ParseError> {
    let table = unsafe { sub.0.as_ref() };
    let mut key = Vec::new();
    let mut val = FieldKind::Empty;
    let mut pos = 0usize;
    while pos < buf.len() {
        let (num, wire) = decode_tag(buf, &mut pos)?;
        let field = table.and_then(|t| t.fields.iter().copied().find(|f| f.number == num));
        match num {
            1 => match field.map(|f| f.ty) {
                Some(FieldType::String) | Some(FieldType::Bytes) | None => {
                    if wire == WIRE_LEN {
                        key = read_len_bytes(buf, &mut pos)?.to_vec();
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
                Some(FieldType::Bool) => {
                    if wire == WIRE_VARINT {
                        key = vec![if decode_varint(buf, &mut pos)? != 0 {
                            1
                        } else {
                            0
                        }];
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
                Some(FieldType::Fixed32 | FieldType::SFixed32) => {
                    key = read_fixed32(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(FieldType::Fixed64 | FieldType::SFixed64) => {
                    key = read_fixed64(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(FieldType::SInt32) => {
                    let v = decode_zigzag32(decode_varint(buf, &mut pos)?);
                    key = v.to_le_bytes().to_vec();
                }
                Some(FieldType::SInt64) => {
                    let v = decode_zigzag64(decode_varint(buf, &mut pos)?);
                    key = v.to_le_bytes().to_vec();
                }
                Some(FieldType::Int64 | FieldType::UInt64) => {
                    key = decode_varint(buf, &mut pos)?.to_le_bytes().to_vec();
                }
                Some(_) => {
                    if wire == WIRE_VARINT {
                        key = (decode_varint(buf, &mut pos)? as i32)
                            .to_le_bytes()
                            .to_vec();
                    } else {
                        skip_field(buf, &mut pos, wire)?;
                    }
                }
            },
            2 => {
                if let Some(f) = field {
                    val = decode_one(f, buf, &mut pos, wire, arena)?;
                } else if wire == WIRE_VARINT {
                    val = FieldKind::I32(decode_varint(buf, &mut pos)? as i32);
                } else {
                    skip_field(buf, &mut pos, wire)?;
                }
            }
            _ => skip_field(buf, &mut pos, wire)?,
        }
    }
    Ok((key, val))
}

fn decode_packed_item(ty: FieldType, buf: &[u8], pos: &mut usize) -> Result<FieldKind, ParseError> {
    match ty {
        FieldType::Int32 | FieldType::Enum => Ok(FieldKind::I32(decode_varint(buf, pos)? as i32)),
        FieldType::Int64 => Ok(FieldKind::I64(decode_varint(buf, pos)? as i64)),
        FieldType::UInt32 => Ok(FieldKind::U32(decode_varint(buf, pos)? as u32)),
        FieldType::UInt64 => Ok(FieldKind::U64(decode_varint(buf, pos)?)),
        FieldType::SInt32 => Ok(FieldKind::I32(decode_zigzag32(decode_varint(buf, pos)?))),
        FieldType::SInt64 => Ok(FieldKind::I64(decode_zigzag64(decode_varint(buf, pos)?))),
        FieldType::Bool => Ok(FieldKind::Bool(decode_varint(buf, pos)? != 0)),
        FieldType::Fixed32 | FieldType::SFixed32 | FieldType::Float => {
            Ok(FieldKind::U32(read_fixed32(buf, pos)?))
        }
        FieldType::Fixed64 | FieldType::SFixed64 | FieldType::Double => {
            Ok(FieldKind::U64(read_fixed64(buf, pos)?))
        }
        _ => Err(ParseError::new("bad packed type")),
    }
}

fn decode_one(
    f: MiniField,
    buf: &[u8],
    pos: &mut usize,
    wire: u32,
    arena: &Arena,
) -> Result<FieldKind, ParseError> {
    match f.ty {
        FieldType::Int32 | FieldType::Enum => {
            if wire != WIRE_VARINT {
                skip_field(buf, pos, wire)?;
                return Ok(FieldKind::Empty);
            }
            Ok(FieldKind::I32(decode_varint(buf, pos)? as i32))
        }
        FieldType::Int64 => Ok(FieldKind::I64(decode_varint(buf, pos)? as i64)),
        FieldType::UInt32 => Ok(FieldKind::U32(decode_varint(buf, pos)? as u32)),
        FieldType::UInt64 => Ok(FieldKind::U64(decode_varint(buf, pos)?)),
        FieldType::SInt32 => Ok(FieldKind::I32(decode_zigzag32(decode_varint(buf, pos)?))),
        FieldType::SInt64 => Ok(FieldKind::I64(decode_zigzag64(decode_varint(buf, pos)?))),
        FieldType::Bool => Ok(FieldKind::Bool(decode_varint(buf, pos)? != 0)),
        FieldType::Fixed32 | FieldType::SFixed32 => Ok(FieldKind::U32(read_fixed32(buf, pos)?)),
        FieldType::Fixed64 | FieldType::SFixed64 => Ok(FieldKind::U64(read_fixed64(buf, pos)?)),
        FieldType::Float => Ok(FieldKind::F32(f32::from_bits(read_fixed32(buf, pos)?))),
        FieldType::Double => Ok(FieldKind::F64(f64::from_bits(read_fixed64(buf, pos)?))),
        FieldType::String | FieldType::Bytes => {
            let p = read_len_bytes(buf, pos)?;
            Ok(FieldKind::Bytes(
                Box::leak(Box::new(p.to_vec())) as *const Vec<u8>
            ))
        }
        FieldType::Message => {
            if wire != WIRE_LEN {
                skip_field(buf, pos, wire)?;
                return Ok(FieldKind::Empty);
            }
            let payload = read_len_bytes(buf, pos)?;
            let child = arena.alloc_msg(f.sub);
            parse_into(child, payload, arena, false)?;
            Ok(FieldKind::Msg(child))
        }
        _ => {
            skip_field(buf, pos, wire)?;
            Ok(FieldKind::Empty)
        }
    }
}

fn encode_map_key(ty: FieldType, k: &[u8], out: &mut Vec<u8>) {
    match ty {
        FieldType::String | FieldType::Bytes => encode_len_field(out, 1, k),
        FieldType::Bool => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, k.first().copied().unwrap_or(0) as u64);
        }
        FieldType::Fixed32 | FieldType::SFixed32 => {
            encode_tag(out, 1, WIRE_I32);
            let mut b = [0u8; 4];
            let n = k.len().min(4);
            b[..n].copy_from_slice(&k[..n]);
            out.extend_from_slice(&b);
        }
        FieldType::Fixed64 | FieldType::SFixed64 => {
            encode_tag(out, 1, WIRE_I64);
            let mut b = [0u8; 8];
            let n = k.len().min(8);
            b[..n].copy_from_slice(&k[..n]);
            out.extend_from_slice(&b);
        }
        FieldType::SInt32 => {
            encode_tag(out, 1, WIRE_VARINT);
            let v = i32_from_key_bytes(k);
            encode_varint(out, crate::wire::encode_zigzag32(v));
        }
        FieldType::SInt64 => {
            encode_tag(out, 1, WIRE_VARINT);
            let v = i64_from_key_bytes(k);
            encode_varint(out, crate::wire::encode_zigzag64(v));
        }
        FieldType::Int64 | FieldType::UInt64 => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, u64_from_key_bytes(k));
        }
        _ => {
            encode_tag(out, 1, WIRE_VARINT);
            encode_varint(out, i32_from_key_bytes(k) as i64 as u64);
        }
    }
}

fn i32_from_key_bytes(k: &[u8]) -> i32 {
    let mut b = [0u8; 4];
    let n = k.len().min(4);
    b[..n].copy_from_slice(&k[..n]);
    i32::from_le_bytes(b)
}

fn i64_from_key_bytes(k: &[u8]) -> i64 {
    let mut b = [0u8; 8];
    let n = k.len().min(8);
    b[..n].copy_from_slice(&k[..n]);
    i64::from_le_bytes(b)
}

fn u64_from_key_bytes(k: &[u8]) -> u64 {
    i64_from_key_bytes(k) as u64
}

fn encode_msg(data: *const MsgData, out: &mut Vec<u8>) {
    unsafe {
        let d = &*data;
        if d.mt.0.is_null() {
            return;
        }
        let table = &*d.mt.0;
        for (i, f) in table.fields.iter().enumerate() {
            if !d.has.get(i).copied().unwrap_or(false) && !f.repeated && !f.is_map {
                continue;
            }
            let slot = d.slots.get(i).copied().unwrap_or(FieldKind::Empty);
            encode_slot(f, slot, out);
        }
        d.unknown.encode(out);
    }
}

fn slot_u32(slot: FieldKind) -> u32 {
    match slot {
        FieldKind::U32(v) => v,
        FieldKind::I32(v) => v as u32,
        FieldKind::F32(v) => v.to_bits(),
        _ => 0,
    }
}

fn slot_u64(slot: FieldKind) -> u64 {
    match slot {
        FieldKind::U64(v) => v,
        FieldKind::I64(v) => v as u64,
        FieldKind::F64(v) => v.to_bits(),
        FieldKind::U32(v) => v as u64,
        FieldKind::I32(v) => v as u64,
        _ => 0,
    }
}

fn slot_i32(slot: FieldKind) -> i32 {
    match slot {
        FieldKind::I32(v) => v,
        FieldKind::U32(v) => v as i32,
        FieldKind::F32(v) => v.to_bits() as i32,
        _ => 0,
    }
}

fn slot_i64(slot: FieldKind) -> i64 {
    match slot {
        FieldKind::I64(v) => v,
        FieldKind::U64(v) => v as i64,
        FieldKind::I32(v) => v as i64,
        FieldKind::U32(v) => v as i64,
        FieldKind::F64(v) => v.to_bits() as i64,
        _ => 0,
    }
}

fn encode_i32_bits(out: &mut Vec<u8>, number: u32, bits: u32) {
    encode_tag(out, number, WIRE_I32);
    out.extend_from_slice(&bits.to_le_bytes());
}

fn encode_i64_bits(out: &mut Vec<u8>, number: u32, bits: u64) {
    encode_tag(out, number, WIRE_I64);
    out.extend_from_slice(&bits.to_le_bytes());
}

fn encode_packed_item(ty: FieldType, slot: FieldKind, out: &mut Vec<u8>) {
    match ty {
        FieldType::Float | FieldType::Fixed32 | FieldType::SFixed32 => {
            out.extend_from_slice(&slot_u32(slot).to_le_bytes());
        }
        FieldType::Double | FieldType::Fixed64 | FieldType::SFixed64 => {
            out.extend_from_slice(&slot_u64(slot).to_le_bytes());
        }
        FieldType::SInt32 => {
            encode_varint(out, crate::wire::encode_zigzag32(slot_i32(slot)));
        }
        FieldType::SInt64 => {
            encode_varint(out, crate::wire::encode_zigzag64(slot_i64(slot)));
        }
        FieldType::Bool => {
            let v = matches!(slot, FieldKind::Bool(true));
            encode_varint(out, v as u64);
        }
        FieldType::Int32 | FieldType::Enum => {
            encode_varint(out, slot_i32(slot) as i64 as u64);
        }
        FieldType::UInt32 => encode_varint(out, slot_u32(slot) as u64),
        FieldType::Int64 => encode_varint(out, slot_i64(slot) as u64),
        FieldType::UInt64 => encode_varint(out, slot_u64(slot)),
        _ => {}
    }
}

fn encode_scalar_slot(f: &MiniField, slot: FieldKind, out: &mut Vec<u8>) {
    match f.ty {
        FieldType::SInt32 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, crate::wire::encode_zigzag32(slot_i32(slot)));
        }
        FieldType::SInt64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, crate::wire::encode_zigzag64(slot_i64(slot)));
        }
        FieldType::Fixed32 | FieldType::SFixed32 | FieldType::Float => {
            encode_i32_bits(out, f.number, slot_u32(slot));
        }
        FieldType::Fixed64 | FieldType::SFixed64 | FieldType::Double => {
            encode_i64_bits(out, f.number, slot_u64(slot));
        }
        FieldType::Int32 | FieldType::Enum => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_i32(slot) as i64 as u64);
        }
        FieldType::UInt32 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_u32(slot) as u64);
        }
        FieldType::Int64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_i64(slot) as u64);
        }
        FieldType::UInt64 => {
            encode_tag(out, f.number, WIRE_VARINT);
            encode_varint(out, slot_u64(slot));
        }
        FieldType::Bool => {
            encode_tag(out, f.number, WIRE_VARINT);
            let v = matches!(slot, FieldKind::Bool(true)) || slot_u32(slot) != 0;
            encode_varint(out, v as u64);
        }
        _ => {}
    }
}

fn encode_slot(f: &MiniField, slot: FieldKind, out: &mut Vec<u8>) {
    match slot {
        FieldKind::Empty => {}
        FieldKind::Bytes(p) if !p.is_null() => unsafe {
            encode_len_field(out, f.number, &*p);
        },
        FieldKind::Msg(p) if !p.is_null() => {
            let mut tmp = Vec::new();
            encode_msg(p, &mut tmp);
            encode_len_field(out, f.number, &tmp);
        }
        FieldKind::Repeated(p) if !p.is_null() => unsafe {
            if f.packed {
                let mut payload = Vec::new();
                for item in (*p).items.borrow().iter() {
                    encode_packed_item(f.ty, *item, &mut payload);
                }
                if !payload.is_empty() {
                    encode_len_field(out, f.number, &payload);
                }
            } else {
                for item in (*p).items.borrow().iter() {
                    encode_slot(
                        &MiniField {
                            repeated: false,
                            packed: false,
                            ..*f
                        },
                        *item,
                        out,
                    );
                }
            }
        },
        FieldKind::Map(p) if !p.is_null() => unsafe {
            let (key_ty, val_f) = f
                .sub
                .0
                .as_ref()
                .map(|t| {
                    let kf = t
                        .fields
                        .iter()
                        .find(|x| x.number == 1)
                        .map(|x| x.ty)
                        .unwrap_or(FieldType::String);
                    let vf =
                        t.fields
                            .iter()
                            .copied()
                            .find(|x| x.number == 2)
                            .unwrap_or(MiniField {
                                number: 2,
                                ty: FieldType::Int32,
                                repeated: false,
                                packed: false,
                                proto3_singular: false,
                                required: false,
                                is_map: false,
                                sub: MiniTablePtr::dangling(),
                                oneof_group: 0,
                            });
                    (kf, vf)
                })
                .unwrap_or((
                    FieldType::String,
                    MiniField {
                        number: 2,
                        ty: FieldType::Int32,
                        repeated: false,
                        packed: false,
                        proto3_singular: false,
                        required: false,
                        is_map: false,
                        sub: MiniTablePtr::dangling(),
                        oneof_group: 0,
                    },
                ));
            for (k, v) in (*p).entries.borrow().iter() {
                let mut tmp = Vec::new();
                encode_map_key(key_ty, k, &mut tmp);
                encode_slot(
                    &MiniField {
                        number: 2,
                        repeated: false,
                        packed: false,
                        is_map: false,
                        ..val_f
                    },
                    *v,
                    &mut tmp,
                );
                encode_len_field(out, f.number, &tmp);
            }
        },
        _ => encode_scalar_slot(f, slot, out),
    }
}

impl<T> Clear for T
where
    Self: SealedInternal + UpbGetMessagePtrMut,
{
    fn clear(&mut self) {
        unsafe { self.get_ptr_mut(Private).clear() }
    }
}

impl<T> ClearAndParse for T
where
    Self: SealedInternal + UpbGetMessagePtrMut + UpbGetArena,
{
    fn clear_and_parse(&mut self, data: &[u8]) -> Result<(), ParseError> {
        Clear::clear(self);
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            true,
        )
    }
    fn clear_and_parse_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        Clear::clear(self);
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            false,
        )
    }
    fn merge_from_bytes(&mut self, data: &[u8]) -> Result<(), ParseError> {
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            true,
        )
    }
    fn merge_from_bytes_dont_enforce_required(&mut self, data: &[u8]) -> Result<(), ParseError> {
        parse_into(
            self.get_ptr_mut(Private).raw,
            data,
            self.get_arena(Private),
            false,
        )
    }
}

impl<T> Serialize for T
where
    Self: SealedInternal + UpbGetMessagePtr,
{
    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        let mut out = Vec::new();
        encode_msg(self.get_ptr(Private).raw, &mut out);
        Ok(out)
    }
    fn serialized_len(&self) -> usize {
        self.serialize().map(|v| v.len()).unwrap_or(0)
    }
}

impl<T> CopyFrom for T
where
    Self: SealedInternal + AsView + UpbGetArena + UpbGetMessagePtr,
    Self::Proxied: AssociatedMiniTable,
    for<'a> View<'a, Self::Proxied>: UpbGetMessagePtr,
{
    fn copy_from(&mut self, src: impl AsView<Proxied = Self::Proxied>) {
        let src_ptr = src.as_view().get_ptr(Private);
        let dst = self.get_ptr(Private);
        let arena = self.get_arena(Private);
        copy_msg(dst.raw, src_ptr.raw, arena);
    }
}

impl<T> MergeFrom for T
where
    Self: SealedInternal + AsView + UpbGetArena + UpbGetMessagePtr,
    Self::Proxied: AssociatedMiniTable,
    for<'a> View<'a, Self::Proxied>: UpbGetMessagePtr,
{
    fn merge_from(&mut self, src: impl AsView<Proxied = Self::Proxied>) {
        if let Ok(bytes) = Serialize::serialize(&src.as_view()) {
            let _ = parse_into(
                self.get_ptr(Private).raw,
                &bytes,
                self.get_arena(Private),
                false,
            );
        }
    }
}

impl<T> crate::message::TakeFrom for T
where
    Self: CopyFrom
        + crate::proxied::AsMut<MutProxied = <Self as crate::proxied::AsView>::Proxied>
        + UpbGetMessagePtrMut,
    <Self as crate::proxied::AsView>::Proxied: crate::proxied::MutProxied,
    for<'a> crate::proxied::Mut<'a, <Self as crate::proxied::AsView>::Proxied>:
        Clear + AsView<Proxied = <Self as crate::proxied::AsView>::Proxied> + UpbGetMessagePtrMut,
{
    fn take_from(&mut self, mut src: impl crate::proxied::AsMut<MutProxied = Self::Proxied>) {
        let mut src = src.as_mut();
        CopyFrom::copy_from(self, AsView::as_view(&src));
        Clear::clear(&mut src);
    }
}

pub fn message_eq<T>(_a: &T, _b: &T) -> bool
where
    T: AsView + Debug,
{
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_mini_table_one_string() {
        let mt = decode_mini_table(b"$M1P");
        assert_eq!(mt.fields.len(), 1);
        assert_eq!(mt.fields[0].number, 1);
        assert_eq!(mt.fields[0].ty, FieldType::String);
    }
}
