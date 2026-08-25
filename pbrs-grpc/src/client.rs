//! HTTP/2 gRPC client over pbrs messages.

use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::stream::{Inbound, StreamingSender};
use crate::wire::{
    encode_msg, finish_stream, finish_unary, grpc_request, pump_outbound, send_bytes,
};
use bytes::Bytes;
use h2::Reason;
use http::uri::Authority;
use pbrs::{Parse, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

struct ChannelInner {
    sends: Vec<h2::client::SendRequest<Bytes>>,
    next: AtomicUsize,
    authority: Authority,
}

/// Prior-knowledge HTTP/2 connection to a gRPC server.
///
/// [`Self::connect`] is one connection. [`Self::connect_pool`] opens several
/// so concurrent RPCs run on independent h2 driver tasks (one per tokio
/// worker that the runtime schedules).
#[derive(Clone)]
pub struct Channel {
    inner: Arc<ChannelInner>,
}

impl Channel {
    /// Dial `addr` with HTTP/2 prior knowledge (cleartext). One connection.
    pub async fn connect(addr: SocketAddr) -> Result<Self, Status> {
        Self::connect_pool(addr, 1).await
    }

    /// Dial `n` prior-knowledge HTTP/2 connections to `addr`. RPCs pick a
    /// connection round-robin so a high-concurrency tokio runtime can drive
    /// more than one h2 task.
    pub async fn connect_pool(addr: SocketAddr, n: usize) -> Result<Self, Status> {
        let n = n.max(1);
        let authority: Authority = addr
            .to_string()
            .parse()
            .map_err(|e| Status::unavailable(format!("authority: {e}")))?;
        let mut sends = Vec::with_capacity(n);
        for _ in 0..n {
            sends.push(handshake(addr).await?);
        }
        Ok(Self {
            inner: Arc::new(ChannelInner {
                sends,
                next: AtomicUsize::new(0),
                authority,
            }),
        })
    }

    fn grab(&self) -> Result<h2::client::SendRequest<Bytes>, Status> {
        let sends = &self.inner.sends;
        let n = sends.len();
        let i = if n == 0 {
            0
        } else {
            self.inner.next.fetch_add(1, Ordering::Relaxed) % n
        };
        sends
            .get(i)
            .cloned()
            .ok_or_else(|| Status::unavailable("empty connection pool"))
    }

    fn authority(&self) -> Authority {
        self.inner.authority.clone()
    }

    /// Unary RPC.
    pub fn unary<Req, Resp>(&self, path: &'static str, req: Request<Req>) -> Call<Response<Resp>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => {
                return Call::new(cancel, Box::pin(async move { Err(e) }));
            }
        };
        let authority = self.authority();
        Call::new(
            cancel,
            Box::pin(async move { run_unary(send, &authority, path, req, cancel_rx).await }),
        )
    }

    /// Server-streaming RPC.
    pub fn server_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<Req>,
    ) -> Call<Response<Inbound<Resp>>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => {
                return Call::new(cancel, Box::pin(async move { Err(e) }));
            }
        };
        let authority = self.authority();
        Call::new(
            cancel,
            Box::pin(
                async move { run_server_stream(send, &authority, path, req, cancel_rx).await },
            ),
        )
    }

    /// Client-streaming RPC.
    pub fn client_streaming<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamingSender<Req>, Call<Response<Resp>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(16);
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => {
                let call = Call::new(cancel, Box::pin(async move { Err(e) }));
                return (StreamingSender::new(tx), call);
            }
        };
        let authority = self.authority();
        let call = Call::new(
            cancel.clone(),
            Box::pin(
                async move { run_client_stream(send, &authority, path, req, rx, cancel_rx).await },
            ),
        );
        (StreamingSender::new(tx), call)
    }

    /// Bidi-streaming RPC.
    pub fn bidi<Req, Resp>(
        &self,
        path: &'static str,
        req: Request<()>,
    ) -> (StreamingSender<Req>, Call<Response<Inbound<Resp>>>)
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(16);
        let (cancel, cancel_rx) = watch::channel(false);
        let send = match self.grab() {
            Ok(s) => s,
            Err(e) => {
                let call = Call::new(cancel, Box::pin(async move { Err(e) }));
                return (StreamingSender::new(tx), call);
            }
        };
        let authority = self.authority();
        let call = Call::new(
            cancel.clone(),
            Box::pin(async move { run_bidi(send, &authority, path, req, rx, cancel_rx).await }),
        );
        (StreamingSender::new(tx), call)
    }
}

async fn handshake(addr: SocketAddr) -> Result<h2::client::SendRequest<Bytes>, Status> {
    let tcp = TcpStream::connect(addr)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    tcp.set_nodelay(true)
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let (send, conn) = h2::client::Builder::new()
        .initial_window_size(16 * 1024 * 1024)
        .initial_connection_window_size(16 * 1024 * 1024)
        .max_frame_size(1024 * 1024)
        .max_concurrent_streams(256)
        .max_send_buffer_size(1024 * 1024)
        .handshake(tcp)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    drop(tokio::spawn(async move {
        conn.await.ok();
    }));
    Ok(send)
}

async fn run_unary<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<Req>,
    cancel_rx: watch::Receiver<bool>,
) -> Result<Response<Resp>, Status>
where
    Req: Serialize,
    Resp: Parse + Default,
{
    let (msg, md, timeout, compress) = req.into_parts();
    let (resp_fut, mut send_stream) =
        open(send_req, authority, path, &md, timeout, compress).await?;
    send_bytes(&mut send_stream, encode_msg(&msg, compress)?, true).await?;
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            finish_unary::<Resp>(response).await
        },
        cancel_rx,
        timeout,
        Some(&mut send_stream),
    )
    .await
}

async fn run_server_stream<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<Req>,
    cancel_rx: watch::Receiver<bool>,
) -> Result<Response<Inbound<Resp>>, Status>
where
    Req: Serialize,
    Resp: Parse + Default + Send + 'static,
{
    let (msg, md, timeout, compress) = req.into_parts();
    let (resp_fut, mut send_stream) =
        open(send_req, authority, path, &md, timeout, compress).await?;
    send_bytes(&mut send_stream, encode_msg(&msg, compress)?, true).await?;
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            finish_stream::<Resp>(response).await
        },
        cancel_rx,
        timeout,
        Some(&mut send_stream),
    )
    .await
}

async fn run_client_stream<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<()>,
    rx: mpsc::Receiver<Result<crate::stream::OutItem<Req>, Status>>,
    cancel_rx: watch::Receiver<bool>,
) -> Result<Response<Resp>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default,
{
    let (_, md, timeout, compress) = req.into_parts();
    let (resp_fut, send_stream) = open(send_req, authority, path, &md, timeout, compress).await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
    )));
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            finish_unary::<Resp>(response).await
        },
        cancel_rx,
        timeout,
        None,
    )
    .await
}

async fn run_bidi<Req, Resp>(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    req: Request<()>,
    rx: mpsc::Receiver<Result<crate::stream::OutItem<Req>, Status>>,
    cancel_rx: watch::Receiver<bool>,
) -> Result<Response<Inbound<Resp>>, Status>
where
    Req: Serialize + Send + 'static,
    Resp: Parse + Default + Send + 'static,
{
    let (_, md, timeout, compress) = req.into_parts();
    let (resp_fut, send_stream) = open(send_req, authority, path, &md, timeout, compress).await?;
    drop(tokio::spawn(pump_outbound(
        send_stream,
        rx,
        cancel_rx.clone(),
    )));
    race(
        async {
            let response = resp_fut
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            finish_stream::<Resp>(response).await
        },
        cancel_rx,
        timeout,
        None,
    )
    .await
}

async fn open(
    send_req: h2::client::SendRequest<Bytes>,
    authority: &Authority,
    path: &'static str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
) -> Result<(h2::client::ResponseFuture, h2::SendStream<Bytes>), Status> {
    let mut send_req = send_req
        .ready()
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let http_req = grpc_request(authority, path, md, timeout, send_gzip)?;
    send_req
        .send_request(http_req, false)
        .map_err(|e| Status::unavailable(e.to_string()))
}

async fn race<T>(
    fut: impl std::future::Future<Output = Result<T, Status>>,
    mut cancel_rx: watch::Receiver<bool>,
    timeout: Option<Duration>,
    send: Option<&mut h2::SendStream<Bytes>>,
) -> Result<T, Status> {
    let result = if let Some(d) = timeout {
        tokio::select! {
            biased;
            r = fut => r,
            _ = tokio::time::sleep(d) => Err(Status::deadline_exceeded()),
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
        }
    } else {
        tokio::select! {
            biased;
            r = fut => r,
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
        }
    };
    if let Some(send) = send {
        if matches!(
            &result,
            Err(s) if s.code() == Code::Cancelled || s.code() == Code::DeadlineExceeded
        ) {
            send.send_reset(Reason::CANCEL);
        }
    }
    result
}
