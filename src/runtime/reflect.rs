#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::internal::{Private, SealedInternal};
use crate::message::{Message, MessageMut, MessageView};
use crate::proxied::{AsView, IntoProxied};
use crate::string::{ProtoBytes, ProtoStr, ProtoString};
use std::fmt::Debug;
use std::marker::PhantomData;

use super::{
    Arena, FieldKind, MessageMutInner, MessagePtr, MessageViewInner, MiniTableEnumPtr,
    MiniTablePtr, MsgData, StringView, kernel_array_push, kernel_map_insert,
};

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
        let ptr = arena.alloc_bytes(bytes);
        // SAFETY: ptr refers to storage owned by the returned arena.
        let view = unsafe { StringView::from((&*ptr).as_slice()) };
        (view, arena)
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

pub(crate) unsafe fn kernel_fieldkind_to_view<'msg, T: crate::proxied::Proxied + 'static>(
    fk: FieldKind,
) -> Option<crate::proxied::View<'msg, T>> {
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
        // PB07-F3: no 4-byte view without a named arm above exists, so the
        // former size-based transmute fallback was unreachable; it is
        // removed rather than fixed, since transmuting into an unknown
        // view type cannot be proven sound.
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

pub fn message_eq<T>(_a: &T, _b: &T) -> bool
where
    T: AsView + Debug,
{
    false
}
