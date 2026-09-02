//! `grpc.health.v1.Health`: the standard health service, generated plus a reporter.
//!
//! Check, List, and Watch are the proto methods. List is a snapshot of every
//! known name (the process `""` and names you set); unknown names are omitted,
//! matching [`HealthReporter::names`]. An inbound Check or Watch over the
//! decoding cap is `RESOURCE_EXHAUSTED` on both, including
//! over TLS, mTLS, Unix, and [`crate::Channel::from_io`]. A [`HealthClient`]
//! `max_encoding_message_size` / `max_decoding_message_size` is
//! `RESOURCE_EXHAUSTED` on Check and Watch on those transports, distinct from
//! the server decoding cap. [`HealthClient::message_limits`] refuses the same
//! oversize, distinct from those single-cap wrappers. `Router::message_limits` /
//! [`HealthServer::message_limits`] refuse the same oversize as
//! `RESOURCE_EXHAUSTED` on both, distinct from
//! [`crate::Router::max_decoding_message_size`].
//! [`HealthClient::connect_tls_with`] / [`HealthClient::connect_unix_with`] /
//! [`HealthClient::from_io_with`] with [`crate::ChannelConfig::message_limits`]
//! refuse the same oversize, distinct from wrapping a live client.
//! [`HealthServer::max_header_list_size`] refuses oversize metadata on Check
//! and Watch, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server. [`HealthServer::max_frame_size`] still serves Check, List, and Watch at
//! the HTTP/2 16 KiB SETTINGS minimum, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server. [`HealthServer::max_pending_accept_reset_streams`] still serves Check,
//! List, and Watch at a pending-reset cap of 1, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. A well-behaved client never fills that
//! queue. Distinct from wrapping only a Greeter server.
//! [`HealthServer::max_send_buffer_size`] still serves Check, List, and Watch at a
//! 16 KiB send buffer, including over TLS, mTLS, Unix, and
//! [`crate::Server::serve_connection`]. Distinct from wrapping only a Greeter
//! server.
//! [`HealthServer::initial_stream_window_size`] /
//! [`HealthServer::initial_connection_window_size`] still serve Check, List, and Watch
//! at a 64 KiB stream / 128 KiB connection window, including over TLS, mTLS,
//! Unix, and [`crate::Server::serve_connection`]. Distinct from wrapping only a
//! Greeter server.
//! A [`HealthClient`] pool larger than
//! [`HealthServer::max_concurrent_connections`] fails the whole dial as
//! `UNAVAILABLE` on TLS, mTLS, and Unix. [`HealthClient::from_io_with`]
//! cannot pool. An interceptor
//! `Err` may carry [`crate::Status::with_error_details`]; those trailers reach
//! the client on Check, List, and Watch.
//! [`crate::Status::from_error_details`] is the typed bag after this health interceptor Err; those trailers reach the client without reading the body.
//! Distinct from a health handler Err: that is after the handler ran; this health interceptor Err is trailers without reading the body.
//! Distinct from a health server on_response Err: that is trailers-only after handler Ok; this health interceptor Err is trailers without reading the body.
//! Distinct from a health client interceptor Err: that is a local reject never opens a stream; this health interceptor Err is trailers without reading the body.
//! Distinct from a health StreamSender fail: that is trailers after any messages already sent; this health interceptor Err is trailers without reading the body.
//! Distinct from a health client interceptor: that runs on the outbound call before the stream opens; this health interceptor runs on the inbound RPC before the handler.
//! A handler `Err` may carry the same packed
//! status; those trailers reach the client on Check, List, and Watch.
//! [`crate::Status::from_error_details`] is the typed bag after this health handler Err; those trailers reach the client.
//! Distinct from a health interceptor Err: that is trailers without reading the body; this health handler Err is after the handler ran.
//! Distinct from a health client interceptor Err: that is a local reject never opens a stream; this health handler Err is after the handler ran.
//! Distinct from a health server on_response Err: that is trailers-only after handler Ok; this health handler Err is after the handler ran.
//! Distinct from a health client on_response Err: that fails the Call after a successful receive; this health handler Err is after the handler ran.
//! Distinct from a health StreamSender fail: that is trailers after any messages already sent; this health handler Err is after the handler ran.
//! Watch
//! [`crate::StreamSender::fail`] after a streamed DATA frame ships those
//! trailers the same way (Check is unary: no response DATA then trailers).
//! [`crate::Status::from_error_details`] is the typed bag after this health StreamSender fail on a server response producer; those trailers ship after any messages already sent.
//! Distinct from a health handler Err: that is after the handler ran; this health StreamSender fail is trailers after any messages already sent.
//! Distinct from a health interceptor Err: that is trailers without reading the body; this health StreamSender fail is trailers after any messages already sent.
//! Distinct from a health server on_response Err: that is trailers-only after handler Ok; this health StreamSender fail is trailers after any messages already sent.
//! Distinct from a health client interceptor Err: that is a local reject never opens a stream; this health StreamSender fail is trailers after any messages already sent.
//! Distinct from a health client on_response Err: that fails the Call after a successful receive; this health StreamSender fail is trailers after any messages already sent.
//! [`crate::Status::from_error_details`] is the typed bag after this health server on_response Err; a local reject is trailers-only after handler Ok.
//! Distinct from a health handler Err: that is after the handler ran; this health server on_response Err is trailers-only after handler Ok.
//! Distinct from a health interceptor Err: that is trailers without reading the body; this health server on_response Err is trailers-only after handler Ok.
//! Distinct from a health StreamSender fail: that is trailers after any messages already sent; this health server on_response Err is trailers-only after handler Ok.
//! [`crate::Status::from_error_details`] is the typed bag after this health client on_response Err; a local reject fails the Call after a successful receive.
//! Distinct from a health handler Err: that is after the handler ran; this health client on_response Err fails the Call after a successful receive.
//! Distinct from a health client interceptor Err: that is a local reject never opens a stream; this health client on_response Err fails the Call after a successful receive.
//! Unix (`serve_unix` /
//! `connect_unix`), TLS (`serve_tls` / `connect_tls`), and
//! [`crate::Server::serve_connection`] / [`crate::Channel::from_io`] serve
//! Check, List, and Watch. Check of a never-set name is [`crate::Code::NotFound`]. Watch
//! of that name streams [`ServingStatus::ServiceUnknown`]. Watch streams later
//! `set_not_serving` / [`HealthReporter::shutdown`] / [`HealthReporter::resume`]
//! changes, including over TLS, mTLS, Unix, and [`crate::Channel::from_io`].
//! Dropping a Watch releases the subscription without waiting for a status
//! change on those transports. [`HealthServer::send_compressed`] gzips Check,
//! List, and Watch when the client advertises gzip. [`HealthClient::connect_lazy`],
//! [`HealthClient::connect_tls_lazy`] (including mTLS), and
//! [`HealthClient::connect_unix_lazy`] retry Check, List, and Watch until listen when
//! wait-for-ready is set on the request, the client, or a client interceptor.
//! `Request::set_wait_for_ready(false)` and a client interceptor
//! `set_wait_for_ready(false)` opt out of a client default. A waiting Call's
//! deadline applies on those dialers. A client interceptor sees [`crate::Outgoing`]
//! path / service / method / `:authority` / `:scheme` on Check, List, and Watch.
//! [`crate::Outgoing::connected`] is the live-socket snapshot on this health client interceptor path ([`crate::Channel::connected`]), taken when the interceptor runs. Distinct from wait-for-ready: a lazy first RPC sees `false` even when that overlay is on.
//! [`crate::Status::from_error_details`] is the typed bag after this health client interceptor Err; a local reject never opens a stream.
//! Distinct from a health handler Err: that is after the handler ran; this health client interceptor Err is a local reject never opens a stream.
//! Distinct from a health client on_response Err: that fails the Call after a successful receive; this health client interceptor Err is a local reject never opens a stream.
//! Distinct from a health interceptor Err: that is trailers without reading the body; this health client interceptor Err is a local reject never opens a stream.
//! Distinct from a health StreamSender fail: that is trailers after any messages already sent; this health client interceptor Err is a local reject never opens a stream.
//! Distinct from [`crate::Channel::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this health client interceptor already ran, so a local Err never consumes that budget.
//! Distinct from a health interceptor: that runs on the inbound RPC before the handler; this health client interceptor runs on the outbound call before the stream opens.
//!
//! ```no_run
//! # async fn example() -> Result<(), pbrs_grpc::Status> {
//! let (health, reporter) = pbrs_grpc::health::service();
//! reporter.set_serving("helloworld.Greeter");
//! pbrs_grpc::Router::new()
//!     .add_service(health)
//!     .serve("127.0.0.1:50051".parse().expect("addr"))
//!     .await?;
//! # Ok(())
//! # }
//! ```

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/health.rs"));

use crate::request::{Request, Response};
use crate::status::Status;
use crate::stream::Streaming;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::watch;

type Snapshot = Arc<HashMap<String, ServingStatus>>;

/// Drives the serving status of the empty name (the process) and of named
/// services. Cheap to clone; every clone talks to the same map.
#[derive(Clone)]
pub struct HealthReporter {
    tx: watch::Sender<Snapshot>,
}

impl fmt::Debug for HealthReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HealthReporter")
            .field("services", &self.tx.borrow().len())
            .field("watchers", &self.tx.receiver_count())
            .finish()
    }
}

impl HealthReporter {
    fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(String::new(), ServingStatus::Serving);
        let (tx, _) = watch::channel(Arc::new(map));
        Self { tx }
    }

    /// Mark `name` as [`ServingStatus::Serving`]. The empty name is the process.
    pub fn set_serving(&self, name: impl AsRef<str>) {
        self.set(name.as_ref(), ServingStatus::Serving);
    }

    /// Mark `name` as [`ServingStatus::NotServing`].
    pub fn set_not_serving(&self, name: impl AsRef<str>) {
        self.set(name.as_ref(), ServingStatus::NotServing);
    }

    /// Replace the status of `name`.
    pub fn set_status(&self, name: impl AsRef<str>, status: ServingStatus) {
        self.set(name.as_ref(), status);
    }

    /// Forget `name`. [`Health::check`] then returns `NOT_FOUND`;
    /// [`Health::watch`] reports [`ServingStatus::ServiceUnknown`];
    /// [`Health::list`] omits the name.
    pub fn clear(&self, name: impl AsRef<str>) {
        let name = name.as_ref();
        self.tx.send_modify(|snap| {
            let mut map = HashMap::clone(snap);
            map.remove(name);
            *snap = Arc::new(map);
        });
    }

    /// Current status of `name`, if it has been set.
    ///
    /// The empty name is the process and starts as [`ServingStatus::Serving`].
    /// Unknown names are `None`, matching [`Health::check`]'s `NOT_FOUND`.
    /// [`Health::watch`] reports [`ServingStatus::ServiceUnknown`] for those
    /// instead.
    #[must_use]
    pub fn status(&self, name: impl AsRef<str>) -> Option<ServingStatus> {
        self.snapshot().get(name.as_ref()).copied()
    }

    /// Known service names, including the process (`""`).
    ///
    /// Sorted lexicographically, so the empty process name is first. Names
    /// you never set are omitted. [`Health::list`] returns the same set.
    /// After [`Self::shutdown`] the names are
    /// still here, all [`ServingStatus::NotServing`]. After [`Self::resume`]
    /// they are all [`ServingStatus::Serving`].
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.snapshot().keys().cloned().collect();
        names.sort();
        names
    }

    /// In-flight [`Health::watch`] streams.
    ///
    /// Each Watch holds a subscription until the client cancels, the stream
    /// ends, or this reporter is dropped. Zero when no one is watching.
    #[must_use]
    pub fn watchers(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Mark every known name, including the process (`""`), as
    /// [`ServingStatus::NotServing`].
    ///
    /// Load balancers that probe `Check` then stop sending traffic before
    /// you [`crate::Server::serve_until_shutdown`]. Names you never set stay
    /// unknown (`NOT_FOUND` on `Check`, `SERVICE_UNKNOWN` on `Watch`).
    /// [`Self::set_serving`] after this is allowed and brings a name back.
    /// [`Self::resume`] marks every known name [`ServingStatus::Serving`]
    /// without enumerating them.
    pub fn shutdown(&self) {
        self.set_all(ServingStatus::NotServing);
    }

    /// Mark every known name, including the process (`""`), as
    /// [`ServingStatus::Serving`].
    ///
    /// Inverse of [`Self::shutdown`]: abort a drain and advertise again
    /// without enumerating names. Unknown names stay unknown.
    /// [`Self::set_not_serving`] after this is allowed.
    pub fn resume(&self) {
        self.set_all(ServingStatus::Serving);
    }

    fn set_all(&self, status: ServingStatus) {
        self.tx.send_modify(|snap| {
            let mut map = HashMap::clone(snap);
            for value in map.values_mut() {
                *value = status;
            }
            *snap = Arc::new(map);
        });
    }

    fn set(&self, name: &str, status: ServingStatus) {
        self.tx.send_modify(|snap| {
            let mut map = HashMap::clone(snap);
            map.insert(name.to_owned(), status);
            *snap = Arc::new(map);
        });
    }

    fn snapshot(&self) -> Snapshot {
        Arc::clone(&self.tx.borrow())
    }

    fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.tx.subscribe()
    }
}

/// Implementation of [`Health`] backed by a [`HealthReporter`].
#[derive(Clone)]
pub struct HealthService {
    reporter: HealthReporter,
}

impl fmt::Debug for HealthService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HealthService")
            .field("reporter", &self.reporter)
            .finish()
    }
}

impl Health for HealthService {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let name = request
            .get_ref()
            .service()
            .to_str()
            .unwrap_or("")
            .to_owned();
        match self.reporter.snapshot().get(&name).copied() {
            Some(status) => Ok(Response::new(response(status))),
            None => Err(Status::not_found(format!("unknown service {name:?}"))),
        }
    }

    async fn list(
        &self,
        request: Request<HealthListRequest>,
    ) -> Result<Response<HealthListResponse>, Status> {
        drop(request);
        let snap = self.reporter.snapshot();
        let mut msg = HealthListResponse::new();
        for (name, status) in snap.iter() {
            msg.statuses_mut().insert(name.as_str(), response(*status));
        }
        Ok(Response::new(msg))
    }

    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Streaming<HealthCheckResponse>>, Status> {
        let cancelled = request.cancelled();
        let name = request
            .get_ref()
            .service()
            .to_str()
            .unwrap_or("")
            .to_owned();
        let mut rx = self.reporter.subscribe();
        let (tx, stream) = Streaming::channel(4);
        drop(tokio::spawn(async move {
            let mut last = None;
            tokio::pin!(cancelled);
            loop {
                let status = rx
                    .borrow_and_update()
                    .get(&name)
                    .copied()
                    .unwrap_or(ServingStatus::ServiceUnknown);
                if last != Some(status) {
                    last = Some(status);
                    if tx.send(response(status)).await.is_err() {
                        break;
                    }
                }
                tokio::select! {
                    biased;
                    () = cancelled.as_mut() => break,
                    () = tx.closed() => break,
                    result = rx.changed() => {
                        if result.is_err() {
                            break;
                        }
                    }
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

fn response(status: ServingStatus) -> HealthCheckResponse {
    let mut msg = HealthCheckResponse::new();
    msg.set_status(status);
    msg
}

/// The standard health service and a reporter that drives it.
///
/// The empty service name starts as [`ServingStatus::Serving`]. Named services
/// are unknown until you [`HealthReporter::set_serving`] them.
#[must_use]
pub fn service() -> (HealthServer<HealthService>, HealthReporter) {
    let reporter = HealthReporter::new();
    (
        HealthServer::new(HealthService {
            reporter: reporter.clone(),
        }),
        reporter,
    )
}
