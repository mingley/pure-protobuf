//! Small-response reserve behavior after a transparent Channel redial.
#![allow(
    clippy::disallowed_methods,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "bounded integration fixture"
)]

mod common;

use bytes::Bytes;
use common::{name_of, reply, req};
use http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE};
use pbrs::Serialize;
use pbrs_grpc::hello::GreeterClient;
use pbrs_grpc::{Channel, ChannelConfig, Request, codec};
use std::collections::HashSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const N: usize = 1024;
const WAIT: Duration = Duration::from_secs(45);

struct DataWriteWitness {
    socket: TcpStream,
    pending: Vec<u8>,
    observed: HashSet<u32>,
    body_len: usize,
    all_written: Option<oneshot::Sender<()>>,
}

impl DataWriteWitness {
    fn record(&mut self, bytes: &[u8]) {
        if self.all_written.is_none() {
            return;
        }
        self.pending.extend_from_slice(bytes);
        while self.pending.len() >= 9 {
            let len = (usize::from(self.pending[0]) << 16)
                | (usize::from(self.pending[1]) << 8)
                | usize::from(self.pending[2]);
            if self.pending.len() < 9 + len {
                break;
            }
            let stream = u32::from_be_bytes(self.pending[5..9].try_into().unwrap()) & 0x7fff_ffff;
            if self.pending[3] == 0 && len == self.body_len && stream != 1 {
                self.observed.insert(stream);
                if self.observed.len() == 103 {
                    println!("REDIAL_103_DATA_WRITTEN");
                }
                if self.observed.len() == N {
                    self.all_written.take().unwrap().send(()).unwrap();
                }
            }
            self.pending.drain(..9 + len);
        }
        assert!(self.pending.len() < 128 * 1024);
    }
}

impl AsyncRead for DataWriteWitness {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}

impl AsyncWrite for DataWriteWitness {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.socket).poll_write(cx, bytes);
        if let Poll::Ready(Ok(n)) = result {
            self.record(&bytes[..n]);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

fn headers() -> http::Response<()> {
    http::Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/grpc")
        .body(())
        .unwrap()
}

fn trailers() -> HeaderMap {
    let mut t = HeaderMap::new();
    t.insert("grpc-status", HeaderValue::from_static("0"));
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reserved_burst_is_released_after_transparent_redial() {
    tokio::time::timeout(WAIT, async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let frame = codec::encode(&reply("x").serialize().unwrap(), false).unwrap();
        assert!(!frame.is_empty() && frame.len() < 256);
        let (write_tx, write_rx) = oneshot::channel();
        let (go_tx, go_rx) = oneshot::channel();
        let (processed_tx, processed_rx) = oneshot::channel();

        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut first = h2::server::Builder::new()
                .handshake::<_, Bytes>(socket)
                .await
                .unwrap();
            let (_request, mut send) = first.accept().await.unwrap().unwrap();
            send.send_reset(h2::Reason::REFUSED_STREAM);
            drop(tokio::spawn(async move {
                while let Some(Ok(_)) = first.accept().await {}
            }));

            let (socket, _) = listener.accept().await.unwrap();
            let observed = DataWriteWitness {
                socket,
                pending: Vec::new(),
                observed: HashSet::new(),
                body_len: frame.len(),
                all_written: Some(write_tx),
            };
            let mut connection = h2::server::Builder::new()
                .max_concurrent_streams(2048)
                .handshake::<_, Bytes>(observed)
                .await
                .unwrap();

            // The retried first unary call completes before the burst starts.
            let (first_request, mut first_reply) = connection.accept().await.unwrap().unwrap();
            let mut first_send = first_reply.send_response(headers(), false).unwrap();
            first_send.send_data(frame.clone(), false).unwrap();
            first_send.send_trailers(trailers()).unwrap();
            let mut requests = vec![first_request.into_body()];
            let mut senders = Vec::with_capacity(N);
            for _ in 0..N {
                let (request, mut respond) = connection.accept().await.unwrap().unwrap();
                requests.push(request.into_body());
                senders.push(respond.send_response(headers(), false).unwrap());
            }
            let mut ping = connection.ping_pong().unwrap();
            let driver =
                tokio::spawn(async move { while let Some(Ok(_)) = connection.accept().await {} });
            go_rx.await.unwrap();
            for mut send in senders {
                send.send_data(frame.clone(), false).unwrap();
                send.send_trailers(trailers()).unwrap();
            }
            write_rx.await.unwrap();
            // PING is issued only after every response DATA frame reached TCP;
            // the ACK proves the new client connection processed those frames.
            ping.ping(h2::Ping::opaque()).await.unwrap();
            processed_tx.send(()).unwrap();
            std::future::pending::<()>().await;
            drop((requests, driver));
        });

        let channel = Channel::connect_with(addr, ChannelConfig::new().connections(1))
            .await
            .unwrap();
        let client = GreeterClient::new(channel);
        let first = client.say_hello(Request::new(req("a"))).await.unwrap();
        assert_eq!(name_of(first.get_ref()), "x");

        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..N {
            let client = client.clone();
            tasks.spawn(async move {
                client
                    .server_hello(Request::new(req("a")))
                    .await
                    .unwrap()
                    .into_inner()
            });
        }
        let mut streams = Vec::with_capacity(N);
        while let Some(result) = tasks.join_next().await {
            streams.push(result.unwrap());
        }
        assert_eq!(streams.len(), N);
        go_tx.send(()).unwrap();
        processed_rx.await.unwrap();
        println!("REDIAL_BURST_1024_PROCESSED");
        // Both consumption and dropping a buffered response return slots.
        drop(streams.split_off(N / 2));
        for mut stream in streams {
            let response = stream.message().await.unwrap().unwrap();
            assert_eq!(name_of(&response), "x");
            assert!(stream.message().await.unwrap().is_none());
        }
        tokio::task::yield_now().await;
        peer.abort();
    })
    .await
    .expect("transparent-redial burst stalled");
}
