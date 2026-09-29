//! Prost message codec coverage for the native transport.

#![cfg(feature = "prost")]
#![allow(
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "integration tests"
)]

mod common;

use common::ServerGuard;
use pbrs_grpc::codec::prost::{Message as ProstMessage, Streaming as ProstStreaming};
use pbrs_grpc::{Channel, Request, Response, Router, Rpc, Service, Status};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;

#[derive(Clone, PartialEq, prost::Message)]
struct ProstRequest {
    #[prost(string, tag = "1")]
    value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct ProstReply {
    #[prost(string, tag = "1")]
    value: String,
}

struct ProstEcho;

impl Service for ProstEcho {
    const NAME: &'static str = "prost.Echo";

    async fn call(&self, rpc: Rpc) {
        match rpc.method() {
            "Unary" => {
                rpc.unary(|req: Request<ProstMessage<ProstRequest>>| async move {
                    Ok::<_, Status>(Response::new(ProstMessage(ProstReply {
                        value: format!("unary:{}", req.into_inner().0.value),
                    })))
                })
                .await;
            }
            "ServerStream" => {
                rpc.server_streaming(|req: Request<ProstMessage<ProstRequest>>| async move {
                    let (tx, stream) = pbrs_grpc::Streaming::channel(4);
                    let body = req.into_inner().0.value;
                    drop(tokio::spawn(async move {
                        for part in body.split(',') {
                            if tx
                                .send(ProstMessage(ProstReply {
                                    value: format!("server:{part}"),
                                }))
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
                rpc.client_streaming(
                    |req: Request<pbrs_grpc::Streaming<ProstMessage<ProstRequest>>>| async move {
                        let mut inbound = req.into_inner();
                        let mut values = Vec::new();
                        while let Some(msg) = inbound.message().await? {
                            values.push(msg.0.value);
                        }
                        Ok::<_, Status>(Response::new(ProstMessage(ProstReply {
                            value: format!("client:{}", values.join("|")),
                        })))
                    },
                )
                .await;
            }
            "Bidi" => {
                rpc.bidi_streaming(
                    |req: Request<pbrs_grpc::Streaming<ProstMessage<ProstRequest>>>| async move {
                        let mut inbound = req.into_inner();
                        let (tx, stream) = pbrs_grpc::Streaming::channel(4);
                        drop(tokio::spawn(async move {
                            loop {
                                match inbound.message().await {
                                    Ok(Some(msg)) => {
                                        if tx
                                            .send(ProstMessage(ProstReply {
                                                value: format!("bidi:{}", msg.0.value),
                                            }))
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
                    },
                )
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

async fn spawn_prost() -> Result<(SocketAddr, Channel, ServerGuard), Status> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(ProstEcho)
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = Channel::connect(addr).await?;
    Ok((addr, channel, ServerGuard(handle)))
}

#[tokio::test]
async fn prost_codec_works_for_all_four_rpc_shapes() {
    let (_addr, channel, _guard) = spawn_prost().await.expect("prost service");

    let unary = channel
        .unary::<ProstMessage<ProstRequest>, ProstMessage<ProstReply>>(
            "/prost.Echo/Unary",
            Request::new(ProstMessage(ProstRequest {
                value: "ada".to_owned(),
            })),
        )
        .await
        .expect("unary")
        .into_inner()
        .0;
    assert_eq!(unary.value, "unary:ada");

    let mut server = channel
        .server_streaming::<ProstMessage<ProstRequest>, ProstMessage<ProstReply>>(
            "/prost.Echo/ServerStream",
            Request::new(ProstMessage(ProstRequest {
                value: "a,b".to_owned(),
            })),
        )
        .await
        .expect("server stream")
        .map(ProstStreaming::from_native)
        .into_inner();
    assert_eq!(
        server.collect().await.expect("collect server"),
        vec![
            ProstReply {
                value: "server:a".to_owned(),
            },
            ProstReply {
                value: "server:b".to_owned(),
            },
        ]
    );

    let (tx, client_call) = channel
        .client_streaming::<ProstMessage<ProstRequest>, ProstMessage<ProstReply>>(
            "/prost.Echo/ClientStream",
            Request::new(()),
        );
    tx.send(ProstMessage(ProstRequest {
        value: "c".to_owned(),
    }))
    .await
    .expect("send c");
    tx.send(ProstMessage(ProstRequest {
        value: "d".to_owned(),
    }))
    .await
    .expect("send d");
    tx.close();
    let client = client_call.await.expect("client stream").into_inner().0;
    assert_eq!(client.value, "client:c|d");

    let (tx, bidi_call) = channel.bidi::<ProstMessage<ProstRequest>, ProstMessage<ProstReply>>(
        "/prost.Echo/Bidi",
        Request::new(()),
    );
    tx.send(ProstMessage(ProstRequest {
        value: "e".to_owned(),
    }))
    .await
    .expect("send e");
    tx.send(ProstMessage(ProstRequest {
        value: "f".to_owned(),
    }))
    .await
    .expect("send f");
    tx.close();
    let mut bidi = bidi_call
        .await
        .expect("bidi headers")
        .map(ProstStreaming::from_native)
        .into_inner();
    assert_eq!(
        bidi.collect().await.expect("collect bidi"),
        vec![
            ProstReply {
                value: "bidi:e".to_owned(),
            },
            ProstReply {
                value: "bidi:f".to_owned(),
            },
        ]
    );
}
