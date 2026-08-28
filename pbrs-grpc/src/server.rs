//! HTTP/2 accept loop and per-RPC dispatch helpers.

use crate::codec::SizeLimits;
use crate::metadata::Metadata;
use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Inbound;
use crate::wire::{
    check_request, encode_msg, grpc_trailers, one_or_default, pump_inbound, read_all_messages,
    send_bytes, send_ok_headers, send_trailers_only, timeout_from_headers, wrap_timeout,
};
use bytes::Bytes;
use h2::RecvStream;
use pbrs::{Parse, Serialize};
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

/// HTTP/2 prior-knowledge acceptor. `H` is usually [`crate::hello::GreeterServer`].
pub struct Server<H> {
    handler: Arc<H>,
}

impl<H: Http2Handler> Server<H> {
    /// Wrap a handler.
    pub fn new(handler: H) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }

    /// Bind and serve until the listener fails.
    pub async fn serve(self, addr: SocketAddr) -> Result<(), Status> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Status::unavailable(e.to_string()))?;
        self.serve_listener(listener).await
    }

    /// Accept connections on an existing listener.
    pub async fn serve_listener(self, listener: TcpListener) -> Result<(), Status> {
        loop {
            let (tcp, _) = listener
                .accept()
                .await
                .map_err(|e| Status::unavailable(e.to_string()))?;
            let handler = Arc::clone(&self.handler);
            drop(tokio::spawn(async move {
                serve_conn(handler, tcp).await;
            }));
        }
    }
}

/// Per-stream handler (path dispatch lives on the Greeter server).
pub trait Http2Handler: Send + Sync + 'static {
    /// Drive one HTTP/2 request to completion.
    fn handle(
        &self,
        request: http::Request<RecvStream>,
        respond: h2::server::SendResponse<Bytes>,
    ) -> impl Future<Output = ()> + Send;
}

async fn serve_conn<H: Http2Handler>(handler: Arc<H>, tcp: TcpStream) {
    tcp.set_nodelay(true).ok();
    let Ok(mut conn) = h2::server::Builder::new()
        .initial_window_size(16 * 1024 * 1024)
        .initial_connection_window_size(16 * 1024 * 1024)
        .max_frame_size(1024 * 1024)
        .max_concurrent_streams(256)
        .max_send_buffer_size(1024 * 1024)
        .handshake(tcp)
        .await
    else {
        return;
    };
    while let Some(item) = conn.accept().await {
        let Ok((request, respond)) = item else {
            break;
        };
        let handler = Arc::clone(&handler);
        drop(tokio::spawn(async move {
            handler.handle(request, respond).await;
        }));
    }
}

pub(crate) async fn dispatch_unary<Req, Resp, F, Fut>(
    request: http::Request<RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
    f: F,
) where
    Req: Parse + Default,
    Resp: Serialize,
    F: FnOnce(Request<Req>) -> Fut,
    Fut: Future<Output = Result<Response<Resp>, Status>>,
{
    if let Err(st) = check_request(&request) {
        send_trailers_only(&mut respond, st, &Metadata::new());
        return;
    }
    let timeout = timeout_from_headers(request.headers());
    let header_md = Metadata::from_headers(request.headers());
    let (_, mut recv) = request.into_parts();
    let prepared = wrap_timeout(timeout, async {
        let (msgs, _) = read_all_messages::<Req>(&mut recv, limits.max_decoding).await?;
        let item = one_or_default(msgs)?;
        let mut req = Request::new(item.message);
        req.set_metadata(header_md);
        req.set_compressed(item.compressed);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        f(req).await
    })
    .await;
    finish_handler(prepared, respond, limits).await;
}

pub(crate) async fn dispatch_client_stream<Req, Resp, F, Fut>(
    request: http::Request<RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
    f: F,
) where
    Req: Parse + Default + Send + 'static,
    Resp: Serialize,
    F: FnOnce(Request<Inbound<Req>>) -> Fut,
    Fut: Future<Output = Result<Response<Resp>, Status>>,
{
    if let Err(st) = check_request(&request) {
        send_trailers_only(&mut respond, st, &Metadata::new());
        return;
    }
    let timeout = timeout_from_headers(request.headers());
    let header_md = Metadata::from_headers(request.headers());
    let (_, recv) = request.into_parts();
    let (tx, inbound) = Inbound::channel(16);
    drop(tokio::spawn(async move {
        pump_inbound::<Req>(recv, tx, limits.max_decoding).await;
    }));
    let mut req = Request::new(inbound);
    req.set_metadata(header_md);
    if let Some(d) = timeout {
        req.set_timeout(d);
    }
    let prepared = wrap_timeout(timeout, f(req)).await;
    finish_handler(prepared, respond, limits).await;
}

pub(crate) async fn dispatch_server_stream<Req, Resp, F, Fut>(
    request: http::Request<RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
    f: F,
) where
    Req: Parse + Default,
    Resp: Serialize + Send,
    F: FnOnce(Request<Req>) -> Fut,
    Fut: Future<Output = Result<Response<Inbound<Resp>>, Status>>,
{
    if let Err(st) = check_request(&request) {
        send_trailers_only(&mut respond, st, &Metadata::new());
        return;
    }
    let timeout = timeout_from_headers(request.headers());
    let header_md = Metadata::from_headers(request.headers());
    let (_, mut recv) = request.into_parts();
    let prepared = wrap_timeout(timeout, async {
        let (msgs, _) = read_all_messages::<Req>(&mut recv, limits.max_decoding).await?;
        let item = one_or_default(msgs)?;
        let mut req = Request::new(item.message);
        req.set_metadata(header_md);
        req.set_compressed(item.compressed);
        if let Some(d) = timeout {
            req.set_timeout(d);
        }
        f(req).await
    })
    .await;
    finish_stream_handler(prepared, respond, limits).await;
}

pub(crate) async fn dispatch_bidi<Req, Resp, F, Fut>(
    request: http::Request<RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
    f: F,
) where
    Req: Parse + Default + Send + 'static,
    Resp: Serialize + Send,
    F: FnOnce(Request<Inbound<Req>>) -> Fut,
    Fut: Future<Output = Result<Response<Inbound<Resp>>, Status>>,
{
    if let Err(st) = check_request(&request) {
        send_trailers_only(&mut respond, st, &Metadata::new());
        return;
    }
    let timeout = timeout_from_headers(request.headers());
    let header_md = Metadata::from_headers(request.headers());
    let (_, recv) = request.into_parts();
    let (tx, inbound) = Inbound::channel(16);
    drop(tokio::spawn(async move {
        pump_inbound::<Req>(recv, tx, limits.max_decoding).await;
    }));
    let mut req = Request::new(inbound);
    req.set_metadata(header_md);
    if let Some(d) = timeout {
        req.set_timeout(d);
    }
    let prepared = wrap_timeout(timeout, f(req)).await;
    finish_stream_handler(prepared, respond, limits).await;
}

async fn finish_handler<Resp: Serialize>(
    prepared: Result<Response<Resp>, Status>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
) {
    match prepared {
        Err(st) => send_trailers_only(&mut respond, st, &Metadata::new()),
        Ok(resp) => {
            let (msg, md, trailers, compress) = resp.split();
            let frame = match encode_msg(&msg, compress, limits) {
                Ok(frame) => frame,
                Err(st) => {
                    send_trailers_only(&mut respond, st, &Metadata::new());
                    return;
                }
            };
            let Ok(mut send) = send_ok_headers(&mut respond, &md, compress) else {
                return;
            };
            send_bytes(&mut send, frame, false).await.ok();
            let mut st = Status::new(Code::Ok, "");
            *st.metadata_mut() = trailers;
            if let Ok(t) = grpc_trailers(&st) {
                send.send_trailers(t).ok();
            }
        }
    }
}

async fn finish_stream_handler<Resp: Serialize + Send>(
    prepared: Result<Response<Inbound<Resp>>, Status>,
    mut respond: h2::server::SendResponse<Bytes>,
    limits: SizeLimits,
) {
    match prepared {
        Err(st) => send_trailers_only(&mut respond, st, &Metadata::new()),
        Ok(resp) => {
            let (mut inbound, md, trailers, compress_hdr) = resp.split();
            let Ok(mut send) = send_ok_headers(&mut respond, &md, compress_hdr) else {
                return;
            };
            let mut stream_status = Status::new(Code::Ok, "");
            *stream_status.metadata_mut() = trailers;
            loop {
                match inbound.next_item().await {
                    Ok(Some(item)) => {
                        let frame = match encode_msg(&item.message, item.compressed, limits) {
                            Ok(frame) => frame,
                            Err(st) => {
                                stream_status = st;
                                break;
                            }
                        };
                        if send_bytes(&mut send, frame, false).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(st) => {
                        stream_status = st;
                        break;
                    }
                }
            }
            if let Ok(t) = grpc_trailers(&stream_status) {
                send.send_trailers(t).ok();
            }
        }
    }
}

pub(crate) fn reject_unknown(mut respond: h2::server::SendResponse<Bytes>, path: &str) {
    send_trailers_only(
        &mut respond,
        Status::unimplemented(path.to_string()),
        &Metadata::new(),
    );
}
