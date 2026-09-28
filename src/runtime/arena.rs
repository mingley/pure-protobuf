#![allow(
    clippy::unimplemented,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "null MiniTable/MessagePtr is a rust_out kernel invariant, not a user Result"
)]

use crate::wire::UnknownFields;
use std::cell::RefCell;
use std::fmt::Debug;
use std::rc::Rc;

use super::{FieldKind, MiniTablePtr, MsgData, RawArrayInner, RawMapInner};

#[derive(Debug)]
pub struct ArenaInner {
    // Boxed so pointers handed to rust_out stay valid across Vec growth.
    #[expect(
        clippy::vec_box,
        reason = "upb-shaped repeated message slots are Box<Msg>"
    )]
    msgs: Vec<Box<MsgData>>,
    #[expect(
        clippy::vec_box,
        reason = "upb-shaped repeated message slots are Box<Msg>"
    )]
    arrays: Vec<Box<RawArrayInner>>,
    #[expect(
        clippy::vec_box,
        reason = "upb-shaped repeated message slots are Box<Msg>"
    )]
    maps: Vec<Box<RawMapInner>>,
    #[allow(
        clippy::vec_box,
        reason = "FieldKind::Bytes points to stable Vec headers owned across arena fusion"
    )]
    bytes: Vec<Box<Vec<u8>>>,
}

#[derive(Clone, Debug)]
pub struct Arena {
    pub(crate) inner: Rc<RefCell<ArenaInner>>,
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
                bytes: Vec::new(),
            })),
        }
    }

    pub fn fuse(&self, other: &Arena) {
        if Rc::ptr_eq(&self.inner, &other.inner) {
            return;
        }
        let mut o = other.inner.borrow_mut();
        let mut s = self.inner.borrow_mut();
        let array_start = s.arrays.len();
        let map_start = s.maps.len();
        s.msgs.append(&mut o.msgs);
        s.arrays.append(&mut o.arrays);
        s.maps.append(&mut o.maps);
        s.bytes.append(&mut o.bytes);
        // Raw mutators must resolve the owner after a collection changes arenas.
        for array in &mut s.arrays[array_start..] {
            array.owner = Rc::downgrade(&self.inner);
        }
        for map in &mut s.maps[map_start..] {
            map.owner = Rc::downgrade(&self.inner);
        }
    }

    pub(crate) fn alloc_msg(&self, mt: MiniTablePtr) -> *mut MsgData {
        let n = unsafe { mt.0.as_ref().map(|t| t.fields.len()).unwrap_or(0) };
        let b = Box::new(MsgData {
            slots: vec![FieldKind::Empty; n],
            has: vec![false; n],
            strs: Vec::new(),
            unknown: UnknownFields::default(),
            mt,
        });
        let mut owner = self.inner.borrow_mut();
        owner.msgs.push(b);
        owner
            .msgs
            .last_mut()
            .expect("message was just inserted")
            .as_mut() as *mut MsgData
    }

    pub(crate) fn alloc_array(&self) -> *const RawArrayInner {
        let b = Box::new(RawArrayInner {
            items: RefCell::new(Vec::new()),
            strs: RefCell::new(Vec::new()),
            owner: Rc::downgrade(&self.inner),
        });
        let mut owner = self.inner.borrow_mut();
        owner.arrays.push(b);
        owner
            .arrays
            .last()
            .expect("array was just inserted")
            .as_ref() as *const RawArrayInner
    }

    pub(crate) fn alloc_map(&self) -> *const RawMapInner {
        let b = Box::new(RawMapInner {
            entries: RefCell::new(Vec::new()),
            strs: RefCell::new(Vec::new()),
            owner: Rc::downgrade(&self.inner),
        });
        let mut owner = self.inner.borrow_mut();
        owner.maps.push(b);
        owner.maps.last().expect("map was just inserted").as_ref() as *const RawMapInner
    }

    pub(crate) fn alloc_bytes(&self, bytes: Vec<u8>) -> *const Vec<u8> {
        let mut owner = self.inner.borrow_mut();
        owner.bytes.push(Box::new(bytes));
        // Taking the pointer before moving the Box here would invalidate its provenance.
        owner
            .bytes
            .last()
            .expect("bytes were just inserted")
            .as_ref() as *const Vec<u8>
    }
}
