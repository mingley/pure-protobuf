//! Same-process unary Codec bench: `ProtobufCodec` vs tonic+prost `ProstCodec`.
//!
//! This is **not** `bench/` (kernel encode/decode vs prost / v4 / buffa).
//! This is **not** a Google C++/Go peer. hello.proto strings only; not
//! official interop `SimpleRequest.payload.body`.
//!
//! tonic 0.14 `EncodeBuf` / `DecodeBuf` have no public constructor. The
//! loops below run the same Encoder/Decoder bodies those codecs use:
//!
//! - `ProtobufCodec` encode: `Serialize` to a new `Vec`, then `put_slice`
//!   into `BytesMut` (the `EncodeBuf` inner buffer).
//! - `ProtobufCodec` decode: copy-all into a new `Vec`, then `Parse`.
//! - `ProstCodec` encode: `prost::Message::encode` into `BytesMut`.
//! - `ProstCodec` decode: `prost::Message::decode` from the buffer.
//!
//! `ProtobufCodec` currently allocates a `Vec` per message on both sides.

use bytes::{Buf, BufMut, BytesMut};
use pbrs::{Parse, Serialize};
use protobuf_tonic::hello::{
    Greeter as PbrsGreeter, GreeterClient as PbrsClient, GreeterServer as PbrsServer,
    HelloReply as PbrsReply, HelloRequest as PbrsHello,
};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

pub mod helloworld {
    include!(concat!(env!("OUT_DIR"), "/helloworld.rs"));
}

use helloworld::HelloRequest as ProstHello;

fn median_ns<F: FnMut()>(samples: usize, iters: u32, mut f: F) -> f64 {
    let mut xs: Vec<f64> = (0..samples).map(|_| bench_ns(iters, &mut f)).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[samples / 2]
}

fn bench_ns<F: FnMut()>(iters: u32, mut f: F) -> f64 {
    for _ in 0..iters / 10 {
        f();
    }
    let t = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(f());
    }
    t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

fn pbrs_hello(name: &str) -> PbrsHello {
    let mut m = PbrsHello::new();
    m.set_name(name);
    m
}

fn prost_hello(name: &str) -> ProstHello {
    ProstHello {
        name: name.to_string(),
    }
}

/// `ProtobufEncoder`: serialize-to-Vec, then `put_slice` into the dst buffer.
fn pbrs_codec_encode(item: &PbrsHello, dst: &mut BytesMut) {
    dst.clear();
    let bytes = Serialize::serialize(item).expect("pbrs serialize");
    dst.put_slice(&bytes);
}

/// `ProtobufDecoder`: copy remaining bytes into a fresh Vec, then `Parse`.
fn pbrs_codec_decode(src: &[u8]) -> PbrsHello {
    let mut buf = src;
    let mut copy = vec![0u8; buf.remaining()];
    buf.copy_to_slice(&mut copy);
    Parse::parse(&copy).expect("pbrs parse")
}

/// `ProstEncoder`: prost writes directly into the dst buffer.
fn prost_codec_encode(item: &ProstHello, dst: &mut BytesMut) {
    dst.clear();
    prost::Message::encode(item, dst).expect("prost encode");
}

/// `ProstDecoder`: prost parses from the buffer (no pre-copy Vec).
fn prost_codec_decode(src: &[u8]) -> ProstHello {
    prost::Message::decode(src).expect("prost decode")
}

struct CodecRow {
    name: &'static str,
    payload: usize,
    pbrs_enc: f64,
    pbrs_dec: f64,
    prost_enc: f64,
    prost_dec: f64,
}

fn run_codec(name: &'static str, field: &str, iters: u32, samples: usize) -> CodecRow {
    let pbrs = pbrs_hello(field);
    let prost = prost_hello(field);
    let pbrs_wire = Serialize::serialize(&pbrs).expect("pbrs wire");
    let mut prost_wire = Vec::new();
    prost::Message::encode(&prost, &mut prost_wire).expect("prost wire");
    assert_eq!(
        pbrs_wire, prost_wire,
        "pbrs and prost must encode the same hello.proto bytes"
    );
    let payload = pbrs_wire.len();

    let mut dst = BytesMut::new();
    let pbrs_enc = median_ns(samples, iters, || {
        pbrs_codec_encode(&pbrs, &mut dst);
        std::hint::black_box(dst.len());
    });
    let pbrs_dec = median_ns(samples, iters, || {
        std::hint::black_box(pbrs_codec_decode(&pbrs_wire));
    });
    let prost_enc = median_ns(samples, iters, || {
        prost_codec_encode(&prost, &mut dst);
        std::hint::black_box(dst.len());
    });
    let prost_dec = median_ns(samples, iters, || {
        std::hint::black_box(prost_codec_decode(&prost_wire));
    });

    CodecRow {
        name,
        payload,
        pbrs_enc,
        pbrs_dec,
        prost_enc,
        prost_dec,
    }
}

struct PbrsEcho;

impl PbrsGreeter for PbrsEcho {
    async fn say_hello(&self, request: Request<PbrsHello>) -> Result<Response<PbrsReply>, Status> {
        let name = request
            .into_inner()
            .name()
            .to_str()
            .unwrap_or("")
            .to_string();
        let mut reply = PbrsReply::new();
        reply.set_message(name);
        Ok(Response::new(reply))
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<PbrsHello>>,
    ) -> Result<Response<PbrsReply>, Status> {
        Err(Status::unimplemented("codec bench"))
    }

    type ServerHelloStream = tokio_stream::wrappers::ReceiverStream<Result<PbrsReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<PbrsHello>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("codec bench"))
    }

    type StreamHelloStream = tokio_stream::wrappers::ReceiverStream<Result<PbrsReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<PbrsHello>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("codec bench"))
    }
}

struct ProstEcho;

#[tonic::async_trait]
impl helloworld::greeter_server::Greeter for ProstEcho {
    async fn say_hello(
        &self,
        request: Request<ProstHello>,
    ) -> Result<Response<helloworld::HelloReply>, Status> {
        Ok(Response::new(helloworld::HelloReply {
            message: request.into_inner().name,
        }))
    }

    async fn client_hello(
        &self,
        _request: Request<tonic::Streaming<ProstHello>>,
    ) -> Result<Response<helloworld::HelloReply>, Status> {
        Err(Status::unimplemented("codec bench"))
    }

    type ServerHelloStream =
        tokio_stream::wrappers::ReceiverStream<Result<helloworld::HelloReply, Status>>;

    async fn server_hello(
        &self,
        _request: Request<ProstHello>,
    ) -> Result<Response<Self::ServerHelloStream>, Status> {
        Err(Status::unimplemented("codec bench"))
    }

    type StreamHelloStream =
        tokio_stream::wrappers::ReceiverStream<Result<helloworld::HelloReply, Status>>;

    async fn stream_hello(
        &self,
        _request: Request<tonic::Streaming<ProstHello>>,
    ) -> Result<Response<Self::StreamHelloStream>, Status> {
        Err(Status::unimplemented("codec bench"))
    }
}

async fn listen() -> (SocketAddr, tokio::net::TcpListener) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    (addr, listener)
}

async fn connect(addr: SocketAddr) -> Channel {
    let url = format!("http://{addr}");
    for _ in 0..50 {
        if let Ok(ch) = Channel::from_shared(url.clone())
            .expect("uri")
            .connect()
            .await
        {
            return ch;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("connect {addr}");
}

async fn spawn_pbrs() -> PbrsClient {
    let (addr, listener) = listen().await;
    tokio::spawn(async move {
        Server::builder()
            .add_service(PbrsServer::new(PbrsEcho))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    PbrsClient::new(connect(addr).await)
}

async fn spawn_prost() -> helloworld::greeter_client::GreeterClient<Channel> {
    let (addr, listener) = listen().await;
    tokio::spawn(async move {
        Server::builder()
            .add_service(helloworld::greeter_server::GreeterServer::new(ProstEcho))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .ok();
    });
    helloworld::greeter_client::GreeterClient::new(connect(addr).await)
}

async fn bench_unary_pbrs(client: &mut PbrsClient, req: &PbrsHello, iters: u32) -> f64 {
    for _ in 0..iters / 10 {
        let resp = client
            .say_hello(Request::new(req.clone()))
            .await
            .expect("pbrs unary");
        std::hint::black_box(resp);
    }
    let t = Instant::now();
    for _ in 0..iters {
        let resp = client
            .say_hello(Request::new(req.clone()))
            .await
            .expect("pbrs unary");
        std::hint::black_box(resp);
    }
    t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

async fn bench_unary_prost(
    client: &mut helloworld::greeter_client::GreeterClient<Channel>,
    req: &ProstHello,
    iters: u32,
) -> f64 {
    for _ in 0..iters / 10 {
        let resp = client
            .say_hello(Request::new(req.clone()))
            .await
            .expect("prost unary");
        std::hint::black_box(resp);
    }
    let t = Instant::now();
    for _ in 0..iters {
        let resp = client
            .say_hello(Request::new(req.clone()))
            .await
            .expect("prost unary");
        std::hint::black_box(resp);
    }
    t.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

struct UnaryRow {
    name: &'static str,
    payload: usize,
    pbrs_ns: f64,
    prost_ns: f64,
}

async fn run_unary(
    name: &'static str,
    field: &str,
    iters: u32,
    samples: usize,
    pbrs_client: &mut PbrsClient,
    prost_client: &mut helloworld::greeter_client::GreeterClient<Channel>,
) -> UnaryRow {
    let pbrs = pbrs_hello(field);
    let prost = prost_hello(field);
    let payload = Serialize::serialize(&pbrs).expect("pbrs wire").len();

    let mut xs: Vec<f64> = Vec::with_capacity(samples);
    for _ in 0..samples {
        xs.push(bench_unary_pbrs(pbrs_client, &pbrs, iters).await);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pbrs_ns = xs[samples / 2];

    let mut ys: Vec<f64> = Vec::with_capacity(samples);
    for _ in 0..samples {
        ys.push(bench_unary_prost(prost_client, &prost, iters).await);
    }
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let prost_ns = ys[samples / 2];

    UnaryRow {
        name,
        payload,
        pbrs_ns,
        prost_ns,
    }
}

#[tokio::main]
async fn main() {
    let codec_iters = 40_000u32;
    let codec_samples = 15usize;
    let codec_rows = [
        run_codec("hello", "ada", codec_iters, codec_samples),
        run_codec("hello_4kib", &"x".repeat(4096), codec_iters, codec_samples),
    ];

    println!("# Codec encode+decode (one unary HelloRequest; no transport)");
    println!();
    println!("Encoder/Decoder body only. tonic EncodeBuf/DecodeBuf are crate-private");
    println!("newtypes over BytesMut / Buf; these are the same operations");
    println!("ProtobufCodec and ProstCodec run. Not kernel encode vs prost (see bench/).");
    println!("Not a Google C++/Go peer. ProtobufCodec allocates a Vec per message.");
    println!("hello.proto name/message strings only. Not interop payload.body.");
    println!();
    println!("iters={codec_iters} samples={codec_samples} (median) release thin-LTO");
    println!();
    println!("| case | payload | ProtobufCodec enc / dec | ProstCodec enc / dec |");
    println!("|---|---:|---:|---:|");
    for r in &codec_rows {
        println!(
            "| {} | {} | {:.1} / {:.1} | {:.1} / {:.1} |",
            r.name, r.payload, r.pbrs_enc, r.pbrs_dec, r.prost_enc, r.prost_dec
        );
    }
    println!();
    println!("ns/op. Combined encode+decode:");
    for r in &codec_rows {
        println!(
            "  {}  ProtobufCodec {:.1}  ProstCodec {:.1}",
            r.name,
            r.pbrs_enc + r.pbrs_dec,
            r.prost_enc + r.prost_dec
        );
    }

    // Localhost unary on this host is ~ms, not tens of ns. Keep the
    // process in tens of seconds (codec table is cheap; this is the bulk).
    let unary_iters = 200u32;
    let unary_samples = 5usize;
    println!();
    println!("# Same-process tonic unary RPC (localhost TCP + HTTP/2 + Codec)");
    println!();
    println!("Both sides in one process. Reused Channel. Echo SayHello.");
    println!("Includes transport, h2 framing, and handler work; codec is a fraction.");
    println!("Request is cloned each call. Not a Google peer. Not kernel encode.");
    println!();
    println!("iters={unary_iters} samples={unary_samples} (median) release thin-LTO");
    println!();

    let mut pbrs_client = spawn_pbrs().await;
    let mut prost_client = spawn_prost().await;
    // One warmup RPC so the HTTP/2 session is up before the timed samples.
    let warm = pbrs_hello("ada");
    pbrs_client
        .say_hello(Request::new(warm))
        .await
        .expect("pbrs warm");
    prost_client
        .say_hello(Request::new(prost_hello("ada")))
        .await
        .expect("prost warm");

    let unary_rows = [
        run_unary(
            "hello",
            "ada",
            unary_iters,
            unary_samples,
            &mut pbrs_client,
            &mut prost_client,
        )
        .await,
        run_unary(
            "hello_4kib",
            &"x".repeat(4096),
            unary_iters,
            unary_samples,
            &mut pbrs_client,
            &mut prost_client,
        )
        .await,
    ];

    println!("| case | payload | protobuf-tonic | tonic-prost |");
    println!("|---|---:|---:|---:|");
    for r in &unary_rows {
        println!(
            "| {} | {} | {:.0} | {:.0} |",
            r.name, r.payload, r.pbrs_ns, r.prost_ns
        );
    }
    println!();
    println!("ns/op for one unary SayHello.");
}
