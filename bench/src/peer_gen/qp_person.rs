// SB-08 checked-in peer gencode. DO NOT EDIT BY HAND.
// generator: pb-rs 0.10.0 (single_module)
// source: proto/person.proto (sha256 13ec16099b636bb46467e28199347c79ba26e72223462c1c3060e28d6c1620c5)
// rewrite: dropped `use super::*;` (single-module output needs no parent imports); moved #![allow] inner attrs to the peer_gen wrapper module
// Regenerate: see bench/src/peer_gen/SB08_PROVENANCE.md.
// Automatically generated rust module for 'person.proto' file



use std::borrow::Cow;
use std::collections::HashMap;
type KVMap<K, V> = HashMap<K, V>;
use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use quick_protobuf::sizeofs::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Address<'a> {
    pub city: Cow<'a, str>,
}

impl<'a> MessageRead<'a> for Address<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(10) => msg.city = r.read_string(bytes).map(Cow::Borrowed)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for Address<'a> {
    fn get_size(&self) -> usize {
        0
        + if self.city == "" { 0 } else { 1 + sizeof_len((&self.city).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.city != "" { w.write_with_tag(10, |w| w.write_string(&**&self.city))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Person<'a> {
    pub id: i32,
    pub name: Cow<'a, str>,
    pub email: Cow<'a, str>,
    pub tags: Vec<Cow<'a, str>>,
    pub scores: KVMap<Cow<'a, str>, i32>,
    pub address: Option<Address<'a>>,
    pub extras: KVMap<Cow<'a, str>, i32>,
}

impl<'a> MessageRead<'a> for Person<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.id = r.read_int32(bytes)?,
                Ok(18) => msg.name = r.read_string(bytes).map(Cow::Borrowed)?,
                Ok(26) => msg.email = r.read_string(bytes).map(Cow::Borrowed)?,
                Ok(34) => msg.tags.push(r.read_string(bytes).map(Cow::Borrowed)?),
                Ok(42) => {
                    let (key, value) = r.read_map(bytes, |r, bytes| Ok(r.read_string(bytes).map(Cow::Borrowed)?), |r, bytes| Ok(r.read_int32(bytes)?))?;
                    msg.scores.insert(key, value);
                }
                Ok(50) => msg.address = Some(r.read_message::<Address>(bytes)?),
                Ok(130) => {
                    let (key, value) = r.read_map(bytes, |r, bytes| Ok(r.read_string(bytes).map(Cow::Borrowed)?), |r, bytes| Ok(r.read_int32(bytes)?))?;
                    msg.extras.insert(key, value);
                }
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for Person<'a> {
    fn get_size(&self) -> usize {
        0
        + if self.id == 0i32 { 0 } else { 1 + sizeof_varint(*(&self.id) as u64) }
        + if self.name == "" { 0 } else { 1 + sizeof_len((&self.name).len()) }
        + if self.email == "" { 0 } else { 1 + sizeof_len((&self.email).len()) }
        + self.tags.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + self.scores.iter().map(|(k, v)| 1 + sizeof_len(2 + sizeof_len((k).len()) + sizeof_varint(*(v) as u64))).sum::<usize>()
        + self.address.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.extras.iter().map(|(k, v)| 2 + sizeof_len(2 + sizeof_len((k).len()) + sizeof_varint(*(v) as u64))).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.id != 0i32 { w.write_with_tag(8, |w| w.write_int32(*&self.id))?; }
        if self.name != "" { w.write_with_tag(18, |w| w.write_string(&**&self.name))?; }
        if self.email != "" { w.write_with_tag(26, |w| w.write_string(&**&self.email))?; }
        for s in &self.tags { w.write_with_tag(34, |w| w.write_string(&**s))?; }
        for (k, v) in self.scores.iter() { w.write_with_tag(42, |w| w.write_map(2 + sizeof_len((k).len()) + sizeof_varint(*(v) as u64), 10, |w| w.write_string(&**k), 16, |w| w.write_int32(*v)))?; }
        if let Some(ref s) = self.address { w.write_with_tag(50, |w| w.write_message(s))?; }
        for (k, v) in self.extras.iter() { w.write_with_tag(130, |w| w.write_map(2 + sizeof_len((k).len()) + sizeof_varint(*(v) as u64), 10, |w| w.write_string(&**k), 16, |w| w.write_int32(*v)))?; }
        Ok(())
    }
}

