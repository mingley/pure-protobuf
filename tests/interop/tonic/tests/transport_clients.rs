//! Unmodified tonic-prost generated TC-22 clients on the pbrs HTTP transport.

use futures_util::StreamExt;
use pbrs_grpc::Channel;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{Request, Response, Status};

mod routeguide {
    tonic::include_proto!("routeguide");
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type NoteStream =
    Pin<Box<dyn futures_util::Stream<Item = Result<routeguide::RouteNote, Status>> + Send>>;

#[derive(Default)]
struct RouteGuide {
    authorized: Arc<AtomicUsize>,
}

impl RouteGuide {
    fn authorize<T>(&self, request: &Request<T>) -> Result<(), Status> {
        if request
            .metadata()
            .get("authorization")
            .is_none_or(|value| value != "Bearer fixture")
        {
            return Err(Status::unauthenticated("missing fixture token"));
        }
        self.authorized.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tonic::async_trait]
impl routeguide::route_guide_server::RouteGuide for RouteGuide {
    async fn get_feature(
        &self,
        request: Request<routeguide::Point>,
    ) -> Result<Response<routeguide::Feature>, Status> {
        self.authorize(&request)?;
        if request.get_ref().latitude == -1 {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let mut response = Response::new(routeguide::Feature {
            name: "large feature ".repeat(128),
            location: Some(request.into_inner()),
        });
        response
            .metadata_mut()
            .insert("x-initial", "preserved".parse().expect("initial metadata"));
        Ok(response)
    }

    type ListFeaturesStream =
        Pin<Box<dyn futures_util::Stream<Item = Result<routeguide::Feature, Status>> + Send>>;

    async fn list_features(
        &self,
        request: Request<routeguide::Rectangle>,
    ) -> Result<Response<Self::ListFeaturesStream>, Status> {
        self.authorize(&request)?;
        let rectangle = request.into_inner();
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert(
            "x-terminal",
            "preserved".parse().expect("terminal metadata"),
        );
        let terminal = Status::with_details_and_metadata(
            tonic::Code::PermissionDenied,
            "fixture terminal status",
            bytes::Bytes::from_static(b"public details"),
            metadata,
        );
        Ok(Response::new(Box::pin(tokio_stream::iter([
            Ok(routeguide::Feature {
                name: "first".to_owned(),
                location: rectangle.lo,
            }),
            Ok(routeguide::Feature {
                name: "second".to_owned(),
                location: rectangle.hi,
            }),
            Err(terminal),
        ]))))
    }

    async fn record_route(
        &self,
        request: Request<tonic::Streaming<routeguide::Point>>,
    ) -> Result<Response<routeguide::RouteSummary>, Status> {
        self.authorize(&request)?;
        let mut input = request.into_inner();
        let mut points = 0;
        while input.message().await?.is_some() {
            points += 1;
        }
        Ok(Response::new(routeguide::RouteSummary {
            point_count: points,
            ..Default::default()
        }))
    }

    type RouteChatStream = NoteStream;

    async fn route_chat(
        &self,
        request: Request<tonic::Streaming<routeguide::RouteNote>>,
    ) -> Result<Response<NoteStream>, Status> {
        self.authorize(&request)?;
        Ok(Response::new(Box::pin(request.into_inner())))
    }
}

struct ServerGuard(tokio::task::JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn server() -> Result<(Channel, Arc<AtomicUsize>, ServerGuard), BoxError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let authorized = Arc::new(AtomicUsize::new(0));
    let service = routeguide::route_guide_server::RouteGuideServer::new(RouteGuide {
        authorized: authorized.clone(),
    })
    .accept_compressed(tonic::codec::CompressionEncoding::Gzip)
    .send_compressed(tonic::codec::CompressionEncoding::Gzip);
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("server");
    });
    Ok((
        Channel::connect(addr).await?,
        authorized,
        ServerGuard(server),
    ))
}

fn token(mut request: Request<()>) -> Result<Request<()>, Status> {
    request
        .metadata_mut()
        .insert("authorization", "Bearer fixture".parse().expect("token"));
    Ok(request)
}

fn point(latitude: i32) -> routeguide::Point {
    routeguide::Point {
        latitude,
        longitude: -100,
    }
}

fn note(message: &str) -> routeguide::RouteNote {
    routeguide::RouteNote {
        location: Some(point(10)),
        message: message.to_owned(),
    }
}

#[tokio::test]
async fn generated_client_all_shapes_interceptor_gzip_metadata_and_terminal_status()
-> Result<(), BoxError> {
    let (channel, authorized, _server) = server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel, token)
            .send_compressed(tonic::codec::CompressionEncoding::Gzip)
            .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
    let response = client.get_feature(point(10)).await?;
    assert_eq!(
        response
            .metadata()
            .get("grpc-encoding")
            .expect("gzip encoding"),
        "gzip"
    );
    assert_eq!(
        response.metadata().get("x-initial").expect("metadata"),
        "preserved"
    );
    assert_eq!(response.get_ref().location, Some(point(10)));
    assert_eq!(response.get_ref().name, "large feature ".repeat(128));

    let mut features = client
        .list_features(routeguide::Rectangle {
            lo: Some(point(1)),
            hi: Some(point(2)),
        })
        .await?
        .into_inner();
    assert_eq!(
        features.message().await?.expect("first").location,
        Some(point(1))
    );
    assert_eq!(
        features.message().await?.expect("second").location,
        Some(point(2))
    );
    let error = features.message().await.expect_err("terminal status");
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert_eq!(error.message(), "fixture terminal status");
    assert_eq!(error.details(), b"public details");
    assert_eq!(
        error
            .metadata()
            .get("x-terminal")
            .expect("terminal metadata"),
        "preserved"
    );

    let summary = client
        .record_route(tokio_stream::iter([point(1), point(2), point(3)]))
        .await?
        .into_inner();
    assert_eq!(summary.point_count, 3);
    let notes: Vec<_> = client
        .route_chat(tokio_stream::iter([note("one"), note("two")]))
        .await?
        .into_inner()
        .collect()
        .await;
    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes.into_iter().collect::<Result<Vec<_>, _>>()?,
        [note("one"), note("two")]
    );
    assert_eq!(authorized.load(Ordering::SeqCst), 4);
    Ok(())
}

#[tokio::test]
async fn generated_client_uses_native_tc22_server_without_stub_edits() -> Result<(), BoxError> {
    let (addr, _server) = pbrs_grpc_example_tonic_ports::spawn_pbrs_routeguide_server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
    let berkshire = routeguide::Point {
        latitude: 409_146_138,
        longitude: -746_188_906,
    };
    assert_eq!(
        client.get_feature(berkshire).await?.into_inner().name,
        "Berkshire Valley"
    );
    let rectangle = routeguide::Rectangle {
        lo: Some(routeguide::Point {
            latitude: 400_000_000,
            longitude: -750_000_000,
        }),
        hi: Some(routeguide::Point {
            latitude: 420_000_000,
            longitude: -730_000_000,
        }),
    };
    let mut features = client.list_features(rectangle).await?.into_inner();
    assert!(features.message().await?.is_some());
    while features.message().await?.is_some() {}
    assert_eq!(
        client
            .record_route(tokio_stream::iter([point(1), point(2)]))
            .await?
            .into_inner()
            .point_count,
        2
    );
    let mut chat = client
        .route_chat(tokio_stream::iter([note("native peer")]))
        .await?
        .into_inner();
    assert_eq!(chat.message().await?.expect("note").message, "native peer");
    assert!(chat.message().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn generated_client_uses_resolver_managed_channel() -> Result<(), BoxError> {
    let (addr, _server) = pbrs_grpc_example_tonic_ports::spawn_pbrs_routeguide_server().await?;
    let channel = Channel::connect_uri(
        &format!("passthrough:///{addr}"),
        pbrs_grpc::resolver::ResolverConfig::static_only(),
    )
    .await?;
    let mut client = routeguide::route_guide_client::RouteGuideClient::new(channel);
    assert_eq!(
        client
            .record_route(tokio_stream::iter([point(1), point(2)]))
            .await?
            .into_inner()
            .point_count,
        2
    );
    Ok(())
}

#[tokio::test]
async fn generated_client_enforces_encode_and_decode_limits() -> Result<(), BoxError> {
    let (channel, _, _server) = server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel.clone(), token)
            .max_decoding_message_size(8);
    assert_eq!(
        client
            .get_feature(point(10))
            .await
            .expect_err("decode limit")
            .code(),
        tonic::Code::OutOfRange
    );
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel, token)
            .max_encoding_message_size(1);
    assert_eq!(
        client
            .get_feature(point(10))
            .await
            .expect_err("encode limit")
            .code(),
        tonic::Code::OutOfRange
    );
    Ok(())
}

#[tokio::test]
async fn generated_client_preserves_grpc_timeout_deadline() -> Result<(), BoxError> {
    let (channel, _, _server) = server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel, token);
    let mut request = Request::new(point(-1));
    request.set_timeout(Duration::from_millis(30));
    assert_eq!(
        client
            .get_feature(request)
            .await
            .expect_err("deadline")
            .code(),
        tonic::Code::DeadlineExceeded
    );
    Ok(())
}

#[tokio::test]
async fn bidi_transmits_after_response_headers_and_closes_when_producer_is_idle()
-> Result<(), BoxError> {
    let (channel, _, _server) = server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel, token);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(note("first")).await?;
    let mut response = client
        .route_chat(ReceiverStream::new(rx))
        .await?
        .into_inner();
    assert_eq!(response.message().await?.expect("first"), note("first"));
    tx.send(note("after headers")).await?;
    assert_eq!(
        response.message().await?.expect("second"),
        note("after headers")
    );
    drop(response);
    tokio::time::timeout(Duration::from_secs(1), tx.closed()).await?;
    Ok(())
}

#[tokio::test]
async fn generated_interceptor_rejection_never_reaches_peer() -> Result<(), BoxError> {
    let (channel, authorized, _server) = server().await?;
    let mut client = routeguide::route_guide_client::RouteGuideClient::with_interceptor(
        channel,
        |_: Request<()>| Err(Status::unauthenticated("local reject")),
    );
    let error = client.get_feature(point(1)).await.expect_err("rejected");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    assert_eq!(error.message(), "local reject");
    assert_eq!(authorized.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn live_bidi_encoder_error_preserves_local_status_after_headers() -> Result<(), BoxError> {
    let (channel, _, _server) = server().await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::with_interceptor(channel, token)
            .max_encoding_message_size(64);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(note("small")).await?;
    let mut response = client
        .route_chat(ReceiverStream::new(rx))
        .await?
        .into_inner();
    assert_eq!(
        response.message().await?.expect("first reply"),
        note("small")
    );
    tx.send(note(&"x".repeat(256))).await?;
    let error = response.message().await.expect_err("local encoder limit");
    assert_eq!(error.code(), tonic::Code::OutOfRange);
    tokio::time::timeout(Duration::from_secs(1), tx.closed()).await?;
    Ok(())
}
