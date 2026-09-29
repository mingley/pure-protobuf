//! Richer error details tonic example port.

#[tokio::main]
async fn main() -> pbrs_grpc_example_tonic_ports::ExampleResult {
    pbrs_grpc_example_tonic_ports::run_error_details().await
}
