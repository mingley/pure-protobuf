//! Adoption codec operations. Preparation qualifies every output before timing.

use crate::*;
use pbrs::{Parse, Serialize};
use prost::Message;
use serde_json::{Value, json};
use std::fmt::Debug;
use std::hint::black_box;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Specimen {
    Query { depth: usize, variant: usize },
    Entities(usize),
    Sparse(usize),
    Maps(usize),
}

#[derive(Clone, Copy, Debug)]
pub enum Codec {
    Pbrs,
    Prost,
}

impl Codec {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pbrs => "pbrs",
            Self::Prost => "prost",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Operation {
    FreshEncode,
    OwnedDecode,
    ReadAll,
    FullyReadClone,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub id: &'static str,
    pub codec: Codec,
    pub operation: Operation,
    pub specimen: Specimen,
}

include!("cells.rs");

struct Pair<N, P> {
    // Both inputs are owned and completely read, including nested Any payloads.
    native: N,
    prost: P,
    // Both decode operations begin from this same borrowed byte slice.
    wire: Vec<u8>,
    checksum: u64,
    native_fresh_bytes: usize,
}

/// Stabilize only the frozen MapHeavy message's unique-key map entries.
/// Moving complete top-level chunks preserves every encoded subfield and
/// length; nested/repeated message order is untouched. This is fixture
/// preparation, not a shipping serialization policy or a peer codec change.
pub(crate) fn stable_map_wire(wire: Vec<u8>) -> Vec<u8> {
    let mut chunks = Vec::new();
    let mut offset = 0;
    while offset < wire.len() {
        let start = offset;
        let tag = pbrs::rt::decode_varint(&wire, &mut offset).unwrap();
        assert!(matches!(tag, 10 | 18 | 26), "frozen MapHeavy map fields");
        let length = usize::try_from(pbrs::rt::decode_varint(&wire, &mut offset).unwrap()).unwrap();
        offset = offset.checked_add(length).unwrap();
        chunks.push(wire.get(start..offset).unwrap());
    }
    chunks.sort_unstable();
    let mut stable = Vec::with_capacity(wire.len());
    for chunk in chunks {
        stable.extend_from_slice(chunk);
    }
    assert_eq!(stable.len(), wire.len());
    stable
}

fn wire_fingerprint(wire: &[u8]) -> String {
    // Portable FNV-1a input identifier, not a security/integrity hash.
    let hash = wire.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}

impl<N, P> Pair<N, P>
where
    N: Parse + Serialize + Clone,
    P: Message + Default + Clone + PartialEq + Debug,
{
    fn prepare(
        fresh_native: N,
        fresh_prost: P,
        read_n: fn(&N) -> u64,
        read_p: fn(&P) -> u64,
        map_wire: bool,
    ) -> Self {
        let wire = fresh_prost.encode_to_vec();
        let wire = if map_wire {
            stable_map_wire(wire)
        } else {
            wire
        };
        let checksum = read_p(&fresh_prost);
        let native_wire = fresh_native.serialize().unwrap();
        let native_fresh_bytes = native_wire.len();
        assert_eq!(P::decode(native_wire.as_slice()).unwrap(), fresh_prost);
        assert_eq!(read_n(&fresh_native), checksum);
        let native = N::parse(&wire).unwrap();
        let prost = P::decode(wire.as_slice()).unwrap();
        assert_eq!(prost, fresh_prost);
        assert_eq!(read_n(&native), checksum);
        assert_eq!(read_p(&prost), checksum);
        // Qualify the clones of the fully read inputs with full equality and
        // independent field walks. These checks stay outside every window.
        let cloned_n = native.clone();
        let cloned_p = prost.clone();
        assert_eq!(
            P::decode(cloned_n.serialize().unwrap().as_slice()).unwrap(),
            fresh_prost
        );
        assert_eq!(cloned_p, fresh_prost);
        assert_eq!(read_n(&cloned_n), checksum);
        assert_eq!(read_p(&cloned_p), checksum);
        Self {
            native,
            prost,
            wire,
            checksum,
            native_fresh_bytes,
        }
    }

    fn work(
        &self,
        cell: Cell,
        fresh_n: impl FnOnce() -> N,
        fresh_p: impl FnOnce() -> P,
        read_n: fn(&N) -> u64,
        read_p: fn(&P) -> u64,
    ) -> u64 {
        // The black boxes consume owned outputs before they are dropped inside
        // the caller's allocation/timing window. Clone adds no extra read walk.
        match (cell.codec, cell.operation) {
            (Codec::Pbrs, Operation::FreshEncode) => {
                black_box(fresh_n().serialize().unwrap()).len() as u64
            }
            (Codec::Prost, Operation::FreshEncode) => {
                black_box(fresh_p().encode_to_vec()).len() as u64
            }
            (Codec::Pbrs, Operation::OwnedDecode) => {
                black_box(N::parse(black_box(self.wire.as_slice())).unwrap());
                1
            }
            (Codec::Prost, Operation::OwnedDecode) => {
                black_box(P::decode(black_box(self.wire.as_slice())).unwrap());
                1
            }
            (Codec::Pbrs, Operation::ReadAll) => {
                black_box(read_n(&N::parse(black_box(self.wire.as_slice())).unwrap()))
            }
            (Codec::Prost, Operation::ReadAll) => {
                black_box(read_p(&P::decode(black_box(self.wire.as_slice())).unwrap()))
            }
            (Codec::Pbrs, Operation::FullyReadClone) => {
                black_box(black_box(&self.native).clone());
                1
            }
            (Codec::Prost, Operation::FullyReadClone) => {
                black_box(black_box(&self.prost).clone());
                1
            }
        }
    }
}

enum Inputs {
    Query(Pair<native::Query, prost_types::Query>),
    Entities(Pair<native::EntityList, prost_types::EntityList>),
    Sparse(Box<Pair<native::Sparse, prost_types::Sparse>>),
    Maps(Pair<native::MapHeavy, prost_types::MapHeavy>),
}

/// One fully qualified cell, reused for warmup and every measured operation.
pub struct CodecCase {
    cell: Cell,
    inputs: Inputs,
}

impl CodecCase {
    pub fn prepare(id: &str) -> Self {
        let cell = *CELLS
            .iter()
            .find(|c| c.id == id)
            .expect("registered adoption cell");
        let inputs = match cell.specimen {
            Specimen::Query { depth, variant } => Inputs::Query(Pair::prepare(
                query_native(depth, variant),
                query(depth, variant),
                touch_query_native,
                touch_query_prost,
                false,
            )),
            Specimen::Entities(n) => Inputs::Entities(Pair::prepare(
                entity_list_native(n),
                entity_list(n),
                touch_entity_list_native,
                touch_entity_list_prost,
                false,
            )),
            Specimen::Sparse(v) => Inputs::Sparse(Box::new(Pair::prepare(
                sparse_native(v),
                sparse_prost(v),
                touch_sparse_native,
                touch_sparse_prost,
                false,
            ))),
            Specimen::Maps(n) => Inputs::Maps(Pair::prepare(
                maps_native(n),
                maps(n),
                touch_maps_native,
                touch_maps_prost,
                true,
            )),
        };
        Self { cell, inputs }
    }

    pub fn work(&self) -> u64 {
        match (&self.inputs, self.cell.specimen) {
            (Inputs::Query(pair), Specimen::Query { depth, variant }) => pair.work(
                self.cell,
                || query_native(depth, variant),
                || query(depth, variant),
                touch_query_native,
                touch_query_prost,
            ),
            (Inputs::Entities(pair), Specimen::Entities(n)) => pair.work(
                self.cell,
                || entity_list_native(n),
                || entity_list(n),
                touch_entity_list_native,
                touch_entity_list_prost,
            ),
            (Inputs::Sparse(pair), Specimen::Sparse(v)) => pair.work(
                self.cell,
                || sparse_native(v),
                || sparse_prost(v),
                touch_sparse_native,
                touch_sparse_prost,
            ),
            (Inputs::Maps(pair), Specimen::Maps(n)) => pair.work(
                self.cell,
                || maps_native(n),
                || maps(n),
                touch_maps_native,
                touch_maps_prost,
            ),
            _ => unreachable!("prepared specimen matches its registered cell"),
        }
    }

    pub fn qualification(&self) -> Value {
        let wire = match &self.inputs {
            Inputs::Query(p) => &p.wire,
            Inputs::Entities(p) => &p.wire,
            Inputs::Sparse(p) => &p.wire,
            Inputs::Maps(p) => &p.wire,
        };
        let (wire_bytes, native_fresh_bytes, checksum) = match &self.inputs {
            Inputs::Query(p) => (p.wire.len(), p.native_fresh_bytes, p.checksum),
            Inputs::Entities(p) => (p.wire.len(), p.native_fresh_bytes, p.checksum),
            Inputs::Sparse(p) => (p.wire.len(), p.native_fresh_bytes, p.checksum),
            Inputs::Maps(p) => (p.wire.len(), p.native_fresh_bytes, p.checksum),
        };
        json!({
            "id": self.cell.id, "codec": self.cell.codec.name(),
            "specimen": format!("{:?}", self.cell.specimen),
            "operation": format!("{:?}", self.cell.operation),
            "wire_bytes": wire_bytes, "read_checksum": checksum,
            "wire_fingerprint": wire_fingerprint(wire),
            "fresh_wire_bytes": match self.cell.codec {
                Codec::Pbrs => native_fresh_bytes,
                Codec::Prost => wire_bytes,
            },
            "fresh_decode_clone_full_equality": "passed", "complete_reads": "passed",
        })
    }
}

/// Actual preparation checks and one consumed output for all 512 cells.
pub fn codec_inventory() -> Value {
    let rows: Vec<_> = CELLS
        .iter()
        .map(|cell| {
            let case = CodecCase::prepare(cell.id);
            black_box(case.work());
            case.qualification()
        })
        .collect();
    json!({"schema": "pbrs-adoption-codec-inventory/1", "cells": rows})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn map_input_order_is_stable_and_preserves_full_values_and_lengths() {
        for n in [8, 64, 512] {
            let specimen = maps(n);
            let wire = specimen.encode_to_vec();
            let stable = stable_map_wire(wire.clone());
            assert_eq!(stable.len(), wire.len());
            assert_eq!(
                prost_types::MapHeavy::decode(stable.as_slice()).unwrap(),
                specimen
            );
            for _ in 0..12 {
                let independent = maps(n);
                assert_eq!(independent, specimen);
                assert_eq!(stable_map_wire(independent.encode_to_vec()), stable);
            }
            assert_eq!(stable_map_wire(stable.clone()), stable);
            let native = native::MapHeavy::parse(&stable).unwrap();
            assert_eq!(touch_maps_native(&native), touch_maps_prost(&specimen));
        }
    }

    #[test]
    fn every_cell_qualifies_fresh_decode_read_and_fully_read_clone() {
        let ids: HashSet<_> = CELLS.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), 512);
        for cell in CELLS {
            let case = CodecCase::prepare(cell.id);
            let q = case.qualification();
            let result = case.work();
            match cell.operation {
                Operation::FreshEncode => assert_eq!(
                    result,
                    q["fresh_wire_bytes"].as_u64().unwrap(),
                    "{}",
                    cell.id
                ),
                Operation::ReadAll => assert_eq!(result, q["read_checksum"].as_u64().unwrap()),
                Operation::OwnedDecode | Operation::FullyReadClone => assert_eq!(result, 1),
            }
        }
    }
}
