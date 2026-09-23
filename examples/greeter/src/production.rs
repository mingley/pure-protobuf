//! Bounded, loopback-only TLS and mTLS greeter recipes.
//!
//! `run_local_fixture_demo` reads the existing test certificates by path; it
//! does not embed, write to disk, print, or generate private keys. It is not a
//! production identity or authorization policy.

use crate::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::health::{
    service as health_service, HealthCheckRequest, HealthClient, HealthReporter, ServingStatus,
};
use pbrs_grpc::{
    Channel, ChannelConfig, ClientTls, Code, Identity, Request, Response, Router, ServerConfig,
    ServerTls, Status, Streaming,
};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::task::{JoinHandle, JoinSet};

const MAX_NAME_BYTES: usize = 128;
const MAX_STREAM_MESSAGES: usize = 4;
const MAX_DOWNLOAD_MESSAGES: usize = 3;
const GRACE: Duration = Duration::from_millis(250);

fn server_config() -> ServerConfig {
    ServerConfig::new()
        .max_concurrent_connections(4)
        .max_concurrent_rpcs(1)
        .max_concurrent_streams(4)
        .initial_connection_window_size(64 * 1024)
        .initial_stream_window_size(32 * 1024)
        .max_send_buffer_size(32 * 1024)
        .max_header_list_size(4 * 1024)
        .header_table_size(1024)
        .max_decoding_message_size(1024)
        .max_encoding_message_size(1024)
        .handshake_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(6))
        .max_connection_idle(Duration::from_secs(10))
        .max_connection_age_grace(GRACE)
}

fn client_config() -> ChannelConfig {
    ChannelConfig::new()
        .connections(1)
        .max_concurrent_rpcs(4)
        .stream_buffer(2)
        .initial_connection_window_size(64 * 1024)
        .initial_stream_window_size(32 * 1024)
        .max_send_buffer_size(32 * 1024)
        .max_header_list_size(4 * 1024)
        .header_table_size(1024)
        .max_decoding_message_size(1024)
        .max_encoding_message_size(1024)
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
}

#[derive(Clone, Default)]
struct Activity {
    uploads: Arc<AtomicUsize>,
    producers: Arc<AtomicUsize>,
}

struct Active(Arc<AtomicUsize>);

impl Active {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(counter))
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct BoundedGreeter {
    activity: Activity,
}

fn name_of(message: &HelloRequest) -> Result<&str, Status> {
    let name = message
        .name()
        .to_str()
        .map_err(|_| Status::invalid_argument("name must be valid UTF-8"))?;
    if name.is_empty() {
        return Err(Status::invalid_argument("name must not be empty"));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(Status::resource_exhausted("name exceeds 128 bytes"));
    }
    Ok(name)
}

fn hello(name: &str) -> HelloRequest {
    let mut message = HelloRequest::new();
    message.set_name(name);
    message
}

fn reply(text: String) -> HelloReply {
    let mut message = HelloReply::new();
    message.set_message(text);
    message
}

fn text_of(message: &HelloReply) -> Result<String, Status> {
    message
        .message()
        .to_str()
        .map(str::to_owned)
        .map_err(|_| Status::internal("reply was not valid UTF-8"))
}

impl Greeter for BoundedGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(reply(format!(
            "hello {}",
            name_of(request.get_ref())?
        ))))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let _active = Active::new(&self.activity.uploads);
        let mut inbound = request.into_inner();
        let mut names = Vec::with_capacity(MAX_STREAM_MESSAGES);
        while let Some(message) = inbound.message().await? {
            if names.len() == MAX_STREAM_MESSAGES {
                return Err(Status::resource_exhausted("too many streamed names"));
            }
            names.push(name_of(&message)?.to_owned());
        }
        if names.is_empty() {
            return Err(Status::invalid_argument(
                "stream must contain at least one name",
            ));
        }
        Ok(Response::new(reply(format!("hello {}", names.join(", ")))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = name_of(request.get_ref())?.to_owned();
        let (tx, stream) = Streaming::channel(2);
        let active = Active::new(&self.activity.producers);
        drop(tokio::spawn(async move {
            let _active = active;
            for i in 1..=MAX_DOWNLOAD_MESSAGES {
                if tx.send(reply(format!("hello {name} #{i}"))).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }

    async fn stream_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let cancelled = request.cancelled();
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(2);
        let active = Active::new(&self.activity.producers);
        drop(tokio::spawn(async move {
            let _active = active;
            tokio::pin!(cancelled);
            for n in 0..=MAX_STREAM_MESSAGES {
                let next = tokio::select! {
                    () = cancelled.as_mut() => break,
                    () = tx.closed() => break,
                    next = inbound.message() => next,
                };
                match next {
                    Ok(Some(_)) if n == MAX_STREAM_MESSAGES => {
                        tx.fail(Status::resource_exhausted("too many streamed names"))
                            .await;
                        break;
                    }
                    Ok(Some(message)) => match name_of(&message) {
                        Ok(name) => {
                            if tx.send(reply(format!("hello {name}"))).await.is_err() {
                                break;
                            }
                        }
                        Err(status) => {
                            tx.fail(status).await;
                            break;
                        }
                    },
                    Ok(None) => break,
                    Err(status) => {
                        tx.fail(status).await;
                        break;
                    }
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

/// A bounded loopback TLS server; the existing plaintext `serve()` is unchanged.
pub struct ProductionLive {
    pub addr: SocketAddr,
    pub reporter: HealthReporter,
    activity: Activity,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    server: Option<JoinHandle<Result<usize, Status>>>,
}

/// Observable resource state after the listener and connection tasks drain.
pub struct DrainReport {
    pub active_uploads: usize,
    pub active_producers: usize,
    pub allocated_bytes: usize,
}

impl ProductionLive {
    /// Number of admitted, unfinished client-streaming handlers.
    pub fn active_uploads(&self) -> usize {
        self.activity.uploads.load(Ordering::SeqCst)
    }

    /// Advertise NOT_SERVING before the load balancer's drain delay.
    pub fn mark_not_ready(&self) {
        self.reporter.shutdown();
    }

    /// Stop admitting connections, send GOAWAY, and wait for bounded drain.
    pub async fn shutdown(mut self) -> Result<DrainReport, Status> {
        self.mark_not_ready();
        if let Some(tx) = self.shutdown_tx.take() {
            crate::signal_shutdown(tx);
        }
        let server = self
            .server
            .as_mut()
            .ok_or_else(|| Status::internal("server task missing"))?;
        let allocated_bytes = tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .map_err(|_| {
                Status::new(
                    Code::DeadlineExceeded,
                    "TLS server drain exceeded 2 seconds",
                )
            })?
            .map_err(|e| Status::internal(format!("TLS server task failed: {e}")))??;
        self.server.take();
        tokio::time::timeout(Duration::from_secs(1), async {
            while self.activity.uploads.load(Ordering::SeqCst) != 0
                || self.activity.producers.load(Ordering::SeqCst) != 0
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| Status::internal("stream tasks remained after TLS drain"))?;
        let report = DrainReport {
            active_uploads: self.active_uploads(),
            active_producers: self.activity.producers.load(Ordering::SeqCst),
            allocated_bytes,
        };
        if report.allocated_bytes != 0 {
            return Err(Status::internal("transport bytes remained after TLS drain"));
        }
        Ok(report)
    }
}

impl Drop for ProductionLive {
    fn drop(&mut self) {
        self.mark_not_ready();
        if let Some(tx) = self.shutdown_tx.take() {
            crate::signal_shutdown(tx);
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

/// Start a TLS or mTLS service on loopback with a process-wide byte/RPC cap.
///
/// The caller constructs `ServerTls` with its own trust policy. Reflection is
/// deliberately not mounted; health and greeter share the RPC admission cap.
pub async fn serve_bounded_tls(tls: ServerTls) -> Result<ProductionLive, Status> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let addr = listener.local_addr()?;
    let (health, reporter) = health_service();
    reporter.set_serving(GreeterServer::<BoundedGreeter>::NAME);
    let activity = Activity::default();
    let router = Router::new()
        .config(server_config())
        .byte_budget(64 * 1024)
        .add_service(health)
        .add_service(GreeterServer::new(BoundedGreeter {
            activity: activity.clone(),
        }));
    let budget = router.byte_budget_tracker().clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        router
            .serve_tls_with_shutdown(
                listener,
                async {
                    drop(shutdown_rx.await);
                },
                tls,
            )
            .await?;
        Ok(budget.allocated())
    });
    Ok(ProductionLive {
        addr,
        reporter,
        activity,
        shutdown_tx: Some(shutdown_tx),
        server: Some(server),
    })
}

/// Dial the bounded service with verified TLS and a finite per-call deadline.
pub async fn connect_bounded_tls(
    addr: SocketAddr,
    tls: ClientTls,
) -> Result<GreeterClient, Status> {
    Ok(GreeterClient::new(
        Channel::connect_tls_with(addr, client_config(), tls).await?,
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureMode {
    Tls,
    Mtls,
}

/// A local-only end-to-end proof; no PEM bytes are embedded or logged.
pub struct DemoReport {
    pub auth_failure: Code,
    pub ready: ServingStatus,
    pub greeting: String,
    pub upload_reply: String,
    pub download_replies: Vec<String>,
    pub bidi_replies: Vec<String>,
    pub stream_overflow: Code,
    pub overload: Code,
    pub recovered: String,
    pub not_ready: ServingStatus,
    pub active_before_drain: usize,
    pub inflight_end: Code,
    pub drain_elapsed: Duration,
    pub drain: DrainReport,
}

fn fixture(dir: &Path, file: &str) -> Result<Vec<u8>, Status> {
    std::fs::read(dir.join(file))
        .map_err(|e| Status::invalid_argument(format!("local TLS fixture {file}: {e}")))
}

async fn health_status(client: &HealthClient) -> Result<ServingStatus, Status> {
    let mut check = HealthCheckRequest::new();
    check.set_service(GreeterServer::<BoundedGreeter>::NAME);
    let mut request = Request::new(check);
    request.set_timeout(Duration::from_secs(1));
    Ok(client.check(request).await?.get_ref().status())
}

async fn read_replies(
    mut stream: Streaming<HelloReply>,
    max: usize,
) -> Result<Vec<String>, Status> {
    let mut replies = Vec::with_capacity(max);
    while let Some(reply) = stream.message().await? {
        if replies.len() == max {
            return Err(Status::resource_exhausted("too many streamed replies"));
        }
        replies.push(text_of(&reply)?);
    }
    Ok(replies)
}

async fn wait_for_upload(live: &ProductionLive) -> Result<(), Status> {
    tokio::time::timeout(Duration::from_secs(2), async {
        while live.active_uploads() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| Status::new(Code::DeadlineExceeded, "upload was not admitted"))?;
    Ok(())
}

fn checked_error<T>(
    result: Result<T, Status>,
    expected: Code,
    context: &str,
) -> Result<Code, Status> {
    match result {
        Err(status) if status.code() == expected => Ok(status.code()),
        Err(status) => Err(Status::internal(format!("{context}: {status}"))),
        Ok(_) => Err(Status::internal(format!(
            "{context}: unexpectedly succeeded"
        ))),
    }
}

/// Exercise real TLS/mTLS, bounded streams, overload, readiness and drain.
///
/// `fixtures` must point at the repository's `pbrs-grpc/tests/tls_data`.
/// These public test keys are not suitable for a deployment.
pub async fn run_local_fixture_demo(
    fixtures: &Path,
    mode: FixtureMode,
) -> Result<DemoReport, Status> {
    let ca = fixture(fixtures, "ca.crt")?;
    let identity = Identity::from_pem(
        fixture(fixtures, "server.crt")?,
        fixture(fixtures, "server.key")?,
    )?;
    let server_tls = match mode {
        FixtureMode::Tls => ServerTls::new(identity)?,
        FixtureMode::Mtls => ServerTls::mtls(identity, &ca)?,
    };
    let client_tls = match mode {
        FixtureMode::Tls => ClientTls::ca("localhost", &ca)?,
        FixtureMode::Mtls => ClientTls::ca_mtls(
            "localhost",
            &ca,
            Identity::from_pem(
                fixture(fixtures, "client.crt")?,
                fixture(fixtures, "client.key")?,
            )?,
        )?,
    };
    let live = serve_bounded_tls(server_tls).await?;
    let addr = live.addr;
    let negative_tls = match mode {
        FixtureMode::Tls => ClientTls::ca("localhost", fixture(fixtures, "other.crt")?)?,
        FixtureMode::Mtls => ClientTls::ca("localhost", &ca)?,
    };
    let auth_failure = checked_error(
        Channel::connect_tls_with(addr, client_config(), negative_tls).await,
        Code::Unauthenticated,
        "untrusted CA or missing client identity",
    )?;
    let client = connect_bounded_tls(addr, client_tls.clone()).await?;
    let health = HealthClient::new(client.channel().clone());
    let ready = health_status(&health).await?;
    if ready != ServingStatus::Serving {
        return Err(Status::failed_precondition("greeter did not become ready"));
    }

    let mut unary_request = Request::new(hello("ada"));
    unary_request.set_timeout(Duration::from_secs(1));
    let greeting = text_of(client.say_hello(unary_request).await?.get_ref())?;
    if greeting != "hello ada" {
        return Err(Status::internal("unexpected TLS unary reply"));
    }

    let mut upload_request = Request::new(());
    upload_request.set_timeout(Duration::from_secs(2));
    let (sender, upload) = client.client_hello(upload_request);
    let send = async {
        sender.send(hello("ada")).await?;
        sender.send(hello("grace")).await?;
        sender.close();
        Ok::<(), Status>(())
    };
    let (sent, response) = tokio::join!(send, upload);
    sent?;
    let upload_reply = text_of(response?.get_ref())?;
    if upload_reply != "hello ada, grace" {
        return Err(Status::internal("unexpected TLS upload reply"));
    }

    let mut download_request = Request::new(hello("ada"));
    download_request.set_timeout(Duration::from_secs(2));
    let download_replies = read_replies(
        client.server_hello(download_request).await?.into_inner(),
        MAX_DOWNLOAD_MESSAGES,
    )
    .await?;
    if download_replies != ["hello ada #1", "hello ada #2", "hello ada #3"] {
        return Err(Status::internal("unexpected TLS download replies"));
    }

    let mut bidi_request = Request::new(());
    bidi_request.set_timeout(Duration::from_secs(2));
    let (sender, bidi) = client.stream_hello(bidi_request);
    sender.send(hello("ada")).await?;
    sender.send(hello("grace")).await?;
    sender.close();
    let bidi_replies = read_replies(bidi.await?.into_inner(), MAX_STREAM_MESSAGES).await?;
    if bidi_replies != ["hello ada", "hello grace"] {
        return Err(Status::internal("unexpected TLS bidi replies"));
    }

    let (sender, overflow) = client.client_hello(Request::new(()));
    let send = async {
        for _ in 0..=MAX_STREAM_MESSAGES {
            if sender.send(hello("extra")).await.is_err() {
                break;
            }
        }
        sender.close();
    };
    let (_, result) = tokio::join!(send, overflow);
    let stream_overflow = checked_error(result, Code::ResourceExhausted, "stream limit")?;

    let mut calls = JoinSet::new();
    let (sender, held) = client.client_hello(Request::new(()));
    calls.spawn(held);
    sender.send(hello("held")).await?;
    wait_for_upload(&live).await?;
    let overload = match client.say_hello(Request::new(hello("overload"))).await {
        Err(status)
            if status.code() == Code::ResourceExhausted
                && status.message().contains("too many concurrent RPCs") =>
        {
            status.code()
        }
        Err(status) => {
            return Err(Status::internal(format!(
                "expected server RPC admission rejection: {status}"
            )));
        }
        Ok(_) => return Err(Status::internal("server admitted an RPC above its limit")),
    };
    sender.close();
    let uploaded = calls
        .join_next()
        .await
        .ok_or_else(|| Status::internal("held upload missing"))?
        .map_err(|e| Status::internal(format!("held upload task failed: {e}")))??;
    if text_of(uploaded.get_ref())? != "hello held" {
        return Err(Status::internal("held upload did not complete"));
    }
    let recovered = text_of(
        client
            .say_hello(Request::new(hello("after")))
            .await?
            .get_ref(),
    )?;
    if recovered != "hello after" {
        return Err(Status::internal("server did not recover from overload"));
    }

    live.mark_not_ready();
    let not_ready = health_status(&health).await?;
    if not_ready != ServingStatus::NotServing {
        return Err(Status::internal("readiness did not change before drain"));
    }
    let (sender, held) = client.client_hello(Request::new(()));
    calls.spawn(held);
    sender.send(hello("held on drain")).await?;
    wait_for_upload(&live).await?;
    let active_before_drain = live.active_uploads();
    let started = Instant::now();
    let drain = live.shutdown().await?;
    let drain_elapsed = started.elapsed();
    let result = tokio::time::timeout(Duration::from_secs(1), calls.join_next())
        .await
        .map_err(|_| Status::new(Code::DeadlineExceeded, "drained RPC did not finish"))?
        .ok_or_else(|| Status::internal("drained upload missing"))?
        .map_err(|e| Status::internal(format!("drained upload task failed: {e}")))?;
    sender.close();
    let inflight_end = match result {
        Err(status) if matches!(status.code(), Code::Cancelled | Code::Unavailable) => {
            status.code()
        }
        Err(status) => {
            return Err(Status::internal(format!(
                "unexpected drain status: {status}"
            )))
        }
        Ok(_) => return Err(Status::internal("unending upload succeeded after drain")),
    };
    checked_error(
        Channel::connect_tls_with(addr, client_config(), client_tls).await,
        Code::Unavailable,
        "post-drain dial",
    )?;
    Ok(DemoReport {
        auth_failure,
        ready,
        greeting,
        upload_reply,
        download_replies,
        bidi_replies,
        stream_overflow,
        overload,
        recovered,
        not_ready,
        active_before_drain,
        inflight_end,
        drain_elapsed,
        drain,
    })
}
