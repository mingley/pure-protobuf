//! Implementation of the official gRPC `WorkerService`.
//!
//! Provides worker lifecycle management, core count introspection,
//! benchmark server lifecycle, mark reset semantics, and bounded server shutdown
//! adhering to the official gRPC benchmark control protocol.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    missing_docs,
    reason = "bench worker service implementation"
)]

pub mod proto {
    #![allow(
        missing_docs,
        unused,
        clippy::all,
        clippy::pedantic,
        clippy::restriction,
        reason = "generated protobuf worker_service stubs"
    )]
    include!(concat!(env!("OUT_DIR"), "/worker_service.rs"));
}

pub use proto::{
    ClientArgs, ClientConfig, ClientStatus, CoreRequest, CoreResponse, Mark, ServerArgs,
    ServerConfig, ServerStats, ServerStatus, ServerType, Void, WorkerService, WorkerServiceClient,
    WorkerServiceServer,
};

use pbrs_grpc::{Request, Response, Status, Streaming};
use std::sync::Arc;
use std::time::Duration;

use crate::benchmark_service::{BenchmarkServiceImpl, BenchmarkServiceServer};
use crate::resources::ResourceSnapshot;

pub(crate) type ResourceCapture = Arc<dyn Fn() -> std::io::Result<ResourceSnapshot> + Send + Sync>;

fn default_resource_capture() -> ResourceCapture {
    Arc::new(ResourceSnapshot::capture)
}

pub(crate) fn require_snapshot(
    snapshot: std::io::Result<ResourceSnapshot>,
    stage: &'static str,
) -> Result<ResourceSnapshot, Status> {
    snapshot.map_err(|error| {
        Status::unavailable(format!(
            "WorkerService {stage} process resource capture failed: {error}"
        ))
    })
}

pub(crate) fn checked_core_count(count: usize) -> Result<i32, Status> {
    if count == 0 {
        return Err(Status::invalid_argument(
            "WorkerService core count must be positive",
        ));
    }
    i32::try_from(count)
        .map_err(|_| Status::resource_exhausted("WorkerService core count exceeds i32 range"))
}

pub(crate) fn checked_system_cores(
    available: std::io::Result<std::num::NonZeroUsize>,
) -> Result<i32, Status> {
    let count = available.map_err(|error| {
        Status::unavailable(format!(
            "WorkerService system CPU count unavailable: {error}"
        ))
    })?;
    checked_core_count(count.get())
}

/// Sanity cap for `async_server_threads` / `async_client_threads`: an
/// explicit control is honored literally, but thread spawns are real
/// resources, so absurd counts fail fast instead of OOMing the worker.
pub(crate) const MAX_WORKER_RUNTIME_THREADS: usize = 1024;

/// Map an async thread control to a dedicated runtime size: 0/absent
/// runs inline on the shared worker runtime, a positive count up to
/// the cap gets a dedicated Tokio runtime with exactly that many
/// workers, and anything else fails before bind/dial.
pub(crate) fn checked_thread_count(
    value: i32,
    name: &'static str,
) -> Result<Option<usize>, Status> {
    if value < 0 {
        return Err(Status::invalid_argument(format!(
            "{name} cannot be negative"
        )));
    }
    let count = usize::try_from(value).map_err(|_| {
        Status::resource_exhausted(format!("{name} {value} exceeds the worker thread range"))
    })?;
    if count == 0 {
        return Ok(None);
    }
    if count > MAX_WORKER_RUNTIME_THREADS {
        return Err(Status::resource_exhausted(format!(
            "{name} {count} exceeds the worker limit of {MAX_WORKER_RUNTIME_THREADS}"
        )));
    }
    Ok(Some(count))
}

pub(crate) fn unsupported_server_option(config: &ServerConfig) -> Option<&'static str> {
    if config.has_security_params() {
        Some("security_params")
    } else if config.core_limit() > 0 {
        Some("core_limit")
    } else if !config.core_list().is_empty() {
        Some("core_list")
    } else if config.threads_per_cq() != 0 {
        Some("threads_per_cq")
    } else if config.resource_quota_size() != 0 {
        Some("resource_quota_size")
    } else if !config.channel_args().is_empty() {
        Some("channel_args")
    } else if config.server_processes() != 0 {
        Some("server_processes")
    } else if !config.other_server_api().as_bytes().is_empty() {
        Some("other_server_api")
    } else {
        None
    }
}

pub(crate) async fn stop_owned_server(
    shutdown: tokio::sync::oneshot::Sender<()>,
    handle: &mut tokio::task::JoinHandle<()>,
) -> Result<(), Status> {
    let _ = shutdown.send(());
    match tokio::time::timeout(Duration::from_secs(5), &mut *handle).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(Status::internal(format!(
            "benchmark server task failed during shutdown: {error}"
        ))),
        Err(_) => {
            handle.abort();
            if let Err(error) = handle.await
                && !error.is_cancelled()
            {
                return Err(Status::internal(format!(
                    "benchmark server abort failed: {error}"
                )));
            }
            Err(Status::unavailable(
                "benchmark server did not shut down within 5 seconds; aborted",
            ))
        }
    }
}

async fn fail_after_server_cleanup(
    tx: pbrs_grpc::StreamSender<ServerStatus>,
    status: Status,
    shutdown: tokio::sync::oneshot::Sender<()>,
    owned: OwnedServer,
) {
    let status = match shutdown_owned_server(shutdown, owned).await {
        Ok(()) => status,
        Err(error) => Status::internal(format!("{status}; cleanup failed: {error}")),
    };
    tx.fail(status).await;
}

/// A benchmark server owned by one `RunServer` stream: the serve task
/// plus the Tokio runtime it runs on (`None` runs inline on the
/// shared worker runtime; `Some` is a dedicated `async_server_threads`
/// runtime). Always shut down via [`shutdown_owned_server`]: dropping
/// a live `Runtime` on an async path panics.
pub(crate) struct OwnedServer {
    handle: tokio::task::JoinHandle<()>,
    runtime: Option<tokio::runtime::Runtime>,
}

async fn serve_benchmark(
    listener: std::net::TcpListener,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let router = create_benchmark_router();
    match tokio::net::TcpListener::from_std(listener) {
        Ok(listener) => {
            if let Err(error) = router
                .serve_with_shutdown(listener, async move {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                eprintln!("benchmark server exited with error: {error}");
            }
        }
        Err(error) => eprintln!("benchmark server failed to adopt listener: {error}"),
    }
}

impl OwnedServer {
    /// Spawn the benchmark server: inline on the worker runtime for
    /// `None`, or on a dedicated runtime with exactly `count` workers.
    /// The std listener is adopted inside the task so its I/O always
    /// registers with the runtime that polls it.
    fn spawn(
        threads: Option<usize>,
        listener: std::net::TcpListener,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<Self, Status> {
        match threads {
            None => Ok(Self {
                handle: tokio::spawn(serve_benchmark(listener, shutdown_rx)),
                runtime: None,
            }),
            Some(count) => {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(count)
                    .thread_name("qps-bench-server")
                    .enable_all()
                    .build()
                    .map_err(|error| {
                        Status::internal(format!(
                            "failed to build {count}-worker benchmark server runtime: {error}"
                        ))
                    })?;
                let handle = runtime.spawn(serve_benchmark(listener, shutdown_rx));
                Ok(Self {
                    handle,
                    runtime: Some(runtime),
                })
            }
        }
    }
}

/// Stop an owned server and release its runtime. Consumes the server:
/// every `RunServer` exit path ends the stream right after.
async fn shutdown_owned_server(
    shutdown: tokio::sync::oneshot::Sender<()>,
    mut owned: OwnedServer,
) -> Result<(), Status> {
    let result = stop_owned_server(shutdown, &mut owned.handle).await;
    if let Some(runtime) = owned.runtime.take() {
        // Runtime::drop blocks for worker shutdown; keep it off the
        // async path. The serve task is already gone, so this only
        // stops idle workers.
        let _ = tokio::task::spawn_blocking(move || drop(runtime)).await;
    }
    result
}

/// Create a router mounting the benchmark and test services.
pub fn create_benchmark_router() -> pbrs_grpc::Router {
    pbrs_grpc::Router::new()
        .add_service(pbrs_grpc::TestServiceServer::new(
            pbrs_grpc::InteropTestService,
        ))
        .add_service(BenchmarkServiceServer::new(BenchmarkServiceImpl))
}

/// Implementation of the official gRPC `WorkerService`.
#[derive(Clone)]
pub struct WorkerServiceImpl {
    quit_tx: Option<tokio::sync::watch::Sender<bool>>,
    resource_capture: ResourceCapture,
}

impl Default for WorkerServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerServiceImpl {
    /// Create a new worker service instance without an external shutdown trigger.
    #[must_use]
    pub fn new() -> Self {
        Self {
            quit_tx: None,
            resource_capture: default_resource_capture(),
        }
    }

    /// Create a new worker service instance with a shutdown watch channel sender.
    #[must_use]
    pub fn with_shutdown(quit_tx: tokio::sync::watch::Sender<bool>) -> Self {
        Self {
            quit_tx: Some(quit_tx),
            resource_capture: default_resource_capture(),
        }
    }

    #[cfg(test)]
    #[allow(dead_code, reason = "used by the standalone worker integration tests")]
    pub(crate) fn with_capture(
        quit_tx: tokio::sync::watch::Sender<bool>,
        resource_capture: ResourceCapture,
    ) -> Self {
        Self {
            quit_tx: Some(quit_tx),
            resource_capture,
        }
    }
}

impl WorkerService for WorkerServiceImpl {
    /// Return the system CPU core count.
    async fn core_count(
        &self,
        _request: Request<CoreRequest>,
    ) -> Result<Response<CoreResponse>, Status> {
        let cores = checked_system_cores(std::thread::available_parallelism())?;
        let mut resp = CoreResponse::new();
        resp.set_cores(cores);
        Ok(Response::new(resp))
    }

    /// Signal worker process shutdown.
    async fn quit_worker(&self, _request: Request<Void>) -> Result<Response<Void>, Status> {
        if let Some(ref tx) = self.quit_tx {
            let _ = tx.send(true);
        }
        Ok(Response::new(Void::new()))
    }

    /// Start and manage the benchmark server lifecycle and marks.
    async fn run_server(
        &self,
        request: Request<Streaming<ServerArgs>>,
    ) -> Result<Response<Streaming<ServerStatus>>, Status> {
        let mut in_stream = request.into_inner();
        let (tx, out_stream) = Streaming::channel(32);
        let resource_capture = self.resource_capture.clone();

        tokio::spawn(async move {
            // 1. First request received must specify ServerConfig.
            let first_arg = match in_stream.message().await {
                Ok(Some(arg)) => arg,
                Ok(None) => {
                    tx.fail(Status::invalid_argument(
                        "empty ServerArgs stream: expected ServerConfig setup",
                    ))
                    .await;
                    return;
                }
                Err(e) => {
                    tx.fail(e).await;
                    return;
                }
            };

            if !first_arg.has_setup() {
                tx.fail(Status::invalid_argument(
                    "first request in stream must specify ServerConfig setup",
                ))
                .await;
                return;
            }

            let cfg = first_arg.setup();

            // 2. Validate configuration.
            let port = cfg.port();
            if !(0..=65535).contains(&port) {
                tx.fail(Status::invalid_argument(format!("invalid port: {port}")))
                    .await;
                return;
            }
            if cfg.core_limit() < 0 {
                tx.fail(Status::invalid_argument("core_limit cannot be negative"))
                    .await;
                return;
            }
            if cfg.server_type() != ServerType::AsyncServer {
                tx.fail(Status::invalid_argument(format!(
                    "unsupported server_type {:?}: only ASYNC_SERVER is implemented",
                    cfg.server_type()
                )))
                .await;
                return;
            }
            if cfg.has_payload_config() {
                tx.fail(Status::invalid_argument(
                    "payload_config is only valid for unsupported generic servers",
                ))
                .await;
                return;
            }
            if let Some(option) = unsupported_server_option(cfg) {
                tx.fail(Status::invalid_argument(format!(
                    "unsupported server config option {option}"
                )))
                .await;
                return;
            }
            let server_threads =
                match checked_thread_count(cfg.async_server_threads(), "async_server_threads") {
                    Ok(threads) => threads,
                    Err(status) => {
                        tx.fail(status).await;
                        return;
                    }
                };

            // 3. Bind TCP listener and determine effective listening port.
            // A std listener binds without a runtime; the serve task
            // adopts it so I/O registers with whichever runtime polls
            // it (shared worker or dedicated thread runtime).
            let std_listener = match std::net::TcpListener::bind(format!("0.0.0.0:{port}"))
                .or_else(|_| std::net::TcpListener::bind(format!("127.0.0.1:{port}")))
            {
                Ok(listener) => listener,
                Err(e) => {
                    tx.fail(Status::internal(format!("failed to bind port {port}: {e}")))
                        .await;
                    return;
                }
            };
            if let Err(e) = std_listener.set_nonblocking(true) {
                tx.fail(Status::internal(format!(
                    "failed to set nonblocking on port {port}: {e}"
                )))
                .await;
                return;
            }
            let bound_port = match std_listener.local_addr() {
                Ok(a) => a.port() as i32,
                Err(e) => {
                    tx.fail(Status::internal(format!("failed to get local addr: {e}")))
                        .await;
                    return;
                }
            };

            // Determine server cores.
            let system_cores = match checked_system_cores(std::thread::available_parallelism()) {
                Ok(cores) => cores,
                Err(status) => {
                    tx.fail(status).await;
                    return;
                }
            };
            let cores = if cfg.core_limit() > 0 {
                cfg.core_limit().min(system_cores)
            } else if !cfg.core_list().is_empty() {
                match checked_core_count(cfg.core_list().len()) {
                    Ok(cores) => cores,
                    Err(status) => {
                        tx.fail(status).await;
                        return;
                    }
                }
            } else {
                system_cores
            };

            // 4. Spawn benchmark server with graceful shutdown trigger.
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            let owned = match OwnedServer::spawn(server_threads, std_listener, shutdown_rx) {
                Ok(owned) => owned,
                Err(status) => {
                    tx.fail(status).await;
                    return;
                }
            };
            match server_threads {
                Some(count) => eprintln!(
                    "RunServer port={bound_port}: async_server_threads={count} -> \
                     dedicated {count}-worker Tokio runtime"
                ),
                None => eprintln!(
                    "RunServer port={bound_port}: async_server_threads=0 -> \
                     shared worker runtime"
                ),
            }

            // 5. Build and send initial ServerStatus.
            let initial_snapshot = match require_snapshot(resource_capture(), "RunServer initial") {
                Ok(snapshot) => snapshot,
                Err(status) => {
                    fail_after_server_cleanup(tx, status, shutdown_tx, owned).await;
                    return;
                }
            };

            let mut initial_status = ServerStatus::new();
            initial_status.set_port(bound_port);
            initial_status.set_cores(cores);
            let mut initial_stats = ServerStats::new();
            initial_stats.set_time_elapsed(0.0);
            initial_stats.set_time_user(0.0);
            initial_stats.set_time_system(0.0);
            initial_status.set_stats(initial_stats);

            if tx.send(initial_status).await.is_err() {
                if let Err(error) = shutdown_owned_server(shutdown_tx, owned).await {
                    eprintln!("{error}");
                }
                return;
            }

            // 6. Loop for subsequent Mark requests and stream termination.
            let mut baseline_snapshot = initial_snapshot;
            let mut baseline_time = std::time::Instant::now();

            loop {
                let arg = match in_stream.message().await {
                    Ok(Some(a)) => a,
                    Ok(None) => break, // Closing inbound stream triggers graceful shutdown
                    Err(e) => {
                        fail_after_server_cleanup(tx, e, shutdown_tx, owned).await;
                        return;
                    }
                };

                // Reject duplicate setup
                if arg.has_setup() {
                    fail_after_server_cleanup(
                        tx,
                        Status::invalid_argument("duplicate ServerConfig setup received"),
                        shutdown_tx,
                        owned,
                    )
                    .await;
                    return;
                }

                if !arg.has_mark() {
                    fail_after_server_cleanup(
                        tx,
                        Status::invalid_argument("expected Mark in subsequent ServerArgs"),
                        shutdown_tx,
                        owned,
                    )
                    .await;
                    return;
                }

                let mark = arg.mark();
                let now = std::time::Instant::now();
                let time_elapsed = (now - baseline_time).as_secs_f64();

                let current_snapshot = match require_snapshot(resource_capture(), "RunServer Mark")
                {
                    Ok(snapshot) => snapshot,
                    Err(status) => {
                        fail_after_server_cleanup(tx, status, shutdown_tx, owned).await;
                        return;
                    }
                };
                let delta = baseline_snapshot.delta_to(&current_snapshot);

                let mut stats = ServerStats::new();
                stats.set_time_elapsed(time_elapsed);
                stats.set_time_user(delta.user_cpu_seconds);
                stats.set_time_system(delta.system_cpu_seconds);
                if let Some(total_nanos) = delta.total_cpu_nanos() {
                    stats.set_total_cpu_time(total_nanos);
                }

                if mark.reset() {
                    baseline_snapshot = current_snapshot;
                    baseline_time = now;
                }

                let mut status = ServerStatus::new();
                status.set_stats(stats);
                status.set_port(bound_port);
                status.set_cores(cores);

                if tx.send(status).await.is_err() {
                    break;
                }
            }

            // Closing inbound stream triggers graceful shutdown of the test server
            // and terminates the RPC with OK status.
            if let Err(status) = shutdown_owned_server(shutdown_tx, owned).await {
                tx.fail(status).await;
            }
            // Dropping tx ends with OK only after the owned server stops.
        });

        Ok(Response::new(out_stream))
    }

    /// Start and manage benchmark client workload, marks, and statistics accounting.
    async fn run_client(
        &self,
        request: Request<Streaming<ClientArgs>>,
    ) -> Result<Response<Streaming<ClientStatus>>, Status> {
        crate::worker_client::run_client(request, self.resource_capture.clone()).await
    }
}
