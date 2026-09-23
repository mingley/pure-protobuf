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
    use pbrs_grpc_example_greeter::production::{run_local_fixture_demo, FixtureMode};
    use std::ffi::OsStr;

    let args: Vec<_> = std::env::args_os().collect();
    let result = match args.as_slice() {
        [_] => pbrs_grpc_example_greeter::run().await,
        [_, flag, dir] if flag == OsStr::new("--tls-demo") => {
            run_local_fixture_demo(std::path::Path::new(dir), FixtureMode::Tls)
                .await
                .map(|_| "[Tls] overload, readiness and bounded drain verified".to_owned())
        }
        [_, flag, dir] if flag == OsStr::new("--mtls-demo") => {
            run_local_fixture_demo(std::path::Path::new(dir), FixtureMode::Mtls)
                .await
                .map(|_| "[Mtls] overload, readiness and bounded drain verified".to_owned())
        }
        _ => Err(pbrs_grpc::Status::invalid_argument(
            "usage: greeter [--tls-demo <local-fixture-dir> | --mtls-demo <local-fixture-dir>]",
        )),
    };
    match result {
        Ok(msg) => println!("{msg}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
