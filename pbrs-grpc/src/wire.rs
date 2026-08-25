//! HTTP/2 gRPC protocol helpers (headers, framing, trailers).

use crate::codec;
use crate::metadata::Metadata;
use crate::status::{Code, Status};
use crate::stream::Inbound;
use bytes::{Bytes, BytesMut};
use h2::{Reason, RecvStream, SendStream};
use http::{HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode};
use pbrs::{Parse, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) fn grpc_request(
    authority: &str,
    path: &str,
    md: &Metadata,
    timeout: Option<Duration>,
) -> Result<Request<()>, Status> {
    let uri = format!("http://{authority}{path}");
    let mut req = Request::builder()
        .method(http::Method::POST)
        .uri(&uri)
        .header(http::header::CONTENT_TYPE, "application/grpc")
        .header(http::header::TE, "trailers")
        .body(())
        .map_err(|e| Status::internal(e.to_string()))?;
    if let Some(d) = timeout {
        let val = HeaderValue::from_str(&crate::timeout::encode_timeout(d))
            .map_err(|e| Status::internal(e.to_string()))?;
        req.headers_mut()
            .insert(HeaderName::from_static("grpc-timeout"), val);
    }
    md.write_to(req.headers_mut())?;
    Ok(req)
}

pub(crate) fn check_request(request: &Request<RecvStream>) -> Result<(), Status> {
    if request.method() != http::Method::POST {
        return Err(Status::unimplemented("POST required"));
    }
    let Some(ct) = request.headers().get(http::header::CONTENT_TYPE) else {
        return Err(Status::invalid_argument("missing content-type"));
    };
    let Ok(s) = ct.to_str() else {
        return Err(Status::invalid_argument("invalid content-type"));
    };
    if s.starts_with("application/grpc") {
        Ok(())
    } else {
        Err(Status::invalid_argument(
            "content-type must begin with application/grpc",
        ))
    }
}

pub(crate) fn timeout_from_headers(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(HeaderName::from_static("grpc-timeout"))
        .and_then(|v| v.to_str().ok())
        .and_then(crate::timeout::parse_timeout)
}

pub(crate) fn serialize_payload<T: Serialize>(msg: &T) -> Result<Vec<u8>, Status> {
    T::serialize(msg).map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn send_frame(
    send: &mut SendStream<Bytes>,
    payload: &[u8],
    end: bool,
) -> Result<(), Status> {
    let frame = codec::encode(payload)?;
    send.send_data(frame, end)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn grpc_trailers(status: &Status) -> Result<HeaderMap, Status> {
    let mut map = HeaderMap::new();
    let code = HeaderValue::from_str(&status.code().to_i32().to_string())
        .map_err(|e| Status::internal(e.to_string()))?;
    map.insert(HeaderName::from_static("grpc-status"), code);
    if !status.message().is_empty() {
        let encoded = percent_encode(status.message());
        let val = HeaderValue::from_str(&encoded).map_err(|e| Status::internal(e.to_string()))?;
        map.insert(HeaderName::from_static("grpc-message"), val);
    }
    status.metadata().write_to(&mut map)?;
    Ok(map)
}

pub(crate) fn send_trailers_only(
    respond: &mut h2::server::SendResponse<Bytes>,
    status: Status,
    extra_headers: &Metadata,
) {
    let mut res = match Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/grpc")
        .body(())
    {
        Ok(r) => r,
        Err(_) => return,
    };
    extra_headers.write_to(res.headers_mut()).ok();
    if let Ok(trailers) = grpc_trailers(&status) {
        for (k, v) in &trailers {
            res.headers_mut().append(k, v.clone());
        }
    }
    respond.send_response(res, true).ok();
}

pub(crate) fn send_ok_headers(
    respond: &mut h2::server::SendResponse<Bytes>,
    md: &Metadata,
) -> Result<SendStream<Bytes>, Status> {
    let mut res = Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/grpc")
        .body(())
        .map_err(|e| Status::internal(e.to_string()))?;
    md.write_to(res.headers_mut())?;
    respond
        .send_response(res, false)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn status_from(headers: &HeaderMap, trailers: Option<&HeaderMap>) -> Status {
    let pick = |map: &HeaderMap| {
        map.get(HeaderName::from_static("grpc-status"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i32>().ok())
            .map(|n| {
                let msg = map
                    .get(HeaderName::from_static("grpc-message"))
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                (Code::from_i32(n), msg, Metadata::from_headers(map))
            })
    };
    match trailers.and_then(pick).or_else(|| pick(headers)) {
        Some((code, msg, md)) => {
            let mut st = Status::new(code, msg);
            *st.metadata_mut() = md;
            st
        }
        None => Status::unknown("missing grpc-status"),
    }
}

pub(crate) async fn next_data(recv: &mut RecvStream) -> Result<Option<Bytes>, Status> {
    match recv.data().await {
        None => Ok(None),
        Some(Ok(bytes)) => {
            let n = bytes.len();
            if n > 0 {
                recv.flow_control()
                    .release_capacity(n)
                    .map_err(|e| Status::internal(e.to_string()))?;
            }
            Ok(Some(bytes))
        }
        Some(Err(e)) => {
            if e.is_reset() {
                Err(Status::cancelled())
            } else {
                Err(Status::internal(e.to_string()))
            }
        }
    }
}

struct FrameReader {
    buf: BytesMut,
}

impl FrameReader {
    fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    fn push(&mut self, bytes: Bytes) {
        self.buf.extend_from_slice(&bytes);
    }

    fn pop_parsed<T: Parse + Default>(&mut self) -> Result<Option<T>, Status> {
        match codec::pop(&mut self.buf)? {
            None => Ok(None),
            Some(p) => T::parse(&p)
                .map(Some)
                .map_err(|e| Status::internal(e.to_string())),
        }
    }

    fn finish(&self) -> Result<(), Status> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(Status::internal("truncated grpc frame"))
        }
    }
}

pub(crate) async fn read_all_messages<T: Parse + Default>(
    recv: &mut RecvStream,
) -> Result<(Vec<T>, Option<HeaderMap>), Status> {
    let mut reader = FrameReader::new();
    let mut out = Vec::new();
    while let Some(bytes) = next_data(recv).await? {
        reader.push(bytes);
        while let Some(msg) = reader.pop_parsed()? {
            out.push(msg);
        }
    }
    reader.finish()?;
    let trailers = recv
        .trailers()
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    Ok((out, trailers))
}

pub(crate) fn one_or_default<T: Default>(mut msgs: Vec<T>) -> Result<T, Status> {
    match msgs.len() {
        0 => Ok(T::default()),
        1 => msgs
            .pop()
            .ok_or_else(|| Status::internal("missing message")),
        _ => Err(Status::internal("too many messages for this RPC shape")),
    }
}

pub(crate) async fn pump_inbound<T: Parse + Default>(
    mut recv: RecvStream,
    tx: mpsc::Sender<Result<T, Status>>,
) {
    let mut reader = FrameReader::new();
    loop {
        match next_data(&mut recv).await {
            Ok(Some(bytes)) => {
                reader.push(bytes);
                loop {
                    match reader.pop_parsed() {
                        Ok(Some(msg)) => {
                            if tx.send(Ok(msg)).await.is_err() {
                                return;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tx.send(Err(e)).await.ok();
                            return;
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                tx.send(Err(e)).await.ok();
                return;
            }
        }
    }
    if reader.finish().is_err() {
        tx.send(Err(Status::internal("truncated grpc frame")))
            .await
            .ok();
        return;
    }
    match recv.trailers().await {
        Ok(Some(t)) => {
            let st = status_from(&t, Some(&t));
            if st.code() != Code::Ok {
                tx.send(Err(st)).await.ok();
            }
        }
        Ok(None) => {}
        Err(e) => {
            if e.is_reset() {
                tx.send(Err(Status::cancelled())).await.ok();
            } else {
                tx.send(Err(Status::internal(e.to_string()))).await.ok();
            }
        }
    }
}

pub(crate) async fn pump_outbound<T: Serialize>(
    mut send: SendStream<Bytes>,
    mut rx: mpsc::Receiver<Result<T, Status>>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = cancel_rx.wait_for(|v| *v) => {
                send.send_reset(Reason::CANCEL);
                return;
            }
            item = rx.recv() => {
                match item {
                    None => {
                        send.send_data(Bytes::new(), true).ok();
                        return;
                    }
                    Some(Ok(msg)) => {
                        let Ok(payload) = serialize_payload(&msg) else {
                            send.send_reset(Reason::INTERNAL_ERROR);
                            return;
                        };
                        let Ok(frame) = codec::encode(&payload) else {
                            send.send_reset(Reason::INTERNAL_ERROR);
                            return;
                        };
                        if send.send_data(frame, false).is_err() {
                            return;
                        }
                    }
                    Some(Err(_)) => {
                        send.send_reset(Reason::INTERNAL_ERROR);
                        return;
                    }
                }
            }
        }
    }
}

pub(crate) async fn wrap_timeout<T>(
    timeout: Option<Duration>,
    fut: impl std::future::Future<Output = Result<T, Status>>,
) -> Result<T, Status> {
    match timeout {
        Some(d) => match tokio::time::timeout(d, fut).await {
            Ok(r) => r,
            Err(_) => Err(Status::deadline_exceeded()),
        },
        None => fut.await,
    }
}

pub(crate) async fn finish_unary<Resp: Parse + Default>(
    response: http::Response<RecvStream>,
) -> Result<crate::request::Response<Resp>, Status> {
    if response.status() != StatusCode::OK {
        return Err(Status::unknown(format!("http {}", response.status())));
    }
    let (parts, mut body) = response.into_parts();
    if body.is_end_stream() {
        let st = status_from(&parts.headers, None);
        if st.code() != Code::Ok {
            return Err(st);
        }
    }
    let headers_md = Metadata::from_headers(&parts.headers);
    let (msgs, trailers) = read_all_messages::<Resp>(&mut body).await?;
    let st = status_from(&parts.headers, trailers.as_ref());
    if st.code() != Code::Ok {
        return Err(st);
    }
    let msg = one_or_default(msgs)?;
    let trailers_md = trailers
        .as_ref()
        .map(Metadata::from_headers)
        .unwrap_or_default();
    Ok(crate::request::Response::from_parts(
        msg,
        headers_md,
        trailers_md,
    ))
}

pub(crate) async fn finish_stream<Resp: Parse + Default + Send + 'static>(
    response: http::Response<RecvStream>,
) -> Result<crate::request::Response<Inbound<Resp>>, Status> {
    if response.status() != StatusCode::OK {
        return Err(Status::unknown(format!("http {}", response.status())));
    }
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
        let st = status_from(&parts.headers, None);
        if st.code() != Code::Ok {
            return Err(st);
        }
    }
    let headers_md = Metadata::from_headers(&parts.headers);
    let (tx, inbound) = Inbound::channel(16);
    drop(tokio::spawn(async move {
        pump_inbound::<Resp>(body, tx).await;
    }));
    Ok(crate::request::Response::from_parts(
        inbound,
        headers_md,
        Metadata::new(),
    ))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_graphic() && b != b'%' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
