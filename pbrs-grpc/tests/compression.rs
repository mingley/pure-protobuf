//! Compression registry interop tests.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    missing_docs,
    reason = "integration tests"
)]

#[test]
fn published_codec_remains_exhaustive_with_original_discriminants() {
    use pbrs_grpc::Codec;

    fn name(codec: Codec) -> &'static str {
        match codec {
            Codec::Gzip => "gzip",
            Codec::Deflate => "deflate",
        }
    }

    assert_eq!(name(Codec::Gzip), "gzip");
    assert_eq!(name(Codec::Deflate), "deflate");
    assert_eq!(Codec::Gzip as u8, 0);
    assert_eq!(Codec::Deflate as u8, 1);
}

#[test]
fn published_codec_methods_keep_their_wire_behavior() {
    use pbrs_grpc::{Codec, CompressionAlgorithm, MessageLimits};

    let payload = b"published gzip and deflate APIs".repeat(32);
    for codec in [Codec::Gzip, Codec::Deflate] {
        let algorithm = CompressionAlgorithm::from(codec);
        assert_eq!(algorithm.legacy_codec(), Some(codec));
        assert_eq!(algorithm.name(), codec.name());
        assert_eq!(Codec::parse(codec.name()), Some(codec));
        assert_eq!(
            codec.encode(&payload).unwrap(),
            algorithm.encode(&payload).unwrap()
        );
        let compressed = codec.encode_level(&payload, 6).unwrap();
        assert_eq!(codec.decode(&compressed).unwrap(), payload);
        assert_eq!(
            codec
                .decode_limited(&compressed, MessageLimits::unlimited())
                .unwrap(),
            payload
        );
    }
    assert_eq!(Codec::parse(" GZIP;q=0.5 "), Some(Codec::Gzip));
    assert_eq!(Codec::parse("zstd"), None);
    assert_eq!(Codec::parse("identity"), None);
}

#[cfg(feature = "zstd")]
#[test]
fn extended_compression_selection_reports_and_restores_legacy_settings() {
    use pbrs_grpc::{ChannelConfig, Codec, CompressionAlgorithm, ServerConfig};

    macro_rules! check {
        ($config:expr) => {{
            let config = $config
                .compression_codec(Codec::Deflate)
                .compression_algorithm(CompressionAlgorithm::Zstd);
            assert_eq!(config.send_algorithm(), CompressionAlgorithm::Zstd);
            assert_eq!(config.send_codec(), Codec::Deflate);
            assert!(!config.compresses_outbound());
            let config = config.compression_codec(Codec::Gzip);
            assert_eq!(config.send_algorithm(), CompressionAlgorithm::Gzip);
            assert_eq!(config.send_codec(), Codec::Gzip);
            let config = config.compression_algorithm(CompressionAlgorithm::Deflate);
            assert_eq!(config.send_algorithm(), CompressionAlgorithm::Deflate);
            assert_eq!(config.send_codec(), Codec::Deflate);
        }};
    }
    check!(ServerConfig::new());
    check!(ChannelConfig::new());
}

#[cfg(feature = "zstd")]
mod common;

#[cfg(feature = "zstd")]
mod zstd {
    use crate::common::{Echo, name_of, req, serve};
    use pbrs_grpc::hello::GreeterClient;
    use pbrs_grpc::{Channel, ChannelConfig, CompressionAlgorithm, Request, ServerConfig, Status};
    use std::net::SocketAddr;
    use std::time::Duration;

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
            .compression_algorithm(CompressionAlgorithm::Zstd);
        let (addr, _guard) = serve(Echo, config).await.expect("serve");
        let client = client(
            addr,
            ChannelConfig::new()
                .send_compressed(true)
                .compression_algorithm(CompressionAlgorithm::Zstd),
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
