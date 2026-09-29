//! Load balancing tonic example port.

#[tokio::main]
async fn main() -> pbrs_grpc_example_tonic_ports::ExampleResult {
    pbrs_grpc_example_tonic_ports::run_load_balance().await
}
