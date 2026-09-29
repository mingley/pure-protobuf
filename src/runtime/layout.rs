#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::internal::{Private, SealedInternal};
use crate::message::{Clear, CopyFrom};
use crate::proxied::{AsView, View};
use crate::string::ProtoStr;
use crate::wire::UnknownFields;
use std::cell::RefCell;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Weak;
use std::slice;
use std::sync::OnceLock;

use super::{
    Arena, ArenaInner, AssociatedMiniTable, MiniTablePtr, UpbGetArena, UpbGetMessagePtr,
    UpbGetMessagePtrMut,
};

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
    pub strs: RefCell<Vec<Box<Vec<u8>>>>,
    pub(crate) owner: Weak<RefCell<ArenaInner>>,
}

#[derive(Debug)]
pub struct RawMapInner {
    pub entries: RefCell<Vec<(Vec<u8>, FieldKind)>>,
    pub strs: RefCell<Vec<Box<Vec<u8>>>>,
    pub(crate) owner: Weak<RefCell<ArenaInner>>,
}

#[derive(Debug)]
pub struct MsgData {
    pub slots: Vec<FieldKind>,
    pub has: Vec<bool>,
    #[allow(
        clippy::vec_box,
        reason = "FieldKind::Bytes holds pointers to these Vec headers across growth"
    )]
    pub strs: Vec<Box<Vec<u8>>>,
    pub unknown: UnknownFields,
    pub mt: MiniTablePtr,
}

#[repr(C)]
#[derive(Debug)]
pub struct MessagePtr<T> {
    pub(crate) raw: *mut MsgData,
    pub(crate) _phantom: PhantomData<T>,
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

    pub(crate) fn data(&self) -> &MsgData {
        unsafe { &*self.raw }
    }

    #[expect(
        clippy::mut_from_ref,
        reason = "MessagePtr::as_mut matches the 4.35.1-release kernel ABI"
    )]
    pub(crate) fn data_mut(&self) -> &mut MsgData {
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
        d.strs.push(Box::new(bytes));
        let p = d.strs.last().expect("string was just inserted").as_ref() as *const Vec<u8>;
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

    pub(crate) fn slot(self, index: u32) -> FieldKind {
        self.data()
            .slots
            .get(index as usize)
            .copied()
            .unwrap_or(FieldKind::Empty)
    }

    pub(crate) fn set_slot(self, index: u32, v: FieldKind, has: bool) {
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
            // SAFETY: p belongs to the live source field while this copy is made.
            FieldKind::Bytes(arena.alloc_bytes((*p).clone()))
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
                    d.strs.push(Box::new((*p).clone()));
                    *slot =
                        FieldKind::Bytes(d.strs.last().expect("string was just copied").as_ref());
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
    pub(crate) ptr: MessagePtr<T>,
    pub(crate) _phantom: PhantomData<&'msg ()>,
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

#[allow(
    clippy::vec_box,
    reason = "FieldKind::Bytes points at Vec headers that must stay put across storage growth"
)]
pub(crate) fn retain_bytes(storage: &RefCell<Vec<Box<Vec<u8>>>>, bytes: Vec<u8>) -> *const Vec<u8> {
    let mut strings = storage.borrow_mut();
    strings.push(Box::new(bytes));
    strings.last().expect("bytes were just inserted").as_ref()
}

#[allow(
    clippy::vec_box,
    reason = "FieldKind::Bytes points at Vec headers owned by these stable boxes"
)]
pub(crate) fn release_bytes(storage: &RefCell<Vec<Box<Vec<u8>>>>, kind: FieldKind) {
    if let FieldKind::Bytes(ptr) = kind {
        let mut strings = storage.borrow_mut();
        if let Some(index) = strings
            .iter()
            .position(|value| std::ptr::eq::<Vec<u8>>(value.as_ref(), ptr))
        {
            strings.swap_remove(index);
        }
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

pub(crate) fn adopt_owned_msg<T>(
    value: T,
    parent: Option<&Arena>,
    owner: &Weak<RefCell<ArenaInner>>,
) -> FieldKind {
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
    } else {
        let parent = Arena {
            inner: owner.upgrade().expect("raw collection outlived its arena"),
        };
        parent.fuse(&head.arena);
    }
    FieldKind::Msg(head.raw)
}

impl<T> Clear for T
where
    Self: SealedInternal + UpbGetMessagePtrMut,
{
    fn clear(&mut self) {
        unsafe { self.get_ptr_mut(Private).clear() }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::encode::encode_slot;
    use crate::runtime::{Arena, FieldType, MiniField, MiniTablePtr, kernel_array_push};
    use crate::string::ProtoString;

    fn string_field(repeated: bool) -> MiniField {
        MiniField {
            number: 1,
            ty: FieldType::String,
            repeated,
            packed: false,
            proto3_singular: false,
            required: false,
            is_map: false,
            sub: MiniTablePtr::dangling(),
            oneof_group: 0,
        }
    }

    #[test]
    fn cloned_repeated_strings_are_owned_by_the_new_arena() {
        let original = Arena::new();
        let raw = original.alloc_array();
        kernel_array_push(raw, ProtoString::from("beta"), Some(&original), None);
        let clone = Arena::new();
        let kind = clone_field_kind(FieldKind::Repeated(raw), &clone);
        drop(original);
        let mut encoded = Vec::new();
        encode_slot(&string_field(true), kind, &mut encoded);
        assert_eq!(encoded, [0x0a, 4, b'b', b'e', b't', b'a']);
    }
}
