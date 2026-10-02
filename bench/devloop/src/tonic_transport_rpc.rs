//! SB-32: unchanged tonic codegen over the four transport combinations.
//!
//! The old typed `rpc.adoption.*` matrix remains a separate baseline. These
//! services import its existing prost types and use its frozen preparations,
//! complete read/value/byte oracles, handler work and four-reply stream shape.

use crate::{AllocGuard, black_box, child_json_with_input};
use pbrs_adoption_corpus::{
    prost_types,
    rpc::{Inputs, Pair, STREAM_REPLIES},
    workloads::{CELLS, Codec, Operation, Specimen},
};
use pbrs_grpc::tonic_server::TonicServerExt;
use prost::Message;
use std::convert::Infallible;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::body::Body;
use tonic::codegen::{StdError, http};

mod generated {
    tonic::include_proto!("adoption.transport");
}

const PREFIX: &str = "rpc.tonic_transport.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Profile {
    ReferenceReference,
    NativeClientReferenceServer,
    ReferenceClientNativeServer,
    NativeNative,
}

const PROFILES: [Profile; 4] = [
    Profile::ReferenceReference,
    Profile::NativeClientReferenceServer,
    Profile::ReferenceClientNativeServer,
    Profile::NativeNative,
];

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Self::ReferenceReference => "reference_reference",
            Self::NativeClientReferenceServer => "native_client_reference_server",
            Self::ReferenceClientNativeServer => "reference_client_native_server",
            Self::NativeNative => "native_native",
        }
    }

    fn native_client(self) -> bool {
        matches!(self, Self::NativeClientReferenceServer | Self::NativeNative)
    }

    fn native_server(self) -> bool {
        matches!(self, Self::ReferenceClientNativeServer | Self::NativeNative)
    }
}

pub fn cells() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for cell in specimen_cells() {
        let specimen = specimen_name(cell.id);
        for profile in PROFILES {
            for shape in ["unary", "server_stream"] {
                let id: &'static str = Box::leak(
                    format!("{PREFIX}{}.{specimen}.{shape}", profile.name()).into_boxed_str(),
                );
                out.push((id, "rpc", profile.name()));
            }
        }
    }
    out
}

fn specimen_cells() -> impl Iterator<Item = &'static pbrs_adoption_corpus::workloads::Cell> {
    CELLS
        .iter()
        .filter(|c| matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll))
}

fn specimen_name(id: &str) -> &str {
    id.strip_prefix("codec.adoption.prost.")
        .unwrap()
        .strip_suffix(".read_all")
        .unwrap()
}

fn parse_cell(id: &str) -> (Profile, Specimen, bool) {
    let (profile, specimen) = id.strip_prefix(PREFIX).unwrap().split_once('.').unwrap();
    let profile = PROFILES
        .into_iter()
        .find(|p| p.name() == profile)
        .expect("registered tonic transport profile");
    let streaming = specimen.ends_with(".server_stream");
    let specimen = specimen
        .strip_suffix(".unary")
        .or_else(|| specimen.strip_suffix(".server_stream"))
        .expect("registered tonic transport shape");
    let specimen = specimen_cells()
        .find(|cell| specimen_name(cell.id) == specimen)
        .expect("registered tonic transport specimen")
        .specimen;
    (profile, specimen, streaming)
}

trait ClientTransport:
    tonic::client::GrpcService<Body, ResponseBody = Body, Error: Into<StdError>> + Send
{
}

impl<T> ClientTransport for T where
    T: tonic::client::GrpcService<Body, ResponseBody = Body, Error: Into<StdError>> + Send
{
}

type FullValueCheck<'a, M> = Option<&'a dyn Fn(&M)>;

trait CorpusService: 'static {
    type Message: Message + Default + Clone + PartialEq + Debug + Send + Sync + 'static;
    type Server: tonic::server::NamedService
        + tonic::codegen::Service<
            http::Request<Body>,
            Response = http::Response<Body>,
            Error = Infallible,
            Future: Send + 'static,
        > + Clone
        + Send
        + Sync
        + 'static;
    type Client<T: ClientTransport>: Send;

    fn server(checksum: u64) -> Self::Server;
    fn client<T: ClientTransport>(transport: T) -> Self::Client<T>;
    async fn call<T: ClientTransport>(
        client: &mut Self::Client<T>,
        streaming: bool,
        message: &Self::Message,
        checksum: u64,
        validate: FullValueCheck<'_, Self::Message>,
    ) -> u64;
}

#[derive(Clone)]
struct Echo<M> {
    checksum: u64,
    marker: PhantomData<fn() -> M>,
}

impl<M> Echo<M> {
    fn new(checksum: u64) -> Self {
        Self {
            checksum,
            marker: PhantomData,
        }
    }
}

macro_rules! corpus_service {
    ($case:ident, $message:ident, $server_module:ident, $service_trait:ident,
     $server:ident, $client_module:ident, $client:ident, $read:ident) => {
        struct $case;

        #[tonic::async_trait]
        impl generated::$server_module::$service_trait for Echo<prost_types::$message> {
            async fn unary(
                &self,
                request: tonic::Request<prost_types::$message>,
            ) -> Result<tonic::Response<prost_types::$message>, tonic::Status> {
                let message = request.into_inner();
                assert_eq!(
                    black_box(pbrs_adoption_corpus::$read(&message)),
                    self.checksum
                );
                Ok(tonic::Response::new(message))
            }

            type ServerStreamStream = ReceiverStream<Result<prost_types::$message, tonic::Status>>;

            async fn server_stream(
                &self,
                request: tonic::Request<prost_types::$message>,
            ) -> Result<tonic::Response<Self::ServerStreamStream>, tonic::Status> {
                let message = request.into_inner();
                assert_eq!(
                    black_box(pbrs_adoption_corpus::$read(&message)),
                    self.checksum
                );
                // Same bounded channel, spawned producer and four full clones
                // as the frozen typed baseline's reference handler.
                let (tx, rx) = tokio::sync::mpsc::channel(8);
                drop(tokio::spawn(async move {
                    for _ in 0..STREAM_REPLIES {
                        if tx.send(Ok(message.clone())).await.is_err() {
                            break;
                        }
                    }
                }));
                Ok(tonic::Response::new(ReceiverStream::new(rx)))
            }
        }

        impl CorpusService for $case {
            type Message = prost_types::$message;
            type Server = generated::$server_module::$server<Echo<Self::Message>>;
            type Client<T: ClientTransport> = generated::$client_module::$client<T>;

            fn server(checksum: u64) -> Self::Server {
                generated::$server_module::$server::new(Echo::new(checksum))
            }

            fn client<T: ClientTransport>(transport: T) -> Self::Client<T> {
                generated::$client_module::$client::new(transport)
            }

            async fn call<T: ClientTransport>(
                client: &mut Self::Client<T>,
                streaming: bool,
                message: &Self::Message,
                checksum: u64,
                validate: FullValueCheck<'_, Self::Message>,
            ) -> u64 {
                let mut count = 0;
                let mut sink = 0u64;
                let mut consume = |response: Self::Message| {
                    assert_eq!(black_box(pbrs_adoption_corpus::$read(&response)), checksum);
                    if let Some(check) = validate {
                        check(&response);
                    }
                    sink = sink.wrapping_add(checksum);
                    count += 1;
                };
                if streaming {
                    let mut stream = client
                        .server_stream(tonic::Request::new(message.clone()))
                        .await
                        .unwrap()
                        .into_inner();
                    while let Some(response) = stream.message().await.unwrap() {
                        consume(response);
                    }
                } else {
                    consume(
                        client
                            .unary(tonic::Request::new(message.clone()))
                            .await
                            .unwrap()
                            .into_inner(),
                    );
                }
                assert_eq!(count, if streaming { STREAM_REPLIES } else { 1 });
                sink
            }
        }
    };
}

corpus_service!(
    QueryCase,
    Query,
    query_echo_server,
    QueryEcho,
    QueryEchoServer,
    query_echo_client,
    QueryEchoClient,
    touch_query_prost
);
corpus_service!(
    EntityListCase,
    EntityList,
    entity_list_echo_server,
    EntityListEcho,
    EntityListEchoServer,
    entity_list_echo_client,
    EntityListEchoClient,
    touch_entity_list_prost
);
corpus_service!(
    SparseCase,
    Sparse,
    sparse_echo_server,
    SparseEcho,
    SparseEchoServer,
    sparse_echo_client,
    SparseEchoClient,
    touch_sparse_prost
);
corpus_service!(
    MapHeavyCase,
    MapHeavy,
    map_heavy_echo_server,
    MapHeavyEcho,
    MapHeavyEchoServer,
    map_heavy_echo_client,
    MapHeavyEchoClient,
    touch_maps_prost
);

#[derive(Clone, Copy, Debug)]
struct SocketFacts {
    nodelay: bool,
    local: SocketAddr,
    remote: SocketAddr,
}

#[derive(Clone, Default)]
struct SocketProbe(Arc<OnceLock<SocketFacts>>);

impl SocketProbe {
    fn accepted(&self, tcp: &TcpStream) -> std::io::Result<()> {
        let facts = SocketFacts {
            nodelay: tcp.nodelay()?,
            local: tcp.local_addr()?,
            remote: tcp.peer_addr()?,
        };
        assert!(
            facts.nodelay,
            "original accepted-socket TCP_NODELAY default"
        );
        self.0.set(facts).ok();
        Ok(())
    }

    fn facts(&self) -> SocketFacts {
        *self.0.get().expect("actual accepted socket observed")
    }
}

struct ReferenceIncoming {
    inner: tonic::transport::server::TcpIncoming,
    probe: SocketProbe,
}

impl Stream for ReferenceIncoming {
    type Item = std::io::Result<TcpStream>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(tcp))) => {
                Poll::Ready(Some(self.probe.accepted(&tcp).map(|()| tcp)))
            }
            other => other,
        }
    }
}

struct NativeIncoming {
    listener: TcpListener,
    probe: SocketProbe,
}

impl pbrs_grpc::Incoming for NativeIncoming {
    type Io = TcpStream;

    async fn accept(&mut self) -> pbrs_grpc::IncomingAccept<Self::Io> {
        Some(
            async {
                let (tcp, remote) = self.listener.accept().await?;
                // Custom Incoming skips native tcp::tune. Reproduce the
                // existing built-in TCP_NODELAY=true policy before observing
                // the actual stream; do not claim built-in accept telemetry.
                tcp.set_nodelay(true)?;
                self.probe.accepted(&tcp)?;
                Ok((tcp, Some(remote)))
            }
            .await
            .map_err(|error: std::io::Error| pbrs_grpc::Status::unavailable(error.to_string())),
        )
    }

    fn peer(&self, tcp: &Self::Io, remote: Option<SocketAddr>) -> pbrs_grpc::ConnectionInfo {
        let mut info = pbrs_grpc::ConnectionInfo::from_accept(remote).with_scheme("http");
        if let Ok(local) = tcp.local_addr() {
            info = info.with_local_addr(local);
        }
        info
    }
}

struct ServerGuard(Option<tokio::task::JoinHandle<()>>);

impl ServerGuard {
    async fn finish(mut self) {
        if let Some(server) = self.0.take() {
            server.abort();
            server.await.ok();
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        if let Some(server) = &self.0 {
            server.abort();
        }
    }
}

async fn server<C: CorpusService>(
    profile: Profile,
    checksum: u64,
) -> (SocketAddr, SocketProbe, ServerGuard) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let probe = SocketProbe::default();
    let incoming_probe = probe.clone();
    let server = tokio::spawn(async move {
        if profile.native_server() {
            pbrs_grpc::Router::new()
                .add_service(C::server(checksum).into_pbrs_service())
                .serve_with_incoming(NativeIncoming {
                    listener,
                    probe: incoming_probe,
                })
                .await
                .unwrap();
        } else {
            tonic::transport::Server::builder()
                .add_service(C::server(checksum))
                .serve_with_incoming(ReferenceIncoming {
                    // Frozen reference listener policy, equivalent to its
                    // built-in default and the old typed matrix.
                    inner: tonic::transport::server::TcpIncoming::from(listener)
                        .with_nodelay(Some(true)),
                    probe: incoming_probe,
                })
                .await
                .unwrap();
        }
    });
    (addr, probe, ServerGuard(Some(server)))
}

#[derive(Clone, Copy)]
enum RunMode {
    #[cfg(test)]
    Preflight,
    Measure {
        iters: u64,
        warmup: u64,
    },
}

fn wire_fingerprint(wire: &[u8]) -> String {
    // Same portable FNV-1a recipe as the frozen codec corpus. Not a security hash.
    let hash = wire.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}

async fn run_client<C: CorpusService, T: ClientTransport, N>(
    id: &str,
    pair: &Pair<N, C::Message>,
    transport: T,
    probe: &SocketProbe,
    mode: RunMode,
) where
    N: pbrs::Parse + pbrs::Serialize + Clone,
{
    let mut client = C::client(transport);
    let message = pair.prost.clone();
    let checksum = pair.checksum;
    let (profile, _, streaming) = parse_cell(id);
    let fingerprint = wire_fingerprint(&message.encode_to_vec());
    // An actual full-value response/count/read/byte oracle before warmup,
    // identical at N and 2N and outside the exact allocation guard.
    C::call(
        &mut client,
        streaming,
        &message,
        checksum,
        Some(&|response| pair.validate_prost(response)),
    )
    .await;
    let facts = probe.facts();
    eprintln!(
        "__QUALIFICATION__ {}",
        serde_json::json!({
            "id": id, "request_bytes": pair.bytes, "native_request_bytes": pair.native_bytes,
            "response_bytes_per_message": pair.bytes, "request_read_checksum": checksum,
            "response_read_checksum": checksum, "responses": if streaming { STREAM_REPLIES } else { 1 },
            "full_decoded_equality": true, "equal_wire_work": true,
            "input_wire_fingerprint": fingerprint,
            "accepted_tcp_nodelay": facts.nodelay,
            "accepted_local_addr": facts.local, "accepted_remote_addr": facts.remote,
            "accepted_socket_observer": if profile.native_server() {
                "custom Incoming reproduces built-in native tcp::tune TCP_NODELAY=true"
            } else { "frozen reference TcpIncoming.with_nodelay(Some(true))" },
            "message_caps": "unchanged native/tonic defaults",
        })
    );
    let (iters, warmup) = match mode {
        #[cfg(test)]
        RunMode::Preflight => return,
        RunMode::Measure { iters, warmup } => (iters, warmup),
    };
    for _ in 0..warmup {
        black_box(C::call(&mut client, streaming, &message, checksum, None).await);
    }
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        sink = sink.wrapping_add(C::call(&mut client, streaming, &message, checksum, None).await);
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!(
        "__CHILD__ {}",
        child_json_with_input(id, iters, allocs, bytes, wall, Some(fingerprint))
    );
    black_box(sink);
}

async fn run_pair<C: CorpusService, N>(id: &str, pair: Pair<N, C::Message>, mode: RunMode)
where
    N: pbrs::Parse + pbrs::Serialize + Clone,
{
    // Preserve the original corpus's map qualification even though all four
    // new profiles use prost codecs. Unequal original work blocks timing.
    pair.assert_equal_wire_work();
    let (profile, _, _) = parse_cell(id);
    let (addr, probe, server) = server::<C>(profile, pair.checksum).await;
    if profile.native_client() {
        run_client::<C, _, _>(
            id,
            &pair,
            pbrs_grpc::Channel::connect(addr).await.unwrap(),
            &probe,
            mode,
        )
        .await;
    } else {
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        run_client::<C, _, _>(id, &pair, channel, &probe, mode).await;
    }
    // All setup and server termination stay outside the measured window.
    server.finish().await;
}

async fn run_mode(id: &str, mode: RunMode) {
    let (_, specimen, _) = parse_cell(id);
    match Inputs::prepare(specimen) {
        Inputs::Query(pair) => run_pair::<QueryCase, _>(id, pair, mode).await,
        Inputs::Entities(pair) => run_pair::<EntityListCase, _>(id, pair, mode).await,
        Inputs::Sparse(pair) => run_pair::<SparseCase, _>(id, *pair, mode).await,
        Inputs::Maps(pair) => run_pair::<MapHeavyCase, _>(id, pair, mode).await,
    }
}

pub async fn run(id: &str, iters: u64, warmup: u64) {
    run_mode(id, RunMode::Measure { iters, warmup }).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn matrix_preserves_old_ids_and_registers_every_frozen_specimen_shape_and_transport() {
        let old = crate::adoption_rpc::cells();
        let new = cells();
        assert_eq!(old.len(), 512);
        assert_eq!(new.len(), 512);
        let ids: HashSet<_> = old.iter().chain(new.iter()).map(|c| c.0).collect();
        assert_eq!(ids.len(), 1024);
        for profile in PROFILES {
            assert_eq!(new.iter().filter(|c| c.2 == profile.name()).count(), 128);
        }
        assert_eq!(new.iter().filter(|c| c.0.ends_with(".unary")).count(), 256);
        assert_eq!(
            new.iter()
                .filter(|c| c.0.ends_with(".server_stream"))
                .count(),
            256
        );
    }

    #[test]
    fn new_profiles_reject_missing_or_changed_differential_inputs() {
        for profile in PROFILES {
            let inventory = cells();
            let id = inventory.iter().find(|c| c.2 == profile.name()).unwrap().0;
            crate::assert_adoption_input(id, Some("fnv1a64:one"), Some("fnv1a64:one"));
            assert!(
                std::panic::catch_unwind(|| {
                    crate::assert_adoption_input(id, Some("fnv1a64:one"), None);
                })
                .is_err()
            );
            assert!(
                std::panic::catch_unwind(|| {
                    crate::assert_adoption_input(id, Some("fnv1a64:one"), Some("fnv1a64:two"));
                })
                .is_err()
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn complete_network_matrix_preflight_retains_all_24_original_map_blocks() {
        let mut passed = 0;
        let mut blocked = 0;
        for (id, _, _) in cells() {
            let (_, specimen, _) = parse_cell(id);
            let report = Inputs::prepare(specimen).report();
            if report["equal_wire_work"] == false {
                assert!(matches!(specimen, Specimen::Maps(_)));
                assert_eq!(
                    report["native_request_bytes"].as_u64().unwrap(),
                    report["request_bytes"].as_u64().unwrap() + 4
                );
                blocked += 1;
            } else {
                run_mode(id, RunMode::Preflight).await;
                passed += 1;
            }
        }
        assert_eq!((passed, blocked), (488, 24));
    }
}
