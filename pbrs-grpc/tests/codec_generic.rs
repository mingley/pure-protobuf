//! Foreign-message codec coverage for all native RPC shapes.

#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use bytes::Bytes;
use common::ServerGuard;
use pbrs::WireOut;
use pbrs_grpc::{
    Channel, CodecMessage, Request, Response, Router, Rpc, Service, Status, Streaming,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Foreign(String);

impl Foreign {
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl CodecMessage for Foreign {
    fn encoded_len(&self) -> usize {
        self.0.len()
    }

    fn encode_payload<W: WireOut>(&self, out: &mut W) -> Result<(), Status> {
        out.put_slice(self.0.as_bytes());
        Ok(())
    }

    fn decode_payload(payload: Bytes) -> Result<Self, Status> {
        String::from_utf8(payload.to_vec())
            .map(Foreign)
            .map_err(|e| Status::internal(e.to_string()))
    }

    fn empty() -> Self {
        Self::default()
    }
}

struct ForeignEcho;

impl Service for ForeignEcho {
    const NAME: &'static str = "foreign.Echo";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Unary" => {
                rpc.unary(|req: Request<Foreign>| async move {
                    Ok::<_, Status>(Response::new(Foreign::new(format!(
                        "unary:{}",
                        req.get_ref().0
                    ))))
                })
                .await;
            }
            "ServerStream" => {
                rpc.server_streaming(|req: Request<Foreign>| async move {
                    let (tx, stream) = Streaming::channel(4);
                    let body = req.into_inner().0;
                    drop(tokio::spawn(async move {
                        for part in body.split(',') {
                            if tx
                                .send(Foreign::new(format!("server:{part}")))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }));
                    Ok::<_, Status>(Response::new(stream))
                })
                .await;
            }
            "ClientStream" => {
                rpc.client_streaming(|req: Request<Streaming<Foreign>>| async move {
                    let mut inbound = req.into_inner();
                    let mut values = Vec::new();
                    while let Some(msg) = inbound.message().await? {
                        values.push(msg.0);
                    }
                    Ok::<_, Status>(Response::new(Foreign::new(format!(
                        "client:{}",
                        values.join("|")
                    ))))
                })
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(|req: Request<Streaming<Foreign>>| async move {
                    let mut inbound = req.into_inner();
                    let (tx, stream) = Streaming::channel(4);
                    drop(tokio::spawn(async move {
                        loop {
                            match inbound.message().await {
                                Ok(Some(msg)) => {
                                    if tx
                                        .send(Foreign::new(format!("bidi:{}", msg.0)))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Ok(None) => break,
                                Err(status) => {
                                    tx.fail(status).await;
                                    break;
                                }
                            }
                        }
                    }));
                    Ok::<_, Status>(Response::new(stream))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn spawn_foreign() -> Result<(SocketAddr, Channel, ServerGuard), Status> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(ForeignEcho)
            .serve_with_shutdown(listener, std::future::pending())
            .await
            .ok();
    });
    let channel = Channel::connect(addr).await?;
    Ok((addr, channel, ServerGuard(handle)))
}

#[tokio::test]
async fn foreign_codec_works_for_all_four_rpc_shapes() {
    let (_addr, channel, _guard) = spawn_foreign().await.expect("foreign service");

    let unary = channel
        .unary::<Foreign, Foreign>("/foreign.Echo/Unary", Request::new(Foreign::new("ada")))
        .await
        .expect("unary")
        .into_inner();
    assert_eq!(unary, Foreign::new("unary:ada"));

    let mut server = channel
        .server_streaming::<Foreign, Foreign>(
            "/foreign.Echo/ServerStream",
            Request::new(Foreign::new("a,b")),
        )
        .await
        .expect("server stream")
        .into_inner();
    let mut server_values = Vec::new();
    while let Some(msg) = server.message().await.expect("server msg") {
        server_values.push(msg.0);
    }
    assert_eq!(server_values, ["server:a", "server:b"]);

    let (tx, client_call) = channel
        .client_streaming::<Foreign, Foreign>("/foreign.Echo/ClientStream", Request::new(()));
    tx.send(Foreign::new("c")).await.expect("send c");
    tx.send(Foreign::new("d")).await.expect("send d");
    tx.close();
    let client = client_call.await.expect("client stream").into_inner();
    assert_eq!(client, Foreign::new("client:c|d"));

    let (tx, bidi_call) = channel.bidi::<Foreign, Foreign>("/foreign.Echo/Bidi", Request::new(()));
    tx.send(Foreign::new("e")).await.expect("send e");
    tx.send(Foreign::new("f")).await.expect("send f");
    tx.close();
    let mut bidi = bidi_call.await.expect("bidi headers").into_inner();
    let mut bidi_values = Vec::new();
    while let Some(msg) = bidi.message().await.expect("bidi msg") {
        bidi_values.push(msg.0);
    }
    assert_eq!(bidi_values, ["bidi:e", "bidi:f"]);
}
