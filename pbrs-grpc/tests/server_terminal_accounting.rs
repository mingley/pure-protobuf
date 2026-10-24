//! Lifecycle completion reports the committed response, not a later clock read.
//! Registry h2 is the peer; grpc-timeout does not install a client-side timer.
#![allow(
    clippy::disallowed_types,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "bounded raw-peer accounting and destructor regression witnesses"
)]

use bytes::Bytes;
use pbrs_grpc::telemetry::{CallLabels, LifecycleObserver};
use pbrs_grpc::{
    ByteBudgetTracker, Code, CodecMessage, HelloRequest, Request, Response, Rpc, Server, Service,
    Status, Streaming,
};
use std::future::{Future, poll_fn};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy)]
enum ReplyKind {
    Plain,
    AdvanceOnDrop,
    EncodeError,
    BudgetError,
}

#[derive(Default)]
struct Probe {
    starts: AtomicUsize,
    ends: Mutex<Vec<Code>>,
    drops: AtomicUsize,
}

struct Observer(Arc<Probe>);
impl LifecycleObserver for Observer {
    fn on_server_call_start(&self, call: &CallLabels<'_>) {
        if call.method() != "Recovery" {
            self.0.starts.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn on_server_call_end(&self, call: &CallLabels<'_>, status: &Status, _: Duration) {
        if call.method() != "Recovery" {
            self.0.ends.lock().unwrap().push(status.code());
        }
    }
}

struct Reply {
    kind: ReplyKind,
    probe: Option<Arc<Probe>>,
}

impl CodecMessage for Reply {
    fn encoded_len(&self) -> usize {
        if matches!(self.kind, ReplyKind::BudgetError) {
            4096
        } else {
            0
        }
    }
    fn encode_payload<W: pbrs::WireOut>(&self, out: &mut W) -> Result<(), Status> {
        if matches!(self.kind, ReplyKind::EncodeError) {
            return Err(Status::invalid_argument(
                "intentional response encode error",
            ));
        }
        if matches!(self.kind, ReplyKind::BudgetError) {
            // A valid repeated varint field, large enough to exceed the tiny
            // configured writer budget. No such DATA may reach the peer.
            for _ in 0..2048 {
                out.put_slice(&[0x08, 0x00]);
            }
        }
        Ok(())
    }
    fn decode_payload(_: Bytes) -> Result<Self, Status> {
        Ok(Self::empty())
    }
    fn empty() -> Self {
        Self {
            kind: ReplyKind::Plain,
            probe: None,
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        let Some(probe) = &self.probe else { return };
        probe.drops.fetch_add(1, Ordering::SeqCst);
        if matches!(self.kind, ReplyKind::AdvanceOnDrop) {
            let before = tokio::time::Instant::now();
            let mut advance = std::pin::pin!(tokio::time::advance(Duration::from_millis(21)));
            let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
            let _advanced = advance.as_mut().poll(&mut cx);
            assert_eq!(
                tokio::time::Instant::now(),
                before + Duration::from_millis(21),
                "destructor must actually consume the remaining budget"
            );
        }
    }
}

struct Replies {
    kind: ReplyKind,
    probe: Arc<Probe>,
}
impl Service for Replies {
    const NAME: &'static str = "accounting.Replies";
    async fn call(&self, rpc: Rpc) {
        if rpc.method() == "Recovery" {
            rpc.reject(Status::unimplemented("recovered"));
            return;
        }
        let kind = self.kind;
        let probe = Arc::clone(&self.probe);
        if rpc.method() == "ClientStream" {
            rpc.client_streaming(move |_: Request<Streaming<HelloRequest>>| async move {
                Ok::<_, Status>(Response::new(Reply {
                    kind,
                    probe: Some(probe),
                }))
            })
            .await;
        } else {
            rpc.unary(move |_: Request<HelloRequest>| async move {
                Ok::<_, Status>(Response::new(Reply {
                    kind,
                    probe: Some(probe),
                }))
            })
            .await;
        }
    }
}

async fn drive<T>(future: impl Future<Output = T>) -> T {
    let began = std::time::Instant::now();
    tokio::pin!(future);
    poll_fn(|cx| {
        let result = future.as_mut().poll(cx);
        if result.is_pending() {
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "raw accounting peer stalled"
            );
            cx.waker().wake_by_ref();
        }
        result
    })
    .await
}

struct Peer {
    send: h2::client::SendRequest<Bytes>,
    server: tokio::task::JoinHandle<()>,
    driver: tokio::task::JoinHandle<()>,
    probe: Arc<Probe>,
    budget: ByteBudgetTracker,
}

impl Peer {
    async fn new(kind: ReplyKind) -> Self {
        let probe = Arc::new(Probe::default());
        let service = Replies {
            kind,
            probe: Arc::clone(&probe),
        };
        let budget = ByteBudgetTracker::with_limit(64);
        let config = Server::new(service)
            .observer(Observer(Arc::clone(&probe)))
            .with_byte_budget_tracker(budget.clone())
            .max_concurrent_rpcs(1);
        let (client, server) = tokio::io::duplex(4096);
        let server = tokio::spawn(async move {
            config.serve_connection(server).await.unwrap();
        });
        let (send, connection) = drive(h2::client::handshake(client)).await.unwrap();
        let driver = tokio::spawn(async move {
            drop(connection.await);
        });
        Self {
            send,
            server,
            driver,
            probe,
            budget,
        }
    }

    async fn request(
        &mut self,
        method: &str,
        content_type: &str,
    ) -> http::Response<h2::RecvStream> {
        let mut request = http::Request::builder()
            .method("POST")
            .uri(format!("http://local/accounting.Replies/{method}"))
            .header("content-type", content_type)
            .header("te", "trailers");
        if method != "Recovery" {
            request = request.header("grpc-timeout", "20m");
        }
        self.send = drive(self.send.clone().ready()).await.unwrap();
        let (response, mut body) = self
            .send
            .send_request(request.body(()).unwrap(), false)
            .unwrap();
        #[cfg(feature = "grpc-web")]
        let payload = if content_type.contains("-text") {
            use base64::Engine as _;
            Bytes::from(base64::engine::general_purpose::STANDARD.encode([0; 5]))
        } else {
            Bytes::from_static(&[0; 5])
        };
        #[cfg(not(feature = "grpc-web"))]
        let payload = Bytes::from_static(&[0; 5]);
        body.send_data(payload, true).unwrap();
        drive(response)
            .await
            .expect("a valid response, not a reset")
    }

    async fn check(&mut self, method: &str, content_type: &str, expected: Code) {
        let response = self.request(method, content_type).await;
        let initial = response.headers().get("grpc-status").cloned();
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(data) = drive(body.data()).await {
            let data = data.expect("complete terminal response");
            bytes.extend_from_slice(&data);
            body.flow_control().release_capacity(data.len()).unwrap();
        }
        let trailers = drive(body.trailers()).await.unwrap();
        let wire = if content_type.starts_with("application/grpc-web") {
            #[cfg(feature = "grpc-web")]
            {
                web_status(content_type, &bytes)
            }
            #[cfg(not(feature = "grpc-web"))]
            {
                panic!("web case needs the feature")
            }
        } else {
            let value = initial
                .or_else(|| trailers.and_then(|map| map.get("grpc-status").cloned()))
                .unwrap();
            Code::from_i32(value.to_str().unwrap().parse().unwrap())
        };
        assert_eq!(
            wire, expected,
            "wire status establishes the actual committed result"
        );
        for _ in 0..128 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            self.probe.drops.load(Ordering::SeqCst),
            1,
            "observe the response destructor too"
        );
        assert_eq!(self.probe.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            *self.probe.ends.lock().unwrap(),
            [expected],
            "lifecycle must match the committed wire status"
        );
        assert!(self.budget.is_quiescent());
        let recovery = self.request("Recovery", "application/grpc").await;
        assert_eq!(
            recovery.headers().get("grpc-status").unwrap(),
            "12",
            "the sole RPC permit is released"
        );
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.driver.abort();
        self.server.abort();
    }
}

#[cfg(feature = "grpc-web")]
fn web_status(content_type: &str, bytes: &[u8]) -> Code {
    use base64::Engine as _;
    let decoded;
    let mut input = if content_type.contains("-text") {
        decoded = base64::engine::general_purpose::STANDARD
            .decode(bytes)
            .expect("complete base64 response");
        decoded.as_slice()
    } else {
        bytes
    };
    let mut status = None;
    while !input.is_empty() {
        let (header, rest) = input
            .split_at_checked(5)
            .expect("complete web frame prefix");
        let flag = *header.first().unwrap();
        let length = u32::from_be_bytes(header.get(1..5).unwrap().try_into().unwrap());
        let (payload, rest) = rest
            .split_at_checked(usize::try_from(length).unwrap())
            .expect("complete web frame payload");
        if flag == 0x80 {
            let text = std::str::from_utf8(payload).unwrap();
            let code = text
                .split("\r\n")
                .find_map(|line| line.strip_prefix("grpc-status: "))
                .unwrap();
            status = Some(Code::from_i32(code.parse().unwrap()));
        } else {
            assert_eq!(flag, 0);
        }
        input = rest;
    }
    status.expect("body-encoded web status")
}

#[tokio::test(start_paused = true)]
async fn unary_message_drop_after_ok_commit_cannot_change_lifecycle_status() {
    Peer::new(ReplyKind::AdvanceOnDrop)
        .await
        .check("Unary", "application/grpc", Code::Ok)
        .await;
}

#[tokio::test(start_paused = true)]
async fn client_stream_message_drop_after_ok_commit_cannot_change_lifecycle_status() {
    Peer::new(ReplyKind::AdvanceOnDrop)
        .await
        .check("ClientStream", "application/grpc", Code::Ok)
        .await;
}

#[tokio::test(start_paused = true)]
async fn response_encoding_errors_have_matching_wire_and_lifecycle_status() {
    for method in ["Unary", "ClientStream"] {
        Peer::new(ReplyKind::EncodeError)
            .await
            .check(method, "application/grpc", Code::InvalidArgument)
            .await;
    }
}

#[tokio::test(start_paused = true)]
async fn response_budget_errors_have_matching_wire_and_lifecycle_status() {
    for method in ["Unary", "ClientStream"] {
        Peer::new(ReplyKind::BudgetError)
            .await
            .check(method, "application/grpc", Code::ResourceExhausted)
            .await;
    }
}

#[cfg(feature = "grpc-web")]
#[tokio::test(start_paused = true)]
async fn web_unary_commit_and_writer_errors_have_matching_lifecycle_status() {
    for content_type in [
        "application/grpc-web",
        "application/grpc-web+proto",
        "application/grpc-web-text",
        "application/grpc-web-text+proto",
    ] {
        for (kind, expected) in [
            (ReplyKind::AdvanceOnDrop, Code::Ok),
            (ReplyKind::EncodeError, Code::InvalidArgument),
            (ReplyKind::BudgetError, Code::ResourceExhausted),
        ] {
            Peer::new(kind)
                .await
                .check("Unary", content_type, expected)
                .await;
        }
    }
}
