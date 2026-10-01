//! Qualify every RPC specimen before registering or measuring transport work.
use pbrs_adoption_corpus::{
    rpc::Inputs,
    workloads::{CELLS, Codec, Operation},
};

fn main() {
    let rows: Vec<_> = CELLS.iter().filter(|c| {
        matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll)
    }).map(|c| serde_json::json!({
        "specimen": c.id.strip_prefix("codec.adoption.prost.").unwrap().strip_suffix(".read_all").unwrap(),
        "qualification": Inputs::prepare(c.specimen).report(),
    })).collect();
    println!("{}", serde_json::to_string_pretty(&rows).unwrap());
}
