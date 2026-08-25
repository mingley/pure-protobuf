//! HTTP/2 gRPC client over pbrs messages.

use crate::request::{Call, Request, Response};
use crate::status::{Code, Status};
use crate::stream::{Inbound, StreamingSender};
use crate::wire::{
    finish_stream, finish_unary, grpc_request, pump_outbound, send_frame, serialize_payload,
};
use bytes::Bytes;
use h2::Reason;
use pbrs::{Parse, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

/// Prior-knowledge HTTP/2 connection to a gRPC server.
#[derive(Clone)]
pub struct Channel {
    send: h2::client::SendRequest<Bytes>,
    authority: String,
}

impl Channel {
    /// Dial `addr` with HTTP/2 prior knowledge (cleartext).
    pub async fn connect(addr: SocketAddr) -> Result<Self, Status> {
        let authority = addr.to_string();
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
            .handshake(tcp)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        drop(tokio::spawn(async move {
            conn.await.ok();
        }));
        Ok(Self { send, authority })
    }

    /// Unary RPC.
    pub fn unary<Req, Resp>(&self, path: &'static str, req: Request<Req>) -> Call<Response<Resp>>
    where
        Req: Serialize + Send + 'static,
        Resp: Parse + Default + Send + 'static,
    {
        let (cancel, cancel_rx) = watch::channel(false);
        let mut send = self.send.clone();
        let authority = self.authority.clone();
        Call::new(
            cancel,
            Box::pin(async move { run_unary(&mut send, &authority, path, req, cancel_rx).await }),
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
        let mut send = self.send.clone();
        let authority = self.authority.clone();
        Call::new(
            cancel,
            Box::pin(async move {
                run_server_stream(&mut send, &authority, path, req, cancel_rx).await
            }),
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
        let mut send = self.send.clone();
        let authority = self.authority.clone();
        let call = Call::new(
            cancel.clone(),
            Box::pin(async move {
                run_client_stream(&mut send, &authority, path, req, rx, cancel_rx).await
            }),
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
        let mut send = self.send.clone();
        let authority = self.authority.clone();
        let call = Call::new(
            cancel.clone(),
            Box::pin(
                async move { run_bidi(&mut send, &authority, path, req, rx, cancel_rx).await },
            ),
        );
        (StreamingSender::new(tx), call)
    }
}

async fn run_unary<Req, Resp>(
    send_req: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    path: &str,
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
    let payload = serialize_payload(&msg)?;
    send_frame(&mut send_stream, &payload, compress, true)?;
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
    send_req: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    path: &str,
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
    let payload = serialize_payload(&msg)?;
    send_frame(&mut send_stream, &payload, compress, true)?;
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
    send_req: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    path: &str,
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
    send_req: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    path: &str,
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
    send_req: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    path: &str,
    md: &crate::metadata::Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
) -> Result<(h2::client::ResponseFuture, h2::SendStream<Bytes>), Status> {
    let send_req = send_req.clone();
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
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
            _ = tokio::time::sleep(d) => Err(Status::deadline_exceeded()),
            r = fut => r,
        }
    } else {
        tokio::select! {
            _ = cancel_rx.wait_for(|v| *v) => Err(Status::cancelled()),
            r = fut => r,
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
