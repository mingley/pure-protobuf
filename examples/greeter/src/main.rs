//! Loopback greeter: generated stubs, health, and reflection.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "example binary"
)]

#[tokio::main]
async fn main() {
    match pbrs_grpc_example_greeter::run().await {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
