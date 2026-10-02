//! Unchanged tonic-prost generated servers on the native transport (TC-30a).

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use pbrs_grpc::tonic_server::TonicServerExt;
use pbrs_grpc::{Channel, ClientTls, Identity, Router, ServerConfig, ServerTls};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{Notify, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::body::Body;
use tonic::{Request, Response, Status};
use tower_service::Service;

mod routeguide {
    tonic::include_proto!("routeguide");
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type NoteStream = Pin<Box<dyn Stream<Item = Result<routeguide::RouteNote, Status>> + Send>>;
type FeatureStream = Pin<Box<dyn Stream<Item = Result<routeguide::Feature, Status>> + Send>>;

#[derive(Clone)]
struct NativeAuthenticated;
#[derive(Clone)]
struct LayerChecked;
#[derive(Clone)]
struct GeneratedAuthenticated;

#[derive(Default)]
struct State {
    calls: AtomicUsize,
    native: AtomicUsize,
    tls: AtomicUsize,
    #[cfg(unix)]
    uds: AtomicUsize,
    entered: Notify,
    dropped: Notify,
}

fn native_auth(rpc: &mut pbrs_grpc::Rpc) -> Result<(), pbrs_grpc::Status> {
    if rpc.metadata().get("authorization") != Some("Bearer fixture") {
        return Err(pbrs_grpc::Status::unauthenticated("native auth rejected"));
    }
    rpc.metadata_mut().remove("authorization");
    rpc.metadata_mut().set("x-authenticated", "kernel")?;
    rpc.extensions_mut().insert(NativeAuthenticated);
    Ok(())
}

fn generated_auth(mut request: Request<()>) -> Result<Request<()>, Status> {
    if request.metadata().get("authorization").is_some()
        || request
            .metadata()
            .get("x-authenticated")
            .is_none_or(|v| v != "kernel")
        || request.extensions().get::<NativeAuthenticated>().is_none()
        || request.extensions().get::<LayerChecked>().is_none()
    {
        return Err(Status::unauthenticated(
            "generated auth missing native context",
        ));
    }
    request.extensions_mut().insert(GeneratedAuthenticated);
    Ok(request)
}

fn connect_layer(mut request: Request<()>) -> Result<Request<()>, Status> {
    #[cfg(unix)]
    if let Some(native) = request.extensions().get::<pbrs_grpc::UdsConnectInfo>() {
        let tonic = request
            .extensions()
            .get::<tonic::transport::server::UdsConnectInfo>()
            .ok_or_else(|| Status::internal("tonic Unix connect info missing"))?;
        let native_cred = native
            .peer_cred()
            .ok_or_else(|| Status::unauthenticated("native Unix credentials missing"))?;
        let tonic_cred = tonic
            .peer_cred
            .ok_or_else(|| Status::unauthenticated("tonic Unix credentials missing"))?;
        if native_cred.uid() != tonic_cred.uid()
            || native_cred.gid() != tonic_cred.gid()
            || native_cred.pid() != tonic_cred.pid().and_then(|pid| u32::try_from(pid).ok())
            || tonic.peer_addr.is_none()
        {
            return Err(Status::unauthenticated(
                "Unix credentials/context inconsistent",
            ));
        }
        if let Some(native_addr) = native.peer_addr()
            && tonic
                .peer_addr
                .as_ref()
                .is_none_or(|addr| addr.as_pathname() != native_addr.as_pathname())
        {
            return Err(Status::internal("Unix peer address inconsistent"));
        }
        request.extensions_mut().insert(LayerChecked);
        return Ok(request);
    }
    let native = request
        .extensions()
        .get::<pbrs_grpc::TlsConnectInfo>()
        .map(pbrs_grpc::TlsConnectInfo::get_ref)
        .or_else(|| request.extensions().get::<pbrs_grpc::TcpConnectInfo>());
    let tonic = request
        .extensions()
        .get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>()
        .map(tonic::transport::server::TlsConnectInfo::get_ref)
        .or_else(|| {
            request
                .extensions()
                .get::<tonic::transport::server::TcpConnectInfo>()
        });
    if !native.zip(tonic).is_some_and(|(native, tonic)| {
        native.remote_addr().is_some()
            && native.local_addr().is_some()
            && native.remote_addr() == tonic.remote_addr()
            && native.local_addr() == tonic.local_addr()
    }) {
        return Err(Status::internal(
            "kernel native and tonic connect info missing or inconsistent",
        ));
    }
    request.extensions_mut().insert(LayerChecked);
    Ok(request)
}

struct RouteGuide(Arc<State>);
impl RouteGuide {
    fn context<T>(&self, request: &Request<T>) -> Result<(), Status> {
        if request
            .extensions()
            .get::<GeneratedAuthenticated>()
            .is_none()
        {
            return Err(Status::unauthenticated("generated interceptor missing"));
        }
        if let Some(info) = request.extensions().get::<pbrs_grpc::TlsConnectInfo>() {
            let tls =
                request
                    .extensions()
                    .get::<tonic::transport::server::TlsConnectInfo<
                        tonic::transport::server::TcpConnectInfo,
                    >>()
                    .ok_or_else(|| Status::internal("tonic TLS context missing"))?;
            let certs = tls
                .peer_certs()
                .ok_or_else(|| Status::unauthenticated("tonic mTLS identity missing"))?;
            let native_leaf = info.peer_identity().and_then(pbrs_grpc::PeerIdentity::leaf);
            if native_leaf.is_none() || certs.first().map(|cert| cert.as_ref()) != native_leaf {
                return Err(Status::unauthenticated("mTLS identity missing"));
            }
            self.0.tls.fetch_add(1, Ordering::SeqCst);
        }
        #[cfg(unix)]
        if request
            .extensions()
            .get::<tonic::transport::server::UdsConnectInfo>()
            .is_some()
        {
            self.0.uds.fetch_add(1, Ordering::SeqCst);
        }
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct DropSignal(Arc<State>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.dropped.notify_one();
    }
}
struct DroppingNotes {
    inner: tonic::Streaming<routeguide::RouteNote>,
    signal: DropSignal,
}
impl Stream for DroppingNotes {
    type Item = Result<routeguide::RouteNote, Status>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _keep_signal = &self.signal;
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

#[tonic::async_trait]
impl routeguide::route_guide_server::RouteGuide for RouteGuide {
    async fn get_feature(
        &self,
        request: Request<routeguide::Point>,
    ) -> Result<Response<routeguide::Feature>, Status> {
        self.context(&request)?;
        if request.get_ref().latitude == -1 {
            let _drop = DropSignal(self.0.clone());
            self.0.entered.notify_one();
            std::future::pending::<()>().await;
        }
        let mut response = Response::new(routeguide::Feature {
            name: "large feature ".repeat(128),
            location: Some(request.into_inner()),
        });
        response
            .metadata_mut()
            .insert("x-initial", "preserved".parse().expect("metadata"));
        Ok(response)
    }
    type ListFeaturesStream = FeatureStream;
    async fn list_features(
        &self,
        request: Request<routeguide::Rectangle>,
    ) -> Result<Response<FeatureStream>, Status> {
        self.context(&request)?;
        let rectangle = request.into_inner();
        if rectangle.lo.as_ref().is_some_and(|p| p.latitude == -1) {
            let state = self.0.clone();
            return Ok(Response::new(Box::pin(
                futures_util::stream::once(async move {
                    Ok(routeguide::Feature {
                        name: "first".to_owned(),
                        location: rectangle.lo,
                    })
                })
                .chain(futures_util::stream::once(async move {
                    let _drop = DropSignal(state);
                    std::future::pending().await
                })),
            )));
        }
        let mut metadata = tonic::metadata::MetadataMap::new();
        metadata.insert("x-terminal", "preserved".parse().expect("metadata"));
        let terminal = Status::with_details_and_metadata(
            tonic::Code::PermissionDenied,
            "fixture terminal status",
            Bytes::from_static(b"public details"),
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
        self.context(&request)?;
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
        self.context(&request)?;
        Ok(Response::new(Box::pin(DroppingNotes {
            inner: request.into_inner(),
            signal: DropSignal(self.0.clone()),
        })))
    }
}

struct NativeService(Arc<State>);
impl pbrs_grpc::Service for NativeService {
    const NAME: &'static str = "test.Native";
    async fn call(&self, rpc: pbrs_grpc::Rpc) {
        self.0.native.fetch_add(1, Ordering::SeqCst);
        rpc.reject(pbrs_grpc::Status::unimplemented(
            "mixed native service reached",
        ));
    }
}

struct Running {
    task: Option<tokio::task::JoinHandle<Result<(), pbrs_grpc::Status>>>,
    shutdown: Option<oneshot::Sender<()>>,
    #[cfg(unix)]
    path: Option<std::path::PathBuf>,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
        #[cfg(unix)]
        if let Some(path) = &self.path {
            std::fs::remove_file(path).ok();
        }
    }
}

const CA: &str = include_str!("../../../../pbrs-grpc/tests/tls_data/ca.crt");
const SERVER_CERT: &str = include_str!("../../../../pbrs-grpc/tests/tls_data/server.crt");
const SERVER_KEY: &str = include_str!("../../../../pbrs-grpc/tests/tls_data/server.key");
const CLIENT_CERT: &str = include_str!("../../../../pbrs-grpc/tests/tls_data/client.crt");
const CLIENT_KEY: &str = include_str!("../../../../pbrs-grpc/tests/tls_data/client.key");

fn router(config: ServerConfig, limits: Option<(usize, usize)>, state: &Arc<State>) -> Router {
    let mut service =
        routeguide::route_guide_server::RouteGuideServer::new(RouteGuide(state.clone()))
            .accept_compressed(tonic::codec::CompressionEncoding::Gzip)
            .send_compressed(tonic::codec::CompressionEncoding::Gzip);
    if let Some((decode, encode)) = limits {
        service = service
            .max_decoding_message_size(decode)
            .max_encoding_message_size(encode);
    }
    let service = tonic::service::interceptor::InterceptedService::new(service, generated_auth);
    Router::new()
        .add_service(
            service
                .into_pbrs_service()
                .layer(tonic::service::InterceptorLayer::new(connect_layer)),
        )
        .add_service(NativeService(state.clone()))
        .config(config)
        .intercept(native_auth)
}

async fn server(
    config: ServerConfig,
    limits: Option<(usize, usize)>,
    tls: bool,
) -> Result<(std::net::SocketAddr, Arc<State>, Running), BoxError> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let state = Arc::new(State::default());
    let router = router(config, limits, &state);
    let (shutdown, done) = oneshot::channel();
    let task = tokio::spawn(async move {
        let shutdown = async {
            done.await.ok();
        };
        if tls {
            router
                .serve_tls_with_shutdown(
                    listener,
                    shutdown,
                    ServerTls::mtls(Identity::from_pem(SERVER_CERT, SERVER_KEY)?, CA)?,
                )
                .await
        } else {
            router.serve_with_shutdown(listener, shutdown).await
        }
    });
    Ok((
        addr,
        state,
        Running {
            task: Some(task),
            shutdown: Some(shutdown),
            #[cfg(unix)]
            path: None,
        },
    ))
}

#[cfg(unix)]
struct CustomUnix(tokio::net::UnixListener);

#[cfg(unix)]
impl pbrs_grpc::Incoming for CustomUnix {
    type Io = tokio::net::UnixStream;
    async fn accept(&mut self) -> pbrs_grpc::IncomingAccept<Self::Io> {
        Some(
            self.0
                .accept()
                .await
                .map(|(io, _)| (io, None))
                .map_err(|error| pbrs_grpc::Status::unavailable(error.to_string())),
        )
    }
    fn peer(
        &self,
        io: &Self::Io,
        _remote: Option<std::net::SocketAddr>,
    ) -> pbrs_grpc::ConnectionInfo {
        let actual = tonic::transport::server::Connected::connect_info(io);
        let cred = actual.peer_cred.expect("real Unix credentials");
        let native = pbrs_grpc::PeerCred::new(
            cred.uid(),
            cred.gid(),
            cred.pid().and_then(|pid| u32::try_from(pid).ok()),
        );
        let info = pbrs_grpc::ConnectionInfo::new()
            .with_peer_cred(native)
            .with_scheme("http")
            .with_tonic_uds(actual);
        assert_eq!(
            info.peer_cred(),
            Some(native),
            "tonic builder preserves native credential fields"
        );
        assert_eq!(info.scheme(), Some("http"));
        info
    }
}

#[cfg(unix)]
async fn unix_server(custom: bool) -> Result<(Channel, Arc<State>, Running), BoxError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "pbrs-tc30a-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let listener = tokio::net::UnixListener::bind(&path)?;
    let state = Arc::new(State::default());
    let router = router(ServerConfig::default(), None, &state);
    let (shutdown, done) = oneshot::channel();
    let task = tokio::spawn(async move {
        let shutdown = async {
            done.await.ok();
        };
        if custom {
            router
                .serve_with_incoming_shutdown(CustomUnix(listener), shutdown)
                .await
        } else {
            router.serve_unix_with_shutdown(listener, shutdown).await
        }
    });
    let running = Running {
        task: Some(task),
        shutdown: Some(shutdown),
        path: Some(path.clone()),
    };
    let channel = Channel::connect_unix(&path).await?;
    Ok((channel, state, running))
}

fn authorized<T>(value: T) -> Request<T> {
    let mut request = Request::new(value);
    request
        .metadata_mut()
        .insert("authorization", "Bearer fixture".parse().expect("token"));
    request
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

struct OneFrame(Option<Bytes>);
impl http_body::Body for OneFrame {
    type Data = Bytes;
    type Error = std::convert::Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        Poll::Ready(self.0.take().map(|bytes| Ok(http_body::Frame::data(bytes))))
    }
    fn is_end_stream(&self) -> bool {
        self.0.is_none()
    }
}

#[tokio::test]
async fn tonic_generated_decoder_uses_first_gzip_member_and_ignores_encoded_tail()
-> Result<(), BoxError> {
    use prost::Message;
    let first = pbrs_grpc::gzip::encode(&point(10).encode_to_vec())?;
    for tail in [
        pbrs_grpc::gzip::encode(&point(20).encode_to_vec())?,
        vec![255, 0, 123],
    ] {
        let encoded = [first.as_slice(), tail.as_slice()].concat();
        let mut wire = vec![1];
        wire.extend_from_slice(&u32::try_from(encoded.len())?.to_be_bytes());
        wire.extend_from_slice(&encoded);
        let state = Arc::new(State::default());
        let mut service =
            routeguide::route_guide_server::RouteGuideServer::new(RouteGuide(state.clone()))
                .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
        let mut request = http::Request::builder()
            .method("POST")
            .uri("/routeguide.RouteGuide/GetFeature")
            .header("content-type", "application/grpc")
            .header("grpc-encoding", "gzip")
            .body(Body::new(OneFrame(Some(Bytes::from(wire)))))?;
        // Direct generated decoder characterization, independent of transport auth.
        request.extensions_mut().insert(GeneratedAuthenticated);
        let response = service.call(request).await?;
        let headers = response.headers().clone();
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        let mut terminal = headers;
        while let Some(frame) =
            futures_util::future::poll_fn(|cx| http_body::Body::poll_frame(Pin::new(&mut body), cx))
                .await
        {
            let frame = frame?;
            match frame.into_data() {
                Ok(data) => bytes.extend_from_slice(&data),
                Err(frame) => terminal = frame.into_trailers().expect("trailers"),
            }
        }
        assert_eq!(terminal.get("grpc-status").expect("status"), "0");
        let response = routeguide::Feature::decode(bytes.get(5..).expect("framed response"))?;
        assert_eq!(response.location, Some(point(10)));
        assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

async fn all_shapes<T>(transport: T) -> Result<(), BoxError>
where
    T: Service<http::Request<Body>, Response = http::Response<Body>>,
    T::Error: Into<tonic::codegen::StdError>,
{
    all_shapes_codec(transport, true).await
}

async fn all_shapes_codec<T>(transport: T, gzip: bool) -> Result<(), BoxError>
where
    T: Service<http::Request<Body>, Response = http::Response<Body>>,
    T::Error: Into<tonic::codegen::StdError>,
{
    let mut client = routeguide::route_guide_client::RouteGuideClient::new(transport);
    if gzip {
        client = client
            .send_compressed(tonic::codec::CompressionEncoding::Gzip)
            .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
    }
    let response = client.get_feature(authorized(point(10))).await?;
    if gzip {
        assert_eq!(
            response.metadata().get("grpc-encoding").expect("gzip"),
            "gzip"
        );
    } else {
        assert!(response.metadata().get("grpc-encoding").is_none());
    }
    assert_eq!(
        response.metadata().get("x-initial").expect("metadata"),
        "preserved"
    );
    assert_eq!(response.get_ref().name, "large feature ".repeat(128));
    assert_eq!(response.get_ref().location, Some(point(10)));
    let mut features = client
        .list_features(authorized(routeguide::Rectangle {
            lo: Some(point(1)),
            hi: Some(point(2)),
        }))
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
    let error = features.message().await.expect_err("terminal");
    assert_eq!(error.code(), tonic::Code::PermissionDenied);
    assert_eq!(error.message(), "fixture terminal status");
    assert_eq!(error.details(), b"public details");
    assert_eq!(
        error.metadata().get("x-terminal").expect("metadata"),
        "preserved"
    );
    assert_eq!(
        client
            .record_route(authorized(tokio_stream::iter([
                point(1),
                point(2),
                point(3)
            ])))
            .await?
            .get_ref()
            .point_count,
        3
    );
    let mut chat = client
        .route_chat(authorized(tokio_stream::iter([note("one"), note("two")])))
        .await?
        .into_inner();
    assert_eq!(chat.message().await?.expect("first"), note("one"));
    assert_eq!(chat.message().await?.expect("second"), note("two"));
    assert!(chat.message().await?.is_none());
    Ok(())
}

#[tokio::test]
async fn native_finite_gzip_input_caps_all_shapes_plaintext_and_mtls() -> Result<(), BoxError> {
    let config = ServerConfig::default().max_decoding_message_size(8192);
    let (addr, state, _server) = server(config, Some((16_384, 16_384)), false).await?;
    all_shapes(
        tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
            .connect()
            .await?,
    )
    .await?;
    all_shapes(Channel::connect(addr).await?).await?;
    assert_eq!(state.calls.load(Ordering::SeqCst), 8);

    let (addr, state, _server) = server(config, Some((16_384, 16_384)), true).await?;
    let tls = ClientTls::ca_mtls(
        "localhost",
        CA,
        Identity::from_pem(CLIENT_CERT, CLIENT_KEY)?,
    )?;
    all_shapes(Channel::connect_tls(addr, tls).await?).await?;
    assert_eq!(state.calls.load(Ordering::SeqCst), 4);
    assert_eq!(state.tls.load(Ordering::SeqCst), 4);
    Ok(())
}

#[tokio::test]
async fn native_finite_gzip_input_caps_reject_encoded_messages_for_all_shapes()
-> Result<(), BoxError> {
    let (addr, state, _server) = server(
        ServerConfig::default().max_decoding_message_size(1),
        Some((16_384, 16_384)),
        false,
    )
    .await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?)
            .send_compressed(tonic::codec::CompressionEncoding::Gzip);
    let error = client
        .get_feature(authorized(point(10)))
        .await
        .expect_err("native encoded cap");
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    let error = match client
        .list_features(authorized(routeguide::Rectangle {
            lo: Some(point(1)),
            hi: Some(point(2)),
        }))
        .await
    {
        Err(error) => error,
        Ok(response) => response
            .into_inner()
            .message()
            .await
            .expect_err("native encoded cap"),
    };
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    let error = client
        .record_route(authorized(tokio_stream::iter([point(1), point(2)])))
        .await
        .expect_err("native encoded cap");
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    let error = match client
        .route_chat(authorized(tokio_stream::iter([note("one")])))
        .await
    {
        Err(error) => error,
        Ok(response) => response
            .into_inner()
            .message()
            .await
            .expect_err("native encoded cap"),
    };
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    // Streaming handlers receive their stream before reading its messages.
    assert_eq!(state.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn native_finite_gzip_input_cap_rejects_inflated_message_and_preserves_auth()
-> Result<(), BoxError> {
    use prost::Message;
    let bomb = note(&"a".repeat(2048));
    let encoded = pbrs_grpc::gzip::encode(&bomb.encode_to_vec())?;
    assert!(encoded.len() < 512);
    assert!(bomb.encoded_len() > 512);
    let (addr, state, _server) = server(
        ServerConfig::default().max_decoding_message_size(512),
        Some((16_384, 16_384)),
        false,
    )
    .await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?)
            .send_compressed(tonic::codec::CompressionEncoding::Gzip);
    let error = client
        .get_feature(point(10))
        .await
        .expect_err("native auth");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    let error = match client
        .route_chat(authorized(tokio_stream::iter([bomb])))
        .await
    {
        Err(error) => error,
        Ok(response) => response
            .into_inner()
            .message()
            .await
            .expect_err("native inflated cap"),
    };
    assert_eq!(error.code(), tonic::Code::ResourceExhausted);
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn native_finite_gzip_input_cap_does_not_enlarge_generated_codec_cap() -> Result<(), BoxError>
{
    let (addr, state, _server) = server(
        ServerConfig::default().max_decoding_message_size(8192),
        Some((1, 16_384)),
        false,
    )
    .await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?)
            .send_compressed(tonic::codec::CompressionEncoding::Gzip);
    let error = client
        .get_feature(authorized(point(10)))
        .await
        .expect_err("tonic cap remains");
    assert_eq!(error.code(), tonic::Code::OutOfRange);
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn native_identity_caps_qualify_all_shapes_plaintext_and_mtls() -> Result<(), BoxError> {
    let config = ServerConfig::default()
        .max_decoding_message_size(8192)
        .max_encoding_message_size(8192);
    let (addr, state, _server) = server(config, Some((16_384, 16_384)), false).await?;
    all_shapes_codec(
        tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
            .connect()
            .await?,
        false,
    )
    .await?;
    all_shapes_codec(Channel::connect(addr).await?, false).await?;
    assert_eq!(state.calls.load(Ordering::SeqCst), 8);

    let (addr, state, _server) = server(config, Some((16_384, 16_384)), true).await?;
    let tls = ClientTls::ca_mtls(
        "localhost",
        CA,
        Identity::from_pem(CLIENT_CERT, CLIENT_KEY)?,
    )?;
    all_shapes_codec(Channel::connect_tls(addr, tls).await?, false).await?;
    assert_eq!(state.calls.load(Ordering::SeqCst), 4);
    assert_eq!(state.tls.load(Ordering::SeqCst), 4);
    Ok(())
}

#[tokio::test]
async fn native_identity_caps_reject_inbound_and_outbound_for_all_shapes() -> Result<(), BoxError> {
    for inbound in [true, false] {
        let config = if inbound {
            ServerConfig::default().max_decoding_message_size(1)
        } else {
            ServerConfig::default().max_encoding_message_size(1)
        };
        let (addr, state, _server) = server(config, None, false).await?;
        let mut client =
            routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
        let error = client
            .get_feature(authorized(point(10)))
            .await
            .expect_err("native cap");
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);

        let error = match client
            .list_features(authorized(routeguide::Rectangle {
                lo: Some(point(1)),
                hi: Some(point(2)),
            }))
            .await
        {
            Err(error) => error,
            Ok(response) => response
                .into_inner()
                .message()
                .await
                .expect_err("native output cap"),
        };
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);

        let error = client
            .record_route(authorized(tokio_stream::iter([
                point(1),
                point(2),
                point(3),
            ])))
            .await
            .expect_err("native cap");
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);

        let error = match client
            .route_chat(authorized(tokio_stream::iter([note("one")])))
            .await
        {
            Err(error) => error,
            Ok(response) => response
                .into_inner()
                .message()
                .await
                .expect_err("native streamed cap"),
        };
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);
        // Streaming handlers receive their stream before reading its messages.
        assert_eq!(
            state.calls.load(Ordering::SeqCst),
            if inbound { 2 } else { 4 }
        );
    }
    Ok(())
}

#[tokio::test]
async fn native_identity_caps_do_not_enlarge_generated_codec_caps() -> Result<(), BoxError> {
    for (limits, calls) in [((1, usize::MAX), 0), ((16_384, 8), 1)] {
        let config = ServerConfig::default()
            .max_decoding_message_size(8192)
            .max_encoding_message_size(8192);
        let (addr, state, _server) = server(config, Some(limits), false).await?;
        let mut client =
            routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
        let error = client
            .get_feature(authorized(point(10)))
            .await
            .expect_err("tonic cap remains");
        assert_eq!(error.code(), tonic::Code::OutOfRange);
        assert_eq!(state.calls.load(Ordering::SeqCst), calls);
    }
    Ok(())
}

#[tokio::test]
async fn unchanged_generated_server_all_shapes_layers_interceptors_gzip_both_transports()
-> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, false).await?;
    all_shapes(
        tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
            .connect()
            .await?,
    )
    .await?;
    all_shapes(Channel::connect(addr).await?).await?;
    assert_eq!(state.calls.load(Ordering::SeqCst), 8);
    let mut request = pbrs_grpc::Request::new(pbrs_grpc::HelloRequest::new());
    request
        .metadata_mut()
        .insert("authorization", "Bearer fixture")?;
    let result = Channel::connect(addr)
        .await?
        .unary::<_, pbrs_grpc::HelloReply>("/test.Native/Check", request)
        .await
        .expect_err("native route");
    assert_eq!(result.message(), "mixed native service reached");
    assert_eq!(state.native.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn native_auth_rejection_prevents_generated_dispatch() -> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, false).await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
    let error = client.get_feature(point(1)).await.expect_err("auth");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    assert_eq!(error.message(), "native auth rejected");
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn generated_server_owns_decoding_and_encoding_limits() -> Result<(), BoxError> {
    for (limits, expected_calls) in [((1, usize::MAX), 0), ((4 * 1024 * 1024, 8), 1)] {
        let (addr, state, _server) = server(ServerConfig::default(), Some(limits), false).await?;
        let mut client =
            routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
        assert_eq!(
            client
                .get_feature(authorized(point(10)))
                .await
                .expect_err("limit")
                .code(),
            tonic::Code::OutOfRange
        );
        assert_eq!(state.calls.load(Ordering::SeqCst), expected_calls);
    }
    Ok(())
}

#[tokio::test]
async fn native_server_deadline_drops_generated_unary_handler() -> Result<(), BoxError> {
    let (addr, state, _server) = server(
        ServerConfig::default().timeout(Duration::from_millis(20)),
        None,
        false,
    )
    .await?;
    let transport = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
        .connect()
        .await?;
    let mut client = routeguide::route_guide_client::RouteGuideClient::new(transport);
    assert_eq!(
        client
            .get_feature(authorized(point(-1)))
            .await
            .expect_err("deadline")
            .code(),
        tonic::Code::DeadlineExceeded
    );
    tokio::time::timeout(Duration::from_secs(2), state.dropped.notified()).await?;
    Ok(())
}

#[tokio::test]
async fn peer_deadline_drops_generated_streaming_producer_after_headers() -> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, false).await?;
    let transport = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))?
        .connect()
        .await?;
    let mut client = routeguide::route_guide_client::RouteGuideClient::new(transport);
    let mut request = authorized(routeguide::Rectangle {
        lo: Some(point(-1)),
        hi: None,
    });
    request.set_timeout(Duration::from_millis(30));
    let mut response = client.list_features(request).await?.into_inner();
    assert_eq!(response.message().await?.expect("first").name, "first");
    assert_eq!(
        response.message().await.expect_err("deadline").code(),
        tonic::Code::DeadlineExceeded
    );
    tokio::time::timeout(Duration::from_secs(2), state.dropped.notified()).await?;
    Ok(())
}

#[tokio::test]
async fn reset_before_headers_cancels_generated_unary_handler() -> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, false).await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
    let call = tokio::spawn(async move { client.get_feature(authorized(point(-1))).await });
    tokio::time::timeout(Duration::from_secs(2), state.entered.notified()).await?;
    call.abort();
    call.await.ok();
    tokio::time::timeout(Duration::from_secs(2), state.dropped.notified()).await?;
    Ok(())
}

#[tokio::test]
async fn reset_after_headers_cancels_idle_bidi_upload_and_generated_body() -> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, false).await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(note("first")).await?;
    let mut response = client
        .route_chat(authorized(ReceiverStream::new(rx)))
        .await?
        .into_inner();
    assert_eq!(response.message().await?.expect("first"), note("first"));
    tx.send(note("after headers")).await?;
    assert_eq!(
        response.message().await?.expect("second"),
        note("after headers")
    );
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), state.dropped.notified()).await?;
    tokio::time::timeout(Duration::from_secs(2), tx.closed()).await?;
    Ok(())
}

#[tokio::test]
async fn mtls_stamps_native_and_tonic_typed_connect_info_for_generated_layers()
-> Result<(), BoxError> {
    let (addr, state, _server) = server(ServerConfig::default(), None, true).await?;
    let tls = ClientTls::ca_mtls(
        "localhost",
        CA,
        Identity::from_pem(CLIENT_CERT, CLIENT_KEY)?,
    )?;
    all_shapes(Channel::connect_tls(addr, tls).await?).await?;
    assert_eq!(state.tls.load(Ordering::SeqCst), 4);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unix_stamps_actual_tonic_credentials_for_generated_all_shapes() -> Result<(), BoxError> {
    let (channel, state, _server) = unix_server(false).await?;
    all_shapes(channel).await?;
    assert_eq!(state.uds.load(Ordering::SeqCst), 4);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn custom_incoming_public_tonic_unix_builder_preserves_native_fields_and_extensions()
-> Result<(), BoxError> {
    let (channel, state, _server) = unix_server(true).await?;
    all_shapes(channel).await?;
    assert_eq!(state.uds.load(Ordering::SeqCst), 4);
    Ok(())
}

#[tokio::test]
async fn graceful_shutdown_keeps_active_generated_bidi_until_client_cancels() -> Result<(), BoxError>
{
    let (addr, state, mut server) = server(ServerConfig::default(), None, false).await?;
    let mut client =
        routeguide::route_guide_client::RouteGuideClient::new(Channel::connect(addr).await?);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(note("before shutdown")).await?;
    let mut response = client
        .route_chat(authorized(ReceiverStream::new(rx)))
        .await?
        .into_inner();
    assert_eq!(
        response.message().await?.expect("first"),
        note("before shutdown")
    );
    server.shutdown.take().expect("shutdown").send(()).ok();
    let mut task = server.task.take().expect("task");
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut task)
            .await
            .is_err()
    );
    tx.send(note("during drain")).await?;
    assert_eq!(
        response.message().await?.expect("second"),
        note("during drain")
    );
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), state.dropped.notified()).await?;
    drop(tx);
    tokio::time::timeout(Duration::from_secs(2), task).await???;
    Ok(())
}
