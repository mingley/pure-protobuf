//! Ports of selected public tonic examples to pbrs-grpc adoption paths.

#![allow(
    missing_docs,
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::let_underscore_must_use,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    unreachable_pub,
    reason = "example crate and generated modules"
)]

// Portions of the handler/client flow are adapted from tonic's public examples
// (Copyright (c) 2025 Lucio Franco, MIT license). The proto files under
// `proto/` keep the upstream gRPC Apache-2.0 headers.

use pbrs_grpc::compat;
use pbrs_grpc::{
    Channel, ClientTls, Code, Identity, Request, Response, Router, Server, ServerTls, Status,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::net::TcpListener;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type ExampleResult<T = ()> = Result<T, BoxError>;

const CA: &str = include_str!("../../../pbrs-grpc/tests/tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("../../../pbrs-grpc/tests/tls_data/server.crt");
const SERVER_KEY: &str = include_str!("../../../pbrs-grpc/tests/tls_data/server.key");

pub mod compat_gen {
    pub mod echo {
        include!(concat!(env!("OUT_DIR"), "/compat/echo.rs"));
    }
    pub mod helloworld {
        include!(concat!(env!("OUT_DIR"), "/compat/helloworld.rs"));
    }
    pub mod routeguide {
        include!(concat!(env!("OUT_DIR"), "/compat/route_guide.rs"));
    }
    pub mod unary_echo {
        include!(concat!(env!("OUT_DIR"), "/compat/unary_echo.rs"));
    }
}

pub mod prost_gen {
    pub mod echo {
        include!(concat!(env!("OUT_DIR"), "/prost/grpc.examples.echo.rs"));
        include!(concat!(env!("OUT_DIR"), "/prost/echo.pbrs_grpc.rs"));
    }
    pub mod helloworld {
        include!(concat!(env!("OUT_DIR"), "/prost/helloworld.rs"));
        include!(concat!(env!("OUT_DIR"), "/prost/helloworld.pbrs_grpc.rs"));
    }
    pub mod routeguide {
        include!(concat!(env!("OUT_DIR"), "/prost/routeguide.rs"));
        include!(concat!(env!("OUT_DIR"), "/prost/route_guide.pbrs_grpc.rs"));
    }
    pub mod unary_echo {
        include!(concat!(
            env!("OUT_DIR"),
            "/prost/grpc.examples.unaryecho.rs"
        ));
        include!(concat!(env!("OUT_DIR"), "/prost/unary_echo.pbrs_grpc.rs"));
    }
}

struct ServerGuard {
    handle: tokio::task::JoinHandle<()>,
    unix_path: Option<PathBuf>,
}

impl ServerGuard {
    fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle,
            unix_path: None,
        }
    }

    fn with_unix_path(mut self, path: PathBuf) -> Self {
        self.unix_path = Some(path);
        self
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.handle.abort();
        if let Some(path) = &self.unix_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn bind_loopback() -> Result<(TcpListener, SocketAddr), Status> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    Ok((listener, addr))
}

async fn connect(addr: SocketAddr) -> Result<Channel, Status> {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect(addr).await {
            Ok(channel) => return Ok(channel),
            Err(status) => {
                last = status;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

#[cfg(feature = "zstd")]
async fn connect_with(
    addr: SocketAddr,
    config: pbrs_grpc::ChannelConfig,
) -> Result<Channel, Status> {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match Channel::connect_with(addr, config).await {
            Ok(channel) => return Ok(channel),
            Err(status) => {
                last = status;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

fn server_identity() -> Result<Identity, Status> {
    Identity::from_pem(SERVER_CERT, SERVER_KEY)
}

fn client_tls() -> Result<ClientTls, Status> {
    ClientTls::ca("localhost", CA)
}

fn unix_sock(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pbrs-tonic-port-{name}-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    path
}

#[cfg(unix)]
async fn connect_unix(path: &std::path::Path) -> Result<Channel, Status> {
    let mut last = Status::unavailable("connect_unix");
    for _ in 0..80 {
        match Channel::connect_unix(path).await {
            Ok(channel) => return Ok(channel),
            Err(status) => {
                last = status;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    }
    Err(last)
}

fn compat_hello_request(name: &str) -> compat_gen::helloworld::HelloRequest {
    let mut req = compat_gen::helloworld::HelloRequest::new();
    req.set_name(name);
    req
}

fn compat_hello_reply(message: impl Into<String>) -> compat_gen::helloworld::HelloReply {
    let mut reply = compat_gen::helloworld::HelloReply::new();
    reply.set_message(message.into());
    reply
}

fn compat_name(req: &compat_gen::helloworld::HelloRequest) -> String {
    req.name().to_str().unwrap_or("").to_owned()
}

fn compat_reply_message(reply: &compat_gen::helloworld::HelloReply) -> String {
    reply.message().to_str().unwrap_or("").to_owned()
}

fn prost_hello_request(name: &str) -> prost_gen::helloworld::HelloRequest {
    prost_gen::helloworld::HelloRequest {
        name: name.to_owned(),
    }
}

struct CompatGreeter;

impl compat_gen::helloworld::Greeter for CompatGreeter {
    async fn say_hello(
        &self,
        request: compat::Request<compat_gen::helloworld::HelloRequest>,
    ) -> Result<compat::Response<compat_gen::helloworld::HelloReply>, compat::Status> {
        let name = compat_name(request.get_ref());
        Ok(compat::Response::new(compat_hello_reply(format!(
            "Hello {name}!"
        ))))
    }
}

struct ProstGreeter;

impl prost_gen::helloworld::Greeter for ProstGreeter {
    async fn say_hello(
        &self,
        request: Request<prost_gen::helloworld::HelloRequest>,
    ) -> Result<Response<prost_gen::helloworld::HelloReply>, Status> {
        Ok(Response::new(prost_gen::helloworld::HelloReply {
            message: format!("Hello {}!", request.into_inner().name),
        }))
    }
}

async fn serve_compat_greeter() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        compat_gen::helloworld::GreeterServer::new(CompatGreeter)
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

async fn serve_prost_greeter() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

pub async fn run_helloworld() -> ExampleResult {
    let (addr, _guard) = serve_compat_greeter().await?;
    let mut client = compat_gen::helloworld::GreeterClient::connect(addr).await?;
    let reply = client
        .say_hello(compat_hello_request("Tonic"))
        .await?
        .into_inner();
    assert_eq!(compat_reply_message(&reply), "Hello Tonic!");

    let (addr, _guard) = serve_prost_greeter().await?;
    let channel = connect(addr).await?;
    let client = prost_gen::helloworld::GreeterClient::new(channel);
    let reply = client
        .say_hello(Request::new(prost_hello_request("Tonic")))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello Tonic!");
    Ok(())
}

fn compat_echo_request(message: &str) -> compat_gen::echo::EchoRequest {
    let mut req = compat_gen::echo::EchoRequest::new();
    req.set_message(message);
    req
}

fn compat_echo_response(message: impl Into<String>) -> compat_gen::echo::EchoResponse {
    let mut resp = compat_gen::echo::EchoResponse::new();
    resp.set_message(message.into());
    resp
}

fn compat_echo_message(req: &compat_gen::echo::EchoRequest) -> String {
    req.message().to_str().unwrap_or("").to_owned()
}

fn compat_echo_response_message(resp: &compat_gen::echo::EchoResponse) -> String {
    resp.message().to_str().unwrap_or("").to_owned()
}

struct CompatEcho;

impl compat_gen::echo::Echo for CompatEcho {
    type BidirectionalStreamingEchoStream =
        compat::Iter<Result<compat_gen::echo::EchoResponse, Status>>;
    type ServerStreamingEchoStream = compat::Iter<Result<compat_gen::echo::EchoResponse, Status>>;

    async fn unary_echo(
        &self,
        request: compat::Request<compat_gen::echo::EchoRequest>,
    ) -> Result<compat::Response<compat_gen::echo::EchoResponse>, compat::Status> {
        Ok(compat::Response::new(compat_echo_response(
            compat_echo_message(request.get_ref()),
        )))
    }

    async fn server_streaming_echo(
        &self,
        request: compat::Request<compat_gen::echo::EchoRequest>,
    ) -> Result<compat::Response<Self::ServerStreamingEchoStream>, compat::Status> {
        let message = compat_echo_message(request.get_ref());
        let items = (0..3).map(|_| Ok(compat_echo_response(message.clone())));
        Ok(compat::Response::new(compat::iter(items)))
    }

    async fn client_streaming_echo(
        &self,
        request: compat::Request<compat::Streaming<compat_gen::echo::EchoRequest>>,
    ) -> Result<compat::Response<compat_gen::echo::EchoResponse>, compat::Status> {
        let mut inbound = request.into_inner();
        let mut values = Vec::new();
        while let Some(item) = inbound.message().await? {
            values.push(compat_echo_message(&item));
        }
        Ok(compat::Response::new(compat_echo_response(
            values.join("|"),
        )))
    }

    async fn bidirectional_streaming_echo(
        &self,
        request: compat::Request<compat::Streaming<compat_gen::echo::EchoRequest>>,
    ) -> Result<compat::Response<Self::BidirectionalStreamingEchoStream>, compat::Status> {
        let mut inbound = request.into_inner();
        let mut values = Vec::new();
        while let Some(item) = inbound.message().await? {
            values.push(Ok(compat_echo_response(compat_echo_message(&item))));
        }
        Ok(compat::Response::new(compat::iter(values)))
    }
}

struct ProstEcho;

impl prost_gen::echo::Echo for ProstEcho {
    async fn unary_echo(
        &self,
        request: Request<prost_gen::echo::EchoRequest>,
    ) -> Result<Response<prost_gen::echo::EchoResponse>, Status> {
        Ok(Response::new(prost_gen::echo::EchoResponse {
            message: request.into_inner().message,
        }))
    }

    async fn server_streaming_echo(
        &self,
        request: Request<prost_gen::echo::EchoRequest>,
    ) -> Result<Response<pbrs_grpc::codec::prost::Streaming<prost_gen::echo::EchoResponse>>, Status>
    {
        let message = request.into_inner().message;
        let (tx, stream) = pbrs_grpc::codec::prost::Streaming::channel(4);
        for _ in 0..3 {
            tx.send(prost_gen::echo::EchoResponse {
                message: message.clone(),
            })
            .await?;
        }
        tx.close();
        Ok(Response::new(stream))
    }

    async fn client_streaming_echo(
        &self,
        request: Request<pbrs_grpc::codec::prost::Streaming<prost_gen::echo::EchoRequest>>,
    ) -> Result<Response<prost_gen::echo::EchoResponse>, Status> {
        let mut inbound = request.into_inner();
        let mut values = Vec::new();
        while let Some(item) = inbound.message().await? {
            values.push(item.message);
        }
        Ok(Response::new(prost_gen::echo::EchoResponse {
            message: values.join("|"),
        }))
    }

    async fn bidirectional_streaming_echo(
        &self,
        request: Request<pbrs_grpc::codec::prost::Streaming<prost_gen::echo::EchoRequest>>,
    ) -> Result<Response<pbrs_grpc::codec::prost::Streaming<prost_gen::echo::EchoResponse>>, Status>
    {
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::codec::prost::Streaming::channel(4);
        while let Some(item) = inbound.message().await? {
            tx.send(prost_gen::echo::EchoResponse {
                message: item.message,
            })
            .await?;
        }
        tx.close();
        Ok(Response::new(stream))
    }
}

async fn serve_compat_echo() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        compat_gen::echo::EchoServer::new(CompatEcho)
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

async fn serve_prost_echo() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(prost_gen::echo::EchoServer::new(ProstEcho))
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

pub async fn run_streaming() -> ExampleResult {
    let (addr, _guard) = serve_compat_echo().await?;
    let mut client = compat_gen::echo::EchoClient::connect(addr).await?;
    let mut stream = client
        .server_streaming_echo(compat_echo_request("foo"))
        .await?
        .into_inner();
    let mut got = Vec::new();
    while let Some(item) = stream.message().await? {
        got.push(compat_echo_response_message(&item));
    }
    assert_eq!(got, ["foo", "foo", "foo"]);
    let response = client
        .bidirectional_streaming_echo(compat::iter([
            compat_echo_request("msg 01"),
            compat_echo_request("msg 02"),
        ]))
        .await?;
    let mut inbound = response.into_inner();
    let mut got = Vec::new();
    while let Some(item) = inbound.message().await? {
        got.push(item.message().to_str().unwrap_or("").to_owned());
    }
    assert_eq!(got, ["msg 01", "msg 02"]);

    let (addr, _guard) = serve_prost_echo().await?;
    let client = prost_gen::echo::EchoClient::new(connect(addr).await?);
    let mut stream = client
        .server_streaming_echo(Request::new(prost_gen::echo::EchoRequest {
            message: "foo".to_owned(),
        }))
        .await?
        .into_inner();
    assert_eq!(
        stream.collect().await?,
        vec![
            prost_gen::echo::EchoResponse {
                message: "foo".to_owned()
            },
            prost_gen::echo::EchoResponse {
                message: "foo".to_owned()
            },
            prost_gen::echo::EchoResponse {
                message: "foo".to_owned()
            },
        ]
    );
    let (sender, call) = client.bidirectional_streaming_echo(Request::new(()));
    sender
        .send(prost_gen::echo::EchoRequest {
            message: "msg 01".to_owned(),
        })
        .await?;
    sender
        .send(prost_gen::echo::EchoRequest {
            message: "msg 02".to_owned(),
        })
        .await?;
    sender.close();
    let mut stream = call.await?.into_inner();
    assert_eq!(
        stream.collect().await?,
        vec![
            prost_gen::echo::EchoResponse {
                message: "msg 01".to_owned()
            },
            prost_gen::echo::EchoResponse {
                message: "msg 02".to_owned()
            },
        ]
    );
    Ok(())
}

fn compat_point(lat: i32, lon: i32) -> compat_gen::routeguide::Point {
    let mut point = compat_gen::routeguide::Point::new();
    point.set_latitude(lat);
    point.set_longitude(lon);
    point
}

fn compat_feature(name: &str, lat: i32, lon: i32) -> compat_gen::routeguide::Feature {
    let mut feature = compat_gen::routeguide::Feature::new();
    feature.set_name(name);
    feature.set_location(compat_point(lat, lon));
    feature
}

fn compat_rect() -> compat_gen::routeguide::Rectangle {
    let mut rect = compat_gen::routeguide::Rectangle::new();
    rect.set_lo(compat_point(400_000_000, -750_000_000));
    rect.set_hi(compat_point(420_000_000, -730_000_000));
    rect
}

fn compat_note(message: &str, lat: i32) -> compat_gen::routeguide::RouteNote {
    let mut note = compat_gen::routeguide::RouteNote::new();
    note.set_location(compat_point(lat, -746_188_906));
    note.set_message(message);
    note
}

fn compat_in_range(
    point: &compat_gen::routeguide::Point,
    rect: &compat_gen::routeguide::Rectangle,
) -> bool {
    let left = rect.lo().longitude().min(rect.hi().longitude());
    let right = rect.lo().longitude().max(rect.hi().longitude());
    let bottom = rect.lo().latitude().min(rect.hi().latitude());
    let top = rect.lo().latitude().max(rect.hi().latitude());
    point.longitude() >= left
        && point.longitude() <= right
        && point.latitude() >= bottom
        && point.latitude() <= top
}

#[derive(Clone)]
struct CompatRouteGuide {
    features: Arc<Vec<compat_gen::routeguide::Feature>>,
}

impl compat_gen::routeguide::RouteGuide for CompatRouteGuide {
    type ListFeaturesStream = compat::Iter<Result<compat_gen::routeguide::Feature, Status>>;
    type RouteChatStream = compat::Iter<Result<compat_gen::routeguide::RouteNote, Status>>;

    async fn get_feature(
        &self,
        request: compat::Request<compat_gen::routeguide::Point>,
    ) -> Result<compat::Response<compat_gen::routeguide::Feature>, compat::Status> {
        let point = request.get_ref();
        let feature = self
            .features
            .iter()
            .find(|feature| feature.location() == point)
            .cloned()
            .unwrap_or_else(compat_gen::routeguide::Feature::new);
        Ok(compat::Response::new(feature))
    }

    async fn list_features(
        &self,
        request: compat::Request<compat_gen::routeguide::Rectangle>,
    ) -> Result<compat::Response<Self::ListFeaturesStream>, compat::Status> {
        let rect = request.into_inner();
        let items = self
            .features
            .iter()
            .filter(|feature| compat_in_range(feature.location(), &rect))
            .cloned()
            .map(Ok)
            .collect::<Vec<_>>();
        Ok(compat::Response::new(compat::iter(items)))
    }

    async fn record_route(
        &self,
        request: compat::Request<compat::Streaming<compat_gen::routeguide::Point>>,
    ) -> Result<compat::Response<compat_gen::routeguide::RouteSummary>, compat::Status> {
        let mut stream = request.into_inner();
        let started = Instant::now();
        let mut points = Vec::new();
        while let Some(point) = stream.message().await? {
            points.push(point);
        }
        let mut summary = compat_gen::routeguide::RouteSummary::new();
        summary.set_point_count(i32::try_from(points.len()).unwrap_or(i32::MAX));
        summary.set_feature_count(
            i32::try_from(
                points
                    .iter()
                    .filter(|point| {
                        self.features
                            .iter()
                            .any(|feature| feature.location() == *point)
                    })
                    .count(),
            )
            .unwrap_or(i32::MAX),
        );
        summary.set_elapsed_time(i32::try_from(started.elapsed().as_secs()).unwrap_or(i32::MAX));
        Ok(compat::Response::new(summary))
    }

    async fn route_chat(
        &self,
        request: compat::Request<compat::Streaming<compat_gen::routeguide::RouteNote>>,
    ) -> Result<compat::Response<Self::RouteChatStream>, compat::Status> {
        let mut stream = request.into_inner();
        let mut notes = Vec::new();
        while let Some(note) = stream.message().await? {
            notes.push(Ok(note));
        }
        Ok(compat::Response::new(compat::iter(notes)))
    }
}

fn prost_point(lat: i32, lon: i32) -> prost_gen::routeguide::Point {
    prost_gen::routeguide::Point {
        latitude: lat,
        longitude: lon,
    }
}

fn prost_feature(name: &str, lat: i32, lon: i32) -> prost_gen::routeguide::Feature {
    prost_gen::routeguide::Feature {
        name: name.to_owned(),
        location: Some(prost_point(lat, lon)),
    }
}

fn prost_rect() -> prost_gen::routeguide::Rectangle {
    prost_gen::routeguide::Rectangle {
        lo: Some(prost_point(400_000_000, -750_000_000)),
        hi: Some(prost_point(420_000_000, -730_000_000)),
    }
}

fn prost_note(message: &str, lat: i32) -> prost_gen::routeguide::RouteNote {
    prost_gen::routeguide::RouteNote {
        location: Some(prost_point(lat, -746_188_906)),
        message: message.to_owned(),
    }
}

fn prost_features() -> Vec<prost_gen::routeguide::Feature> {
    vec![
        prost_feature("Berkshire Valley", 409_146_138, -746_188_906),
        prost_feature("Patriots Path", 411_733_222, -744_228_360),
    ]
}

fn compat_features() -> Vec<compat_gen::routeguide::Feature> {
    vec![
        compat_feature("Berkshire Valley", 409_146_138, -746_188_906),
        compat_feature("Patriots Path", 411_733_222, -744_228_360),
    ]
}

fn prost_in_range(
    point: &prost_gen::routeguide::Point,
    rect: &prost_gen::routeguide::Rectangle,
) -> bool {
    let Some(lo) = rect.lo.as_ref() else {
        return false;
    };
    let Some(hi) = rect.hi.as_ref() else {
        return false;
    };
    let left = lo.longitude.min(hi.longitude);
    let right = lo.longitude.max(hi.longitude);
    let bottom = lo.latitude.min(hi.latitude);
    let top = lo.latitude.max(hi.latitude);
    point.longitude >= left
        && point.longitude <= right
        && point.latitude >= bottom
        && point.latitude <= top
}

#[derive(Clone)]
struct ProstRouteGuide {
    features: Arc<Vec<prost_gen::routeguide::Feature>>,
}

impl prost_gen::routeguide::RouteGuide for ProstRouteGuide {
    async fn get_feature(
        &self,
        request: Request<prost_gen::routeguide::Point>,
    ) -> Result<Response<prost_gen::routeguide::Feature>, Status> {
        let point = request.get_ref();
        let feature = self
            .features
            .iter()
            .find(|feature| feature.location.as_ref() == Some(point))
            .cloned()
            .unwrap_or_default();
        Ok(Response::new(feature))
    }

    async fn list_features(
        &self,
        request: Request<prost_gen::routeguide::Rectangle>,
    ) -> Result<Response<pbrs_grpc::codec::prost::Streaming<prost_gen::routeguide::Feature>>, Status>
    {
        let rect = request.into_inner();
        let (tx, stream) = pbrs_grpc::codec::prost::Streaming::channel(4);
        for feature in self
            .features
            .iter()
            .filter(|feature| {
                feature
                    .location
                    .as_ref()
                    .is_some_and(|point| prost_in_range(point, &rect))
            })
            .cloned()
        {
            tx.send(feature).await?;
        }
        tx.close();
        Ok(Response::new(stream))
    }

    async fn record_route(
        &self,
        request: Request<pbrs_grpc::codec::prost::Streaming<prost_gen::routeguide::Point>>,
    ) -> Result<Response<prost_gen::routeguide::RouteSummary>, Status> {
        let mut stream = request.into_inner();
        let started = Instant::now();
        let mut points = Vec::new();
        while let Some(point) = stream.message().await? {
            points.push(point);
        }
        Ok(Response::new(prost_gen::routeguide::RouteSummary {
            point_count: i32::try_from(points.len()).unwrap_or(i32::MAX),
            feature_count: i32::try_from(
                points
                    .iter()
                    .filter(|point| {
                        self.features
                            .iter()
                            .any(|feature| feature.location.as_ref() == Some(*point))
                    })
                    .count(),
            )
            .unwrap_or(i32::MAX),
            distance: 0,
            elapsed_time: i32::try_from(started.elapsed().as_secs()).unwrap_or(i32::MAX),
        }))
    }

    async fn route_chat(
        &self,
        request: Request<pbrs_grpc::codec::prost::Streaming<prost_gen::routeguide::RouteNote>>,
    ) -> Result<
        Response<pbrs_grpc::codec::prost::Streaming<prost_gen::routeguide::RouteNote>>,
        Status,
    > {
        let mut inbound = request.into_inner();
        let (tx, stream) = pbrs_grpc::codec::prost::Streaming::channel(4);
        while let Some(note) = inbound.message().await? {
            tx.send(note).await?;
        }
        tx.close();
        Ok(Response::new(stream))
    }
}

async fn serve_compat_routeguide() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let service = CompatRouteGuide {
        features: Arc::new(compat_features()),
    };
    let handle = tokio::spawn(async move {
        compat_gen::routeguide::RouteGuideServer::new(service)
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

async fn serve_prost_routeguide() -> Result<(SocketAddr, ServerGuard), Status> {
    let (listener, addr) = bind_loopback().await?;
    let service = ProstRouteGuide {
        features: Arc::new(prost_features()),
    };
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(prost_gen::routeguide::RouteGuideServer::new(service))
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard::new(handle)))
}

pub async fn run_routeguide() -> ExampleResult {
    let (addr, _guard) = serve_compat_routeguide().await?;
    let mut client = compat_gen::routeguide::RouteGuideClient::connect(addr).await?;
    let feature = client
        .get_feature(compat_point(409_146_138, -746_188_906))
        .await?
        .into_inner();
    assert_eq!(feature.name().to_str().unwrap_or(""), "Berkshire Valley");
    let mut listed = client.list_features(compat_rect()).await?.into_inner();
    assert!(listed.message().await?.is_some());
    let summary = client
        .record_route(compat::iter([
            compat_point(409_146_138, -746_188_906),
            compat_point(411_733_222, -744_228_360),
        ]))
        .await?
        .into_inner();
    assert_eq!(summary.point_count(), 2);
    let mut chat = client
        .route_chat(compat::iter([
            compat_note("first", 409_146_138),
            compat_note("second", 409_146_139),
        ]))
        .await?
        .into_inner();
    assert_eq!(
        chat.message()
            .await?
            .expect("note")
            .message()
            .to_str()
            .unwrap_or(""),
        "first"
    );

    let (addr, _guard) = serve_prost_routeguide().await?;
    let client = prost_gen::routeguide::RouteGuideClient::new(connect(addr).await?);
    let feature = client
        .get_feature(Request::new(prost_point(409_146_138, -746_188_906)))
        .await?
        .into_inner();
    assert_eq!(feature.name, "Berkshire Valley");
    let mut listed = client
        .list_features(Request::new(prost_rect()))
        .await?
        .into_inner();
    assert!(listed.message().await?.is_some());
    let (sender, call) = client.record_route(Request::new(()));
    sender.send(prost_point(409_146_138, -746_188_906)).await?;
    sender.send(prost_point(411_733_222, -744_228_360)).await?;
    sender.close();
    assert_eq!(call.await?.into_inner().point_count, 2);
    let (sender, call) = client.route_chat(Request::new(()));
    sender.send(prost_note("first", 409_146_138)).await?;
    sender.close();
    let mut stream = call.await?.into_inner();
    assert_eq!(stream.message().await?.expect("note").message, "first");
    Ok(())
}

#[derive(Clone)]
struct MyExtension {
    some_piece_of_data: String,
}

struct CompatInterceptGreeter;

impl compat_gen::helloworld::Greeter for CompatInterceptGreeter {
    async fn say_hello(
        &self,
        request: compat::Request<compat_gen::helloworld::HelloRequest>,
    ) -> Result<compat::Response<compat_gen::helloworld::HelloReply>, compat::Status> {
        let ext = request
            .extensions()
            .get::<MyExtension>()
            .map(|ext| ext.some_piece_of_data.as_str())
            .unwrap_or("missing");
        let name = compat_name(request.get_ref());
        Ok(compat::Response::new(compat_hello_reply(format!(
            "Hello {name}! {ext}"
        ))))
    }
}

struct ProstInterceptGreeter;

impl prost_gen::helloworld::Greeter for ProstInterceptGreeter {
    async fn say_hello(
        &self,
        request: Request<prost_gen::helloworld::HelloRequest>,
    ) -> Result<Response<prost_gen::helloworld::HelloReply>, Status> {
        let ext = request
            .extensions()
            .get::<MyExtension>()
            .map(|ext| ext.some_piece_of_data.as_str())
            .unwrap_or("missing");
        Ok(Response::new(prost_gen::helloworld::HelloReply {
            message: format!("Hello {}! {ext}", request.get_ref().name),
        }))
    }
}

fn server_intercept(rpc: &mut pbrs_grpc::Rpc) -> Result<(), Status> {
    rpc.extensions_mut().insert(MyExtension {
        some_piece_of_data: "foo".to_owned(),
    });
    Ok(())
}

fn client_intercept(call: &mut pbrs_grpc::Outgoing<'_>) -> Result<(), Status> {
    call.metadata_mut().insert("x-tonic-port", "interceptor")?;
    Ok(())
}

pub async fn run_interceptor() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Router::new()
            .intercept(server_intercept)
            .add_service(compat_gen::helloworld::GreeterServer::new(
                CompatInterceptGreeter,
            ))
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = connect(addr).await?.intercept(client_intercept);
    let mut client = compat_gen::helloworld::GreeterClient::new(channel);
    let reply = client
        .say_hello(compat_hello_request("Tonic"))
        .await?
        .into_inner();
    assert_eq!(compat_reply_message(&reply), "Hello Tonic! foo");
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Router::new()
            .intercept(server_intercept)
            .add_service(prost_gen::helloworld::GreeterServer::new(
                ProstInterceptGreeter,
            ))
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = connect(addr).await?.intercept(client_intercept);
    let client = prost_gen::helloworld::GreeterClient::new(channel);
    let reply = client
        .say_hello(Request::new(prost_hello_request("Tonic")))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello Tonic! foo");
    drop(ServerGuard::new(handle));
    Ok(())
}

async fn assert_health(addr: SocketAddr) -> Result<(), Status> {
    let channel = connect(addr).await?;
    let client = pbrs_grpc::health::HealthClient::new(channel);
    let mut req = pbrs_grpc::health::HealthCheckRequest::new();
    req.set_service("helloworld.Greeter");
    let response = client.check(Request::new(req)).await?.into_inner();
    assert_eq!(response.status(), pbrs_grpc::health::ServingStatus::Serving);
    Ok(())
}

pub async fn run_health() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let (health, reporter) = pbrs_grpc::health::service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(health)
            .add_service(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health(addr).await?;
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let (health, reporter) = pbrs_grpc::health::service();
    reporter.set_serving("helloworld.Greeter");
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(health)
            .add_service(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_health(addr).await?;
    drop(ServerGuard::new(handle));
    Ok(())
}

async fn assert_reflection(addr: SocketAddr) -> Result<(), Status> {
    let client = pbrs_grpc::reflection::ServerReflectionClient::new(connect(addr).await?);
    let (tx, call) = client.server_reflection_info(Request::new(()));
    let mut inbound = call.await?.into_inner();
    let mut req = pbrs_grpc::reflection::ServerReflectionRequest::new();
    req.set_list_services("");
    tx.send(req).await?;
    let response = inbound
        .message()
        .await?
        .ok_or_else(|| Status::internal("reflection stream ended"))?;
    let names: Vec<String> = response
        .list_services_response()
        .service()
        .iter()
        .map(|service| service.name().to_str().unwrap_or("").to_owned())
        .collect();
    assert!(names.contains(&"helloworld.Greeter".to_owned()));
    tx.close();
    Ok(())
}

pub async fn run_reflection() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let reflection = pbrs_grpc::reflection::service([compat_gen::helloworld::FILE_DESCRIPTOR_SET])?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(reflection)
            .add_service(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reflection(addr).await?;
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let reflection = pbrs_grpc::reflection::service([compat_gen::helloworld::FILE_DESCRIPTOR_SET])?;
    let handle = tokio::spawn(async move {
        Router::new()
            .add_service(reflection)
            .add_service(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    assert_reflection(addr).await?;
    drop(ServerGuard::new(handle));
    Ok(())
}

pub async fn run_tls() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let tls = ServerTls::new(server_identity()?)?;
    let handle = tokio::spawn(async move {
        Server::new(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let mut client = compat_gen::helloworld::GreeterClient::new(
        Channel::connect_tls(addr, client_tls()?).await?,
    );
    let reply = client
        .say_hello(compat_hello_request("TLS"))
        .await?
        .into_inner();
    assert_eq!(compat_reply_message(&reply), "Hello TLS!");
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let tls = ServerTls::new(server_identity()?)?;
    let handle = tokio::spawn(async move {
        Server::new(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .serve_tls_with_shutdown(listener, std::future::pending(), tls)
            .await
            .ok();
    });
    let client =
        prost_gen::helloworld::GreeterClient::new(Channel::connect_tls(addr, client_tls()?).await?);
    let reply = client
        .say_hello(Request::new(prost_hello_request("TLS")))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello TLS!");
    drop(ServerGuard::new(handle));
    Ok(())
}

#[cfg(unix)]
pub async fn run_uds() -> ExampleResult {
    let path = unix_sock("compat");
    let server_path = path.clone();
    let handle = tokio::spawn(async move {
        Server::new(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
            .serve_unix_unlink(server_path)
            .await
            .ok();
    });
    let mut client = compat_gen::helloworld::GreeterClient::new(connect_unix(&path).await?);
    let reply = client
        .say_hello(compat_hello_request("UDS"))
        .await?
        .into_inner();
    assert_eq!(compat_reply_message(&reply), "Hello UDS!");
    drop(ServerGuard::new(handle).with_unix_path(path));

    let path = unix_sock("prost");
    let server_path = path.clone();
    let handle = tokio::spawn(async move {
        Server::new(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .serve_unix_unlink(server_path)
            .await
            .ok();
    });
    let client = prost_gen::helloworld::GreeterClient::new(connect_unix(&path).await?);
    let reply = client
        .say_hello(Request::new(prost_hello_request("UDS")))
        .await?
        .into_inner();
    assert_eq!(reply.message, "Hello UDS!");
    drop(ServerGuard::new(handle).with_unix_path(path));
    Ok(())
}

#[cfg(not(unix))]
pub async fn run_uds() -> ExampleResult {
    Ok(())
}

pub async fn run_compression() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Server::new(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = connect(addr).await?.send_compressed();
    let mut client = compat_gen::helloworld::GreeterClient::new(channel);
    let response = client.say_hello(compat_hello_request("gzip")).await?;
    assert!(response.compressed());
    assert_eq!(response.encoding(), Some("gzip"));
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Server::new(prost_gen::helloworld::GreeterServer::new(ProstGreeter))
            .send_compressed()
            .serve_listener(listener)
            .await
            .ok();
    });
    let channel = connect(addr).await?.send_compressed();
    let client = prost_gen::helloworld::GreeterClient::new(channel);
    let response = client
        .say_hello(Request::new(prost_hello_request("gzip")))
        .await?;
    assert!(response.compressed());
    assert_eq!(response.encoding(), Some("gzip"));
    drop(ServerGuard::new(handle));

    #[cfg(feature = "zstd")]
    {
        let (listener, addr) = bind_loopback().await?;
        let handle = tokio::spawn(async move {
            let config = pbrs_grpc::ServerConfig::new()
                .send_compressed(true)
                .compression_codec(pbrs_grpc::Codec::Zstd);
            Server::new(compat_gen::helloworld::GreeterServer::new(CompatGreeter))
                .config(config)
                .serve_listener(listener)
                .await
                .ok();
        });
        let channel = connect_with(
            addr,
            pbrs_grpc::ChannelConfig::new()
                .send_compressed(true)
                .compression_codec(pbrs_grpc::Codec::Zstd),
        )
        .await?;
        let mut client = compat_gen::helloworld::GreeterClient::new(channel);
        let response = client.say_hello(compat_hello_request("zstd")).await?;
        assert!(response.compressed());
        assert_eq!(response.encoding(), Some("zstd"));
        drop(ServerGuard::new(handle));
    }
    Ok(())
}

struct CompatErrorGreeter;

impl compat_gen::helloworld::Greeter for CompatErrorGreeter {
    async fn say_hello(
        &self,
        request: compat::Request<compat_gen::helloworld::HelloRequest>,
    ) -> Result<compat::Response<compat_gen::helloworld::HelloReply>, compat::Status> {
        validate_name(&compat_name(request.get_ref()))?;
        Ok(compat::Response::new(compat_hello_reply("ok")))
    }
}

struct ProstErrorGreeter;

impl prost_gen::helloworld::Greeter for ProstErrorGreeter {
    async fn say_hello(
        &self,
        request: Request<prost_gen::helloworld::HelloRequest>,
    ) -> Result<Response<prost_gen::helloworld::HelloReply>, Status> {
        validate_name(&request.get_ref().name)?;
        Ok(Response::new(prost_gen::helloworld::HelloReply {
            message: "ok".to_owned(),
        }))
    }
}

fn validate_name(name: &str) -> Result<(), Status> {
    if !name.is_empty() && name.len() <= 20 {
        return Ok(());
    }
    let description = if name.is_empty() {
        "name cannot be empty"
    } else {
        "name is too long"
    };
    let details = pbrs_grpc::pb::ErrorDetails::new()
        .with_bad_request(pbrs_grpc::pb::BadRequest::with_field("name", description))
        .with_help(pbrs_grpc::pb::Help::with_link(
            "description of link",
            "https://resource.example.local",
        ))
        .with_localized_message(pbrs_grpc::pb::LocalizedMessage::with_locale(
            "en-US",
            "message for the user",
        ));
    Err(Status::from_error_details(
        Code::InvalidArgument,
        "request contains invalid arguments",
        &details,
    )?)
}

fn assert_error_details(status: &Status) -> Result<(), Status> {
    assert_eq!(status.code(), Code::InvalidArgument);
    let details = status.error_details()?;
    assert!(details.bad_request.is_some());
    assert!(details.help.is_some());
    assert!(details.localized_message.is_some());
    Ok(())
}

pub async fn run_error_details() -> ExampleResult {
    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Server::new(compat_gen::helloworld::GreeterServer::new(
            CompatErrorGreeter,
        ))
        .serve_listener(listener)
        .await
        .ok();
    });
    let mut client = compat_gen::helloworld::GreeterClient::connect(addr).await?;
    let err = client
        .say_hello(compat_hello_request(""))
        .await
        .expect_err("compat invalid argument");
    assert_error_details(&err)?;
    drop(ServerGuard::new(handle));

    let (listener, addr) = bind_loopback().await?;
    let handle = tokio::spawn(async move {
        Server::new(prost_gen::helloworld::GreeterServer::new(ProstErrorGreeter))
            .serve_listener(listener)
            .await
            .ok();
    });
    let client = prost_gen::helloworld::GreeterClient::new(connect(addr).await?);
    let err = client
        .say_hello(Request::new(prost_hello_request("")))
        .await
        .expect_err("prost invalid argument");
    assert_error_details(&err)?;
    drop(ServerGuard::new(handle));
    Ok(())
}
