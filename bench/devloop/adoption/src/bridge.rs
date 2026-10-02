//! Real mixed-runtime conversions over every public adoption specimen.
//!
//! Preparation and complete semantic/round-trip oracles are outside timing.
//! `work` calls the shipping adapter API and drops the owned target before
//! returning. It neither reconstructs that API nor adds codec cost estimates.

use crate::*;
use pbrs::{Parse, Serialize};
use prost::Message;
use protobuf_tonic::{pbrs_to_prost, prost_to_pbrs};
use serde_json::{Value, json};
use std::fmt::Debug;
use std::hint::black_box;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Specimen {
    Query { depth: usize, variant: usize },
    Entities(usize),
    Sparse(usize),
    Maps(usize),
    Options(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    ProstToPbrs,
    PbrsToProst,
}

impl Direction {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProstToPbrs => "prost_to_pbrs",
            Self::PbrsToProst => "pbrs_to_prost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    ApiOnly,
    ReadAll,
}

impl Mode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ApiOnly => "api_only",
            Self::ReadAll => "read_all",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub id: &'static str,
    pub direction: Direction,
    pub mode: Mode,
    pub specimen: Specimen,
}

// Options have their own generated independent field walks, including every
// explicitly present default. Runtime specimens reuse their existing walks.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn word(&mut self, value: u64) {
        for byte in value.to_le_bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn bytes(&mut self, value: &[u8]) {
        self.word(value.len() as u64);
        for byte in value {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    fn finish(self) -> u64 {
        self.0
    }
}

include!("bridge_options.rs");
include!("bridge_cells.rs");

fn wire_fingerprint(wire: &[u8]) -> String {
    let hash = wire.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    });
    format!("fnv1a64:{hash:016x}")
}

struct Pair<N, P> {
    native: N,
    prost: P,
    read_native: fn(&N) -> u64,
    read_prost: fn(&P) -> u64,
    checksum: u64,
    common_wire: Vec<u8>,
    native_source_wire: Vec<u8>,
    prost_source_wire: Vec<u8>,
    native_target_wire: Vec<u8>,
    prost_target_wire: Vec<u8>,
    map: bool,
}

impl<N, P> Pair<N, P>
where
    N: Parse + Serialize,
    P: Message + Default + PartialEq + Debug,
{
    fn prepare(
        fresh_native: N,
        fresh_prost: P,
        read_native: fn(&N) -> u64,
        read_prost: fn(&P) -> u64,
        map: bool,
    ) -> Self {
        let checksum = read_prost(&fresh_prost);
        assert_eq!(read_native(&fresh_native), checksum);
        assert_eq!(
            P::decode(fresh_native.serialize().unwrap().as_slice()).unwrap(),
            fresh_prost
        );
        let common_wire = fresh_prost.encode_to_vec();
        let common_wire = if map {
            // This stabilizes only the qualified fixture input. The real API's
            // own source serialization remains untouched and separately pinned.
            workloads::stable_map_wire(common_wire)
        } else {
            common_wire
        };
        let native = N::parse(&common_wire).unwrap();
        let prost = P::decode(common_wire.as_slice()).unwrap();
        assert_eq!(prost, fresh_prost);
        assert_eq!(read_native(&native), checksum);
        assert_eq!(read_prost(&prost), checksum);

        // Preview precisely the same fully read source encoder used by the API.
        // These bytes are never fed into a replacement timed conversion.
        let native_source_wire = native.serialize().unwrap();
        let prost_source_wire = prost.encode_to_vec();
        assert_eq!(P::decode(native_source_wire.as_slice()).unwrap(), prost);
        assert_eq!(
            read_native(&N::parse(&prost_source_wire).unwrap()),
            checksum
        );

        let native_target: N = prost_to_pbrs(&prost).unwrap();
        let prost_target: P = pbrs_to_prost(&native).unwrap();
        assert_eq!(read_native(&native_target), checksum);
        assert_eq!(read_prost(&prost_target), checksum);
        assert_eq!(prost_target, prost);
        let native_target_wire = native_target.serialize().unwrap();
        let prost_target_wire = prost_target.encode_to_vec();
        assert_eq!(P::decode(native_target_wire.as_slice()).unwrap(), prost);

        // Exercise both actual API round trips, then apply an independent
        // ordinary decoder and full-message equality rather than checksum alone.
        let prost_roundtrip: P = pbrs_to_prost(&native_target).unwrap();
        let native_roundtrip: N = prost_to_pbrs(&prost_target).unwrap();
        assert_eq!(prost_roundtrip, prost);
        assert_eq!(read_prost(&prost_roundtrip), checksum);
        assert_eq!(read_native(&native_roundtrip), checksum);
        assert_eq!(
            P::decode(native_roundtrip.serialize().unwrap().as_slice()).unwrap(),
            prost
        );

        if !map {
            for wire in [
                &native_source_wire,
                &prost_source_wire,
                &native_target_wire,
                &prost_target_wire,
            ] {
                assert_eq!(
                    wire, &common_wire,
                    "non-map bridge source/output wire equality"
                );
            }
        }
        Self {
            native,
            prost,
            read_native,
            read_prost,
            checksum,
            common_wire,
            native_source_wire,
            prost_source_wire,
            native_target_wire,
            prost_target_wire,
            map,
        }
    }

    fn work(&self, direction: Direction, mode: Mode) -> u64 {
        match direction {
            Direction::ProstToPbrs => {
                let target: N =
                    prost_to_pbrs(black_box(&self.prost)).expect("qualified bridge API");
                let value = if mode == Mode::ReadAll {
                    (self.read_native)(black_box(&target))
                } else {
                    1
                };
                black_box(target);
                value
            }
            Direction::PbrsToProst => {
                let target: P =
                    pbrs_to_prost(black_box(&self.native)).expect("qualified bridge API");
                let value = if mode == Mode::ReadAll {
                    (self.read_prost)(black_box(&target))
                } else {
                    1
                };
                black_box(target);
                value
            }
        }
    }

    fn qualification(&self, cell: Cell) -> Value {
        let source_wire = self.source_wire(cell.direction);
        let output_wire = match cell.direction {
            Direction::ProstToPbrs => &self.native_target_wire,
            Direction::PbrsToProst => &self.prost_target_wire,
        };
        let common = wire_fingerprint(&self.common_wire);
        let source = wire_fingerprint(source_wire);
        // Bind the collector's input identifier to the actual API's source wire,
        // not merely to the sorted map fixture used for semantic preparation.
        let identity = format!("bridge-v1:{}:{common}:{source}", source_wire.len());
        json!({
            "id": cell.id, "direction": cell.direction.name(), "mode": cell.mode.name(),
            "api": format!("protobuf_tonic::{}", cell.direction.name()),
            "specimen": format!("{:?}", cell.specimen),
            "wire_fingerprint": identity, "common_wire_fingerprint": common,
            "common_wire_bytes": self.common_wire.len(),
            "actual_source_wire_fingerprint": source,
            "actual_source_wire_bytes": source_wire.len(),
            "target_reencoded_wire_fingerprint": wire_fingerprint(output_wire),
            "target_reencoded_wire_bytes": output_wire.len(),
            "source_output_equal_wire": source_wire == output_wire,
            "read_checksum": self.checksum, "full_equality": "passed",
            "complete_reads": "passed", "both_api_roundtrips": "passed",
            "timing_qualification": if self.map { "blocked" } else { "passed" },
            "timing_blocked_reason": if self.map {
                Some("map bridge cost needs reviewed wire policy and independent actual-source/output order reproducibility; no normalized replacement API")
            } else { None },
            "numeric_cost": "not_run", "instructions": "not_run",
        })
    }

    fn source_wire(&self, direction: Direction) -> &[u8] {
        match direction {
            Direction::ProstToPbrs => &self.prost_source_wire,
            Direction::PbrsToProst => &self.native_source_wire,
        }
    }
}

/// One qualified source pair reused for warmup and every actual API operation.
pub struct BridgeCase {
    cell: Cell,
    inputs: Inputs,
}

impl BridgeCase {
    pub fn prepare(id: &str) -> Self {
        let cell = *CELLS
            .iter()
            .find(|cell| cell.id == id)
            .expect("registered bridge cell");
        Self {
            cell,
            inputs: Inputs::prepare(cell.specimen),
        }
    }

    pub fn work(&self) -> u64 {
        self.inputs.work(self.cell.direction, self.cell.mode)
    }

    pub fn qualification(&self) -> Value {
        self.inputs.qualification(self.cell)
    }

    /// Semantic map coverage is permitted; qualified map timing stays blocked.
    pub fn require_timing_qualification(&self) {
        assert!(
            !matches!(self.cell.specimen, Specimen::Maps(_)),
            "map bridge timing qualification remains blocked"
        );
    }
}

/// Require the full actual-source identity before accepting N/2N or replay rows.
pub fn require_matching_input(first: Option<&str>, second: Option<&str>) {
    assert!(
        first.is_some_and(|value| !value.is_empty())
            && second.is_some_and(|value| !value.is_empty()),
        "bridge input fingerprint missing"
    );
    assert_eq!(first, second, "bridge actual-source fingerprint differs");
}

/// All 336 real-API semantic checks, with blocked cost states left visible.
pub fn inventory() -> Value {
    let rows: Vec<_> = CELLS
        .iter()
        .map(|cell| {
            let case = BridgeCase::prepare(cell.id);
            let value = case.work();
            let row = case.qualification();
            if cell.mode == Mode::ReadAll {
                assert_eq!(value, row["read_checksum"].as_u64().unwrap());
            } else {
                assert_eq!(value, 1);
            }
            row
        })
        .collect();
    json!({"schema": "pbrs-adoption-bridge-inventory/1", "cells": rows,
           "cost_measurement": "not_run; TC32b collector integration remains pending"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn complete_registry_and_actual_apis_cover_all_public_specimens() {
        assert_eq!(CELLS.len(), 336);
        let ids: HashSet<_> = CELLS.iter().map(|cell| cell.id).collect();
        assert_eq!(ids.len(), CELLS.len());
        let rows = inventory();
        let rows = rows["cells"].as_array().unwrap();
        assert_eq!(rows.len(), 336);
        assert_eq!(
            rows.iter()
                .filter(|r| r["timing_qualification"] == "blocked")
                .count(),
            12
        );
        assert_eq!(
            CELLS
                .iter()
                .filter(|c| matches!(c.specimen, Specimen::Options(_)))
                .count(),
            80
        );
        for row in rows {
            assert_eq!(row["full_equality"], "passed");
            assert_eq!(row["complete_reads"], "passed");
            assert_eq!(row["both_api_roundtrips"], "passed");
            assert_eq!(row["numeric_cost"], "not_run");
            if row["timing_qualification"] == "passed" {
                assert_eq!(row["source_output_equal_wire"], true);
                require_matching_input(
                    row["wire_fingerprint"].as_str(),
                    row["wire_fingerprint"].as_str(),
                );
            }
        }
    }

    #[test]
    fn every_options_field_walk_distinguishes_explicit_defaults() {
        check_option_field_coverage();
    }

    #[test]
    #[should_panic(expected = "map bridge timing qualification remains blocked")]
    fn semantic_map_success_cannot_certify_timing() {
        let case = BridgeCase::prepare("codec.adoption.bridge.prost_to_pbrs.maps.n8.read_all");
        let checksum = case.qualification()["read_checksum"].as_u64().unwrap();
        assert_eq!(case.work(), checksum);
        case.require_timing_qualification();
    }

    #[test]
    fn missing_or_unequal_actual_source_identity_is_rejected() {
        for (first, second) in [
            (None, None),
            (Some("source"), None),
            (None, Some("source")),
            (Some(""), Some("")),
            (Some("source"), Some("different")),
        ] {
            assert!(std::panic::catch_unwind(|| require_matching_input(first, second)).is_err());
        }
        require_matching_input(Some("source"), Some("source"));
    }

    #[test]
    fn actual_source_inputs_match_independent_processes() {
        const CHILD: &str = "PBRS_BRIDGE_INPUTS_CHILD";
        const PREFIX: &str = "__BRIDGE_INPUTS__ ";
        if std::env::var_os(CHILD).is_some() {
            let rows: Vec<_> = CELLS
                .iter()
                .filter(|cell| cell.mode == Mode::ApiOnly)
                .map(|cell| {
                    let case = BridgeCase::prepare(cell.id);
                    let wire = case.inputs.source_wire(cell.direction);
                    let bytes = wire
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    json!({"id":cell.id,"bytes":bytes,"qualification":case.qualification()})
                })
                .collect();
            println!("{PREFIX}{}", serde_json::to_string(&rows).unwrap());
            return;
        }
        let mut expected: Option<Value> = None;
        for _ in 0..3 {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "bridge::tests::actual_source_inputs_match_independent_processes",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            let rows: Value = serde_json::from_str(
                output
                    .lines()
                    .find_map(|line| line.strip_prefix(PREFIX))
                    .unwrap(),
            )
            .unwrap();
            let rows = rows.as_array().unwrap();
            assert_eq!(rows.len(), 168);
            assert_eq!(
                rows.iter()
                    .filter(|row| row["qualification"]["timing_qualification"] == "blocked")
                    .count(),
                6
            );
            let comparable = Value::Array(
                rows.iter()
                    .filter(|row| row["qualification"]["timing_qualification"] == "passed")
                    .cloned()
                    .collect(),
            );
            assert_eq!(comparable.as_array().unwrap().len(), 162);
            if let Some(first) = &expected {
                assert_eq!(
                    &comparable, first,
                    "complete actual-source bytes and identities differ between fresh processes"
                );
            } else {
                expected = Some(comparable);
            }
        }
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn full_equality_rejects_changed_last_entity_even_with_equal_checksums() {
        let mut changed = entity_list(1000);
        changed.records.last_mut().unwrap().key.push('!');
        Pair::prepare(entity_list_native(1000), changed, |_| 0, |_| 0, false);
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn full_equality_rejects_removed_present_default_even_with_equal_checksums() {
        let mut changed = sparse_prost(0);
        assert_eq!(changed.f000, Some(false));
        changed.f000 = None;
        Pair::prepare(sparse_native(0), changed, |_| 0, |_| 0, false);
    }
}
