//! Compression registry interop tests.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    missing_docs,
    reason = "integration tests"
)]

#[cfg(feature = "zstd")]
mod common;

#[cfg(feature = "zstd")]
mod zstd {
    use crate::common::{Echo, name_of, req, serve};
    use pbrs_grpc::hello::GreeterClient;
    use pbrs_grpc::{Channel, ChannelConfig, Codec, Request, ServerConfig, Status};
    use std::net::SocketAddr;
    use std::time::Duration;

    #[test]
    fn zstd_rs_interops_with_tonics_c_zstd_codec() {
        let payload = b"tonic zstd interop payload".repeat(256);
        let ours = Codec::Zstd.encode(&payload).expect("zstd-rs encode");
        let decoded_by_c = zstd_c::stream::decode_all(ours.as_slice()).expect("zstd-c decode");
        assert_eq!(decoded_by_c, payload);

        let c = zstd_c::stream::encode_all(payload.as_slice(), 1).expect("zstd-c encode");
        let decoded = Codec::Zstd
            .decode_limited(&c, pbrs_grpc::MessageLimits::unlimited())
            .expect("zstd-rs decode");
        assert_eq!(decoded, payload);
    }

    async fn client(addr: SocketAddr, config: ChannelConfig) -> GreeterClient {
        let mut last = Status::unavailable("connect");
        for _ in 0..80 {
            match Channel::connect_with(addr, config).await {
                Ok(channel) => return GreeterClient::new(channel),
                Err(e) => {
                    last = e;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        }
        panic!("connect {addr}: {last}");
    }

    #[tokio::test]
    async fn zstd_negotiates_for_unary_requests_and_responses() {
        let config = ServerConfig::new()
            .send_compressed(true)
            .compression_codec(Codec::Zstd);
        let (addr, _guard) = serve(Echo, config).await.expect("serve");
        let client = client(
            addr,
            ChannelConfig::new()
                .send_compressed(true)
                .compression_codec(Codec::Zstd),
        )
        .await;
        let response = client
            .say_hello(Request::new(req("zara")))
            .await
            .expect("unary");
        assert!(response.compressed(), "server response must be zstd");
        assert_eq!(response.encoding(), Some("zstd"));
        assert_eq!(name_of(response.get_ref()), "zara");
    }
}
