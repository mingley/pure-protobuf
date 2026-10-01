//! Whole-message request/response qualification for the four RPC profiles.

use crate::{native, prost_types, workloads::Specimen, *};
use pbrs::{Parse, Serialize};
use prost::Message;
use serde_json::{Value, json};
use std::fmt::Debug;

/// Unary returns one complete message; streaming returns four complete messages.
pub const STREAM_REPLIES: usize = 4;

/// Parsed, fully read messages of the same public schema and encoded length.
/// Preparation and full semantic comparisons happen outside measurement.
pub struct Pair<N, P> {
    pub native: N,
    pub prost: P,
    pub bytes: usize,
    pub native_bytes: usize,
    pub checksum: u64,
    pub read_native: fn(&N) -> u64,
    pub read_prost: fn(&P) -> u64,
}

impl<N, P> Pair<N, P>
where
    N: Parse + Serialize + Clone,
    P: Message + Default + Clone + PartialEq + Debug,
{
    fn prepare(prost: P, read_native: fn(&N) -> u64, read_prost: fn(&P) -> u64) -> Self {
        // Match input ownership and preparation: both requests are decoded
        // from the same schema-qualified wire and completely read first.
        // Fresh construction remains a separate codec workload.
        let wire = prost.encode_to_vec();
        let native = N::parse(&wire).expect("RPC native request");
        let prost = P::decode(wire.as_slice()).expect("RPC prost request");
        let checksum = read_prost(&prost);
        let native_bytes = native.serialize().unwrap().len();
        let pair = Self {
            native,
            prost,
            bytes: wire.len(),
            native_bytes,
            checksum,
            read_native,
            read_prost,
        };
        pair.validate_native(&pair.native);
        pair.validate_prost(&pair.prost);
        pair.validate_native(&pair.native.clone());
        pair.validate_prost(&pair.prost.clone());
        pair
    }

    /// Validate every response field independently of the complete read digest.
    pub fn validate_native(&self, response: &N) {
        assert_eq!((self.read_native)(response), self.checksum);
        let wire = response.serialize().expect("RPC native response");
        assert_eq!(wire.len(), self.native_bytes);
        assert_eq!(P::decode(wire.as_slice()).unwrap(), self.prost);
    }

    pub fn validate_prost(&self, response: &P) {
        assert_eq!(response, &self.prost);
        assert_eq!((self.read_prost)(response), self.checksum);
        assert_eq!(response.encoded_len(), self.bytes);
    }

    /// Unequal byte counts must block timing, even when decoded values agree.
    pub fn assert_equal_wire_work(&self) {
        assert_eq!(
            self.native_bytes, self.bytes,
            "RPC encoded bytes must agree"
        );
    }

    fn report(&self) -> Value {
        json!({
            "request_bytes": self.bytes,
            "native_request_bytes": self.native_bytes,
            "response_bytes_per_message": self.bytes,
            "unary_responses": 1,
            "stream_responses": STREAM_REPLIES,
            "request_read_checksum": self.checksum,
            "response_read_checksum": self.checksum,
            "full_decoded_equality": true,
            "equal_wire_work": self.native_bytes == self.bytes,
            "input": "both decoded from common prost wire, fully read, then cloned per RPC",
        })
    }
}

pub enum Inputs {
    Query(Pair<native::Query, prost_types::Query>),
    Entities(Pair<native::EntityList, prost_types::EntityList>),
    Sparse(Box<Pair<native::Sparse, prost_types::Sparse>>),
    Maps(Pair<native::MapHeavy, prost_types::MapHeavy>),
}

impl Inputs {
    pub fn prepare(specimen: Specimen) -> Self {
        match specimen {
            Specimen::Query { depth, variant } => Self::Query(Pair::prepare(
                query(depth, variant),
                touch_query_native,
                touch_query_prost,
            )),
            Specimen::Entities(n) => Self::Entities(Pair::prepare(
                entity_list(n),
                touch_entity_list_native,
                touch_entity_list_prost,
            )),
            Specimen::Sparse(v) => Self::Sparse(Box::new(Pair::prepare(
                sparse_prost(v),
                touch_sparse_native,
                touch_sparse_prost,
            ))),
            Specimen::Maps(n) => {
                Self::Maps(Pair::prepare(maps(n), touch_maps_native, touch_maps_prost))
            }
        }
    }

    pub fn report(&self) -> Value {
        match self {
            Self::Query(p) => p.report(),
            Self::Entities(p) => p.report(),
            Self::Sparse(p) => p.report(),
            Self::Maps(p) => p.report(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workloads::{CELLS, Codec, Operation};

    #[test]
    fn all_64_rpc_specimens_check_full_values_reads_and_expose_unequal_wire_work() {
        let mut count = 0;
        let mut unequal = 0;
        for cell in CELLS.iter().filter(|c| {
            matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll)
        }) {
            let report = Inputs::prepare(cell.specimen).report();
            assert_eq!(report["full_decoded_equality"], true);
            if report["equal_wire_work"] == false {
                assert!(matches!(cell.specimen, Specimen::Maps(_)));
                assert_eq!(
                    report["native_request_bytes"].as_u64().unwrap(),
                    report["request_bytes"].as_u64().unwrap() + 4
                );
                unequal += 1;
            }
            count += 1;
        }
        assert_eq!(count, 64);
        assert_eq!(unequal, 3);
    }

    #[test]
    #[should_panic(expected = "RPC encoded bytes must agree")]
    fn maps_cannot_enter_a_timing_window_with_unequal_wire_work() {
        let Inputs::Maps(pair) = Inputs::prepare(Specimen::Maps(8)) else {
            unreachable!()
        };
        pair.assert_equal_wire_work();
    }

    #[test]
    #[should_panic]
    fn rpc_oracle_rejects_changes_in_the_last_entity() {
        let Inputs::Entities(pair) = Inputs::prepare(Specimen::Entities(1000)) else {
            unreachable!();
        };
        let mut changed = pair.prost.clone();
        changed.records.last_mut().unwrap().revision += 1;
        pair.validate_prost(&changed);
    }
}
