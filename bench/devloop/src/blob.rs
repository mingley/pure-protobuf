//! SB-13 large-payload codec cells: 64 KiB - 8 MiB `bytes` payloads.
//!
//! `BlobChunk` carries small metadata plus one large `bytes` field;
//! `BlobMixed` carries eight 4 KiB fields to catch buffer-pinning
//! regressions. pbrs and prost encode the same schema; `prepare` asserts
//! byte-identical wire output so the comparison is fair by construction.

use pbrs::{Parse, Serialize};
use prost::Message as _;
use std::hint::black_box;

pub mod pbrs_blob {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/blob.rs"));
}

use pbrs_blob::{BlobChunk, BlobMixed};

#[derive(Clone, PartialEq, prost::Message)]
pub struct ProstBlobChunk {
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(string, tag = "2")]
    pub owner: String,
    #[prost(fixed64, tag = "3")]
    pub checksum: u64,
    #[prost(bytes, tag = "4")]
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ProstBlobMixed {
    #[prost(uint64, tag = "1")]
    pub id: u64,
    #[prost(bytes, repeated, tag = "2")]
    pub parts: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobSize {
    Kb64,
    Mib1,
    Mib4,
    Mib8,
    Mixed,
}

impl BlobSize {
    pub fn id(self) -> &'static str {
        match self {
            BlobSize::Kb64 => "64kib",
            BlobSize::Mib1 => "1mib",
            BlobSize::Mib4 => "4mib",
            BlobSize::Mib8 => "8mib",
            BlobSize::Mixed => "mixed",
        }
    }

    pub fn data_bytes(self) -> usize {
        match self {
            BlobSize::Kb64 => 64 * 1024,
            BlobSize::Mib1 => 1024 * 1024,
            BlobSize::Mib4 => 4 * 1024 * 1024,
            BlobSize::Mib8 => 8 * 1024 * 1024,
            BlobSize::Mixed => 8 * 4096,
        }
    }

    pub fn all() -> [BlobSize; 5] {
        [
            BlobSize::Kb64,
            BlobSize::Mib1,
            BlobSize::Mib4,
            BlobSize::Mib8,
            BlobSize::Mixed,
        ]
    }

    /// Iters cap for matrix runs: ~256 MiB of payload traffic per cell so
    /// the full-matrix CI lane stays bounded. Explicit `run-cell` iters
    /// are never scaled.
    pub fn matrix_iters(self, requested: u64) -> u64 {
        let cap = (256 * 1024 * 1024 / self.data_bytes() as u64).max(16);
        requested.min(cap)
    }
}

pub struct BlobCase {
    pub size: BlobSize,
    pub wire: Vec<u8>,
    /// Same bytes as `wire` in a shareable buffer. `parse_shared` cells
    /// clone this per iter (a refcount bump): the "frame arrival", owned by
    /// the harness like the network owns a received frame.
    pub frame: pbrs::rt::Bytes,
    pub shared: Vec<BlobChunk>,
    pub shared_mixed: Vec<BlobMixed>,
    pub prost_wire: Vec<u8>,
}

impl BlobCase {
    pub fn prepare(size: BlobSize) -> Self {
        if size == BlobSize::Mixed {
            return Self::prepare_mixed();
        }
        let n = size.data_bytes();
        let mut msg = BlobChunk::new();
        msg.set_id(0xB10B);
        msg.set_owner("lpbench");
        msg.set_checksum(0xC0FFEE);
        msg.set_data(vec![0xAB; n]);
        let wire = msg.serialize().expect("pbrs blob serialize");

        let prost = ProstBlobChunk {
            id: 0xB10B,
            owner: "lpbench".to_owned(),
            checksum: 0xC0FFEE,
            data: vec![0xAB; n],
        };
        let prost_wire = prost.encode_to_vec();
        assert_eq!(wire, prost_wire, "pbrs/prost wire mismatch for {:?}", size);

        let mut shared = Vec::with_capacity(4);
        for _ in 0..4 {
            shared.push(BlobChunk::parse(&wire).expect("pbrs blob parse"));
        }
        let frame = pbrs::rt::Bytes::copy_from_slice(&wire);
        BlobCase {
            size,
            wire,
            frame,
            shared,
            shared_mixed: Vec::new(),
            prost_wire,
        }
    }

    fn prepare_mixed() -> Self {
        let mut msg = BlobMixed::new();
        msg.set_id(0xB10B);
        let parts: Vec<pbrs::rt::LazyBytes> = (0..8)
            .map(|i| pbrs::rt::LazyBytes::from_bytes(&vec![i as u8; 4096]))
            .collect();
        msg.set_parts(parts);
        let wire = msg.serialize().expect("pbrs mixed serialize");

        let prost = ProstBlobMixed {
            id: 0xB10B,
            parts: (0..8).map(|i| vec![i as u8; 4096]).collect(),
        };
        let prost_wire = prost.encode_to_vec();
        assert_eq!(wire, prost_wire, "pbrs/prost wire mismatch for mixed");

        let mut shared_mixed = Vec::with_capacity(4);
        for _ in 0..4 {
            shared_mixed.push(BlobMixed::parse(&wire).expect("pbrs mixed parse"));
        }
        let frame = pbrs::rt::Bytes::copy_from_slice(&wire);
        BlobCase {
            size: BlobSize::Mixed,
            wire,
            frame,
            shared: Vec::new(),
            shared_mixed,
            prost_wire,
        }
    }
}

pub fn is_blob_cell(cell: &str) -> bool {
    cell.contains(".blob_") || cell.contains(".blob.")
}

pub fn blob_size_of(cell: &str) -> Option<BlobSize> {
    for size in BlobSize::all() {
        if cell.ends_with(size.id()) {
            return Some(size);
        }
    }
    None
}

pub fn blob_cells() -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    // (suffix, codec) tables are expanded below into 'static ids via Box::leak:
    // cell ids must be 'static for all_cells().
    let pbrs_ops = ["parse", "parse_shared", "touch", "encode", "encode_shared"];
    let prost_ops = ["parse", "touch", "encode"];
    for size in BlobSize::all() {
        for op in pbrs_ops {
            let id = format!("codec.pbrs.blob_{op}_{}", size.id());
            out.push((leak(id), "pbrs"));
        }
        for op in prost_ops {
            let id = format!("codec.prost.blob_{op}_{}", size.id());
            out.push((leak(id), "prost"));
        }
    }
    out
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn sum_bytes(data: &[u8]) -> u64 {
    // Touch every page without autovectorizing into a memset-shaped loop
    // the optimizer could fold: wrapping add over u64 lanes.
    let mut acc = 0u64;
    for chunk in data.chunks(8) {
        let mut lane = [0u8; 8];
        lane[..chunk.len()].copy_from_slice(chunk);
        acc = acc.wrapping_add(u64::from_le_bytes(lane));
    }
    acc
}

/// Run one blob iteration; returns a sink defeated only by real work.
pub fn blob_work(cell: &str, case: &BlobCase, i: usize) -> u64 {
    // "codec.pbrs.blob_encode_shared_1mib" -> ["codec.pbrs.blob", "encode", ...].
    let second = cell.split('_').nth(1).expect("blob cell op");
    let op = if cell.contains("encode_shared") {
        "encode_shared"
    } else if cell.contains("parse_shared") {
        "parse_shared"
    } else {
        second
    };
    let codec = if cell.contains(".pbrs.") {
        "pbrs"
    } else {
        "prost"
    };
    let mixed = case.size == BlobSize::Mixed;
    match (codec, op, mixed) {
        ("pbrs", "parse", false) => {
            let m = BlobChunk::parse(black_box(&case.wire)).expect("parse");
            m.id().wrapping_add(m.checksum())
        }
        ("pbrs", "parse_shared", false) => {
            let m = BlobChunk::parse_bytes(black_box(case.frame.clone())).expect("parse");
            m.id().wrapping_add(m.checksum())
        }
        ("pbrs", "touch", false) => {
            let m = BlobChunk::parse(black_box(&case.wire)).expect("parse");
            sum_bytes(m.data())
        }
        ("pbrs", "encode", false) => {
            let mut m = BlobChunk::new();
            m.set_id(i as u64);
            m.set_owner("lpbench");
            m.set_checksum(i as u64);
            m.set_data(vec![0xAB; case.size.data_bytes()]);
            m.serialize().expect("encode").len() as u64
        }
        ("pbrs", "encode_shared", false) => case.shared[i % case.shared.len()]
            .serialize()
            .expect("encode")
            .len() as u64,
        ("prost", "parse", false) => {
            let m = <ProstBlobChunk as prost::Message>::decode(black_box(&case.prost_wire[..]))
                .expect("decode");
            m.id.wrapping_add(m.checksum)
        }
        ("prost", "touch", false) => {
            let m = <ProstBlobChunk as prost::Message>::decode(black_box(&case.prost_wire[..]))
                .expect("decode");
            sum_bytes(&m.data)
        }
        ("prost", "encode", false) => {
            let m = ProstBlobChunk {
                id: i as u64,
                owner: "lpbench".to_owned(),
                checksum: i as u64,
                data: vec![0xAB; case.size.data_bytes()],
            };
            m.encode_to_vec().len() as u64
        }
        ("pbrs", "parse", true) => {
            let m = BlobMixed::parse(black_box(&case.wire)).expect("parse");
            m.id().wrapping_add(m.parts().len() as u64)
        }
        ("pbrs", "parse_shared", true) => {
            let m = BlobMixed::parse_bytes(black_box(case.frame.clone())).expect("parse");
            m.id().wrapping_add(m.parts().len() as u64)
        }
        ("pbrs", "touch", true) => {
            let m = BlobMixed::parse(black_box(&case.wire)).expect("parse");
            let mut acc = 0u64;
            for part in m.parts() {
                acc = acc.wrapping_add(sum_bytes(part.as_bytes()));
            }
            acc
        }
        ("pbrs", "encode", true) => {
            let mut m = BlobMixed::new();
            m.set_id(i as u64);
            let parts: Vec<pbrs::rt::LazyBytes> = (0..8)
                .map(|k| pbrs::rt::LazyBytes::from_bytes(&vec![k as u8; 4096]))
                .collect();
            m.set_parts(parts);
            m.serialize().expect("encode").len() as u64
        }
        ("pbrs", "encode_shared", true) => case.shared_mixed[i % case.shared_mixed.len()]
            .serialize()
            .expect("encode")
            .len() as u64,
        ("prost", "parse", true) => {
            let m = <ProstBlobMixed as prost::Message>::decode(black_box(&case.prost_wire[..]))
                .expect("decode");
            m.id.wrapping_add(m.parts.len() as u64)
        }
        ("prost", "touch", true) => {
            let m = <ProstBlobMixed as prost::Message>::decode(black_box(&case.prost_wire[..]))
                .expect("decode");
            let mut acc = 0u64;
            for part in &m.parts {
                acc = acc.wrapping_add(sum_bytes(part));
            }
            acc
        }
        ("prost", "encode", true) => {
            let m = ProstBlobMixed {
                id: i as u64,
                parts: (0..8).map(|k| vec![k as u8; 4096]).collect(),
            };
            m.encode_to_vec().len() as u64
        }
        other => panic!("unknown blob cell shape: {other:?}"),
    }
}

/// `devloop sizes`: struct layout report for the zero-copy plan.
pub fn print_sizes() {
    let rows = [
        ("pbrs::rt::Wire", std::mem::size_of::<pbrs::rt::Wire>()),
        (
            "pbrs::rt::LazyBytes",
            std::mem::size_of::<pbrs::rt::LazyBytes>(),
        ),
        (
            "pbrs::rt::LazyStr",
            std::mem::size_of::<pbrs::rt::LazyStr>(),
        ),
        ("lpbench::BlobChunk", std::mem::size_of::<BlobChunk>()),
        ("lpbench::BlobMixed", std::mem::size_of::<BlobMixed>()),
        ("prost::BlobChunk", std::mem::size_of::<ProstBlobChunk>()),
        ("prost::BlobMixed", std::mem::size_of::<ProstBlobMixed>()),
    ];
    let mut out = String::from("{\n");
    for (i, (name, size)) in rows.iter().enumerate() {
        out.push_str(&format!(
            "  \"{name}\": {size}{}\n",
            if i + 1 == rows.len() { "" } else { "," }
        ));
    }
    out.push('}');
    println!("{out}");
}
