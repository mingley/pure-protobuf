//! HTTP/2 gRPC protocol helpers (headers, framing, trailers).

use crate::codec::{self, SizeLimits};
use crate::gzip;
use crate::metadata::Metadata;
use crate::status::{Code, Status};
use crate::stream::{InItem, Inbound, OutItem};
use bytes::{BufMut, Bytes, BytesMut};
use h2::{Reason, RecvStream, SendStream};
use http::uri::{Authority, PathAndQuery, Scheme};
use http::{HeaderMap, HeaderName, HeaderValue, Request, Response, StatusCode};
use pbrs::{Parse, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) fn grpc_request(
    authority: &Authority,
    path: &'static str,
    md: &Metadata,
    timeout: Option<Duration>,
    send_gzip: bool,
) -> Result<Request<()>, Status> {
    let mut parts = http::uri::Parts::default();
    parts.scheme = Some(Scheme::HTTP);
    parts.authority = Some(authority.clone());
    parts.path_and_query = Some(PathAndQuery::from_static(path));
    let uri = http::Uri::from_parts(parts).map_err(|e| Status::internal(e.to_string()))?;
    let mut builder = Request::builder()
        .method(http::Method::POST)
        .uri(uri)
        .header(http::header::CONTENT_TYPE, "application/grpc")
        .header(http::header::TE, "trailers")
        .header(
            HeaderName::from_static("grpc-accept-encoding"),
            "identity,gzip",
        );
    if send_gzip {
        builder = builder.header(HeaderName::from_static("grpc-encoding"), "gzip");
    }
    let mut req = builder
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

fn frame_from_msg<T: Serialize>(msg: &T) -> Result<Bytes, Status> {
    let n = T::serialized_len(msg);
    let len = u32::try_from(n).map_err(|_| Status::internal("message too large"))?;
    let mut buf = BytesMut::with_capacity(5 + n);
    buf.put_u8(0);
    buf.put_u32(len);
    T::encode(msg, &mut buf).map_err(|e| Status::internal(e.to_string()))?;
    Ok(buf.freeze())
}

async fn wait_capacity(send: &mut SendStream<Bytes>, n: usize) -> Result<(), Status> {
    if send.capacity() >= n {
        return Ok(());
    }
    send.reserve_capacity(n);
    while send.capacity() < n {
        match std::future::poll_fn(|cx| send.poll_capacity(cx)).await {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(Status::internal(e.to_string())),
            None => return Err(Status::internal("stream closed")),
        }
    }
    Ok(())
}

pub(crate) fn encode_msg<T: Serialize>(
    msg: &T,
    compress: bool,
    limits: SizeLimits,
) -> Result<Bytes, Status> {
    limits.check_encode(T::serialized_len(msg))?;
    if compress {
        let body = serialize_payload(msg)?;
        let gz = gzip::encode(&body)?;
        codec::encode(&gz, true)
    } else {
        frame_from_msg(msg)
    }
}

pub(crate) async fn send_bytes(
    send: &mut SendStream<Bytes>,
    frame: Bytes,
    end: bool,
) -> Result<(), Status> {
    // Empty/small frames fit the send buffer. Polling capacity on every
    // 5-byte unary serializes the connection task.
    if frame.len() > 16 * 1024 {
        wait_capacity(send, frame.len()).await?;
    }
    send.send_data(frame, end)
        .map_err(|e| Status::internal(e.to_string()))
}

pub(crate) fn grpc_trailers(status: &Status) -> Result<HeaderMap, Status> {
    if status.code() == Code::Ok && status.message().is_empty() && status.metadata().is_empty() {
        let mut map = HeaderMap::with_capacity(1);
        map.insert(
            HeaderName::from_static("grpc-status"),
            HeaderValue::from_static("0"),
        );
        return Ok(map);
    }
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
    send_gzip: bool,
) -> Result<SendStream<Bytes>, Status> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/grpc");
    if send_gzip {
        builder = builder.header(HeaderName::from_static("grpc-encoding"), "gzip");
    }
    let mut res = builder
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
                    .map(percent_decode)
                    .unwrap_or_default();
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
    max_decoding: Option<usize>,
}

impl FrameReader {
    fn new(max_decoding: Option<usize>) -> Self {
        Self {
            buf: BytesMut::new(),
            max_decoding,
        }
    }

    fn push(&mut self, bytes: Bytes) {
        self.buf.extend_from_slice(&bytes);
    }

    fn pop_parsed<T: Parse + Default>(&mut self) -> Result<Option<InItem<T>>, Status> {
        match codec::pop_limited(&mut self.buf, self.max_decoding)? {
            None => Ok(None),
            Some(frame) => {
                let message = if frame.compressed {
                    let raw = gzip::decode(&frame.payload)?;
                    SizeLimits {
                        max_decoding: self.max_decoding,
                        max_encoding: None,
                    }
                    .check_decode(raw.len())?;
                    T::parse(&raw).map_err(|e| Status::internal(e.to_string()))?
                } else {
                    T::parse(frame.payload.as_ref()).map_err(|e| Status::internal(e.to_string()))?
                };
                Ok(Some(InItem {
                    message,
                    compressed: frame.compressed,
                }))
            }
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
    max_decoding: Option<usize>,
) -> Result<(Vec<InItem<T>>, Option<HeaderMap>), Status> {
    let mut reader = FrameReader::new(max_decoding);
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

pub(crate) fn one_or_default<T: Default>(mut msgs: Vec<InItem<T>>) -> Result<InItem<T>, Status> {
    match msgs.len() {
        0 => Ok(InItem {
            message: T::default(),
            compressed: false,
        }),
        1 => msgs
            .pop()
            .ok_or_else(|| Status::internal("missing message")),
        _ => Err(Status::internal("too many messages for this RPC shape")),
    }
}

pub(crate) async fn pump_inbound<T: Parse + Default>(
    mut recv: RecvStream,
    tx: mpsc::Sender<Result<InItem<T>, Status>>,
    max_decoding: Option<usize>,
) -> Metadata {
    let mut reader = FrameReader::new(max_decoding);
    loop {
        match next_data(&mut recv).await {
            Ok(Some(bytes)) => {
                reader.push(bytes);
                loop {
                    match reader.pop_parsed() {
                        Ok(Some(msg)) => {
                            if tx.send(Ok(msg)).await.is_err() {
                                return Metadata::new();
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tx.send(Err(e)).await.ok();
                            return Metadata::new();
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                tx.send(Err(e)).await.ok();
                return Metadata::new();
            }
        }
    }
    if reader.finish().is_err() {
        tx.send(Err(Status::internal("truncated grpc frame")))
            .await
            .ok();
        return Metadata::new();
    }
    match recv.trailers().await {
        Ok(Some(t)) => {
            let st = status_from(&t, Some(&t));
            let md = Metadata::from_headers(&t);
            if st.code() != Code::Ok {
                tx.send(Err(st)).await.ok();
            }
            md
        }
        Ok(None) => Metadata::new(),
        Err(e) => {
            if e.is_reset() {
                tx.send(Err(Status::cancelled())).await.ok();
            } else {
                tx.send(Err(Status::internal(e.to_string()))).await.ok();
            }
            Metadata::new()
        }
    }
}

pub(crate) async fn pump_outbound<T: Serialize>(
    mut send: SendStream<Bytes>,
    mut rx: mpsc::Receiver<Result<OutItem<T>, Status>>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    limits: SizeLimits,
) {
    let mut watch_cancel = true;
    loop {
        tokio::select! {
            cancelled = async {
                cancel_rx.wait_for(|v| *v).await.is_ok()
            }, if watch_cancel => {
                if cancelled {
                    send.send_reset(Reason::CANCEL);
                    return;
                }
                watch_cancel = false;
            }
            item = rx.recv() => {
                match item {
                    None => {
                        send.send_data(Bytes::new(), true).ok();
                        return;
                    }
                    Some(Ok(item)) => {
                        let Ok(frame) = encode_msg(&item.message, item.compress, limits) else {
                            send.send_reset(Reason::INTERNAL_ERROR);
                            return;
                        };
                        if send_bytes(&mut send, frame, false).await.is_err() {
                            send.send_reset(Reason::INTERNAL_ERROR);
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
    max_decoding: Option<usize>,
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
    let (msgs, trailers) = read_all_messages::<Resp>(&mut body, max_decoding).await?;
    let st = status_from(&parts.headers, trailers.as_ref());
    if st.code() != Code::Ok {
        return Err(st);
    }
    let item = one_or_default(msgs)?;
    let trailers_md = trailers
        .as_ref()
        .map(Metadata::from_headers)
        .unwrap_or_default();
    Ok(crate::request::Response::from_parts_compress(
        item.message,
        headers_md,
        trailers_md,
        item.compressed,
    ))
}

pub(crate) async fn finish_stream<Resp: Parse + Default + Send + 'static>(
    response: http::Response<RecvStream>,
    max_decoding: Option<usize>,
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
    let (tx, mut inbound) = Inbound::channel(16);
    let (tr_tx, tr_rx) = tokio::sync::oneshot::channel();
    inbound.set_trailers(tr_rx);
    drop(tokio::spawn(async move {
        let trailers = pump_inbound::<Resp>(body, tx, max_decoding).await;
        tr_tx.send(trailers).ok();
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

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.get(i).copied() == Some(b'%') {
            let h1 = bytes.get(i + 1).copied();
            let h2 = bytes.get(i + 2).copied();
            if let (Some(a), Some(b)) = (h1, h2) {
                if let Ok(v) = u8::from_str_radix(core::str::from_utf8(&[a, b]).unwrap_or("00"), 16)
                {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(&b) = bytes.get(i) {
            out.push(b);
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}
