//! Native socket policy works for plaintext TCP and TLS.
#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::let_underscore_must_use,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    unreachable_pub,
    missing_docs,
    reason = "loopback transport assertions"
)]
mod common;

use common::{Echo, name_of, req};
use pbrs_grpc::{
    Channel, ChannelConfig, ClientTls, GreeterClient, GreeterServer, Identity, Request, ServerTls,
};
use tokio::net::TcpListener;

const CA: &str = include_str!("tls_data/ca.crt");
const CERT: &str = include_str!("tls_data/server.crt");
const KEY: &str = include_str!("tls_data/server.key");

#[tokio::test]
async fn configurable_nodelay_serves_plaintext_and_tls_with_either_setting() {
    for tls in [false, true] {
        for enabled in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("address");
            let server = GreeterServer::new(Echo).tcp_nodelay(enabled);
            let task = tokio::spawn(async move {
                if tls {
                    server
                        .serve_tls_with_shutdown(
                            listener,
                            std::future::pending(),
                            ServerTls::new(Identity::from_pem(CERT, KEY).expect("identity"))
                                .expect("TLS"),
                        )
                        .await
                        .expect("serve TLS");
                } else {
                    server.serve_listener(listener).await.expect("serve TCP");
                }
            });
            let config = ChannelConfig::new().tcp_nodelay(enabled);
            let channel = if tls {
                Channel::connect_tls_with(
                    addr,
                    config,
                    ClientTls::ca("localhost", CA).expect("client TLS"),
                )
                .await
            } else {
                Channel::connect_with(addr, config).await
            }
            .expect("connect");
            let client = GreeterClient::new(channel);
            let reply = client
                .say_hello(Request::new(req("socket policy")))
                .await
                .expect("unary reply");
            assert_eq!(name_of(reply.get_ref()), "socket policy");
            task.abort();
            let _ = task.await;
        }
    }
}
