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
use std::time::Duration;
use tokio::net::TcpListener;

use crate::benchmark_service::{BenchmarkServiceImpl, BenchmarkServiceServer};
use crate::resources::ResourceSnapshot;

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
            if let Err(error) = handle.await {
                if !error.is_cancelled() {
                    return Err(Status::internal(format!(
                        "benchmark server abort failed: {error}"
                    )));
                }
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
    handle: &mut tokio::task::JoinHandle<()>,
) {
    let status = match stop_owned_server(shutdown, handle).await {
        Ok(()) => status,
        Err(error) => Status::internal(format!("{status}; cleanup failed: {error}")),
    };
    tx.fail(status).await;
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
#[derive(Clone, Default)]
pub struct WorkerServiceImpl {
    quit_tx: Option<tokio::sync::watch::Sender<bool>>,
}

impl WorkerServiceImpl {
    /// Create a new worker service instance without an external shutdown trigger.
    #[must_use]
    pub fn new() -> Self {
        Self { quit_tx: None }
    }

    /// Create a new worker service instance with a shutdown watch channel sender.
    #[must_use]
    pub fn with_shutdown(quit_tx: tokio::sync::watch::Sender<bool>) -> Self {
        Self {
            quit_tx: Some(quit_tx),
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
            if i32::from(cfg.server_type()) < 0 {
                tx.fail(Status::invalid_argument("invalid server_type"))
                    .await;
                return;
            }

            // 3. Bind TCP listener and determine effective listening port.
            let listener = match TcpListener::bind(format!("0.0.0.0:{port}")).await {
                Ok(l) => l,
                Err(_) => match TcpListener::bind(format!("127.0.0.1:{port}")).await {
                    Ok(l) => l,
                    Err(e) => {
                        tx.fail(Status::internal(format!("failed to bind port {port}: {e}")))
                            .await;
                        return;
                    }
                },
            };
            let bound_port = match listener.local_addr() {
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
            let mut server_handle = tokio::spawn(async move {
                let router = create_benchmark_router();
                let _ = router
                    .serve_with_shutdown(listener, async move {
                        let _ = shutdown_rx.await;
                    })
                    .await;
            });

            // 5. Build and send initial ServerStatus.
            let initial_snapshot =
                match require_snapshot(ResourceSnapshot::capture(), "RunServer initial") {
                    Ok(snapshot) => snapshot,
                    Err(status) => {
                        fail_after_server_cleanup(tx, status, shutdown_tx, &mut server_handle)
                            .await;
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
                if let Err(error) = stop_owned_server(shutdown_tx, &mut server_handle).await {
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
                        fail_after_server_cleanup(tx, e, shutdown_tx, &mut server_handle).await;
                        return;
                    }
                };

                // Reject duplicate setup
                if arg.has_setup() {
                    fail_after_server_cleanup(
                        tx,
                        Status::invalid_argument("duplicate ServerConfig setup received"),
                        shutdown_tx,
                        &mut server_handle,
                    )
                    .await;
                    return;
                }

                if !arg.has_mark() {
                    fail_after_server_cleanup(
                        tx,
                        Status::invalid_argument("expected Mark in subsequent ServerArgs"),
                        shutdown_tx,
                        &mut server_handle,
                    )
                    .await;
                    return;
                }

                let mark = arg.mark();
                let now = std::time::Instant::now();
                let time_elapsed = (now - baseline_time).as_secs_f64();

                let current_snapshot =
                    match require_snapshot(ResourceSnapshot::capture(), "RunServer Mark") {
                        Ok(snapshot) => snapshot,
                        Err(status) => {
                            fail_after_server_cleanup(tx, status, shutdown_tx, &mut server_handle)
                                .await;
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
            if let Err(status) = stop_owned_server(shutdown_tx, &mut server_handle).await {
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
        crate::worker_client::run_client(request).await
    }
}
