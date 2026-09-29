//! In-process smoke tests for every tonic example port.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "integration test assertions"
)]

#[tokio::test]
async fn runs_every_tonic_example_port() -> pbrs_grpc_example_tonic_ports::ExampleResult {
    pbrs_grpc_example_tonic_ports::run_helloworld().await?;
    pbrs_grpc_example_tonic_ports::run_routeguide().await?;
    pbrs_grpc_example_tonic_ports::run_streaming().await?;
    pbrs_grpc_example_tonic_ports::run_interceptor().await?;
    pbrs_grpc_example_tonic_ports::run_health().await?;
    pbrs_grpc_example_tonic_ports::run_reflection().await?;
    pbrs_grpc_example_tonic_ports::run_tls().await?;
    pbrs_grpc_example_tonic_ports::run_uds().await?;
    pbrs_grpc_example_tonic_ports::run_compression().await?;
    pbrs_grpc_example_tonic_ports::run_error_details().await?;
    pbrs_grpc_example_tonic_ports::run_load_balance().await?;
    pbrs_grpc_example_tonic_ports::run_dynamic_load_balance().await?;
    pbrs_grpc_example_tonic_ports::run_json_codec().await?;
    pbrs_grpc_example_tonic_ports::run_tracing().await?;
    pbrs_grpc_example_tonic_ports::run_authentication().await?;
    pbrs_grpc_example_tonic_ports::run_cancellation().await?;
    pbrs_grpc_example_tonic_ports::run_h2c().await?;
    pbrs_grpc_example_tonic_ports::run_tower().await?;
    Ok(())
}
