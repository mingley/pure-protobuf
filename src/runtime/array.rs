#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::string::{ProtoBytes, ProtoString};
use std::fmt::Debug;

use super::{
    Arena, FieldKind, RawRepeatedField, adopt_owned_msg, kernel_collection_value_to_view,
    kernel_msg_ptr_to_mut, release_bytes, retain_bytes,
};

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

pub fn empty_array<T>() -> crate::repeated::RepeatedView<'static, T> {
    crate::repeated::RepeatedView::from_slice(&[])
}

pub(crate) fn kernel_array_push<T: 'static>(
    raw: RawRepeatedField,
    value: T,
    arena: Option<&Arena>,
    enum_codec: Option<super::KernelEnumCodec<T>>,
) {
    use std::any::TypeId;
    unsafe {
        let arr = &*raw;
        if let Some(codec) = enum_codec {
            arr.items
                .borrow_mut()
                .push(FieldKind::I32(codec.into_i32(value)));
        } else if TypeId::of::<T>() == TypeId::of::<ProtoString>() {
            let s = std::ptr::read(&value as *const T as *const ProtoString);
            std::mem::forget(value);
            arr.items.borrow_mut().push(FieldKind::Bytes(retain_bytes(
                &arr.strs,
                s.as_bytes().to_vec(),
            )));
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
            arr.items.borrow_mut().push(FieldKind::Bytes(retain_bytes(
                &arr.strs,
                s.as_bytes().to_vec(),
            )));
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
        } else {
            arr.items
                .borrow_mut()
                .push(adopt_owned_msg(value, arena, &arr.owner));
        }
    }
}

pub(crate) unsafe fn kernel_repeated_get<'msg, T: crate::proxied::Proxied + 'static>(
    raw: RawRepeatedField,
    index: usize,
    enum_codec: Option<super::KernelEnumCodec<T>>,
) -> Option<crate::proxied::View<'msg, T>> {
    let items = unsafe { (*raw).items.borrow() };
    let fk = *items.get(index)?;
    unsafe { kernel_collection_value_to_view::<'msg, T>(fk, enum_codec) }
}

pub(crate) unsafe fn kernel_repeated_set<T: 'static>(
    raw: RawRepeatedField,
    index: usize,
    value: T,
    enum_codec: Option<super::KernelEnumCodec<T>>,
) {
    let arr = unsafe { &*raw };
    let old = arr.items.borrow_mut().remove(index);
    release_bytes(&arr.strs, old);
    kernel_array_push(raw, value, None, enum_codec);
    let mut items = arr.items.borrow_mut();
    let last = items.pop().unwrap();
    items.insert(index, last);
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
