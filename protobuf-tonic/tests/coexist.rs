//! Prost and pbrs representations may coexist and cross an incremental
//! migration boundary without generated field-by-field adapters.

#![allow(missing_docs, reason = "integration fixture")]

use pbrs::Serialize;
use prost::Message as _;
use protobuf_tonic::{pbrs_to_prost, prost_to_pbrs};

#[derive(Clone, PartialEq, prost::Message)]
struct ProstHelloRequest {
    #[prost(string, tag = "1")]
    name: String,
}

fn pbrs_request(name: &str) -> protobuf_tonic::hello::HelloRequest {
    let mut request = protobuf_tonic::hello::HelloRequest::new();
    request.set_name(name.to_owned());
    request
}

#[test]
fn prost_and_pbrs_types_coexist_and_convert_in_both_directions() {
    let prost = ProstHelloRequest {
        name: "shared-schema".to_owned(),
    };
    let pbrs: protobuf_tonic::hello::HelloRequest = prost_to_pbrs(&prost).expect("prost to pbrs");
    assert_eq!(pbrs.name().to_str(), Ok("shared-schema"));

    let prost_again: ProstHelloRequest = pbrs_to_prost(&pbrs).expect("pbrs to prost");
    assert_eq!(prost_again, prost);
    assert_eq!(prost.encode_to_vec(), pbrs.serialize().expect("pbrs wire"));
}

#[test]
fn conversions_round_trip_production_shaped_payload_sizes() {
    // SB-26 covers roughly 100--800 byte requests and Any-bearing records at
    // 10/100/1000 entries. Exercise those public size/count boundaries here;
    // conversion is schema-agnostic and operates on the complete wire image.
    for size in [100, 256, 512, 800] {
        let name = "x".repeat(size);
        let original = pbrs_request(&name);
        let prost: ProstHelloRequest = pbrs_to_prost(&original).expect("to prost");
        let round_trip: protobuf_tonic::hello::HelloRequest =
            prost_to_pbrs(&prost).expect("to pbrs");
        assert_eq!(round_trip.name().as_bytes(), name.as_bytes());
        assert_eq!(
            Serialize::serialize(&round_trip).expect("round-trip wire"),
            Serialize::serialize(&original).expect("original wire")
        );
    }

    for records in [10, 100, 1000] {
        let name = "r".repeat(records * 384);
        let original = ProstHelloRequest { name };
        let pbrs: protobuf_tonic::hello::HelloRequest =
            prost_to_pbrs(&original).expect("large prost to pbrs");
        let round_trip: ProstHelloRequest = pbrs_to_prost(&pbrs).expect("large pbrs to prost");
        assert_eq!(round_trip, original);
    }
}
