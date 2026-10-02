//! Source-bound input oracle. No timing window or shipping/collector changes.
use pbrs_adoption_corpus::{
    rpc::Inputs,
    workloads::{CELLS, Codec, Operation},
};
use prost::Message;

fn main() {
    let mut reports = Vec::new();
    for cell in CELLS
        .iter()
        .filter(|c| matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll))
    {
        let specimen = cell
            .id
            .strip_prefix("codec.adoption.prost.")
            .unwrap()
            .strip_suffix(".read_all")
            .unwrap();
        // Existing preparation validates complete values, read checksums and clones.
        let input = Inputs::prepare(cell.specimen);
        let mut report = input.report();
        let wire = match &input {
            Inputs::Query(pair) => pair.prost.encode_to_vec(),
            Inputs::Entities(pair) => pair.prost.encode_to_vec(),
            Inputs::Sparse(pair) => pair.prost.encode_to_vec(),
            Inputs::Maps(pair) => pair.prost.encode_to_vec(),
        };
        report["specimen"] = specimen.into();
        if specimen.starts_with("maps.") {
            assert_eq!(report["equal_wire_work"], false);
            // No timing bypass: the unchanged collector blocks these profiles.
            report["state"] = "blocked".into();
            report["input_wire_fingerprint"] = serde_json::Value::Null;
            report["reason"] = "unchanged RPC assert_equal_wire_work rejects native map wire +4 bytes; unordered map encoding has no stable fingerprint here".into();
        } else {
            assert_eq!(report["equal_wire_work"], true);
            assert_eq!(wire.len() as u64, report["request_bytes"].as_u64().unwrap());
            let hash = wire.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            report["state"] = "eligible".into();
            report["input_wire_fingerprint"] = format!("fnv1a64:{hash:016x}").into();
        }
        reports.push(report);
    }
    assert_eq!(reports.len(), 64);
    println!("{}", serde_json::to_string_pretty(&reports).unwrap());
}
