//! SB-26d: identical whole-message work on four codec/transport profiles.
//! These are typed tonic/native calls, not the TC-29/30 transport adapters.

use crate::{AllocGuard, black_box, child_json};
use pbrs_adoption_corpus::{
    native, prost_types,
    rpc::{Inputs, Pair, STREAM_REPLIES},
    workloads::{CELLS, Codec, Operation, Specimen},
};
use pbrs_grpc::{CodecMessage, Request, Response, Rpc, Service, Streaming};
use prost::{Message, bytes::Buf};
use std::{
    convert::Infallible,
    marker::PhantomData,
    task::{Context, Poll},
    time::Instant,
};
use tonic::{
    codec::{DecodeBuf, Decoder, EncodeBuf, Encoder},
    codegen::{BoxFuture, http},
};

const UNARY: &str = "/adoption.Echo/Unary";
const STREAM: &str = "/adoption.Echo/ServerStream";

trait BenchMessage: CodecMessage + Clone + Send + Sync + 'static {
    fn read_all(&self) -> u64;
    fn tonic_encode(self, dst: &mut EncodeBuf<'_>) -> Result<(), tonic::Status>;
    fn tonic_decode(src: &mut DecodeBuf<'_>) -> Result<Self, tonic::Status>;
}

macro_rules! messages {
    ($name:ident, $native_read:ident, $prost_read:ident) => {
        impl BenchMessage for native::$name {
            fn read_all(&self) -> u64 {
                pbrs_adoption_corpus::$native_read(self)
            }
            fn tonic_encode(self, dst: &mut EncodeBuf<'_>) -> Result<(), tonic::Status> {
                pbrs::Serialize::encode(&self, dst)
                    .map_err(|e| tonic::Status::internal(e.to_string()))
            }
            fn tonic_decode(src: &mut DecodeBuf<'_>) -> Result<Self, tonic::Status> {
                let bytes = src.copy_to_bytes(src.remaining());
                pbrs::Parse::parse_bytes(bytes).map_err(|e| tonic::Status::internal(e.to_string()))
            }
        }
        impl BenchMessage for pbrs_grpc::codec::prost::Message<prost_types::$name> {
            fn read_all(&self) -> u64 {
                pbrs_adoption_corpus::$prost_read(&self.0)
            }
            fn tonic_encode(self, dst: &mut EncodeBuf<'_>) -> Result<(), tonic::Status> {
                dst.reserve(self.0.encoded_len());
                self.0
                    .encode(dst)
                    .map_err(|e| tonic::Status::internal(e.to_string()))
            }
            fn tonic_decode(src: &mut DecodeBuf<'_>) -> Result<Self, tonic::Status> {
                prost_types::$name::decode(src)
                    .map(Self)
                    .map_err(|e| tonic::Status::internal(e.to_string()))
            }
        }
    };
}
messages!(Query, touch_query_native, touch_query_prost);
messages!(
    EntityList,
    touch_entity_list_native,
    touch_entity_list_prost
);
messages!(Sparse, touch_sparse_native, touch_sparse_prost);
messages!(MapHeavy, touch_maps_native, touch_maps_prost);

struct TonicCodec<M>(PhantomData<fn() -> M>);
impl<M> Default for TonicCodec<M> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<M: BenchMessage> tonic::codec::Codec for TonicCodec<M> {
    type Encode = M;
    type Decode = M;
    type Encoder = Self;
    type Decoder = Self;
    fn encoder(&mut self) -> Self {
        Self::default()
    }
    fn decoder(&mut self) -> Self {
        Self::default()
    }
}
impl<M: BenchMessage> Encoder for TonicCodec<M> {
    type Item = M;
    type Error = tonic::Status;
    fn encode(&mut self, item: M, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.tonic_encode(dst)
    }
}
impl<M: BenchMessage> Decoder for TonicCodec<M> {
    type Item = M;
    type Error = tonic::Status;
    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<M>, Self::Error> {
        M::tonic_decode(src).map(Some)
    }
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
impl<M: BenchMessage> Service for Echo<M> {
    const NAME: &'static str = "adoption.Echo";
    async fn call(&self, rpc: Rpc) {
        let checksum = self.checksum;
        match rpc.method() {
            "Unary" => {
                rpc.unary(move |req: Request<M>| async move {
                    let message = req.into_inner();
                    assert_eq!(black_box(message.read_all()), checksum);
                    Ok::<_, pbrs_grpc::Status>(Response::new(message))
                })
                .await
            }
            "ServerStream" => {
                rpc.server_streaming(move |req: Request<M>| async move {
                    let message = req.into_inner();
                    assert_eq!(black_box(message.read_all()), checksum);
                    let (tx, stream) = Streaming::channel(8);
                    drop(tokio::spawn(async move {
                        for _ in 0..STREAM_REPLIES {
                            if tx.send(message.clone()).await.is_err() {
                                break;
                            }
                        }
                    }));
                    Ok::<_, pbrs_grpc::Status>(Response::new(stream))
                })
                .await
            }
            _ => rpc.unimplemented(),
        }
    }
}

impl<M: BenchMessage> tonic::server::UnaryService<M> for Echo<M> {
    type Response = M;
    type Future = BoxFuture<tonic::Response<M>, tonic::Status>;
    fn call(&mut self, req: tonic::Request<M>) -> Self::Future {
        let checksum = self.checksum;
        Box::pin(async move {
            let message = req.into_inner();
            assert_eq!(black_box(message.read_all()), checksum);
            Ok(tonic::Response::new(message))
        })
    }
}
impl<M: BenchMessage> tonic::server::ServerStreamingService<M> for Echo<M> {
    type Response = M;
    type ResponseStream = tokio_stream::wrappers::ReceiverStream<Result<M, tonic::Status>>;
    type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
    fn call(&mut self, req: tonic::Request<M>) -> Self::Future {
        let checksum = self.checksum;
        Box::pin(async move {
            let message = req.into_inner();
            assert_eq!(black_box(message.read_all()), checksum);
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            drop(tokio::spawn(async move {
                for _ in 0..STREAM_REPLIES {
                    if tx.send(Ok(message.clone())).await.is_err() {
                        break;
                    }
                }
            }));
            Ok(tonic::Response::new(
                tokio_stream::wrappers::ReceiverStream::new(rx),
            ))
        })
    }
}
impl<M: BenchMessage> tonic::server::NamedService for Echo<M> {
    const NAME: &'static str = "adoption.Echo";
}
impl<M: BenchMessage> tonic::codegen::Service<http::Request<tonic::body::Body>> for Echo<M> {
    type Response = http::Response<tonic::body::Body>;
    type Error = Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
        let handler = self.clone();
        Box::pin(async move {
            let mut grpc = tonic::server::Grpc::new(TonicCodec::<M>::default());
            let response = match request.uri().path() {
                UNARY => grpc.unary(handler, request).await,
                STREAM => grpc.server_streaming(handler, request).await,
                other => panic!("unregistered adoption RPC {other}"),
            };
            Ok(response)
        })
    }
}

pub fn cells() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for cell in CELLS
        .iter()
        .filter(|c| matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll))
    {
        let specimen = cell
            .id
            .strip_prefix("codec.adoption.prost.")
            .unwrap()
            .strip_suffix(".read_all")
            .unwrap();
        for profile in ["native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost"] {
            for shape in ["unary", "server_stream"] {
                let id: &'static str = Box::leak(
                    format!("rpc.adoption.{profile}.{specimen}.{shape}").into_boxed_str(),
                );
                out.push((id, "rpc", profile));
            }
        }
    }
    out
}

fn specimen(id: &str) -> Specimen {
    CELLS
        .iter()
        .filter(|c| matches!(c.codec, Codec::Prost) && matches!(c.operation, Operation::ReadAll))
        .find(|c| {
            let name =
                c.id.strip_prefix("codec.adoption.prost.")
                    .unwrap()
                    .strip_suffix(".read_all")
                    .unwrap();
            id.strip_prefix("rpc.adoption.")
                .unwrap()
                .split_once('.')
                .unwrap()
                .1
                .strip_suffix(".unary")
                .or_else(|| {
                    id.strip_prefix("rpc.adoption.")
                        .unwrap()
                        .split_once('.')
                        .unwrap()
                        .1
                        .strip_suffix(".server_stream")
                })
                == Some(name)
        })
        .expect("registered RPC specimen")
        .specimen
}

async fn run_typed<M: BenchMessage>(
    id: &str,
    message: M,
    checksum: u64,
    validate: impl Fn(&M),
    iters: u64,
    warmup: u64,
) {
    let tonic = id.starts_with("rpc.adoption.tonic_");
    let streaming = id.ends_with(".server_stream");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (native, tonic_client, server) = if tonic {
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(Echo::<M>::new(checksum))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        (None, Some(tonic::client::Grpc::new(channel)), server)
    } else {
        let server = tokio::spawn(async move {
            pbrs_grpc::Router::new()
                .add_service(Echo::<M>::new(checksum))
                .serve_listener(listener)
                .await
                .unwrap();
        });
        (
            Some(pbrs_grpc::Channel::connect(addr).await.unwrap()),
            None,
            server,
        )
    };
    let mut tonic_client = tonic_client;
    // Full decoded equality validates an actual network response before warmup.
    // It is identical at N and 2N and never counted by the allocation guard.
    async fn call<M: BenchMessage>(
        native: &Option<pbrs_grpc::Channel>,
        tonic: &mut Option<tonic::client::Grpc<tonic::transport::Channel>>,
        streaming: bool,
        message: &M,
        checksum: u64,
        validate: Option<&dyn Fn(&M)>,
    ) -> u64 {
        let mut count = 0;
        let mut sink = 0u64;
        let mut consume = |response: M| {
            assert_eq!(black_box(response.read_all()), checksum);
            if let Some(check) = validate {
                check(&response);
            }
            sink = sink.wrapping_add(checksum);
            count += 1;
        };
        if let Some(channel) = native {
            if streaming {
                let mut stream = channel
                    .server_streaming::<M, M>(STREAM, Request::new(message.clone()))
                    .await
                    .unwrap()
                    .into_inner();
                while let Some(response) = stream.message().await.unwrap() {
                    consume(response);
                }
            } else {
                consume(
                    channel
                        .unary::<M, M>(UNARY, Request::new(message.clone()))
                        .await
                        .unwrap()
                        .into_inner(),
                );
            }
        } else {
            let client = tonic.as_mut().unwrap();
            client.ready().await.unwrap();
            if streaming {
                let mut stream = client
                    .server_streaming(
                        tonic::Request::new(message.clone()),
                        http::uri::PathAndQuery::from_static(STREAM),
                        TonicCodec::<M>::default(),
                    )
                    .await
                    .unwrap()
                    .into_inner();
                while let Some(response) = stream.message().await.unwrap() {
                    consume(response);
                }
            } else {
                consume(
                    client
                        .unary(
                            tonic::Request::new(message.clone()),
                            http::uri::PathAndQuery::from_static(UNARY),
                            TonicCodec::<M>::default(),
                        )
                        .await
                        .unwrap()
                        .into_inner(),
                );
            }
        }
        assert_eq!(count, if streaming { STREAM_REPLIES } else { 1 });
        sink
    }
    call(
        &native,
        &mut tonic_client,
        streaming,
        &message,
        checksum,
        Some(&validate),
    )
    .await;
    for _ in 0..warmup {
        black_box(
            call(
                &native,
                &mut tonic_client,
                streaming,
                &message,
                checksum,
                None,
            )
            .await,
        );
    }
    let guard = AllocGuard::arm();
    let start = Instant::now();
    let mut sink = 0u64;
    for _ in 0..iters {
        sink = sink.wrapping_add(
            call(
                &native,
                &mut tonic_client,
                streaming,
                &message,
                checksum,
                None,
            )
            .await,
        );
    }
    let wall = start.elapsed();
    let (allocs, bytes) = guard.totals();
    drop(guard);
    eprintln!("__CHILD__ {}", child_json(id, iters, allocs, bytes, wall));
    black_box(sink);
    // Processes are not retained after a cell; server shutdown is outside
    // the measured window and awaits termination of our accept task.
    server.abort();
    let _ = server.await;
}

async fn run_pair<N, P>(id: &str, pair: Pair<N, P>, iters: u64, warmup: u64)
where
    N: BenchMessage + pbrs::Parse + pbrs::Serialize,
    P: Message + Default + Clone + PartialEq + std::fmt::Debug,
    pbrs_grpc::codec::prost::Message<P>: BenchMessage,
{
    pair.assert_equal_wire_work();
    eprintln!("__QUALIFICATION__ {}", pair_report(&pair));
    if id.starts_with("rpc.adoption.native_pbrs.") || id.starts_with("rpc.adoption.tonic_pbrs.") {
        run_typed(
            id,
            pair.native.clone(),
            pair.checksum,
            |response| pair.validate_native(response),
            iters,
            warmup,
        )
        .await;
    } else {
        run_typed(
            id,
            pbrs_grpc::codec::prost::Message(pair.prost.clone()),
            pair.checksum,
            |response| pair.validate_prost(&response.0),
            iters,
            warmup,
        )
        .await;
    }
}
fn pair_report<N, P>(pair: &Pair<N, P>) -> serde_json::Value {
    serde_json::json!({"request_bytes": pair.bytes, "response_bytes": pair.bytes, "checksum": pair.checksum, "stream_replies": STREAM_REPLIES})
}

pub async fn run(id: &str, iters: u64, warmup: u64) {
    assert!(
        cells().iter().any(|c| c.0 == id),
        "registered adoption RPC cell"
    );
    match Inputs::prepare(specimen(id)) {
        Inputs::Query(pair) => run_pair(id, pair, iters, warmup).await,
        Inputs::Entities(pair) => run_pair(id, pair, iters, warmup).await,
        Inputs::Sparse(pair) => run_pair(id, *pair, iters, warmup).await,
        Inputs::Maps(pair) => run_pair(id, pair, iters, warmup).await,
    }
}
