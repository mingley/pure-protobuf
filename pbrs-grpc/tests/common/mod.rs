//! Shared helpers: spawn a Greeter, connect a client, build messages.

#![allow(
    dead_code,
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    unreachable_pub,
    missing_docs,
    reason = "integration test helpers"
)]

use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
use pbrs_grpc::{Request, Response, ServerConfig, Status, Streaming};
use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Loopback TCP socket that is bound but not listening.
///
/// Occupies the port so another test cannot steal it: Tokio's
/// [`TcpListener::bind`] sets `SO_REUSEADDR`, which can reuse a port after
/// `drop(listener)` while wait-for-ready is still connecting. Connects get
/// `ECONNREFUSED` until [`Self::listen`].
pub struct ReservedLoopback {
    #[cfg(not(target_os = "macos"))]
    socket: socket2::Socket,
    addr: SocketAddr,
}

/// Bind `127.0.0.1:0` without `listen` and without `SO_REUSEADDR`.
pub fn reserve_loopback() -> ReservedLoopback {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .expect("socket");
    socket
        .bind(&socket2::SockAddr::from(SocketAddr::from((
            [127, 0, 0, 1],
            0,
        ))))
        .expect("bind");
    socket.set_nonblocking(true).expect("nonblocking");
    let addr = socket
        .local_addr()
        .expect("local_addr")
        .as_socket()
        .expect("tcp");
    #[cfg(target_os = "macos")]
    {
        // On macOS (Darwin BSD socket stack), a socket in BOUND state without LISTEN
        // does not send RST on incoming SYN; instead it hangs SYN packets in limbo,
        // causing connects to hang for 75s rather than failing fast with ECONNREFUSED.
        // Dropping the socket closes it, producing immediate ECONNREFUSED until listen().
        drop(socket);
        ReservedLoopback { addr }
    }
    #[cfg(not(target_os = "macos"))]
    ReservedLoopback { socket, addr }
}

impl ReservedLoopback {
    /// Address wait-for-ready clients connect to while this socket holds the port.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Start accepting so the wait-for-ready client can complete.
    pub fn listen(self) -> TcpListener {
        #[cfg(target_os = "macos")]
        {
            let std_listener = std::net::TcpListener::bind(self.addr).expect("listen");
            std_listener.set_nonblocking(true).expect("nonblocking");
            TcpListener::from_std(std_listener).expect("tokio listener")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.socket.listen(1024).expect("listen");
            TcpListener::from_std(self.socket.into()).expect("tokio listener")
        }
    }
}

/// Keeps the server task alive for the duration of a test and aborts it after.
pub struct ServerGuard(pub JoinHandle<()>);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Retry `attempt` while a rebound listener comes up after a slot death.
pub async fn until_ok<T, F, Fut>(label: &'static str, mut attempt: F) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Status>>,
{
    let mut last = None;
    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_secs(2), attempt()).await {
            Ok(Ok(value)) => return value,
            Ok(Err(status)) => last = Some(status),
            Err(_) => last = Some(Status::unavailable(format!("{label} timed out"))),
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{label}: {last:?}");
}

/// Bind an ephemeral port and serve `service` on it.
pub async fn serve<G: Greeter>(
    service: G,
    config: ServerConfig,
) -> Result<(SocketAddr, ServerGuard), Status> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| Status::unavailable(e.to_string()))?;
    let handle = tokio::spawn(async move {
        GreeterServer::new(service)
            .config(config)
            .serve_listener(listener)
            .await
            .ok();
    });
    Ok((addr, ServerGuard(handle)))
}

/// Serve the echo Greeter used by the hostile-peer tests.
pub async fn spawn_greeter_server(config: ServerConfig) -> (SocketAddr, ServerGuard) {
    serve(Echo, config).await.expect("spawn greeter")
}

/// Connect a client, retrying while the listener comes up.
pub async fn greeter_client(addr: SocketAddr) -> GreeterClient {
    let mut last = Status::unavailable("connect");
    for _ in 0..80 {
        match GreeterClient::connect(addr).await {
            Ok(client) => return client,
            Err(e) => {
                last = e;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    panic!("could not connect to {addr}: {last}");
}

/// Serve `service` and return a connected client.
pub async fn spawn_greeter<G: Greeter>(
    service: G,
) -> Result<(SocketAddr, GreeterClient, ServerGuard), Status> {
    let (addr, guard) = serve(service, ServerConfig::default()).await?;
    Ok((addr, greeter_client(addr).await, guard))
}

/// Bind `addr`, retrying through `TIME_WAIT`, and serve `service` on it.
pub async fn serve_at<G: Greeter>(
    addr: SocketAddr,
    service: G,
    config: ServerConfig,
) -> Result<ServerGuard, Status> {
    let mut last = Status::unavailable("bind");
    for _ in 0..100 {
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                let handle = tokio::spawn(async move {
                    GreeterServer::new(service)
                        .config(config)
                        .serve_listener(listener)
                        .await
                        .ok();
                });
                return Ok(ServerGuard(handle));
            }
            Err(e) => {
                last = Status::unavailable(e.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(last)
}

/// Serve `service` on an already-bound listener.
pub fn serve_on<G: Greeter>(
    listener: TcpListener,
    service: G,
    config: ServerConfig,
) -> ServerGuard {
    let handle = tokio::spawn(async move {
        GreeterServer::new(service)
            .config(config)
            .serve_listener(listener)
            .await
            .ok();
    });
    ServerGuard(handle)
}

pub fn name_of(reply: &HelloReply) -> String {
    reply.message().to_str().unwrap_or("").to_string()
}

pub fn req(name: &str) -> HelloRequest {
    let mut r = HelloRequest::new();
    r.set_name(name);
    r
}

pub fn reply(message: impl Into<String>) -> HelloReply {
    let mut r = HelloReply::new();
    r.set_message(message.into());
    r
}

/// The reference echo Greeter: unary echoes the name, client-stream joins with
/// commas, server-stream splits on commas, bidi echoes each request.
pub struct Echo;

impl Greeter for Echo {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        Ok(Response::new(reply(name_of_request(request.get_ref()))))
    }

    async fn client_hello(
        &self,
        request: Request<Streaming<HelloRequest>>,
    ) -> Result<Response<HelloReply>, Status> {
        let mut stream = request.into_inner();
        let mut names = Vec::new();
        while let Some(msg) = stream.message().await? {
            names.push(name_of_request(&msg));
        }
        Ok(Response::new(reply(names.join(","))))
    }

    async fn server_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<Streaming<HelloReply>>, Status> {
        let name = name_of_request(request.get_ref());
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            for part in name.split(',') {
                if tx.send(reply(part.to_string())).await.is_err() {
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
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            loop {
                match inbound.message().await {
                    Ok(Some(msg)) => {
                        if tx.send(reply(name_of_request(&msg))).await.is_err() {
                            break;
                        }
                    }
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

pub fn name_of_request(request: &HelloRequest) -> String {
    request.name().to_str().unwrap_or("").to_string()
}

pub mod lifecycle {
    use super::{greeter_client, name_of, name_of_request, reply, req};
    use pbrs_grpc::hello::{Greeter, GreeterClient, GreeterServer, HelloReply, HelloRequest};
    use pbrs_grpc::{
        CallHandle, Channel, ChannelConfig, ClientTls, Code, Identity, Request, Response,
        ResponseParts, ServerConfig, ServerTls, Status, Streaming,
    };
    use std::io;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
    use tokio::sync::oneshot;

    /// Byte-forwarding TCP proxy that applies [`ProxyState`] faults.
    ///
    /// Forwards handshake and record bytes opaquely, so TLS/mTLS sessions pass
    /// through untouched while `TcpReset`/`TcpDisconnect` faults still break
    /// the connection at the byte level. Raw RST/GOAWAY injection stays
    /// plaintext-only; encrypted arms map those faults to call cancel and
    /// server shutdown (see `fire_fault`).
    fn spawn_tcp_byte_proxy(
        proxy_listener: TcpListener,
        server_addr: SocketAddr,
        proxy_state: Arc<Mutex<ProxyState>>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Ok((client_tcp, _)) = proxy_listener.accept().await {
                let p_state_clone = proxy_state.clone();
                tokio::spawn(async move {
                    if let Ok(server_tcp) = TcpStream::connect(server_addr).await {
                        let wrapped_server =
                            FaultInjectingStream::new(server_tcp, p_state_clone.clone(), true);
                        let wrapped_client =
                            FaultInjectingStream::new(client_tcp, p_state_clone, false);
                        let (mut cr, mut cw) = tokio::io::split(wrapped_client);
                        let (mut sr, mut sw) = tokio::io::split(wrapped_server);
                        tokio::select! {
                            _ = tokio::io::copy(&mut cr, &mut sw) => {}
                            _ = tokio::io::copy(&mut sr, &mut cw) => {}
                        }
                    }
                });
            }
        })
    }

    static UNIX_SOCK_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Unique Unix socket path for one lifecycle scenario.
    fn lifecycle_unix_sock(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "pbrs-lc-{prefix}-{}-{}.sock",
            std::process::id(),
            UNIX_SOCK_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    const CA: &str = include_str!("../tls_data/ca.crt");
    const SERVER_CERT: &str = include_str!("../tls_data/server.crt");
    const SERVER_KEY: &str = include_str!("../tls_data/server.key");
    const CLIENT_CERT: &str = include_str!("../tls_data/client.crt");
    const CLIENT_KEY: &str = include_str!("../tls_data/client.key");

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum CallShape {
        Unary,
        ClientStreaming,
        ServerStreaming,
        Bidi,
    }

    /// Advertised transports with explicit lifecycle cells (RT-04).
    ///
    /// Every variant has its own PR scenarios and its own 1000-seed
    /// qualification schedule; no transport is inferred from another.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum TransportKind {
        FromIo,
        Tcp,
        Tls,
        Mtls,
        Uds,
    }

    /// All advertised transports, in qualification order.
    pub const ALL_TRANSPORTS: [TransportKind; 5] = [
        TransportKind::FromIo,
        TransportKind::Tcp,
        TransportKind::Tls,
        TransportKind::Mtls,
        TransportKind::Uds,
    ];

    /// Qualification cycles recorded per advertised transport (RT-04).
    pub const QUALIFICATION_CYCLES_PER_TRANSPORT: usize = 1000;

    /// Deterministic master seed per advertised transport. Cycle `i` for a
    /// transport uses `master.wrapping_add(i * STRIDE)`, so every recorded
    /// cycle is reproducible from `(transport, index)`.
    pub const QUALIFICATION_MASTER_SEEDS: [(TransportKind, u64); 5] = [
        (TransportKind::FromIo, 0x5e3d_f00d_cafe_babe),
        (TransportKind::Tcp, 0x1f2b_3c4d_5e6f_7081),
        (TransportKind::Tls, 0x8bad_f00d_d15e_a5ed),
        (TransportKind::Mtls, 0xc0ff_ee11_5eed_beef),
        (TransportKind::Uds, 0xdec0_ded0_badc_0de0),
    ];

    const QUALIFICATION_SEED_STRIDE: u64 = 0x9e37_79b9_7f4a_7c15;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum LifecycleBoundary {
        Queued,
        HeadersSent,
        BodyStarted,
        ResponseHeadersReceived,
        ResponseBodyReceived,
        TrailersReceived,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum RstReason {
        RefusedStream,
        Cancel,
        InternalError,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum FaultKind {
        TcpReset,
        TcpDisconnect,
        RstStream(RstReason),
        Goaway,
        FutureDropClient,
        FutureDropServer,
        StreamHalfClose,
        Cancel,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct LifecycleScenario {
        pub shape: CallShape,
        pub transport: TransportKind,
        pub boundary: LifecycleBoundary,
        pub fault: FaultKind,
        pub seed: u64,
    }

    #[derive(Debug, Clone)]
    pub struct SeededRng {
        state: u64,
    }

    impl SeededRng {
        pub fn new(seed: u64) -> Self {
            Self {
                state: if seed == 0 {
                    0x517c_c1b7_2722_0a95
                } else {
                    seed
                },
            }
        }

        pub fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }

        pub fn next_range(&mut self, max: usize) -> usize {
            (self.next_u64() % (max as u64)) as usize
        }
    }

    impl LifecycleScenario {
        pub fn from_seed(seed: u64) -> Self {
            let mut rng = SeededRng::new(seed);
            let shapes = [
                CallShape::Unary,
                CallShape::ClientStreaming,
                CallShape::ServerStreaming,
                CallShape::Bidi,
            ];
            let shape = shapes[rng.next_range(shapes.len())];

            let transport = ALL_TRANSPORTS[rng.next_range(ALL_TRANSPORTS.len())];

            let boundaries = [
                LifecycleBoundary::Queued,
                LifecycleBoundary::HeadersSent,
                LifecycleBoundary::BodyStarted,
                LifecycleBoundary::ResponseHeadersReceived,
                LifecycleBoundary::ResponseBodyReceived,
                LifecycleBoundary::TrailersReceived,
            ];
            let boundary = boundaries[rng.next_range(boundaries.len())];

            let faults = [
                FaultKind::TcpReset,
                FaultKind::TcpDisconnect,
                FaultKind::RstStream(RstReason::RefusedStream),
                FaultKind::RstStream(RstReason::Cancel),
                FaultKind::RstStream(RstReason::InternalError),
                FaultKind::Goaway,
                FaultKind::FutureDropClient,
                FaultKind::FutureDropServer,
                FaultKind::StreamHalfClose,
                FaultKind::Cancel,
            ];
            let fault = faults[rng.next_range(faults.len())];

            Self {
                shape,
                transport,
                boundary,
                fault,
                seed,
            }
        }

        /// Deterministic scenario for `seed` pinned to `transport`.
        ///
        /// Shape, boundary, and fault are drawn from the seed; only the
        /// transport is fixed, so per-transport qualification schedules vary
        /// the call matrix while holding the transport cell constant.
        pub fn from_seed_for_transport(seed: u64, transport: TransportKind) -> Self {
            let mut scenario = Self::from_seed(seed);
            scenario.transport = transport;
            scenario
        }

        /// Recorded qualification schedule for one transport (RT-04).
        ///
        /// Returns exactly [`QUALIFICATION_CYCLES_PER_TRANSPORT`] scenarios
        /// derived from that transport's master seed, so the full 1000-cycle
        /// qualification run is reproducible from `(transport, index)`.
        pub fn qualification_schedule(transport: TransportKind) -> Vec<Self> {
            let master = QUALIFICATION_MASTER_SEEDS
                .iter()
                .find(|(t, _)| *t == transport)
                .map(|(_, seed)| *seed)
                .expect("master seed for every advertised transport");
            (0..QUALIFICATION_CYCLES_PER_TRANSPORT)
                .map(|cycle| {
                    let seed =
                        master.wrapping_add((cycle as u64).wrapping_mul(QUALIFICATION_SEED_STRIDE));
                    Self::from_seed_for_transport(seed, transport)
                })
                .collect()
        }
    }

    pub struct TaskGuard {
        counter: Arc<AtomicUsize>,
    }

    impl TaskGuard {
        pub fn new(counter: &Arc<AtomicUsize>) -> Self {
            counter.fetch_add(1, Ordering::SeqCst);
            Self {
                counter: counter.clone(),
            }
        }
    }

    impl Drop for TaskGuard {
        fn drop(&mut self) {
            self.counter.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    pub struct ProxyState {
        pub is_reset: bool,
        pub is_disconnected: bool,
        pub active_stream_id: u32,
        pub headers_observed: bool,
        pub deferred_rst: Option<u32>,
        pub pending_client_injections: Vec<u8>,
        pub pending_server_injections: Vec<u8>,
        pub client_read_waker: Option<std::task::Waker>,
        pub server_read_waker: Option<std::task::Waker>,
    }

    impl ProxyState {
        pub fn new() -> Self {
            Self {
                is_reset: false,
                is_disconnected: false,
                active_stream_id: 1,
                headers_observed: false,
                deferred_rst: None,
                pending_client_injections: Vec::new(),
                pending_server_injections: Vec::new(),
                client_read_waker: None,
                server_read_waker: None,
            }
        }

        fn rst_frame(stream_id: u32, reason: u32) -> Vec<u8> {
            let mut frame = Vec::with_capacity(13);
            frame.extend_from_slice(&[0x00, 0x00, 0x04]);
            frame.push(0x03);
            frame.push(0x00);
            frame.extend_from_slice(&(stream_id & 0x7FFFFFFF).to_be_bytes());
            frame.extend_from_slice(&reason.to_be_bytes());
            frame
        }

        pub fn inject_rst(&mut self, reason: u32) {
            if self.headers_observed {
                let frame = Self::rst_frame(self.active_stream_id, reason);
                self.pending_client_injections.extend_from_slice(&frame);
                self.pending_server_injections.extend_from_slice(&frame);
                if let Some(waker) = self.client_read_waker.take() {
                    waker.wake();
                }
                if let Some(waker) = self.server_read_waker.take() {
                    waker.wake();
                }
            } else {
                // No stream is open yet (e.g. fault at `Queued`): hold the
                // RST until the first HEADERS is forwarded, otherwise both
                // peers would see a reset for an idle stream and ignore it.
                self.deferred_rst = Some(reason);
            }
        }

        /// Deliver a deferred RST once the first HEADERS has been observed.
        ///
        /// Both copies are injected, matching immediate injection: the client
        /// has the stream open and resets it, while the server-side copy
        /// (read before the forwarded HEADERS) ends the server handler so no
        /// task or permit leaks.
        pub fn deliver_deferred_rst(&mut self, stream_id: u32) {
            if let Some(reason) = self.deferred_rst.take() {
                let frame = Self::rst_frame(stream_id, reason);
                self.pending_client_injections.extend_from_slice(&frame);
                self.pending_server_injections.extend_from_slice(&frame);
                if let Some(waker) = self.client_read_waker.take() {
                    waker.wake();
                }
                if let Some(waker) = self.server_read_waker.take() {
                    waker.wake();
                }
            }
        }

        pub fn inject_goaway(&mut self, last_stream_id: u32, error_code: u32) {
            let mut frame = Vec::with_capacity(17);
            frame.extend_from_slice(&[0x00, 0x00, 0x08]);
            frame.push(0x07);
            frame.push(0x00);
            frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            frame.extend_from_slice(&(last_stream_id & 0x7FFFFFFF).to_be_bytes());
            frame.extend_from_slice(&error_code.to_be_bytes());
            self.pending_client_injections.extend_from_slice(&frame);
            self.pending_server_injections.extend_from_slice(&frame);
            if let Some(waker) = self.client_read_waker.take() {
                waker.wake();
            }
            if let Some(waker) = self.server_read_waker.take() {
                waker.wake();
            }
        }

        pub fn trigger_reset(&mut self) {
            self.is_reset = true;
            if let Some(waker) = self.client_read_waker.take() {
                waker.wake();
            }
            if let Some(waker) = self.server_read_waker.take() {
                waker.wake();
            }
        }

        pub fn trigger_disconnect(&mut self) {
            self.is_disconnected = true;
            if let Some(waker) = self.client_read_waker.take() {
                waker.wake();
            }
            if let Some(waker) = self.server_read_waker.take() {
                waker.wake();
            }
        }
    }

    pub struct FaultInjectingStream<S> {
        inner: S,
        state: Arc<Mutex<ProxyState>>,
        injected_cursor: usize,
        injected_buf: Vec<u8>,
        is_client_side: bool,
        sniff_buf: Vec<u8>,
        sniff_skip: usize,
    }

    impl<S> FaultInjectingStream<S> {
        pub fn new(inner: S, state: Arc<Mutex<ProxyState>>, is_client_side: bool) -> Self {
            Self {
                inner,
                state,
                injected_cursor: 0,
                injected_buf: Vec::new(),
                is_client_side,
                sniff_buf: Vec::new(),
                // Every h2 client opens with the 24-byte connection preface,
                // which is not a frame and must not be frame-parsed.
                sniff_skip: 24,
            }
        }
    }

    /// Record every HEADERS frame in a client-to-server byte chunk.
    ///
    /// Chunks are arbitrary TCP segmentations: a HEADERS frame may be
    /// coalesced behind a SETTINGS ack or split across writes, so an
    /// offset-zero check misses it nondeterministically. `sniff_buf` carries
    /// the incomplete tail across calls and parses complete frames from the
    /// head, keeping deferred-RST delivery deterministic. TLS ciphertext
    /// never parses as valid frames; oversize garbage is discarded cheaply
    /// (deferred RST is plaintext-only, so nothing is lost there).
    fn sniff_headers_frames(
        sniff_buf: &mut Vec<u8>,
        sniff_skip: &mut usize,
        chunk: &[u8],
        state: &Mutex<ProxyState>,
    ) {
        const FRAME_HEADER: usize = 9;
        const MAX_FRAME: usize = 16 * 1024 * 1024;
        sniff_buf.extend_from_slice(chunk);
        if *sniff_skip > 0 {
            let drop = (*sniff_skip).min(sniff_buf.len());
            sniff_buf.drain(..drop);
            *sniff_skip -= drop;
            if *sniff_skip > 0 {
                return;
            }
        }
        loop {
            if sniff_buf.len() < FRAME_HEADER {
                return;
            }
            let len = ((sniff_buf[0] as usize) << 16)
                | ((sniff_buf[1] as usize) << 8)
                | sniff_buf[2] as usize;
            if len > MAX_FRAME {
                sniff_buf.clear();
                return;
            }
            let total = FRAME_HEADER + len;
            if sniff_buf.len() < total {
                return;
            }
            if sniff_buf[3] == 0x01 {
                let stream_id = u32::from_be_bytes([
                    sniff_buf[5] & 0x7f,
                    sniff_buf[6],
                    sniff_buf[7],
                    sniff_buf[8],
                ]);
                if stream_id > 0 {
                    let mut st = state.lock().unwrap();
                    st.active_stream_id = stream_id;
                    st.headers_observed = true;
                    st.deliver_deferred_rst(stream_id);
                }
            }
            sniff_buf.drain(..total);
        }
    }

    impl<S: Unpin> Unpin for FaultInjectingStream<S> {}

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for FaultInjectingStream<S> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let this = self.get_mut();
            let mut pending = Vec::new();
            {
                let mut st = this.state.lock().unwrap();
                if st.is_reset {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionReset,
                        "connection reset by peer",
                    )));
                }
                if st.is_disconnected {
                    return Poll::Ready(Ok(()));
                }

                if this.is_client_side {
                    if !st.pending_client_injections.is_empty() {
                        pending = std::mem::take(&mut st.pending_client_injections);
                    }
                    st.client_read_waker = Some(cx.waker().clone());
                } else {
                    if !st.pending_server_injections.is_empty() {
                        pending = std::mem::take(&mut st.pending_server_injections);
                    }
                    st.server_read_waker = Some(cx.waker().clone());
                }
            }

            if !pending.is_empty() {
                this.injected_buf.extend_from_slice(&pending);
            }

            if this.injected_cursor < this.injected_buf.len() {
                let remaining = &this.injected_buf[this.injected_cursor..];
                let to_write = std::cmp::min(remaining.len(), buf.remaining());
                buf.put_slice(&remaining[..to_write]);
                this.injected_cursor += to_write;
                if this.injected_cursor >= this.injected_buf.len() {
                    this.injected_buf.clear();
                    this.injected_cursor = 0;
                }
                return Poll::Ready(Ok(()));
            }

            Pin::new(&mut this.inner).poll_read(cx, buf)
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for FaultInjectingStream<S> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let this = self.get_mut();
            {
                let st = this.state.lock().unwrap();
                if st.is_reset {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::ConnectionReset,
                        "connection reset by peer",
                    )));
                }
                if st.is_disconnected {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "broken pipe",
                    )));
                }

                if this.is_client_side {
                    drop(st);
                    sniff_headers_frames(
                        &mut this.sniff_buf,
                        &mut this.sniff_skip,
                        buf,
                        &this.state,
                    );
                }
            }

            Pin::new(&mut this.inner).poll_write(cx, buf)
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_flush(cx)
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
        }
    }

    pub struct LifecycleCoordinator {
        pub scenario: LifecycleScenario,
        pub has_faulted: Arc<AtomicBool>,
        pub reached_boundaries: Arc<Mutex<Vec<LifecycleBoundary>>>,
        pub proxy_state: Arc<Mutex<ProxyState>>,
        pub cancel_handle: Arc<Mutex<Option<CallHandle>>>,
        pub client_drop_trigger: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        pub server_abort_trigger: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        pub stream_close_trigger: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        pub server_shutdown_trigger: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        pub client_active_tasks: Arc<AtomicUsize>,
        pub server_active_tasks: Arc<AtomicUsize>,
        pub client_sent: Arc<Mutex<Vec<String>>>,
        pub server_received: Arc<Mutex<Vec<String>>>,
        pub server_sent: Arc<Mutex<Vec<String>>>,
        pub client_received: Arc<Mutex<Vec<String>>>,
    }

    impl LifecycleCoordinator {
        pub fn new(scenario: LifecycleScenario, proxy_state: Arc<Mutex<ProxyState>>) -> Self {
            Self {
                scenario,
                has_faulted: Arc::new(AtomicBool::new(false)),
                reached_boundaries: Arc::new(Mutex::new(Vec::new())),
                proxy_state,
                cancel_handle: Arc::new(Mutex::new(None)),
                client_drop_trigger: Arc::new(Mutex::new(None)),
                server_abort_trigger: Arc::new(Mutex::new(None)),
                stream_close_trigger: Arc::new(Mutex::new(None)),
                server_shutdown_trigger: Arc::new(Mutex::new(None)),
                client_active_tasks: Arc::new(AtomicUsize::new(0)),
                server_active_tasks: Arc::new(AtomicUsize::new(0)),
                client_sent: Arc::new(Mutex::new(Vec::new())),
                server_received: Arc::new(Mutex::new(Vec::new())),
                server_sent: Arc::new(Mutex::new(Vec::new())),
                client_received: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn record_client_sent(&self, msg: &str) {
            self.client_sent.lock().unwrap().push(msg.to_string());
        }

        pub fn record_server_received(&self, msg: &str) {
            self.server_received.lock().unwrap().push(msg.to_string());
        }

        pub fn record_server_sent(&self, msg: &str) {
            self.server_sent.lock().unwrap().push(msg.to_string());
        }

        pub fn record_client_received(&self, msg: &str) {
            self.client_received.lock().unwrap().push(msg.to_string());
        }

        pub fn set_cancel_handle(&self, handle: CallHandle) {
            *self.cancel_handle.lock().unwrap() = Some(handle);
        }

        pub fn reach_sync(&self, boundary: LifecycleBoundary) {
            self.reached_boundaries.lock().unwrap().push(boundary);
            if boundary == self.scenario.boundary && !self.has_faulted.swap(true, Ordering::SeqCst)
            {
                self.fire_fault();
            }
        }

        pub async fn reach(&self, boundary: LifecycleBoundary) {
            self.reach_sync(boundary);
            tokio::task::yield_now().await;
        }

        pub async fn check_server_abort(&self) -> bool {
            self.scenario.fault == FaultKind::FutureDropServer
                && self.has_faulted.load(Ordering::SeqCst)
        }

        fn fire_fault(&self) {
            match self.scenario.fault {
                FaultKind::Cancel => {
                    if let Some(handle) = self.cancel_handle.lock().unwrap().as_ref() {
                        handle.cancel();
                    }
                }
                FaultKind::FutureDropClient => {
                    if let Some(tx) = self.client_drop_trigger.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
                FaultKind::FutureDropServer => {
                    if let Some(tx) = self.server_abort_trigger.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
                FaultKind::StreamHalfClose => {
                    if let Some(tx) = self.stream_close_trigger.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
                FaultKind::TcpReset => {
                    self.proxy_state.lock().unwrap().trigger_reset();
                }
                FaultKind::TcpDisconnect => {
                    self.proxy_state.lock().unwrap().trigger_disconnect();
                }
                FaultKind::RstStream(reason) => {
                    // TLS and mTLS wires are encrypted, so raw frames cannot be
                    // injected; cancelling the call observes the same terminal path.
                    if matches!(
                        self.scenario.transport,
                        TransportKind::Tls | TransportKind::Mtls
                    ) {
                        if let Some(handle) = self.cancel_handle.lock().unwrap().as_ref() {
                            handle.cancel();
                        }
                    } else {
                        let code = match reason {
                            RstReason::RefusedStream => 7,
                            RstReason::Cancel => 8,
                            RstReason::InternalError => 2,
                        };
                        self.proxy_state.lock().unwrap().inject_rst(code);
                    }
                }
                FaultKind::Goaway => {
                    if matches!(
                        self.scenario.transport,
                        TransportKind::Tls | TransportKind::Mtls
                    ) {
                        if let Some(tx) = self.server_shutdown_trigger.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                    } else {
                        self.proxy_state.lock().unwrap().inject_goaway(0, 0);
                        if let Some(tx) = self.server_shutdown_trigger.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                    }
                }
            }
        }

        pub fn assert_message_ordering(&self) {
            let s_rec = self.server_received.lock().unwrap();
            if self.scenario.shape == CallShape::Unary
                || self.scenario.shape == CallShape::ServerStreaming
            {
                for (i, msg) in s_rec.iter().enumerate() {
                    assert_eq!(
                        msg, "msg-0",
                        "Server received unexpected message at index {i}"
                    );
                }
            } else {
                for (i, msg) in s_rec.iter().enumerate() {
                    assert_eq!(
                        msg,
                        &format!("msg-{i}"),
                        "Server received message out of order at index {i}"
                    );
                }
            }
            let c_rec = self.client_received.lock().unwrap();
            if self.scenario.shape == CallShape::Unary
                || self.scenario.shape == CallShape::ClientStreaming
            {
                for (i, msg) in c_rec.iter().enumerate() {
                    assert_eq!(
                        msg, "reply-0",
                        "Client received unexpected reply at index {i}"
                    );
                }
            } else {
                for (i, msg) in c_rec.iter().enumerate() {
                    assert_eq!(
                        msg,
                        &format!("reply-{i}"),
                        "Client received message out of order at index {i}"
                    );
                }
            }
        }

        pub fn assert_status(&self, result: &Result<(), Status>) {
            if !self.has_faulted.load(Ordering::SeqCst) {
                assert!(
                    result.is_ok(),
                    "Call failed unexpectedly without fault: {result:?}"
                );
                return;
            }
            match self.scenario.fault {
                FaultKind::Cancel => {
                    assert!(
                        result.as_ref().is_err_and(|s| s.code() == Code::Cancelled),
                        "Expected CANCELLED status, got {result:?}"
                    );
                }
                FaultKind::FutureDropClient => {
                    assert!(
                        result.as_ref().is_err_and(|s| s.code() == Code::Cancelled),
                        "Expected CANCELLED status on client drop, got {result:?}"
                    );
                }
                FaultKind::FutureDropServer => {
                    assert!(
                        result.as_ref().is_err_and(|s| matches!(s.code(), Code::Unavailable | Code::Internal | Code::Cancelled)),
                        "Expected UNAVAILABLE, INTERNAL, or CANCELLED on server drop, got {result:?}"
                    );
                }
                FaultKind::StreamHalfClose => {
                    assert!(
                        result.is_ok(),
                        "Expected OK status on stream half-close, got {result:?}"
                    );
                }
                FaultKind::RstStream(RstReason::RefusedStream) => {
                    assert!(
                        result.is_ok() || result.as_ref().is_err_and(|s| matches!(s.code(), Code::Unavailable | Code::Cancelled)),
                        "Expected OK (transparent retry), UNAVAILABLE or CANCELLED on REFUSED_STREAM, got {result:?}"
                    );
                }
                FaultKind::RstStream(RstReason::Cancel) => {
                    assert!(
                        result.as_ref().is_err_and(|s| matches!(
                            s.code(),
                            Code::Cancelled | Code::Unavailable
                        )),
                        "Expected CANCELLED or UNAVAILABLE on RST CANCEL, got {result:?}"
                    );
                }
                FaultKind::RstStream(RstReason::InternalError) => {
                    assert!(
                        result.as_ref().is_err_and(|s| matches!(s.code(), Code::Internal | Code::Unavailable | Code::Cancelled)),
                        "Expected INTERNAL, UNAVAILABLE, or CANCELLED on RST INTERNAL_ERROR, got {result:?}"
                    );
                }
                FaultKind::Goaway => {
                    assert!(
                        result.is_ok()
                            || result
                                .as_ref()
                                .is_err_and(|s| s.code() == Code::Unavailable),
                        "Expected UNAVAILABLE or OK on GOAWAY, got {result:?}"
                    );
                }
                FaultKind::TcpReset | FaultKind::TcpDisconnect => {
                    assert!(
                        result
                            .as_ref()
                            .is_err_and(|s| s.code() == Code::Unavailable),
                        "Expected UNAVAILABLE on TCP reset/disconnect, got {result:?}"
                    );
                }
            }
        }

        pub async fn assert_quiescent(&self) {
            let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
            while tokio::time::Instant::now() < deadline {
                if self.client_active_tasks.load(Ordering::SeqCst) == 0
                    && self.server_active_tasks.load(Ordering::SeqCst) == 0
                {
                    return;
                }
                tokio::task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            let c = self.client_active_tasks.load(Ordering::SeqCst);
            let s = self.server_active_tasks.load(Ordering::SeqCst);
            assert_eq!(c, 0, "Leaked client tasks: {c}");
            assert_eq!(s, 0, "Leaked server tasks: {s}");
        }

        pub async fn assert_permit_release(&self, probe_client: &GreeterClient) {
            let probe_res = probe_client.say_hello(Request::new(req("probe"))).await;
            if let Err(status) = &probe_res {
                assert_ne!(
                    status.code(),
                    Code::ResourceExhausted,
                    "Permit leak detected! Probe failed with RESOURCE_EXHAUSTED: {status}"
                );
            }
        }
    }

    pub struct LifecycleGreeter {
        coordinator: Arc<LifecycleCoordinator>,
    }

    impl LifecycleGreeter {
        pub fn new(coordinator: Arc<LifecycleCoordinator>) -> Self {
            Self { coordinator }
        }
    }

    impl Greeter for LifecycleGreeter {
        async fn say_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<HelloReply>, Status> {
            let _guard = TaskGuard::new(&self.coordinator.server_active_tasks);
            self.coordinator.reach(LifecycleBoundary::HeadersSent).await;
            let name = name_of_request(request.get_ref());
            self.coordinator.record_server_received(&name);
            self.coordinator.reach(LifecycleBoundary::BodyStarted).await;

            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }

            let reply_msg = "reply-0".to_string();
            self.coordinator.record_server_sent(&reply_msg);
            self.coordinator
                .reach(LifecycleBoundary::ResponseHeadersReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            self.coordinator
                .reach(LifecycleBoundary::ResponseBodyReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            self.coordinator
                .reach(LifecycleBoundary::TrailersReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            Ok(Response::new(reply(reply_msg)))
        }

        async fn client_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<HelloReply>, Status> {
            let _guard = TaskGuard::new(&self.coordinator.server_active_tasks);
            self.coordinator.reach(LifecycleBoundary::HeadersSent).await;

            let mut stream = request.into_inner();
            let mut names = Vec::new();
            let mut first = true;
            while let Some(msg) = stream.message().await? {
                let name = name_of_request(&msg);
                self.coordinator.record_server_received(&name);
                names.push(name);
                if first {
                    first = false;
                    self.coordinator.reach(LifecycleBoundary::BodyStarted).await;
                }
                if self.coordinator.check_server_abort().await {
                    return Err(Status::internal("server handler aborted"));
                }
            }

            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }

            let reply_msg = "reply-0".to_string();
            self.coordinator.record_server_sent(&reply_msg);
            self.coordinator
                .reach(LifecycleBoundary::ResponseHeadersReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            self.coordinator
                .reach(LifecycleBoundary::ResponseBodyReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            self.coordinator
                .reach(LifecycleBoundary::TrailersReceived)
                .await;
            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }
            Ok(Response::new(reply(reply_msg)))
        }

        async fn server_hello(
            &self,
            request: Request<HelloRequest>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            let _guard = TaskGuard::new(&self.coordinator.server_active_tasks);
            self.coordinator.reach(LifecycleBoundary::HeadersSent).await;
            let name = name_of_request(request.get_ref());
            self.coordinator.record_server_received(&name);
            self.coordinator.reach(LifecycleBoundary::BodyStarted).await;

            if self.coordinator.check_server_abort().await {
                return Err(Status::internal("server handler aborted"));
            }

            let (tx, stream) = Streaming::channel(4);
            let coord = self.coordinator.clone();
            tokio::spawn(async move {
                let _guard = TaskGuard::new(&coord.server_active_tasks);
                for i in 0..3 {
                    if coord.has_faulted.load(Ordering::SeqCst)
                        && coord.scenario.fault != FaultKind::StreamHalfClose
                    {
                        break;
                    }
                    if coord.check_server_abort().await {
                        tx.fail(Status::internal("server handler aborted")).await;
                        return;
                    }
                    let reply_msg = format!("reply-{i}");
                    coord.record_server_sent(&reply_msg);
                    if tx.send(reply(reply_msg)).await.is_err() {
                        return;
                    }
                    if i == 0 {
                        coord.reach(LifecycleBoundary::ResponseBodyReceived).await;
                        if coord.check_server_abort().await {
                            tx.fail(Status::internal("server handler aborted")).await;
                            return;
                        }
                    }
                    tokio::task::yield_now().await;
                }
                coord.reach(LifecycleBoundary::TrailersReceived).await;
                if coord.check_server_abort().await {
                    tx.fail(Status::internal("server handler aborted")).await;
                }
            });
            // Response headers go out on return: reach the boundary here so a
            // fault fires while the producer task is still alive. Without
            // this, a client-side-only reach races with handler completion.
            self.coordinator
                .reach(LifecycleBoundary::ResponseHeadersReceived)
                .await;
            Ok(Response::new(stream))
        }

        async fn stream_hello(
            &self,
            request: Request<Streaming<HelloRequest>>,
        ) -> Result<Response<Streaming<HelloReply>>, Status> {
            let _guard = TaskGuard::new(&self.coordinator.server_active_tasks);
            self.coordinator.reach(LifecycleBoundary::HeadersSent).await;

            let mut inbound = request.into_inner();
            let (tx, stream) = Streaming::channel(4);
            let coord = self.coordinator.clone();
            tokio::spawn(async move {
                let _guard = TaskGuard::new(&coord.server_active_tasks);
                let mut idx = 0;
                loop {
                    if coord.has_faulted.load(Ordering::SeqCst)
                        && coord.scenario.fault != FaultKind::StreamHalfClose
                    {
                        break;
                    }
                    if coord.check_server_abort().await {
                        tx.fail(Status::internal("server handler aborted")).await;
                        return;
                    }
                    match inbound.message().await {
                        Ok(Some(msg)) => {
                            let name = name_of_request(&msg);
                            coord.record_server_received(&name);
                            if idx == 0 {
                                coord.reach(LifecycleBoundary::BodyStarted).await;
                            }
                            if coord.check_server_abort().await {
                                tx.fail(Status::internal("server handler aborted")).await;
                                return;
                            }
                            let reply_msg = format!("reply-{idx}");
                            coord.record_server_sent(&reply_msg);
                            if tx.send(reply(reply_msg)).await.is_err() {
                                return;
                            }
                            if idx == 0 {
                                coord.reach(LifecycleBoundary::ResponseBodyReceived).await;
                            }
                            if coord.check_server_abort().await {
                                tx.fail(Status::internal("server handler aborted")).await;
                                return;
                            }
                            idx += 1;
                            tokio::task::yield_now().await;
                        }
                        Ok(None) => break,
                        Err(status) => {
                            tx.fail(status).await;
                            return;
                        }
                    }
                }
                coord.reach(LifecycleBoundary::TrailersReceived).await;
                if coord.check_server_abort().await {
                    tx.fail(Status::internal("server handler aborted")).await;
                }
            });
            // Response headers go out on return: reach the boundary here so a
            // fault fires while the producer task is still alive. Without
            // this, a client-side-only reach races with handler completion.
            self.coordinator
                .reach(LifecycleBoundary::ResponseHeadersReceived)
                .await;
            Ok(Response::new(stream))
        }
    }

    pub struct LifecycleRunner;

    impl LifecycleRunner {
        pub async fn run_scenario(scenario: LifecycleScenario) {
            let proxy_state = Arc::new(Mutex::new(ProxyState::new()));
            let coord = Arc::new(LifecycleCoordinator::new(scenario, proxy_state.clone()));

            match scenario.transport {
                TransportKind::FromIo => {
                    let (client_raw, server_raw) = tokio::io::duplex(64 * 1024);
                    let client_stream =
                        FaultInjectingStream::new(client_raw, proxy_state.clone(), true);
                    let server_stream =
                        FaultInjectingStream::new(server_raw, proxy_state.clone(), false);

                    let server = GreeterServer::new(LifecycleGreeter::new(coord.clone()))
                        .config(ServerConfig::new().max_concurrent_rpcs(1));
                    let server_task = tokio::spawn(async move {
                        server.serve_connection(server_stream).await.ok();
                    });

                    let channel = Channel::from_io_with(
                        client_stream,
                        "localhost",
                        ChannelConfig::default().max_concurrent_rpcs(1),
                    )
                    .await
                    .expect("channel from_io");
                    let client = GreeterClient::new(channel);

                    Self::execute_and_verify(client, coord.clone(), scenario).await;
                    server_task.abort();
                }
                TransportKind::Tcp => {
                    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind server");
                    let server_addr = listener.local_addr().expect("server addr");
                    let server = GreeterServer::new(LifecycleGreeter::new(coord.clone()))
                        .config(ServerConfig::new().max_concurrent_rpcs(1));
                    let server_task = tokio::spawn(async move {
                        server.serve_listener(listener).await.ok();
                    });

                    let proxy_listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind proxy");
                    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
                    let proxy_task =
                        spawn_tcp_byte_proxy(proxy_listener, server_addr, proxy_state.clone());

                    let client = greeter_client(proxy_addr).await.max_concurrent_rpcs(1);

                    Self::execute_and_verify(client, coord.clone(), scenario).await;
                    proxy_task.abort();
                    server_task.abort();
                }
                TransportKind::Tls => {
                    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind server");
                    let server_addr = listener.local_addr().expect("server addr");
                    let server_tls = ServerTls::new(
                        Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity"),
                    )
                    .expect("server tls");

                    let (shutdown_tx, shutdown_rx) = oneshot::channel();
                    *coord.server_shutdown_trigger.lock().unwrap() = Some(shutdown_tx);

                    let server = GreeterServer::new(LifecycleGreeter::new(coord.clone()))
                        .config(ServerConfig::new().max_concurrent_rpcs(1));
                    let server_task = tokio::spawn(async move {
                        server
                            .serve_tls_with_shutdown(
                                listener,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                                server_tls,
                            )
                            .await
                            .ok();
                    });

                    let proxy_listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind proxy");
                    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
                    let proxy_task =
                        spawn_tcp_byte_proxy(proxy_listener, server_addr, proxy_state.clone());

                    let client_tls = ClientTls::ca("localhost", CA).expect("client tls");
                    let mut last = Status::unavailable("connect");
                    let mut client_opt = None;
                    for _ in 0..80 {
                        match GreeterClient::connect_tls(proxy_addr, client_tls.clone()).await {
                            Ok(c) => {
                                client_opt = Some(c.max_concurrent_rpcs(1));
                                break;
                            }
                            Err(e) => {
                                last = e;
                                tokio::time::sleep(Duration::from_millis(5)).await;
                            }
                        }
                    }
                    let client = client_opt
                        .unwrap_or_else(|| panic!("could not connect tls to {proxy_addr}: {last}"));

                    Self::execute_and_verify(client, coord.clone(), scenario).await;
                    proxy_task.abort();
                    server_task.abort();
                }
                TransportKind::Mtls => {
                    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind server");
                    let server_addr = listener.local_addr().expect("server addr");
                    let server_tls = ServerTls::mtls(
                        Identity::from_pem(SERVER_CERT, SERVER_KEY).expect("server identity"),
                        CA,
                    )
                    .expect("mtls server tls");

                    let (shutdown_tx, shutdown_rx) = oneshot::channel();
                    *coord.server_shutdown_trigger.lock().unwrap() = Some(shutdown_tx);

                    let server = GreeterServer::new(LifecycleGreeter::new(coord.clone()))
                        .config(ServerConfig::new().max_concurrent_rpcs(1));
                    let server_task = tokio::spawn(async move {
                        server
                            .serve_tls_with_shutdown(
                                listener,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                                server_tls,
                            )
                            .await
                            .ok();
                    });

                    let proxy_listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                        .await
                        .expect("bind proxy");
                    let proxy_addr = proxy_listener.local_addr().expect("proxy addr");
                    let proxy_task =
                        spawn_tcp_byte_proxy(proxy_listener, server_addr, proxy_state.clone());

                    let client_identity =
                        Identity::from_pem(CLIENT_CERT, CLIENT_KEY).expect("client identity");
                    let client_tls =
                        ClientTls::ca_mtls("localhost", CA, client_identity).expect("mtls client");
                    let mut last = Status::unavailable("connect");
                    let mut client_opt = None;
                    for _ in 0..80 {
                        match GreeterClient::connect_tls(proxy_addr, client_tls.clone()).await {
                            Ok(c) => {
                                client_opt = Some(c.max_concurrent_rpcs(1));
                                break;
                            }
                            Err(e) => {
                                last = e;
                                tokio::time::sleep(Duration::from_millis(5)).await;
                            }
                        }
                    }
                    let client = client_opt.unwrap_or_else(|| {
                        panic!("could not connect mtls to {proxy_addr}: {last}")
                    });

                    Self::execute_and_verify(client, coord.clone(), scenario).await;
                    proxy_task.abort();
                    server_task.abort();
                }
                TransportKind::Uds => {
                    let server_path = lifecycle_unix_sock("srv");
                    let server_listener =
                        UnixListener::bind(&server_path).expect("bind uds server");
                    let server = GreeterServer::new(LifecycleGreeter::new(coord.clone()))
                        .config(ServerConfig::new().max_concurrent_rpcs(1));
                    let server_task = tokio::spawn(async move {
                        server.serve_unix_listener(server_listener).await.ok();
                    });

                    let proxy_path = lifecycle_unix_sock("pxy");
                    let proxy_listener = UnixListener::bind(&proxy_path).expect("bind uds proxy");
                    let p_state = proxy_state.clone();
                    let server_path_clone = server_path.clone();

                    let proxy_task = tokio::spawn(async move {
                        while let Ok((client_sock, _)) = proxy_listener.accept().await {
                            let p_state_clone = p_state.clone();
                            let server_path_clone = server_path_clone.clone();
                            tokio::spawn(async move {
                                if let Ok(server_sock) =
                                    UnixStream::connect(server_path_clone).await
                                {
                                    let wrapped_server = FaultInjectingStream::new(
                                        server_sock,
                                        p_state_clone.clone(),
                                        true,
                                    );
                                    let wrapped_client = FaultInjectingStream::new(
                                        client_sock,
                                        p_state_clone,
                                        false,
                                    );
                                    let (mut cr, mut cw) = tokio::io::split(wrapped_client);
                                    let (mut sr, mut sw) = tokio::io::split(wrapped_server);
                                    tokio::select! {
                                        _ = tokio::io::copy(&mut cr, &mut sw) => {}
                                        _ = tokio::io::copy(&mut sr, &mut cw) => {}
                                    }
                                }
                            });
                        }
                    });

                    let mut last = Status::unavailable("connect");
                    let mut client_opt = None;
                    for _ in 0..80 {
                        match GreeterClient::connect_unix(&proxy_path).await {
                            Ok(c) => {
                                client_opt = Some(c.max_concurrent_rpcs(1));
                                break;
                            }
                            Err(e) => {
                                last = e;
                                tokio::time::sleep(Duration::from_millis(5)).await;
                            }
                        }
                    }
                    let client = client_opt.unwrap_or_else(|| {
                        panic!("could not connect uds to {}: {last}", proxy_path.display())
                    });

                    Self::execute_and_verify(client, coord.clone(), scenario).await;
                    proxy_task.abort();
                    server_task.abort();
                    let _ = std::fs::remove_file(&server_path);
                    let _ = std::fs::remove_file(&proxy_path);
                }
            }
        }

        async fn execute_and_verify(
            client: GreeterClient,
            coord: Arc<LifecycleCoordinator>,
            scenario: LifecycleScenario,
        ) {
            let (drop_tx, drop_rx) = oneshot::channel();
            *coord.client_drop_trigger.lock().unwrap() = Some(drop_tx);

            let (close_tx, close_rx) = oneshot::channel();
            *coord.stream_close_trigger.lock().unwrap() = Some(close_tx);

            let coord_on_resp = coord.clone();
            let client = client.on_response(move |_parts: &mut ResponseParts| {
                coord_on_resp.reach_sync(LifecycleBoundary::ResponseHeadersReceived);
                Ok(())
            });

            let probe_client = client.clone();

            let call_res = match scenario.shape {
                CallShape::Unary => Self::run_unary(&client, &coord, drop_rx).await,
                CallShape::ClientStreaming => {
                    Self::run_client_streaming(&client, &coord, drop_rx, close_rx).await
                }
                CallShape::ServerStreaming => {
                    Self::run_server_streaming(&client, &coord, drop_rx).await
                }
                CallShape::Bidi => Self::run_bidi(&client, &coord, drop_rx, close_rx).await,
            };

            coord.assert_message_ordering();
            coord.assert_status(&call_res);
            coord.assert_quiescent().await;
            coord.assert_permit_release(&probe_client).await;
        }

        async fn run_unary(
            client: &GreeterClient,
            coord: &Arc<LifecycleCoordinator>,
            mut drop_rx: oneshot::Receiver<()>,
        ) -> Result<(), Status> {
            let _guard = TaskGuard::new(&coord.client_active_tasks);
            coord.record_client_sent("msg-0");
            let mut call = client.say_hello(Request::new(req("msg-0")));
            coord.set_cancel_handle(call.handle());
            coord.reach(LifecycleBoundary::Queued).await;

            let res = tokio::select! {
                biased;
                _ = &mut drop_rx => {
                    return Err(Status::cancelled());
                }
                r = &mut call => r,
            };
            match res {
                Ok(reply) => {
                    coord.record_client_received(&name_of(reply.get_ref()));
                    Ok(())
                }
                Err(status) => Err(status),
            }
        }

        async fn run_client_streaming(
            client: &GreeterClient,
            coord: &Arc<LifecycleCoordinator>,
            mut drop_rx: oneshot::Receiver<()>,
            mut close_rx: oneshot::Receiver<()>,
        ) -> Result<(), Status> {
            let _guard = TaskGuard::new(&coord.client_active_tasks);
            let (tx, mut call) = client.client_hello(Request::new(()));
            coord.set_cancel_handle(call.handle());
            coord.reach(LifecycleBoundary::Queued).await;

            let coord_sender = coord.clone();
            let send_task = tokio::spawn(async move {
                let _guard = TaskGuard::new(&coord_sender.client_active_tasks);
                for i in 0..3 {
                    let msg = format!("msg-{i}");
                    coord_sender.record_client_sent(&msg);
                    tokio::select! {
                        biased;
                        _ = &mut close_rx => {
                            tx.close();
                            return;
                        }
                        send_res = tx.send(req(&msg)) => {
                            if send_res.is_err() {
                                return;
                            }
                        }
                    }
                }
                tx.close();
            });

            let res = tokio::select! {
                biased;
                _ = &mut drop_rx => {
                    let _ = send_task.await;
                    return Err(Status::cancelled());
                }
                r = &mut call => r,
            };
            let _ = send_task.await;
            match res {
                Ok(reply) => {
                    coord.record_client_received(&name_of(reply.get_ref()));
                    Ok(())
                }
                Err(status) => Err(status),
            }
        }

        async fn run_server_streaming(
            client: &GreeterClient,
            coord: &Arc<LifecycleCoordinator>,
            mut drop_rx: oneshot::Receiver<()>,
        ) -> Result<(), Status> {
            let _guard = TaskGuard::new(&coord.client_active_tasks);
            coord.record_client_sent("msg-0");
            let mut call = client.server_hello(Request::new(req("msg-0")));
            coord.set_cancel_handle(call.handle());
            coord.reach(LifecycleBoundary::Queued).await;

            let res = tokio::select! {
                biased;
                _ = &mut drop_rx => {
                    return Err(Status::cancelled());
                }
                r = &mut call => r,
            };
            match res {
                Ok(reply_stream) => {
                    let mut stream = reply_stream.into_inner();
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut drop_rx => {
                                return Err(Status::cancelled());
                            }
                            next = stream.message() => {
                                match next {
                                    Ok(Some(msg)) => {
                                        coord.record_client_received(&name_of(&msg));
                                    }
                                    Ok(None) => return Ok(()),
                                    Err(status) => return Err(status),
                                }
                            }
                        }
                    }
                }
                Err(status) => Err(status),
            }
        }

        async fn run_bidi(
            client: &GreeterClient,
            coord: &Arc<LifecycleCoordinator>,
            mut drop_rx: oneshot::Receiver<()>,
            mut close_rx: oneshot::Receiver<()>,
        ) -> Result<(), Status> {
            let _guard = TaskGuard::new(&coord.client_active_tasks);
            let (tx, mut call) = client.stream_hello(Request::new(()));
            coord.set_cancel_handle(call.handle());
            coord.reach(LifecycleBoundary::Queued).await;

            let coord_sender = coord.clone();
            let send_task = tokio::spawn(async move {
                let _guard = TaskGuard::new(&coord_sender.client_active_tasks);
                for i in 0..3 {
                    let msg = format!("msg-{i}");
                    coord_sender.record_client_sent(&msg);
                    tokio::select! {
                        biased;
                        _ = &mut close_rx => {
                            tx.close();
                            return;
                        }
                        send_res = tx.send(req(&msg)) => {
                            if send_res.is_err() {
                                return;
                            }
                        }
                    }
                }
                tx.close();
            });

            let res = tokio::select! {
                biased;
                _ = &mut drop_rx => {
                    let _ = send_task.await;
                    return Err(Status::cancelled());
                }
                r = &mut call => r,
            };

            let result = match res {
                Ok(reply_stream) => {
                    let mut stream = reply_stream.into_inner();
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut drop_rx => {
                                drop(stream);
                                break Err(Status::cancelled());
                            }
                            next = stream.message() => {
                                match next {
                                    Ok(Some(msg)) => {
                                        coord.record_client_received(&name_of(&msg));
                                    }
                                    Ok(None) => break Ok(()),
                                    Err(status) => break Err(status),
                                }
                            }
                        }
                    }
                }
                Err(status) => Err(status),
            };
            let _ = send_task.await;
            result
        }
    }
}
