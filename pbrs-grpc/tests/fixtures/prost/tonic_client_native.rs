use pbrs_grpc::codec::prost::Message as ProstMessage;
use pbrs_grpc::{Channel, Request, Response, Router, Rpc, Service, Status};
use std::convert::TryFrom;
use tonic::codegen::http::uri::PathAndQuery;

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

struct Echo;

impl Service for Echo {
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
                    tokio::spawn(async move {
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
                    });
                    Ok::<_, Status>(Response::new(stream))
                })
                .await;
            }
            _ => rpc.unimplemented(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        Router::new()
            .add_service(Echo)
            .serve_listener(listener)
            .await
            .ok();
    });
    let _native = Channel::connect(addr).await?;
    let endpoint = tonic::transport::Endpoint::try_from(format!("http://{addr}"))?
        .connect()
        .await?;
    let mut grpc = tonic::client::Grpc::new(endpoint);
    grpc.ready().await?;
    let unary = grpc
        .unary(
            tonic::Request::new(ProstRequest {
                value: "tonic".to_owned(),
            }),
            PathAndQuery::from_static("/prost.Echo/Unary"),
            tonic_prost::ProstCodec::<ProstRequest, ProstReply>::default(),
        )
        .await?
        .into_inner();
    assert_eq!(unary.value, "unary:tonic");

    grpc.ready().await?;
    let mut stream = grpc
        .server_streaming(
            tonic::Request::new(ProstRequest {
                value: "x,y".to_owned(),
            }),
            PathAndQuery::from_static("/prost.Echo/ServerStream"),
            tonic_prost::ProstCodec::<ProstRequest, ProstReply>::default(),
        )
        .await?
        .into_inner();
    let mut values = Vec::new();
    while let Some(message) = stream.message().await? {
        values.push(message.value);
    }
    assert_eq!(values, ["server:x", "server:y"]);
    server.abort();
    println!("tonic interop ok");
    Ok(())
}
