//! Cross-stack tonic example interop.

#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::panic,
    reason = "standalone interop tests"
)]

use pbrs_grpc_example_tonic_ports as ports;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub mod helloworld {
    tonic::include_proto!("helloworld");
}

pub mod routeguide {
    tonic::include_proto!("routeguide");
}

pub mod echo {
    tonic::include_proto!("grpc.examples.echo");
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

async fn bind() -> Result<(TcpListener, SocketAddr), BoxError> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

async fn tonic_channel(addr: SocketAddr) -> Result<tonic::transport::Channel, BoxError> {
    let endpoint = tonic::transport::Endpoint::try_from(format!("http://{addr}"))?;
    for _ in 0..80 {
        match endpoint.connect().await {
            Ok(channel) => return Ok(channel),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
        }
    }
    Ok(endpoint.connect().await?)
}

#[derive(Default)]
struct TonicGreeter;

#[tonic::async_trait]
impl helloworld::greeter_server::Greeter for TonicGreeter {
    async fn say_hello(
        &self,
        request: Request<helloworld::HelloRequest>,
    ) -> Result<Response<helloworld::HelloReply>, Status> {
        let name = request.into_inner().name;
        Ok(Response::new(helloworld::HelloReply {
            message: format!("Hello {name}!"),
        }))
    }
}

fn pbrs_hello_request(name: &str) -> ports::prost_gen::helloworld::HelloRequest {
    ports::prost_gen::helloworld::HelloRequest {
        name: name.to_owned(),
    }
}

async fn serve_tonic_greeter() -> Result<(SocketAddr, tokio::task::JoinHandle<()>), BoxError> {
    let (listener, addr) = bind().await?;
    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(helloworld::greeter_server::GreeterServer::new(
                TonicGreeter,
            ))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    Ok((addr, handle))
}

async fn serve_tonic_greeter_gzip() -> Result<(SocketAddr, tokio::task::JoinHandle<()>), BoxError> {
    let (listener, addr) = bind().await?;
    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                helloworld::greeter_server::GreeterServer::new(TonicGreeter)
                    .send_compressed(tonic::codec::CompressionEncoding::Gzip)
                    .accept_compressed(tonic::codec::CompressionEncoding::Gzip),
            )
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    Ok((addr, handle))
}

#[tokio::test]
async fn helloworld_cross_stack() -> Result<(), BoxError> {
    let (addr, _guard) = ports::spawn_pbrs_helloworld_server().await?;
    let mut tonic = helloworld::greeter_client::GreeterClient::new(tonic_channel(addr).await?);
    let reply = tonic
        .say_hello(Request::new(helloworld::HelloRequest {
            name: "Tonic".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello Tonic!");

    let (addr, handle) = serve_tonic_greeter_gzip().await?;
    let pbrs = ports::prost_gen::helloworld::GreeterClient::new(
        pbrs_grpc::Channel::connect(addr).await?,
    );
    let reply = pbrs
        .say_hello(pbrs_grpc::Request::new(pbrs_hello_request("pbrs")))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello pbrs!");
    handle.abort();
    Ok(())
}

#[derive(Default)]
struct TonicRouteGuide;

fn tonic_point(lat: i32, lon: i32) -> routeguide::Point {
    routeguide::Point {
        latitude: lat,
        longitude: lon,
    }
}

fn tonic_feature(name: &str, lat: i32, lon: i32) -> routeguide::Feature {
    routeguide::Feature {
        name: name.to_owned(),
        location: Some(tonic_point(lat, lon)),
    }
}

fn tonic_rect() -> routeguide::Rectangle {
    routeguide::Rectangle {
        lo: Some(tonic_point(400_000_000, -750_000_000)),
        hi: Some(tonic_point(420_000_000, -730_000_000)),
    }
}

fn tonic_note(message: &str) -> routeguide::RouteNote {
    routeguide::RouteNote {
        location: Some(tonic_point(409_146_138, -746_188_906)),
        message: message.to_owned(),
    }
}

#[tonic::async_trait]
impl routeguide::route_guide_server::RouteGuide for TonicRouteGuide {
    async fn get_feature(
        &self,
        request: Request<routeguide::Point>,
    ) -> Result<Response<routeguide::Feature>, Status> {
        if request.get_ref() == &tonic_point(409_146_138, -746_188_906) {
            Ok(Response::new(tonic_feature(
                "Berkshire Valley",
                409_146_138,
                -746_188_906,
            )))
        } else {
            Ok(Response::new(routeguide::Feature::default()))
        }
    }

    type ListFeaturesStream = ReceiverStream<Result<routeguide::Feature, Status>>;

    async fn list_features(
        &self,
        _request: Request<routeguide::Rectangle>,
    ) -> Result<Response<Self::ListFeaturesStream>, Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.send(Ok(tonic_feature(
            "Berkshire Valley",
            409_146_138,
            -746_188_906,
        )))
        .await
        .expect("send feature");
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn record_route(
        &self,
        request: Request<tonic::Streaming<routeguide::Point>>,
    ) -> Result<Response<routeguide::RouteSummary>, Status> {
        let started = Instant::now();
        let mut stream = request.into_inner();
        let mut count = 0;
        while stream.message().await?.is_some() {
            count += 1;
        }
        Ok(Response::new(routeguide::RouteSummary {
            point_count: count,
            elapsed_time: i32::try_from(started.elapsed().as_secs()).unwrap_or(0),
            ..Default::default()
        }))
    }

    type RouteChatStream =
        Pin<Box<dyn futures_util::Stream<Item = Result<routeguide::RouteNote, Status>> + Send>>;

    async fn route_chat(
        &self,
        request: Request<tonic::Streaming<routeguide::RouteNote>>,
    ) -> Result<Response<Self::RouteChatStream>, Status> {
        let stream = request.into_inner();
        Ok(Response::new(Box::pin(stream)))
    }
}

async fn serve_tonic_routeguide() -> Result<(SocketAddr, tokio::task::JoinHandle<()>), BoxError> {
    let (listener, addr) = bind().await?;
    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(routeguide::route_guide_server::RouteGuideServer::new(
                TonicRouteGuide,
            ))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    Ok((addr, handle))
}

#[tokio::test]
async fn routeguide_cross_stack_all_shapes() -> Result<(), BoxError> {
    let (addr, _guard) = ports::spawn_pbrs_routeguide_server().await?;
    let mut tonic = routeguide::route_guide_client::RouteGuideClient::new(tonic_channel(addr).await?);
    assert_eq!(
        tonic
            .get_feature(Request::new(tonic_point(409_146_138, -746_188_906)))
            .await?
            .into_inner()
            .name,
        "Berkshire Valley"
    );
    assert!(
        tonic
            .list_features(Request::new(tonic_rect()))
            .await?
            .into_inner()
            .message()
            .await?
            .is_some()
    );
    assert_eq!(
        tonic
            .record_route(Request::new(tokio_stream::iter([
                tonic_point(409_146_138, -746_188_906),
                tonic_point(411_733_222, -744_228_360),
            ])))
            .await?
            .into_inner()
            .point_count,
        2
    );
    assert_eq!(
        tonic
            .route_chat(Request::new(tokio_stream::iter([tonic_note("hi")])))
            .await?
            .into_inner()
            .message()
            .await?
            .expect("note")
            .message,
        "hi"
    );

    let (addr, handle) = serve_tonic_routeguide().await?;
    let pbrs = ports::prost_gen::routeguide::RouteGuideClient::new(
        pbrs_grpc::Channel::connect(addr).await?,
    );
    let point = ports::prost_gen::routeguide::Point {
        latitude: 409_146_138,
        longitude: -746_188_906,
    };
    assert_eq!(
        pbrs.get_feature(pbrs_grpc::Request::new(point))
            .await?
            .into_inner()
            .name,
        "Berkshire Valley"
    );
    let mut stream = pbrs
        .list_features(pbrs_grpc::Request::new(ports::prost_gen::routeguide::Rectangle {
            lo: Some(ports::prost_gen::routeguide::Point {
                latitude: 400_000_000,
                longitude: -750_000_000,
            }),
            hi: Some(ports::prost_gen::routeguide::Point {
                latitude: 420_000_000,
                longitude: -730_000_000,
            }),
        }))
        .await?
        .into_inner();
    assert!(stream.message().await?.is_some());
    let (sender, call) = pbrs.record_route(pbrs_grpc::Request::new(()));
    sender.send(point).await?;
    sender.close();
    assert_eq!(call.await?.into_inner().point_count, 1);
    let (sender, call) = pbrs.route_chat(pbrs_grpc::Request::new(()));
    sender
        .send(ports::prost_gen::routeguide::RouteNote {
            location: Some(ports::prost_gen::routeguide::Point {
                latitude: 409_146_138,
                longitude: -746_188_906,
            }),
            message: "hi".to_owned(),
        })
        .await?;
    sender.close();
    assert_eq!(call.await?.into_inner().message().await?.expect("note").message, "hi");
    handle.abort();
    Ok(())
}

#[derive(Default)]
struct TonicEcho;

#[tonic::async_trait]
impl echo::echo_server::Echo for TonicEcho {
    async fn unary_echo(
        &self,
        request: Request<echo::EchoRequest>,
    ) -> Result<Response<echo::EchoResponse>, Status> {
        Ok(Response::new(echo::EchoResponse {
            message: request.into_inner().message,
        }))
    }

    type ServerStreamingEchoStream = ReceiverStream<Result<echo::EchoResponse, Status>>;

    async fn server_streaming_echo(
        &self,
        request: Request<echo::EchoRequest>,
    ) -> Result<Response<Self::ServerStreamingEchoStream>, Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.send(Ok(echo::EchoResponse {
            message: request.into_inner().message,
        }))
        .await
        .expect("send echo");
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn client_streaming_echo(
        &self,
        request: Request<tonic::Streaming<echo::EchoRequest>>,
    ) -> Result<Response<echo::EchoResponse>, Status> {
        let mut inbound = request.into_inner();
        let mut values = Vec::new();
        while let Some(item) = inbound.message().await? {
            values.push(item.message);
        }
        Ok(Response::new(echo::EchoResponse {
            message: values.join("|"),
        }))
    }

    type BidirectionalStreamingEchoStream =
        ReceiverStream<Result<echo::EchoResponse, Status>>;

    async fn bidirectional_streaming_echo(
        &self,
        request: Request<tonic::Streaming<echo::EchoRequest>>,
    ) -> Result<Response<Self::BidirectionalStreamingEchoStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tokio::spawn(async move {
            while let Ok(Some(item)) = inbound.message().await {
                if tx.send(Ok(echo::EchoResponse { message: item.message })).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn serve_tonic_echo() -> Result<(SocketAddr, tokio::task::JoinHandle<()>), BoxError> {
    let (listener, addr) = bind().await?;
    let handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(echo::echo_server::EchoServer::new(TonicEcho))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    Ok((addr, handle))
}

#[tokio::test]
async fn streaming_cross_stack() -> Result<(), BoxError> {
    let (addr, _guard) = ports::spawn_pbrs_streaming_server().await?;
    let mut tonic = echo::echo_client::EchoClient::new(tonic_channel(addr).await?);
    assert_eq!(
        tonic
            .unary_echo(Request::new(echo::EchoRequest {
                message: "one".to_owned()
            }))
            .await?
            .into_inner()
            .message,
        "one"
    );
    assert_eq!(
        tonic
            .bidirectional_streaming_echo(Request::new(tokio_stream::iter([echo::EchoRequest {
                message: "two".to_owned(),
            }])))
            .await?
            .into_inner()
            .message()
            .await?
            .expect("echo")
            .message,
        "two"
    );

    let (addr, handle) = serve_tonic_echo().await?;
    let pbrs =
        ports::prost_gen::echo::EchoClient::new(pbrs_grpc::Channel::connect(addr).await?);
    let mut stream = pbrs
        .server_streaming_echo(pbrs_grpc::Request::new(ports::prost_gen::echo::EchoRequest {
            message: "three".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(stream.message().await?.expect("echo").message, "three");
    let (sender, call) = pbrs.bidirectional_streaming_echo(pbrs_grpc::Request::new(()));
    sender
        .send(ports::prost_gen::echo::EchoRequest {
            message: "four".to_owned(),
        })
        .await?;
    sender.close();
    assert_eq!(call.await?.into_inner().message().await?.expect("echo").message, "four");
    handle.abort();
    Ok(())
}

#[tokio::test]
async fn compression_and_error_details_cross_stack() -> Result<(), BoxError> {
    let (addr, _guard) = ports::spawn_pbrs_compression_server().await?;
    let mut tonic = helloworld::greeter_client::GreeterClient::new(tonic_channel(addr).await?)
        .send_compressed(tonic::codec::CompressionEncoding::Gzip)
        .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
    assert_eq!(
        tonic
            .say_hello(Request::new(helloworld::HelloRequest {
                name: "gzip".to_owned(),
            }))
            .await?
            .into_inner()
            .message,
        "Hello gzip!"
    );

    let (addr, _guard) = ports::spawn_pbrs_error_details_server().await?;
    let mut tonic = helloworld::greeter_client::GreeterClient::new(tonic_channel(addr).await?);
    let err = tonic
        .say_hello(Request::new(helloworld::HelloRequest {
            name: String::new(),
        }))
        .await
        .expect_err("error details");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(
        !err.details().is_empty(),
        "tonic status should expose grpc-status-details-bin bytes"
    );

    let (addr, handle) = serve_tonic_greeter().await?;
    let pbrs = ports::prost_gen::helloworld::GreeterClient::new(
        pbrs_grpc::Channel::connect(addr).await?.send_compressed(),
    );
    let err = pbrs
        .say_hello(pbrs_grpc::Request::new(pbrs_hello_request("reject")))
        .await
        .expect_err("tonic default rejects compressed requests");
    assert_eq!(err.code(), pbrs_grpc::Code::Unimplemented);
    handle.abort();

    let (addr, handle) = serve_tonic_greeter_gzip().await?;
    let pbrs = ports::prost_gen::helloworld::GreeterClient::new(
        pbrs_grpc::Channel::connect(addr).await?.send_compressed(),
    );
    assert_eq!(
        pbrs
            .say_hello(pbrs_grpc::Request::new(pbrs_hello_request("gzip")))
            .await?
            .into_inner()
            .message,
        "Hello gzip!"
    );
    handle.abort();
    Ok(())
}
