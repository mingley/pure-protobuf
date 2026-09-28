#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::string::{ProtoBytes, ProtoString};
use std::fmt::Debug;

use super::{
    Arena, FieldKind, RawMap, adopt_owned_msg, kernel_fieldkind_to_view, release_bytes,
    retain_bytes,
};

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

pub fn empty_map<K: crate::map::MapKey, V: crate::map::MapValue>()
-> crate::map::MapView<'static, K, V> {
    crate::map::MapView::from_slice(&[])
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

fn kernel_value_kind<V: 'static>(value: V, arena: Option<&Arena>, raw: RawMap) -> FieldKind {
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
            FieldKind::Bytes(retain_bytes(&(*raw).strs, s))
        } else if std::mem::size_of::<V>() == 4 {
            let v = std::ptr::read(&value as *const V as *const i32);
            std::mem::forget(value);
            FieldKind::I32(v)
        } else {
            adopt_owned_msg(value, arena, &(*raw).owner)
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
    let fk = kernel_value_kind(value, arena, raw);
    unsafe {
        let mut entries = (*raw).entries.borrow_mut();
        if let Some(e) = entries.iter_mut().rev().find(|(k, _)| *k == kb) {
            let old = std::mem::replace(&mut e.1, fk);
            drop(entries);
            kernel_map_release_value(raw, old);
            false
        } else {
            entries.push((kb, fk));
            true
        }
    }
}

pub(crate) fn kernel_map_release_value(raw: RawMap, kind: FieldKind) {
    // SAFETY: callers hold a live arena-owned raw map while replacing or removing its entry.
    unsafe { release_bytes(&(*raw).strs, kind) };
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
