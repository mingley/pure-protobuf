FROM rust:1.94

WORKDIR /app
COPY rust_pbrs_mt_bench /app
COPY proto /app/proto
COPY pbrs-src /app/pbrs-src

RUN cargo build --release --locked

ENTRYPOINT ["/app/target/release/helloworld-server"]
