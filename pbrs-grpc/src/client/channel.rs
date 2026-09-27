//! Channel type: dialing, overlays, and connection metadata.

use super::pool::{self, ChannelInner, Endpoint};
use super::retry::{RetryStats, RetryStatsRecorder};
use crate::config::ChannelConfig;
use crate::interceptor::{ClientHook, ClientInterceptor, ResponseHook};
use crate::limits::ByteBudgetTracker;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::request::{Call, Request};
use crate::resolver::ResolverConfig;
use crate::service_config::{ServiceConfig, SharedServiceConfig};
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::status::Code;
use crate::status::Status;
#[allow(unused_imports, reason = "intra-doc links resolve against these names")]
use crate::stream::Streaming;
use crate::telemetry::{LifecycleObserver, ObserverChain, diagnostic_identity};
use crate::tls::ClientTls;
use http::HeaderValue;
use http::uri::Authority;
use std::fmt;
use std::net::SocketAddr;
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Where a [`Channel`] should dial.
///
/// Built with [`From`], so [`Channel::connect`] (and generated
/// `FooClient::connect`) takes a `SocketAddr`, a `&str` of the form
/// `host:port`, or a `String`.
///
/// Distinct from tonic's `Endpoint`, which takes an `http://` /
/// `https://` URI and infers TLS from the scheme. This kernel does
/// not parse that URI: TLS is [`Channel::connect_tls`] plus
/// [`crate::ClientTls`], Unix is [`Channel::connect_unix`] (a
/// filesystem path, not `unix://`), and `:authority` overlay is
/// [`Channel::origin`]. A URI-shaped string is
/// [`Code::InvalidArgument`] at connect, including
/// [`Channel::connect_lazy`], so wait-for-ready does not retry it.
/// Distinct from a malformed `host:port`, which is
/// [`Code::Unavailable`]. Distinct from tonic's
/// `Endpoint::from_static`, which is that URI constructor.
/// Distinct from grpc-go `NewClient("dns:///host:port")`, which takes
/// a resolver URI (`dns:///` / `passthrough:///` / `xds:///`). Those
/// are [`Code::InvalidArgument`] at connect, not a silent resolver.
/// [`ChannelConfig::connections`] pools to one `host:port`; it does
/// not speak xDS.
/// Distinct from grpc-go `unix-abstract://`, which names a Linux
/// abstract Unix socket. That is [`Code::InvalidArgument`] at connect,
/// not a silent `\0`-prefixed Unix dial. Distinct from tonic `unix://`
/// (also [`Code::InvalidArgument`], a filesystem URI) and from
/// [`Channel::connect_unix`], which takes a filesystem [`std::path::Path`].
/// Distinct from a `grpc://` / `grpcs://` URI, which some stacks use as
/// an h2c / TLS gRPC target. That is [`Code::InvalidArgument`] at connect,
/// not a silent strip of the scheme and not a silent
/// [`Channel::connect_tls`]. Distinct from tonic `https://`, which infers
/// TLS from the scheme. Distinct from a malformed `host:port`, which is
/// [`Code::Unavailable`].
///
/// ```
/// use pbrs_grpc::Target;
///
/// let from_addr: Target = "127.0.0.1:50051".parse::<std::net::SocketAddr>()?.into();
/// let from_name: Target = "greeter.internal:50051".into();
/// assert_eq!(from_addr.authority(), "127.0.0.1:50051");
/// assert_eq!(from_name.authority(), "greeter.internal:50051");
/// # Ok::<(), std::net::AddrParseError>(())
/// ```
///
/// ```
/// use pbrs_grpc::{Channel, Code};
///
/// let err = Channel::connect_lazy("https://example.com:443").expect_err("uri");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// assert!(err.message().contains("not a tonic http://"));
/// let err = Channel::connect_lazy("http://127.0.0.1:50051").expect_err("h2c");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// let err = Channel::connect_lazy("unix:///tmp/grpc.sock").expect_err("unix");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// ```
///
/// ```
/// use pbrs_grpc::{Channel, Code};
///
/// let err = Channel::connect_lazy("dns:///localhost:50051").expect_err("dns");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// assert!(err.message().contains("not a grpc-go dns:///"));
/// let err = Channel::connect_lazy("passthrough:///127.0.0.1:50051").expect_err("pass");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// let err = Channel::connect_lazy("xds:///backend").expect_err("xds");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// ```
///
/// ```
/// use pbrs_grpc::{Channel, Code};
///
/// let err = Channel::connect_lazy("unix-abstract:///grpc.sock").expect_err("abstract");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// assert!(err.message().contains("not a grpc-go unix-abstract://"));
/// ```
///
/// ```
/// use pbrs_grpc::{Channel, Code};
///
/// let err = Channel::connect_lazy("grpc://127.0.0.1:50051").expect_err("grpc");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// assert!(err.message().contains("not a grpc://"));
/// let err = Channel::connect_lazy("grpcs://example.com:443").expect_err("grpcs");
/// assert_eq!(err.code(), Code::InvalidArgument);
/// ```
#[derive(Clone, Debug)]
pub struct Target {
    authority: String,
}

impl Target {
    /// The `host:port` string used both for DNS and for `:authority`.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub(crate) fn parse(&self) -> Result<Authority, Status> {
        let authority = self.authority.as_str();
        if tonic_style_channel_uri(authority) {
            return Err(Status::invalid_argument(format!(
                "Target {authority:?} is host:port, not a tonic http://, https://, or unix:// URI; Channel::connect_tls dials TLS, Channel::connect_unix takes a filesystem path, Channel::origin overlays :authority"
            )));
        }
        if grpc_go_resolver_uri(authority) {
            return Err(Status::invalid_argument(format!(
                "Target {authority:?} is host:port, not a grpc-go dns:///, passthrough:///, or xds:/// URI; Channel::connect dials host:port, ChannelConfig::connections pools to one authority, Channel::connect_unix takes a filesystem path"
            )));
        }
        if grpc_go_unix_abstract_uri(authority) {
            return Err(Status::invalid_argument(format!(
                "Target {authority:?} is host:port, not a grpc-go unix-abstract:// URI; Channel::connect_unix takes a filesystem path, not a Linux abstract name; Channel::connect dials host:port"
            )));
        }
        if grpc_scheme_uri(authority) {
            return Err(Status::invalid_argument(format!(
                "Target {authority:?} is host:port, not a grpc:// or grpcs:// URI; Channel::connect dials host:port, Channel::connect_tls dials TLS, Channel::origin overlays :authority"
            )));
        }
        self.authority.parse().map_err(|e| {
            Status::unavailable(format!("invalid authority {:?}: {e}", self.authority))
        })
    }
}

fn uri_scheme(s: &str) -> Option<&str> {
    s.split_once("://").map(|(scheme, _)| scheme)
}

// tonic Endpoint::from_static takes http:// / https://; grpc-go also uses
// unix://. None of those are a Target.
fn tonic_style_channel_uri(s: &str) -> bool {
    uri_scheme(s).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("unix")
    })
}

// grpc-go NewClient takes dns:/// / passthrough:/// / xds:/// resolver URIs.
fn grpc_go_resolver_uri(s: &str) -> bool {
    uri_scheme(s).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("dns")
            || scheme.eq_ignore_ascii_case("passthrough")
            || scheme.eq_ignore_ascii_case("xds")
    })
}

// grpc-go also takes unix-abstract:// for Linux abstract sockets. That is
// not Target, not tonic unix://, and not Channel::connect_unix.
fn grpc_go_unix_abstract_uri(s: &str) -> bool {
    uri_scheme(s).is_some_and(|scheme| scheme.eq_ignore_ascii_case("unix-abstract"))
}

// Some stacks take grpc:// (h2c) / grpcs:// (TLS). That is not Target, not
// tonic https:// TLS inference, and not Channel::connect_tls.
fn grpc_scheme_uri(s: &str) -> bool {
    uri_scheme(s).is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("grpc") || scheme.eq_ignore_ascii_case("grpcs")
    })
}

impl From<SocketAddr> for Target {
    fn from(addr: SocketAddr) -> Self {
        Self {
            authority: addr.to_string(),
        }
    }
}

impl From<&str> for Target {
    fn from(authority: &str) -> Self {
        Self {
            authority: authority.to_owned(),
        }
    }
}

impl From<String> for Target {
    fn from(authority: String) -> Self {
        Self { authority }
    }
}

impl From<&String> for Target {
    fn from(authority: &String) -> Self {
        Self {
            authority: authority.clone(),
        }
    }
}

/// A prior-knowledge HTTP/2 connection (or small pool) to a gRPC server.
///
/// Cloning is cheap and shares the underlying connections, so a `Channel` is
/// meant to be cloned into every task that needs it. A received
/// [`Streaming`] also holds the HTTP/2 driver, so dropping the last `Channel`
/// clone after headers still lets you read the stream to the end, including
/// over TLS, mTLS, Unix, and [`Self::from_io`].
///
/// If a connection dies — peer `GOAWAY`, TCP reset, keepalive timeout — the
/// next RPC on that slot dials again, including over TLS, mTLS, and Unix.
/// [`Self::from_io`] cannot redial. Unary and server-streaming calls that
/// observe the death after the slot still looked live (a raced `GOAWAY`)
/// retry that redial once on the same RPC, matching gRPC transparent retry.
/// Client-streaming and bidi retry that same redial once when HEADERS never
/// went out; after the stream is open they do not, because the caller already
/// holds the send half.
/// There is no grpc-go `WithDisableRetry` DialOption: that disables
/// service-config retries and does not impact transparent retries.
/// Service-config `retryPolicy`/`hedgingPolicy` attach with
/// [`Self::service_config`]; omit the document (or the method's policy) for
/// no policy retries, in which case application retries stay at the call
/// site ([`Code::is_retryable`]). Transparent retry cannot be turned off.
/// Distinct from [`Self::from_io`] (no transparent retry).
/// There is no grpc-go `WithMaxCallAttempts`: that caps retries and hedging
/// per call (default 5; values below 2 become 5). Here `maxAttempts` comes
/// from the method's `retryPolicy`/`hedgingPolicy` (values above 5 count as
/// 5); transparent retry is at most once on top and cannot be raised.
/// Distinct from [`Code::is_retryable`] (application retries at the call
/// site, unbounded by this kernel).
/// A healthy connection that is only waiting for a free stream
/// (`SETTINGS_MAX_CONCURRENT_STREAMS`) is not replaced. Redial is part of
/// the RPC: it is cancelled if the [`Call`] is cancelled, and it fails with
/// [`Code::DeadlineExceeded`] if the request deadline elapses while connecting.
///
/// A connection with no outstanding RPCs is closed after
/// [`ChannelConfig::max_connection_idle`] when that is set. Keepalive PINGs
/// do not keep it. The next RPC of every call shape redials, including over
/// TLS, mTLS, and Unix. [`Self::from_io`] cannot redial and fails with
/// [`Code::Unavailable`].
/// A connection is also closed after
/// [`ChannelConfig::max_connection_age`] when that is set, even while RPCs
/// are in flight; in-flight RPCs get [`ChannelConfig::max_connection_age_grace`]
/// to finish. Distinct from idle: a long-running stream is not idle, but it
/// does not postpone age. The next RPC of every call shape redials, including over
/// TLS, mTLS, and Unix. [`Self::from_io`] cannot redial and fails with
/// [`Code::Unavailable`].
/// Keepalive PINGs do not postpone age.
/// [`Self::connected`] is whether any slot still holds that socket. Distinct
/// from gRPC `GetState`: it does not dial, wait, or remember a failed attempt.
///
/// [`Self::connect_lazy`] skips the initial dial so a client can exist
/// before its server. The first RPC fails fast with [`Code::Unavailable`]
/// unless that request set [`Request::set_wait_for_ready`] or the channel
/// was built with [`Self::wait_for_ready`] / [`ChannelConfig::wait_for_ready`],
/// in which case it retries until connected, cancelled, or the deadline fires.
///
/// A dial is bounded by [`ChannelConfig::connect_timeout`] (default 20 s),
/// covering TCP or Unix connect, optional TLS, and the peer's HTTP/2
/// SETTINGS. A peer that accepts the socket and never speaks fails with
/// [`Code::Unavailable`] instead of hanging forever. Connection refused
/// still fails immediately.
/// TCP sockets always set `TCP_NODELAY` (Nagle off) at connect; Unix and
/// [`Self::from_io`] skip that. There is no `tcp_nodelay` setter. Distinct
/// from tonic, which defaults Nagle off but lets you turn it back on.
/// There is no grpc-go `WithNoProxy`: grpc-go honors `HTTPS_PROXY` by
/// default; that DialOption disables it. TCP `host:port` is dialed
/// directly; there is no HTTP CONNECT proxy. There is no
/// `WithLocalDNSResolution`: that resolves locally so the proxy CONNECT
/// sees an IP. Distinct from [`Self::from_io`] (already-connected bytes,
/// not a proxy bypass). Distinct from [`Self::connect_unix`] (filesystem
/// path; this dialer is skipped). Distinct from
/// [`ChannelConfig::local_address`] (source bind, not proxy).
/// [`ChannelConfig::local_address`] binds the TCP source before connect
/// (TLS and mTLS included). Unix and [`Self::from_io`] skip that bind.
/// Distinct from [`crate::Rpc::local_addr`], which is the accepted
/// interface after the handshake.
///
/// On Unix, [`Self::connect_unix`] / [`Self::connect_unix_lazy`] speak the
/// same protocol over a domain socket. TLS is TCP-only.
///
/// [`Self::from_io`] speaks over an already-connected byte stream and cannot
/// redial. Pair it with [`crate::Server::serve_connection`] for in-process
/// tests.
///
/// Generated `FooClient` types wrap these constructors as `FooClient::connect`,
/// `connect_tls`, `connect_unix`, and `from_io`, so a service crate rarely
/// constructs a `Channel` by hand. `FooClient::authority`, `FooClient::scheme`,
/// and `FooClient::grpc_user_agent` read the same values interceptors see on
/// [`Outgoing`](crate::Outgoing).
///
/// [`Self::unary`], [`Self::server_streaming`], [`Self::client_streaming`],
/// and [`Self::bidi`] are first-class for a hand-written [`crate::Service`];
/// generated clients call the same methods.
///
/// [`Self::intercept`] runs on every outbound RPC when the method is
/// called — before the stream opens and before the [`Call`] is polled —
/// which is how a client injects auth metadata, a default deadline, or
/// wait-for-ready without touching each call.
///
/// After connect, [`Self::timeout`], [`Self::wait_for_ready`],
/// [`Self::send_compressed`], [`Self::gzip_compression_level`],
/// [`Self::accept_compressed`], the two message-size caps /
/// [`Self::message_limits`], [`Self::stream_buffer`],
/// [`Self::max_send_buffer_size`], [`Self::max_concurrent_rpcs`],
/// [`Self::https_scheme`] (for [`Self::from_io`]), and [`Self::origin`]
/// overlay this clone.
/// There is no grpc-go `WithDefaultCallOptions`: that is a DialOption bag of
/// per-call options (`WaitForReady`, `MaxCallRecvMsgSize`, compressor, …)
/// applied as channel defaults. These methods are typed overlays on this
/// clone, not a `CallOption` list and not a DialOption. Distinct from
/// grpc-go `WithDefaultServiceConfig` (JSON service config, not CallOptions).
/// Distinct from [`ChannelConfig`] (handshake `Copy` fields). Distinct from
/// [`Self::intercept`] (per-RPC mutation after connect).
/// Read those overlays with [`Self::rpc_timeout`], [`Self::waits_for_ready`],
/// [`Self::compresses_outbound`], [`Self::gzip_level`], [`Self::accepts_compressed`],
/// [`Self::concurrent_rpc_limit`], [`Self::stream_buffer_size`],
/// [`Self::send_buffer_size`], [`Self::limits`], and
/// [`Self::config`]. Keepalive, idle, age, TCP
/// keepalive, local bind, connection count, HTTP/2 windows, the HPACK table, the
/// small-DATA budget, the rapid-reset cap, the locally-reset stream memory
/// and duration, and the protocol-error RST cap are set at handshake ([`ChannelConfig`] /
/// [`Self::connect_with`]).
///
/// [`Debug`] prints the authority, pool size, and config. It does not dump
/// live HTTP/2 state.
///
/// ```no_run
/// use pbrs_grpc::{Channel, ChannelConfig};
///
/// # async fn run() -> Result<(), pbrs_grpc::Status> {
/// // One connection, 4 MiB inbound cap.
/// let channel = Channel::connect("127.0.0.1:50051").await?;
///
/// // Four connections, so four cores can drive HTTP/2 framing.
/// let pooled = Channel::connect_with(
///     "127.0.0.1:50051",
///     ChannelConfig::new().connections(4),
/// )
/// .await?;
///
/// // No dial until the first RPC. Pair with `Channel::wait_for_ready`
/// // or `Request::set_wait_for_ready`.
/// let late = Channel::connect_lazy("127.0.0.1:50051")?;
/// # let _ = (channel, pooled, late);
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Channel {
    pub(crate) inner: Arc<ChannelInner>,
    pub(crate) config: ChannelConfig,
    pub(crate) interceptors: Arc<[ClientHook]>,
    pub(crate) response_interceptors: Arc<[ResponseHook]>,
    /// Shared across clones of this lineage. `None` when the cap is unset.
    pub(crate) rpc_slots: Option<Arc<Semaphore>>,
    pub(crate) byte_budget: ByteBudgetTracker,
    pub(crate) user_agent: HeaderValue,
    /// `:scheme` this clone sends. TLS channels start `true`; [`Self::from_io`]
    /// starts `false` until [`Self::https_scheme`].
    pub(crate) https: bool,
    /// `:authority` this clone sends. Defaults to the dial [`Target`];
    /// [`Self::origin`] overrides it.
    pub(crate) authority: Authority,
    pub(crate) observer: Option<Arc<dyn LifecycleObserver>>,
    /// Attached JSON service config (A6/A21/A24), if any. Clones share it.
    pub(crate) service_config: SharedServiceConfig,
    /// Channel-scoped retry statistics. Clones share it.
    pub(crate) retry_stats: Arc<RetryStatsRecorder>,
    /// Binary logger. Clones share it.
    pub(crate) binlog: Option<Arc<crate::binlog::BinaryLogger>>,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field(
                "authority",
                &diagnostic_identity(self.authority.as_str(), None),
            )
            .field("endpoint", &"[REDACTED]")
            .field("connections", &self.inner.slots.len())
            .field("tls", &self.inner.tls.is_some())
            .field("https", &self.https)
            .field("interceptors", &self.interceptors.len())
            .field("response_interceptors", &self.response_interceptors.len())
            .field("config", &self.config)
            .field("byte_budget_allocated", &self.byte_budget.allocated())
            .field("user_agent", &"[REDACTED]")
            .field("observer", &self.observer.is_some())
            .field("binary_logger", &self.binlog.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Target;
    use std::net::SocketAddr;

    #[test]
    fn targets_accept_addresses_and_names() {
        let addr: SocketAddr = "127.0.0.1:50051".parse().expect("addr");
        assert_eq!(Target::from(addr).authority(), "127.0.0.1:50051");
        assert_eq!(Target::from("host:1").authority(), "host:1");
        assert_eq!(Target::from("host:1".to_owned()).authority(), "host:1");
    }

    #[test]
    fn bad_authority_is_unavailable_not_a_panic() {
        let err = Target::from("not a host").parse().expect_err("invalid");
        assert_eq!(err.code(), crate::status::Code::Unavailable);
    }

    #[test]
    fn tonic_style_channel_uri_is_invalid_argument() {
        for uri in [
            "https://example.com:443",
            "http://127.0.0.1:50051",
            "unix:///tmp/grpc.sock",
            "HTTPS://example.com:443",
        ] {
            let err = Target::from(uri).parse().expect_err(uri);
            assert_eq!(err.code(), crate::status::Code::InvalidArgument);
            assert!(
                err.message().contains("not a tonic http://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc-go unix-abstract://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc://"),
                "{}",
                err.message()
            );
        }
        let err = Target::from("not a host").parse().expect_err("malformed");
        assert_eq!(err.code(), crate::status::Code::Unavailable);
        assert_eq!(
            Target::from("https://example.com:443").authority(),
            "https://example.com:443"
        );
    }

    #[test]
    fn grpc_go_resolver_uri_is_invalid_argument() {
        for uri in [
            "dns:///localhost:50051",
            "passthrough:///127.0.0.1:50051",
            "xds:///backend",
            "DNS:///example.com:443",
        ] {
            let err = Target::from(uri).parse().expect_err(uri);
            assert_eq!(err.code(), crate::status::Code::InvalidArgument);
            assert!(
                err.message().contains("not a grpc-go dns:///"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a tonic http://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc-go unix-abstract://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc://"),
                "{}",
                err.message()
            );
        }
        let err = Target::from("https://example.com:443")
            .parse()
            .expect_err("tonic");
        assert!(
            err.message().contains("not a tonic http://"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc-go dns:///"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc-go unix-abstract://"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc://"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn grpc_go_unix_abstract_uri_is_invalid_argument() {
        for uri in [
            "unix-abstract:///grpc.sock",
            "unix-abstract://localhost/grpc.sock",
            "UNIX-ABSTRACT:///grpc.sock",
        ] {
            let err = Target::from(uri).parse().expect_err(uri);
            assert_eq!(err.code(), crate::status::Code::InvalidArgument);
            assert!(
                err.message().contains("not a grpc-go unix-abstract://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a tonic http://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc-go dns:///"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc://"),
                "{}",
                err.message()
            );
        }
        let err = Target::from("unix:///tmp/grpc.sock")
            .parse()
            .expect_err("tonic unix");
        assert!(
            err.message().contains("not a tonic http://"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc-go unix-abstract://"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc://"),
            "{}",
            err.message()
        );
        let err = Target::from("not a host").parse().expect_err("malformed");
        assert_eq!(err.code(), crate::status::Code::Unavailable);
    }

    #[test]
    fn grpc_scheme_uri_is_invalid_argument() {
        for uri in [
            "grpc://127.0.0.1:50051",
            "grpcs://example.com:443",
            "GRPC://localhost:50051",
            "GRPCS://example.com:443",
        ] {
            let err = Target::from(uri).parse().expect_err(uri);
            assert_eq!(err.code(), crate::status::Code::InvalidArgument);
            assert!(err.message().contains("not a grpc://"), "{}", err.message());
            assert!(
                !err.message().contains("not a tonic http://"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc-go dns:///"),
                "{}",
                err.message()
            );
            assert!(
                !err.message().contains("not a grpc-go unix-abstract://"),
                "{}",
                err.message()
            );
        }
        let err = Target::from("https://example.com:443")
            .parse()
            .expect_err("tonic https");
        assert!(
            err.message().contains("not a tonic http://"),
            "{}",
            err.message()
        );
        assert!(
            !err.message().contains("not a grpc://"),
            "{}",
            err.message()
        );
        let err = Target::from("not a host").parse().expect_err("malformed");
        assert_eq!(err.code(), crate::status::Code::Unavailable);
        assert!(
            !err.message().contains("not a grpc://"),
            "{}",
            err.message()
        );
    }

    #[test]
    fn channel_debug_masks_the_authority_and_target() {
        let channel = super::Channel::connect_lazy("127.0.0.1:9").expect("lazy");
        let dbg = format!("{channel:?}");
        assert!(dbg.contains("authority: \"[REDACTED]\""), "{dbg}");
        assert!(dbg.contains("endpoint: \"[REDACTED]\""), "{dbg}");
        assert!(dbg.contains("user_agent: \"[REDACTED]\""), "{dbg}");
        assert!(!dbg.contains("127.0.0.1:9"), "{dbg}");
        assert!(dbg.contains("connections: 1"), "{dbg}");
        assert!(dbg.contains("tls: false"), "{dbg}");
        assert!(dbg.contains("interceptors: 0"), "{dbg}");
        assert!(dbg.contains("response_interceptors: 0"), "{dbg}");
    }

    #[test]
    fn stream_buffer_overlays_a_live_channel() {
        let channel = super::Channel::connect_lazy("127.0.0.1:9")
            .expect("lazy")
            .stream_buffer(64);
        assert_eq!(channel.stream_buffer_size(), 64);
        assert_eq!(channel.config().stream_buffer_size(), 64);
    }

    #[test]
    fn send_buffer_overlays_a_live_channel() {
        let channel = super::Channel::connect_lazy("127.0.0.1:9")
            .expect("lazy")
            .max_send_buffer_size(123_456);
        assert_eq!(channel.send_buffer_size(), 123_456);
        assert_eq!(channel.config().send_buffer_size(), 123_456);
    }

    #[test]
    fn overlay_getters_read_timeout_wait_for_ready_and_gzip() {
        use std::time::Duration;

        let channel = super::Channel::connect_lazy("127.0.0.1:9").expect("lazy");
        assert_eq!(channel.rpc_timeout(), None);
        assert!(!channel.waits_for_ready());
        assert!(!channel.compresses_outbound());
        assert_eq!(channel.gzip_level(), 1);
        assert_eq!(channel.stream_buffer_size(), crate::DEFAULT_STREAM_BUFFER);
        assert_eq!(
            channel.send_buffer_size(),
            crate::DEFAULT_MAX_SEND_BUFFER_SIZE
        );
        assert_eq!(channel.limits(), crate::MessageLimits::default());
        let channel = channel
            .timeout(Duration::from_secs(5))
            .wait_for_ready()
            .send_compressed()
            .gzip_compression_level(9)
            .stream_buffer(32)
            .max_send_buffer_size(123_456)
            .message_limits(crate::MessageLimits::unlimited());
        assert_eq!(channel.rpc_timeout(), Some(Duration::from_secs(5)));
        assert!(channel.waits_for_ready());
        assert!(channel.compresses_outbound());
        assert_eq!(channel.gzip_level(), 9);
        assert_eq!(channel.stream_buffer_size(), 32);
        assert_eq!(channel.send_buffer_size(), 123_456);
        assert_eq!(channel.rpc_timeout(), channel.config().rpc_timeout());
        assert_eq!(
            channel.waits_for_ready(),
            channel.config().waits_for_ready()
        );
        assert_eq!(
            channel.compresses_outbound(),
            channel.config().compresses_outbound()
        );
        assert_eq!(channel.gzip_level(), channel.config().gzip_level());
        assert_eq!(
            channel.stream_buffer_size(),
            channel.config().stream_buffer_size()
        );
        assert_eq!(
            channel.send_buffer_size(),
            channel.config().send_buffer_size()
        );
        assert_eq!(channel.limits().max_decoding(), None);
        assert_eq!(channel.limits(), channel.config().limits());
        assert_eq!(
            super::Channel::connect_lazy("127.0.0.1:9")
                .expect("lazy")
                .gzip_compression_level(10)
                .gzip_level(),
            9
        );
    }
}
impl super::Channel {
    /// Dial `target` with default configuration: one connection, 4 MiB
    /// inbound cap. Applies to every call shape.
    /// Distinct from tonic's `Endpoint::from_static`, which takes an
    /// `http://` / `https://` URI: [`Target`] is `host:port` (see [`Target`]).
    /// Distinct from grpc-go `NewClient("dns:///host:port")`: still
    /// `host:port`, not a resolver URI (see [`Target`]).
    /// There is no grpc-go `WithBlock` DialOption: that makes deprecated
    /// `Dial` wait until READY. This constructor already waits for the TCP
    /// dial and HTTP/2 preface; there is no READY state. Distinct from
    /// [`Self::connect_lazy`] (first RPC dials). Distinct from
    /// [`Self::wait_for_ready`] (RPC queue, not Dial). Distinct from
    /// [`Self::connected`] (live-socket snapshot). Distinct from gRPC
    /// `GetState` / `WaitForStateChange`. There is no
    /// `WithReturnConnectionError`: handshake failure is the returned
    /// [`Status`].
    pub async fn connect(target: impl Into<Target>) -> Result<Self, Status> {
        Self::connect_with(target, ChannelConfig::default()).await
    }

    /// Dial `target` with `config`. Applies to every call shape.
    ///
    /// Opens [`ChannelConfig::connections`] connections up front; RPCs are
    /// spread over them round-robin. All of them must succeed. A slot that
    /// later dies is redialed on the next RPC that lands on it. Each dial is
    /// bounded by [`ChannelConfig::connect_timeout`].
    /// There is no grpc-go `WithConnectParams`: that is exponential reconnect
    /// backoff plus `MinConnectTimeout` for creating and maintaining
    /// connections. A dead slot is redialed on the next RPC with no
    /// channel-level reconnect backoff. There is no `WithBackoffMaxDelay` /
    /// `WithBackoffConfig` (deprecated aliases). Distinct from
    /// [`Self::wait_for_ready`] (handshake retries at `[20, 40, 80, 160, 320,
    /// 640, 1000]` ms, not channel reconnect). Distinct from
    /// [`ChannelConfig::connect_timeout`] (max dial bound, default 20 s; not
    /// grpc-go `MinConnectTimeout`, also default 20 s). Distinct from
    /// transparent retry (one redial of the same RPC, not connect backoff).
    pub async fn connect_with(
        target: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        pool::connect_inner(target.into(), config, None).await
    }

    /// Shorthand for [`Self::connect_with`] with `connections` connections.
    ///
    /// One connection means one `h2` driver task, so concurrent small RPCs
    /// serialize behind a single core's framing work. Pooling is the fix.
    /// Applies to every call shape.
    /// TLS (including mTLS) pooling is [`Self::connect_tls_with`] plus
    /// [`ChannelConfig::connections`]; Unix is [`Self::connect_unix_with`].
    /// [`Self::from_io`] cannot pool.
    pub async fn connect_pool(
        target: impl Into<Target>,
        connections: usize,
    ) -> Result<Self, Status> {
        Self::connect_with(target, ChannelConfig::default().connections(connections)).await
    }

    /// Dial `target` over TLS with default configuration.
    ///
    /// `target` is the TCP address; [`ClientTls`] carries the name verified
    /// against the certificate, which can be different (dial `127.0.0.1`,
    /// verify `localhost`). Applies to every call shape.
    /// There is no grpc-go `WithInsecure`: modern grpc-go `NewClient` requires
    /// credentials (`insecure.NewCredentials()` or TLS). [`Self::connect`] is
    /// h2c by default; TLS is this constructor. There is no
    /// `WithTransportCredentials` DialOption (TLS is this constructor plus
    /// [`crate::ClientTls`]). Distinct from a skip-verify constructor (there
    /// is none). Distinct from [`Self::https_scheme`] (`from_io` label; it
    /// does not handshake).
    /// A dropped socket during the handshake is [`Code::Unavailable`].
    /// [`std::io::ErrorKind::AddrNotAvailable`] is that same dropped-socket
    /// set (unroutable bind), Distinct from leftover kinds which stay
    /// [`Code::Unauthenticated`]. Certificate and protocol failures (rustls `InvalidData`) are
    /// [`Code::Unauthenticated`]. Distinct from [`Status`]'s
    /// `From<std::io::Error>`: that maps local I/O `InvalidData` to
    /// [`Code::Internal`].
    pub async fn connect_tls(target: impl Into<Target>, tls: ClientTls) -> Result<Self, Status> {
        Self::connect_tls_with(target, ChannelConfig::default(), tls).await
    }

    /// Dial `target` over TLS with `config`. Applies to every call shape.
    /// [`ChannelConfig::connections`] opens that many TLS sockets (including
    /// mTLS); all of them must succeed. [`Self::from_io`] cannot pool.
    pub async fn connect_tls_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        tls: ClientTls,
    ) -> Result<Self, Status> {
        pool::connect_inner(target.into(), config, Some(tls)).await
    }

    /// Build a channel that dials on the first RPC instead of now.
    /// Applies to every call shape.
    /// There is no grpc-go `WithContextDialer`: that is a DialOption plugging a
    /// custom `func(context.Context, string) (net.Conn, error)` that still
    /// dials. `WithDialer` is the deprecated context-less form. The first RPC
    /// still dials TCP `host:port`; there is no replacement hook. Distinct from
    /// tonic `Endpoint::connect_with_connector` (tower `Service<Uri>` that still
    /// dials). Distinct from [`Self::from_io`] (already-connected bytes; it
    /// does not dial). Distinct from [`Self::connect_unix`] (filesystem path,
    /// not a custom TCP dialer). Distinct from [`ChannelConfig::local_address`]
    /// (source bind, still this kernel's TCP dialer). Distinct from grpc-go
    /// `WithNoProxy` (proxy bypass, not a dial function). Distinct from grpc-go
    /// `WithBlock` (handshake wait, not a dial function).
    ///
    /// Invalid `target` still fails immediately. A closed port, a name that
    /// does not resolve, or a TLS handshake the peer refuses surfaces on the
    /// RPC as [`Code::Unavailable`] (including over TLS, mTLS, and Unix), or
    /// waits until the deadline if that RPC set [`Request::set_wait_for_ready`]
    /// or this channel used [`Self::wait_for_ready`].
    pub fn connect_lazy(target: impl Into<Target>) -> Result<Self, Status> {
        Self::connect_lazy_with(target, ChannelConfig::default())
    }

    /// [`Self::connect_lazy`] with `config`. Each slot dials when an RPC first
    /// lands on it, not all at once. Applies to every call shape.
    pub fn connect_lazy_with(
        target: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        pool::connect_lazy_inner(target.into(), config, None)
    }

    /// [`Self::connect_lazy`] over TLS. Applies to every call shape.
    pub fn connect_tls_lazy(target: impl Into<Target>, tls: ClientTls) -> Result<Self, Status> {
        Self::connect_tls_lazy_with(target, ChannelConfig::default(), tls)
    }

    /// [`Self::connect_lazy_with`] over TLS. Applies to every call shape.
    pub fn connect_tls_lazy_with(
        target: impl Into<Target>,
        config: ChannelConfig,
        tls: ClientTls,
    ) -> Result<Self, Status> {
        pool::connect_lazy_inner(target.into(), config, Some(tls))
    }

    /// Dial a Unix domain socket with default configuration.
    ///
    /// h2c only; TLS over a Unix socket is not supported. `:authority` is
    /// `localhost`. `path` is a filesystem path, not a `unix://` URI.
    /// Applies to every call shape.
    #[cfg(unix)]
    pub async fn connect_unix(path: impl AsRef<Path>) -> Result<Self, Status> {
        Self::connect_unix_with(path, ChannelConfig::default()).await
    }

    /// [`Self::connect_unix`] with `config`. Applies to every call shape.
    /// [`ChannelConfig::connections`] opens that many Unix sockets; all of
    /// them must succeed. [`Self::from_io`] cannot pool.
    #[cfg(unix)]
    pub async fn connect_unix_with(
        path: impl AsRef<Path>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        pool::connect_unix_inner(path.as_ref(), config).await
    }

    /// [`Self::connect_unix`] that dials on the first RPC instead of now.
    /// Applies to every call shape.
    #[cfg(unix)]
    pub fn connect_unix_lazy(path: impl AsRef<Path>) -> Result<Self, Status> {
        Self::connect_unix_lazy_with(path, ChannelConfig::default())
    }

    /// [`Self::connect_unix_lazy`] with `config`. Applies to every call shape.
    #[cfg(unix)]
    pub fn connect_unix_lazy_with(
        path: impl AsRef<Path>,
        config: ChannelConfig,
    ) -> Result<Self, Status> {
        Ok(pool::finish_channel(
            Endpoint::Unix(path.as_ref().to_owned()),
            pool::unix_authority(),
            config,
            None,
            pool::empty_slots(config.connection_count()),
            None,
        ))
    }

    /// Connect to a resolver target URI: `dns:///`, `passthrough:`,
    /// `ipv4:`, `ipv6:`, `unix:`, or `unix-abstract:`.
    ///
    /// The initial lookup runs now, so an unresolvable target fails here;
    /// sockets stay lazy and each dial reads the current snapshot without
    /// awaiting, so resolver updates never block picks. `dns:` needs
    /// explicit bounds via
    /// [`ResolverConfig::with_dns`](crate::resolver::ResolverConfig::with_dns);
    /// the static schemes use
    /// [`ResolverConfig::static_only`](crate::resolver::ResolverConfig::static_only).
    /// Plain `host:port` is not a URI: it stays on [`Self::connect`].
    /// Applies to every call shape.
    pub async fn connect_uri(uri: &str, resolver: ResolverConfig) -> Result<Self, Status> {
        Self::connect_uri_with(uri, ChannelConfig::default(), resolver).await
    }

    /// [`Self::connect_uri`] with `config`. Applies to every call shape.
    pub async fn connect_uri_with(
        uri: &str,
        config: ChannelConfig,
        resolver: ResolverConfig,
    ) -> Result<Self, Status> {
        pool::connect_uri_inner(uri, config, None, resolver).await
    }

    /// [`Self::connect_uri`] over TLS. The configured [`ClientTls`]
    /// server name verifies every resolved address; a resolved IP never
    /// becomes the SNI name. Rejects `unix:`/`unix-abstract:` targets.
    /// Applies to every call shape.
    pub async fn connect_tls_uri(
        uri: &str,
        tls: ClientTls,
        resolver: ResolverConfig,
    ) -> Result<Self, Status> {
        Self::connect_tls_uri_with(uri, ChannelConfig::default(), tls, resolver).await
    }

    /// [`Self::connect_tls_uri`] with `config`. Applies to every call shape.
    pub async fn connect_tls_uri_with(
        uri: &str,
        config: ChannelConfig,
        tls: ClientTls,
        resolver: ResolverConfig,
    ) -> Result<Self, Status> {
        pool::connect_uri_inner(uri, config, Some(tls), resolver).await
    }

    /// Speak gRPC over an already-connected byte stream.
    ///
    /// The channel has one slot and cannot redial: if the stream dies, the
    /// next RPC fails with [`Code::Unavailable`]. There is no TCP connect,
    /// no TLS, and no Unix path. Pair with [`crate::Server::serve_connection`]
    /// over `tokio::io::duplex` or `tokio::net::UnixStream::pair`.
    ///
    /// `authority` is the HTTP/2 `:authority` sent on every RPC.
    /// [`ChannelConfig::connections`] is ignored (always one slot).
    /// [`Outgoing::scheme`](crate::Outgoing::scheme) is `http`. If the byte
    /// stream is already encrypted, call [`Self::https_scheme`].
    /// Applies to every call shape.
    /// There is no tonic `Endpoint::connect_with_connector`: that is a tower
    /// `Service<Uri>` that still dials. This stream is already connected; there
    /// is no connector and no URI. Distinct from [`Self::connect_unix`]
    /// (filesystem path, not a connector). [`ChannelConfig::connect_timeout`]
    /// still bounds the HTTP/2 preface. Distinct from `tower` integration,
    /// which is protobuf-tonic keeping tonic.
    ///
    /// ```no_run
    /// # async fn run(
    /// #     io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    /// # ) -> Result<(), pbrs_grpc::Status> {
    /// let channel = pbrs_grpc::Channel::from_io(io, "localhost").await?;
    /// # let _ = channel;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_io<IO>(io: IO, authority: impl Into<Target>) -> Result<Self, Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        Self::from_io_with(io, authority, ChannelConfig::default()).await
    }

    /// Send `:scheme https` from a [`Self::from_io`] channel.
    ///
    /// [`Self::connect_tls`] already does. [`Self::from_io`] has no TLS
    /// config, so it reports `http` even when you already encrypted the
    /// stream. Call this when you drove TLS yourself. Pair the server with
    /// [`crate::Incoming::peer`] / [`crate::ConnectionInfo::with_scheme`]
    /// when the accept loop should not trust the peer's preface.
    ///
    /// No-op on TCP and Unix channels: those take `:scheme` from whether
    /// the channel was built with [`crate::ClientTls`]. Read the result with
    /// [`Self::scheme`]. Applies to every call shape on this clone.
    #[must_use]
    pub fn https_scheme(mut self) -> Self {
        if matches!(self.inner.endpoint, Endpoint::Once) {
            self.https = true;
        }
        self
    }

    /// Override the HTTP/2 `:authority` this clone sends.
    ///
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`]. Dial still uses the [`Target`] passed to
    /// [`Self::connect`] / [`Self::connect_tls`] / [`Self::connect_unix`];
    /// this overlay does not rebind the socket. Distinct from
    /// [`crate::ClientTls`]: that is SNI / certificate name, not this header.
    /// Distinct from tonic's `Endpoint::origin`, which takes a `Uri` and
    /// also sets `:scheme`; scheme on this kernel is [`Self::connect_tls`]
    /// or [`Self::https_scheme`].
    /// There is no grpc-go `WithAuthority`: that sets `:authority` and the
    /// TLS authentication server name. This overlay is `:authority` only.
    /// Distinct from [`crate::ClientTls`] (SNI / certificate name). Distinct
    /// from tonic `Endpoint::origin` (Uri, also `:scheme`). There is no
    /// `CallAuthority`: interceptors cannot override `:authority` per call.
    ///
    /// Unix defaults to `localhost`; this overlay can replace it.
    /// [`Self::from_io`] already takes an authority argument; this overlay
    /// replaces that value on the clone.
    /// Invalid HTTP `:authority` is [`Code::InvalidArgument`].
    /// Read the result with [`Self::authority`]. Client interceptors see
    /// the same string as [`crate::Outgoing::authority`].
    ///
    /// ```
    /// # fn demo(channel: pbrs_grpc::Channel) -> Result<(), pbrs_grpc::Status> {
    /// let channel = channel.origin("greeter.internal:50051")?;
    /// assert_eq!(channel.authority(), "greeter.internal:50051");
    /// # let _ = channel;
    /// # Ok(())
    /// # }
    /// ```
    pub fn origin(mut self, authority: impl Into<Target>) -> Result<Self, Status> {
        let target = authority.into();
        self.authority = target.authority().parse().map_err(|e| {
            Status::invalid_argument(format!("invalid origin {:?}: {e}", target.authority()))
        })?;
        Ok(self)
    }

    /// [`Self::from_io`] with `config`. Applies to every call shape.
    /// [`ChannelConfig::connections`] is forced to 1: one duplex is one
    /// HTTP/2 connection.
    pub async fn from_io_with<IO>(
        io: IO,
        authority: impl Into<Target>,
        config: ChannelConfig,
    ) -> Result<Self, Status>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let target = authority.into();
        let parsed = target.parse()?;
        let config = config.connections(1);
        let timeout = config.handshake_timeout();
        let send = match tokio::time::timeout(timeout, pool::finish_h2(config, io)).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(Status::unavailable(format!(
                    "connect {parsed}: timed out after {timeout:?}"
                )));
            }
        };
        Ok(pool::finish_channel(
            Endpoint::Once,
            parsed,
            config,
            None,
            pool::live_slots(vec![send]),
            None,
        ))
    }

    /// The configuration in effect. Applies to every call shape.
    #[must_use]
    pub fn config(&self) -> ChannelConfig {
        self.config
    }

    /// Attach a JSON service-config document (gRPC A6/A21/A24).
    ///
    /// The document supplies per-method defaults (`timeout`, `waitForReady`,
    /// message caps, `retryPolicy`/`hedgingPolicy`), channel-wide
    /// `retryThrottling`, and `loadBalancingConfig` selection. Precedence is
    /// explicit request overlay first, then this channel's typed overlays
    /// ([`Self::timeout`], [`Self::wait_for_ready`], message caps), then the
    /// method entry; message caps combine tightest-wins. Cloning shares the
    /// parsed document and its throttling bucket.
    ///
    /// Returns [`Code::InvalidArgument`] naming the first invalid entry when
    /// the document fails validation; the channel is unchanged.
    /// Distinct from [`Self::config`]: that is typed handshake fields; this is
    /// the JSON document a resolver would supply.
    ///
    /// ```
    /// use pbrs_grpc::Channel;
    ///
    /// # fn demo(channel: Channel) -> Result<Channel, pbrs_grpc::Status> {
    /// let channel = channel.service_config(
    ///     r#"{"methodConfig": [{"name": [{}], "timeout": "30s"}]}"#,
    /// )?;
    /// # Ok(channel)
    /// # }
    /// ```
    pub fn service_config(mut self, json: &str) -> Result<Self, Status> {
        let parsed = ServiceConfig::parse(json)?;
        self.service_config = SharedServiceConfig::new(parsed);
        Ok(self)
    }

    /// The attached service-config document, if any.
    /// Distinct from [`Self::config`]: that is typed handshake fields; this is the parsed JSON document.
    #[must_use]
    pub fn service_config_doc(&self) -> Option<ServiceConfig> {
        self.service_config.get().map(|state| state.config.clone())
    }

    /// A snapshot of this channel's retry statistics.
    ///
    /// Counters cover unary and server-streaming calls and are shared by
    /// clones. Recording is always on and lock-free; GF-01 exports these to
    /// OpenTelemetry.
    /// Distinct from [`Self::service_config_doc`]: that is the policy; this
    /// is what the policy did.
    ///
    /// ```
    /// use pbrs_grpc::Channel;
    ///
    /// # fn demo(channel: Channel) {
    /// let stats = channel.retry_stats();
    /// assert_eq!(stats.committed_ok + stats.committed_err, stats.calls);
    /// # }
    /// ```
    #[must_use]
    pub fn retry_stats(&self) -> RetryStats {
        self.retry_stats.snapshot()
    }

    /// Whether any pool slot currently holds a live HTTP/2 connection.
    ///
    /// Distinct from gRPC `GetState` / `WaitForStateChange`: this does not
    /// dial, wait, or remember `TRANSIENT_FAILURE`. A `true` value can still
    /// lose the race with a peer `GOAWAY`. After
    /// [`ChannelConfig::max_connection_idle`] or
    /// [`ChannelConfig::max_connection_age`], this is `false` until the next
    /// RPC redials. [`Self::from_io`] stays `false` after that close.
    /// Applies to every call shape, including over TLS, mTLS, and Unix.
    /// Distinct from [`Self::wait_for_ready`]: that overlay queues; this is a live snapshot.
    /// Client interceptors see the same snapshot as [`crate::Outgoing::connected`].
    #[must_use]
    pub fn connected(&self) -> bool {
        let mut contended = false;
        for slot in &self.inner.slots {
            match slot.try_lock() {
                Ok(guard) => {
                    if guard.send.is_some() {
                        return true;
                    }
                }
                Err(_) => contended = true,
            }
        }
        contended
    }

    /// Cap inbound messages at `limit` bytes. Default 4 MiB.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`].
    #[must_use]
    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_decoding_message_size(limit);
        self
    }

    /// Cap outbound messages at `limit` bytes. Default unlimited.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`].
    #[must_use]
    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {
        self.config = self.config.max_encoding_message_size(limit);
        self
    }

    /// Replace both message caps at once. See [`ChannelConfig::message_limits`].
    ///
    /// Overlay: applies to RPCs from this clone. Does not change how a dead
    /// slot is redialed. Applies to every call shape.
    /// Distinct from [`Self::max_encoding_message_size`] /
    /// [`Self::max_decoding_message_size`]. Oversize is
    /// [`Code::ResourceExhausted`], including over TLS, mTLS, Unix, and
    /// [`Self::from_io`].
    #[must_use]
    pub fn message_limits(mut self, limits: crate::MessageLimits) -> Self {
        self.config = self.config.message_limits(limits);
        self
    }

    /// Configured message caps. See [`Self::message_limits`].
    /// Applies to every call shape.
    /// Distinct from [`Self::message_limits`], which sets them.
    /// Distinct from [`Self::stream_buffer_size`]: that is queue depth, not uncompressed protobuf bytes.
    /// Distinct from [`Self::send_buffer_size`]: that is the HTTP/2 send buffer, not these caps.
    /// Same overlay as [`crate::Outgoing::limits`].
    #[must_use]
    pub fn limits(&self) -> crate::MessageLimits {
        self.config.limits()
    }

    /// gzip every unary and server-streaming request payload, and every
    /// [`crate::StreamSender::send`] on a client- or bidi-stream opened from
    /// this channel. Applies to every call shape, including over TLS, mTLS,
    /// Unix, and [`Self::from_io`].
    ///
    /// Off by default. Equivalent to [`ChannelConfig::send_compressed`].
    /// There is no grpc-go `WithCompressor`: that is a DialOption plugging a
    /// custom `encoding.Compressor` (deprecated; `encoding.RegisterCompressor`
    /// is global). This overlay is gzip on or off, not a compressor plugin.
    /// There is no `WithDecompressor` (deprecated inbound plugin). Distinct
    /// from encodings other than gzip (`UNIMPLEMENTED`, not a plugin). Distinct
    /// from [`Self::gzip_compression_level`] (deflate effort, not a plugin).
    /// Distinct from grpc-go `UseCompressor` (a CallOption name, not this
    /// overlay).
    /// A request that already called [`crate::Request::set_compress`] is
    /// left alone, including `set_compress(false)` to opt out. Interceptors
    /// run before a client- or bidi-stream [`crate::StreamSender`] is
    /// returned, so [`crate::Outgoing::set_compress`] stamps that sender too.
    /// [`crate::Outgoing::clear_compress`] then
    /// [`crate::Outgoing::set_compress`] from [`Self::compresses_outbound`]
    /// reapplies this overlay.
    #[must_use]
    pub fn send_compressed(mut self) -> Self {
        self.config = self.config.send_compressed(true);
        self
    }

    /// Deflate effort for outbound gzip. Default 1 (`flate2` fast).
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`]. See [`ChannelConfig::gzip_compression_level`].
    /// Distinct from [`Self::send_compressed`], which is on or off.
    /// 0 stores; 9 is best. Overlay: does not change how a dead slot is
    /// redialed.
    #[must_use]
    pub fn gzip_compression_level(mut self, level: u32) -> Self {
        self.config = self.config.gzip_compression_level(level);
        self
    }

    /// Inflate inbound gzip. Default `true`. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`Self::from_io`].
    /// Passing `false` omits gzip from `grpc-accept-encoding` and refuses a
    /// `grpc-encoding: gzip` reply as [`Code::Unimplemented`]. Distinct from
    /// [`Self::send_compressed`], which is outbound. See
    /// [`ChannelConfig::accept_compressed`].
    #[must_use]
    pub fn accept_compressed(mut self, accept: bool) -> Self {
        self.config = self.config.accept_compressed(accept);
        self
    }

    /// Default per-RPC deadline when the request omits one. Applies to every
    /// call shape, including over TLS, mTLS, Unix, and [`Self::from_io`].
    /// See [`ChannelConfig::timeout`].
    ///
    /// A request that already called [`crate::Request::set_timeout`] is left
    /// alone. Interceptors run after this fill and can still set or
    /// [`crate::Outgoing::clear_timeout`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.timeout(timeout);
        self
    }

    /// Wait for a connection instead of failing fast. See
    /// [`ChannelConfig::wait_for_ready`]. Applies to every call shape.
    ///
    /// A request that already called [`crate::Request::set_wait_for_ready`]
    /// is left alone. Interceptors run after this fill and can still set
    /// or clear it.
    /// Distinct from [`Self::connected`]: that is a live snapshot; this fill still queues when a slot is empty.
    #[must_use]
    pub fn wait_for_ready(mut self) -> Self {
        self.config = self.config.wait_for_ready(true);
        self
    }

    /// Default per-RPC deadline when the request omits one. Applies to every
    /// call shape. Distinct from [`Self::timeout`], which sets it.
    #[must_use]
    pub fn rpc_timeout(&self) -> Option<Duration> {
        self.config.rpc_timeout()
    }

    /// Whether this clone waits for a connection instead of failing fast.
    /// See [`Self::wait_for_ready`]. Applies to every call shape.
    /// Distinct from [`Self::wait_for_ready`], which sets it.
    /// Distinct from [`Self::connected`]: that is a live snapshot, not this overlay.
    #[must_use]
    pub fn waits_for_ready(&self) -> bool {
        self.config.waits_for_ready()
    }

    /// Whether this clone gzips outbound payloads.
    /// See [`Self::send_compressed`]. Applies to every call shape.
    /// Distinct from [`Self::send_compressed`], which sets it.
    #[must_use]
    pub fn compresses_outbound(&self) -> bool {
        self.config.compresses_outbound()
    }

    /// Configured outbound gzip deflate level. See [`Self::gzip_compression_level`].
    /// Applies to every call shape.
    /// Distinct from [`Self::gzip_compression_level`], which sets it.
    #[must_use]
    pub fn gzip_level(&self) -> u32 {
        self.config.gzip_level()
    }

    /// Whether inbound gzip is inflated. Default `true`.
    /// See [`Self::accept_compressed`]. Applies to every call shape.
    /// Distinct from [`Self::accept_compressed`], which sets it.
    /// Distinct from [`crate::Rpc::accepts_gzip`], which is the peer's
    /// `grpc-accept-encoding`.
    #[must_use]
    pub fn accepts_compressed(&self) -> bool {
        self.config.accepts_compressed()
    }

    /// How many messages sit between a client-streaming caller and the wire.
    /// See [`ChannelConfig::stream_buffer`].
    ///
    /// Applies to client-streaming and bidi request streams opened from this
    /// clone. Unary and server-streaming have no request stream to queue.
    /// Overlay: does not change already-open streams or how a dead slot is
    /// redialed.
    #[must_use]
    pub fn stream_buffer(mut self, messages: usize) -> Self {
        self.config = self.config.stream_buffer(messages);
        self
    }

    /// Configured outbound streaming queue depth. See [`Self::stream_buffer`].
    /// Applies to client-streaming and bidi request streams.
    /// Distinct from [`Self::stream_buffer`], which sets it.
    /// Distinct from [`Self::message_limits`]: that is message size, not queue depth.
    #[must_use]
    pub fn stream_buffer_size(&self) -> usize {
        self.config.stream_buffer_size()
    }

    /// Write-time HTTP/2 send buffer threshold for outbound DATA on this clone.
    /// See [`ChannelConfig::max_send_buffer_size`].
    ///
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`]. Overlay: does not change how a dead slot is
    /// redialed; the handshake h2 send buffer stays the dial-time value.
    /// Distinct from [`Self::stream_buffer`]: that is decoded-message queue depth, not this send buffer.
    /// Distinct from [`Self::message_limits`]: that is uncompressed protobuf bytes, not this send buffer.
    /// Distinct from [`crate::Server::max_send_buffer_size`]: that is the server write buffer, not this client overlay.
    #[must_use]
    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {
        self.config = self.config.max_send_buffer_size(bytes);
        self.byte_budget = ByteBudgetTracker::with_limit(bytes);
        self
    }

    /// Set a transport byte budget for queued and in-flight buffers on this channel.
    #[must_use]
    pub fn byte_budget(mut self, limit: usize) -> Self {
        self.byte_budget = ByteBudgetTracker::with_limit(limit);
        self
    }

    /// Attach an existing [`crate::ByteBudgetTracker`] to this channel.
    #[must_use]
    pub fn with_byte_budget_tracker(mut self, tracker: ByteBudgetTracker) -> Self {
        self.byte_budget = tracker;
        self
    }

    /// Byte budget tracker in effect.
    #[must_use]
    pub fn byte_budget_tracker(&self) -> &ByteBudgetTracker {
        &self.byte_budget
    }

    /// Number of bytes currently allocated in transport buffers on this channel.
    #[must_use]
    pub fn byte_budget_allocated(&self) -> usize {
        self.byte_budget.allocated()
    }

    /// The configured byte budget limit, if any.
    #[must_use]
    pub fn byte_budget_limit(&self) -> Option<usize> {
        self.byte_budget.limit()
    }

    /// Whether transport buffer allocation is currently zero.
    #[must_use]
    pub fn is_byte_budget_quiescent(&self) -> bool {
        self.byte_budget.is_quiescent()
    }

    /// Configured write-time HTTP/2 send buffer. See [`Self::max_send_buffer_size`].
    /// Applies to every call shape.
    /// Distinct from [`Self::max_send_buffer_size`], which sets it.
    /// Distinct from [`Self::stream_buffer_size`]: that is queue depth, not this send buffer.
    /// Distinct from [`Self::message_limits`]: that is message size, not this send buffer.
    #[must_use]
    pub fn send_buffer_size(&self) -> usize {
        self.config.send_buffer_size()
    }

    /// Cap how many RPCs this clone's channel will run at once, across every
    /// connection. Applies to every call shape, including over TLS, mTLS,
    /// Unix, and [`Self::from_io`].
    ///
    /// Further RPCs are refused with [`Code::ResourceExhausted`] before the
    /// stream opens. Distinct from HTTP/2 `SETTINGS_MAX_CONCURRENT_STREAMS`
    /// (a well-behaved peer waits) and from [`crate::Server::max_concurrent_rpcs`]
    /// (the server refuses inbound). Distinct from [`Self::wait_for_ready`],
    /// which waits for a connection rather than refusing. Disabled by default.
    /// Clones share the budget. Calling this twice replaces the cap.
    /// Overlay: does not change how a dead slot is redialed.
    /// A server-streaming or bidi slot is held until the received
    /// [`Streaming`] is dropped.
    ///
    /// Equivalent to [`ChannelConfig::max_concurrent_rpcs`].
    #[must_use]
    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {
        self.config = self.config.max_concurrent_rpcs(n);
        self.rpc_slots = pool::rpc_slots_from(self.config);
        self
    }

    /// Configured channel-wide RPC cap, if any. See [`Self::max_concurrent_rpcs`].
    /// Applies to every call shape. Distinct from [`Self::max_concurrent_rpcs`],
    /// which sets it.
    #[must_use]
    pub fn concurrent_rpc_limit(&self) -> Option<usize> {
        self.config.concurrent_rpc_limit()
    }

    /// Prefix the kernel `user-agent`, matching grpc-go `WithUserAgent`.
    /// Applies to every call shape, including over TLS, mTLS, Unix, and
    /// [`Self::from_io`]. Inserting `user-agent` into request metadata cannot
    /// replace this value on those transports. Distinct from
    /// [`crate::Outgoing::set_user_agent`], which prefixes this RPC.
    /// [`crate::Request::set_user_agent`] is the same prefix at the call site.
    ///
    /// `user_agent("my-app/1.0")` sends `my-app/1.0 pbrs-grpc/<version>`.
    /// The kernel suffix is always present so a peer can identify the stack.
    /// Empty or whitespace-only prefix restores the kernel identity alone.
    ///
    /// ```
    /// # fn demo(channel: pbrs_grpc::Channel) -> Result<(), pbrs_grpc::Status> {
    /// let channel = channel.user_agent("inventory/2.1")?;
    /// assert!(channel.grpc_user_agent().starts_with("inventory/2.1 "));
    /// # let _ = channel;
    /// # Ok(())
    /// # }
    /// ```
    pub fn user_agent(mut self, prefix: impl AsRef<str>) -> Result<Self, Status> {
        self.user_agent = crate::wire::user_agent_value(prefix.as_ref())?;
        Ok(self)
    }

    /// The `user-agent` sent on every RPC. Applies to every call shape,
    /// including over TLS, mTLS, Unix, and [`Self::from_io`].
    #[must_use]
    pub fn grpc_user_agent(&self) -> &str {
        self.user_agent.to_str().unwrap_or(crate::wire::DEFAULT_UA)
    }

    /// Run `interceptor` on every outbound RPC before the stream opens.
    /// Applies to every call shape.
    /// Calling this twice stacks: the first interceptor runs first. The
    /// interceptor sees the method path, service, method, `:authority`,
    /// `:scheme`, `user-agent`, and message caps, and can set metadata, a
    /// user-agent prefix ([`crate::Outgoing::set_user_agent`]), a
    /// timeout / deadline Instant, wait-for-ready, compression, or typed
    /// extensions.
    /// [`crate::Outgoing::user_agent_is_set`] is occupancy after this Channel intercept, so a later interceptor can prefix only when unset.
    /// [`crate::Outgoing::wait_for_ready_is_set`] is occupancy after this Channel intercept, so a later interceptor can fill wait-for-ready only when unset.
    /// [`crate::Outgoing::compress_is_set`] is occupancy after this Channel intercept, so a later interceptor can fill compress only when unset.
    /// Channel overlays (`rpc_timeout`, `waits_for_ready`, `compresses_outbound`, `gzip_level`, `accepts_compressed`, `concurrent_rpc_limit`, `stream_buffer_size`, `send_buffer_size`, `limits`) are visible even after `clear_*` opts out of
    /// the already-applied default.
    /// [`crate::Outgoing::gzip_level`] is deflate effort.
    /// Distinct from [`crate::Outgoing::compresses_outbound`] (on or off).
    /// An interceptor cannot change it.
    /// [`crate::Outgoing::accepts_compressed`] is the inbound gzip overlay
    /// (default on).
    /// [`crate::Outgoing::limits`] is the channel message-cap overlay.
    /// Same overlay as [`crate::Channel::limits`].
    /// [`crate::Outgoing::concurrent_rpc_limit`] is the channel RPC cap overlay.
    /// Distinct from [`crate::Outgoing::waits_for_ready`]: that waits for a connection; this refuses extras.
    /// [`crate::Outgoing::stream_buffer_size`] is the outbound streaming queue overlay.
    /// Distinct from [`crate::Outgoing::limits`]: that is message size, not queue depth.
    /// [`crate::Outgoing::send_buffer_size`] is the outbound HTTP/2 send buffer overlay.
    /// Distinct from [`crate::Outgoing::stream_buffer_size`]: that is queue depth, not this send buffer.
    /// [`crate::Outgoing::connected`] is the live-socket snapshot
    /// ([`crate::Channel::connected`]), taken when this interceptor runs.
    /// Distinct from wait-for-ready: a lazy first RPC sees `false` even when
    /// that overlay is on.
    /// Values the caller put on [`crate::Request::extensions_mut`] are
    /// visible; stacked interceptors share that map.
    ///
    /// Interceptors run when [`Self::unary`] / [`Self::server_streaming`] /
    /// [`Self::client_streaming`] / [`Self::bidi`] (and generated methods)
    /// return, not when the [`crate::Call`] is first polled. `Err` fails that
    /// Call on poll, including [`crate::Status::with_error_details`]; nothing
    /// is sent. A local [`crate::Status::with_error_details`] is
    /// [`crate::Status::rpc`] / [`crate::Status::error_details`] on that Call
    /// for every call shape. [`crate::Outgoing::set_timeout`] is that Call's deadline on
    /// every call shape. [`crate::Outgoing::clear_timeout`] opts out of the
    /// channel timeout on every call shape. [`crate::Outgoing::clear_compress`] then
    /// [`crate::Outgoing::set_compress`] from [`Self::compresses_outbound`]
    /// reapplies channel gzip on every call shape. [`crate::Outgoing::set_compress`]
    /// stamps [`crate::StreamSender::compress`] on client-streaming and bidi
    /// request streams. Outgoing getters apply to
    /// every call shape. [`crate::Request::set_user_agent`] is the same prefix
    /// at the call site; an interceptor [`crate::Outgoing::set_user_agent`]
    /// that runs after wins.
    /// [`crate::Outgoing::clear_user_agent`] restores the channel user-agent after this Channel intercept prefix.
    /// [`crate::Outgoing::clear_wait_for_ready`] restores the channel wait-for-ready overlay after this Channel intercept choice.
    /// [`crate::Outgoing::clear_timeout`] opts out of the channel timeout after this Channel intercept choice.
    /// [`crate::Outgoing::clear_compress`] then [`crate::Outgoing::set_compress`] from [`Self::compresses_outbound`] reapplies channel gzip after this Channel intercept choice.
    /// [`crate::Status::from_error_details`] is the typed bag after this Channel intercept Err; a local reject never opens a stream.
    /// Distinct from a handler Err: that is after the handler ran; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a Channel on_response Err: that fails the Call after a successful receive; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a ResponseInterceptor Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a method-level on_response Err: that is trailers-only after handler Ok, or fails the Call after a successful receive; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a Server on_response Err: that is trailers-only after handler Ok; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a Router on_response Err: that is trailers-only after handler Ok; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from an Intercepted on_response Err: that is trailers-only after handler Ok; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a ServiceExt on_response Err: that is trailers-only after handler Ok; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a Server intercept Err: that is trailers without reading the body; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a Router intercept Err: that is trailers without reading the body; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a ServiceExt intercept Err: that is trailers without reading the body; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from an Interceptor Err: that is trailers without reading the body; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a method-level Interceptor Err: that is trailers without reading the body; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Channel intercept Err is a local reject never opens a stream.
    /// Distinct from [`Self::max_concurrent_rpcs`]: that takes a slot when the [`crate::Call`] is polled; this interceptor already ran, so a local Err never consumes that budget.
    /// Distinct from [`crate::Server::intercept`]: that runs on the inbound RPC before the handler; this runs on the outbound call before the stream opens.
    /// Distinct from [`crate::Server::intercept`]: that runs on the inbound RPC before the handler; this Channel intercept runs on the outbound call before the stream opens.
    /// Distinct from [`crate::Router::intercept`]: that runs on the inbound RPC before the handler; this Channel intercept runs on the outbound call before the stream opens.
    /// Distinct from [`Self::on_response`]: that runs after a successful receive; this runs on the outbound call before the stream opens.
    /// Distinct from [`Self::on_response`]: that runs after a successful receive; this Channel intercept runs on the outbound call before the stream opens.
    ///
    /// ```
    /// # fn demo(channel: pbrs_grpc::Channel) -> pbrs_grpc::Channel {
    /// channel.intercept(|call: &mut pbrs_grpc::Outgoing<'_>| {
    ///     let _ = (
    ///         call.path(),
    ///         call.service(),
    ///         call.method(),
    ///         call.authority(),
    ///         call.scheme(),
    ///         call.user_agent(),
    ///         call.user_agent_is_set(),
    ///         call.metadata(),
    ///         call.timeout(),
    ///         call.deadline(),
    ///         call.rpc_timeout(),
    ///         call.wait_for_ready(),
    ///         call.wait_for_ready_is_set(),
    ///         call.waits_for_ready(),
    ///         call.compress(),
    ///         call.compress_is_set(),
    ///         call.compresses_outbound(),
    ///         call.accepts_compressed(),
    ///         call.gzip_level(),
    ///         call.concurrent_rpc_limit(),
    ///         call.stream_buffer_size(),
    ///         call.send_buffer_size(),
    ///         call.limits(),
    ///         call.connected(),
    ///         call.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn intercept(self, interceptor: impl ClientInterceptor) -> Self {
        let mut hooks: Vec<ClientHook> = self.interceptors.iter().cloned().collect();
        hooks.push(Arc::new(interceptor));
        Self {
            interceptors: hooks.into(),
            ..self
        }
    }

    /// Run `interceptor` after a successful receive, before the [`Call`] is
    /// Ready.
    ///
    /// Closures implement [`crate::ResponseInterceptor`]. The hook sees
    /// [`crate::ResponseParts`]: headers, unary/client-streaming trailers,
    /// compress, and local [`crate::Response::extensions`]. A received reply
    /// starts empty; this is how a client inserts typed context after the
    /// peer cannot. Distinct from [`Self::intercept`], which runs before the
    /// stream opens. Calling this twice stacks: the first interceptor runs
    /// first. Applies to every call shape, including over TLS, mTLS, Unix,
    /// and [`Self::from_io`].
    /// `Err` fails that Call (the peer already sent OK), including
    /// [`crate::Status::with_error_details`]. A non-OK peer status skips
    /// this hook. On server-streaming and bidi, trailers on this envelope
    /// do not replace [`crate::Streaming::trailers`]. Generated clients
    /// expose the same method: `GreeterClient::new(ch).on_response(stamp)`.
    /// [`crate::ResponseParts::path`] is kernel-stamped.
    /// Distinct from [`crate::Outgoing::path`]: that is a client interceptor before send.
    /// [`crate::Response::gzip_level`] on a received reply is not the peer's deflate effort.
    /// Distinct from [`crate::Response::encoding`]: that is the received `grpc-encoding` token.
    /// [`crate::Response::compresses_outbound`] on a received reply is `false` (the overlay is not on the wire).
    /// [`crate::Response::accepts_gzip`] on a received reply is `false` (the advertisement is not on the reply wire).
    /// Distinct from [`crate::Response::encoding`]: that is received `grpc-encoding`, not `grpc-accept-encoding`.
    /// [`crate::Response::deadline`] on a received reply is `None` (the peer deadline is not on the wire).
    /// [`crate::Response::timeout`] on a received reply is `None` (the peer timeout is not on the reply wire).
    /// [`crate::Response::limits`] on a received reply is `None` (the peer encode cap is not on the wire).
    /// [`crate::Response::peer_timeout`] on a received reply is `None` (the client's `grpc-timeout` is not on the reply wire).
    /// [`crate::Response::rpc_timeout`] on a received reply is `None` (the server overlay is not on the reply wire).
    /// [`crate::Response::accepts_compressed`] on a received reply is `false` (this overlay is not a received-reply field).
    /// [`crate::Response::send_buffer_size`] on a received reply is `None` (the peer send buffer is not on the reply wire).
    /// [`crate::ResponseParts::compress_is_set`] is occupancy after this Channel on_response, so a later interceptor can fill compress only when unset.
    /// [`crate::ResponseParts::clear_compress`] drops a compress choice after this Channel on_response; a received reply has no server gzip overlay to restore.
    /// [`crate::Status::from_error_details`] is the typed bag after this Channel on_response Err; a local reject fails the Call after a successful receive.
    /// Distinct from a handler Err: that is after the handler ran; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from an Interceptor Err: that is trailers without reading the body; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a method-level Interceptor Err: that is trailers without reading the body; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a Channel intercept Err: that is a local reject never opens a stream; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a ClientInterceptor Err: that is a local reject never opens a stream; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a method-level intercept Err: that is a local reject never opens a stream; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a Server intercept Err: that is trailers without reading the body; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a Router intercept Err: that is trailers without reading the body; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a ServiceExt intercept Err: that is trailers without reading the body; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a Server on_response Err: that is trailers-only after handler Ok; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a Router on_response Err: that is trailers-only after handler Ok; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from an Intercepted on_response Err: that is trailers-only after handler Ok; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a ServiceExt on_response Err: that is trailers-only after handler Ok; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from a StreamSender fail: that is trailers after any messages already sent; this Channel on_response Err fails the Call after a successful receive.
    /// Distinct from [`Self::intercept`]: that runs on the outbound call before the stream opens; this Channel on_response runs after a successful receive.
    /// Distinct from [`crate::Server::on_response`]: that runs after the handler returns Ok; this Channel on_response runs after a successful receive.
    /// Distinct from [`crate::Router::on_response`]: that runs after the handler returns Ok; this Channel on_response runs after a successful receive.
    ///
    /// ```
    /// # fn demo(channel: pbrs_grpc::Channel) -> pbrs_grpc::Channel {
    /// channel.on_response(|parts: &mut pbrs_grpc::ResponseParts| {
    ///     let _ = (
    ///         parts.path(),
    ///         parts.service(),
    ///         parts.method(),
    ///         parts.metadata(),
    ///         parts.trailers(),
    ///         parts.compress(),
    ///         parts.compress_is_set(),
    ///         parts.encoding(),
    ///         parts.gzip_level(),
    ///         parts.compresses_outbound(),
    ///         parts.accepts_gzip(),
    ///         parts.deadline(),
    ///         parts.timeout(),
    ///         parts.limits(),
    ///         parts.peer_timeout(),
    ///         parts.rpc_timeout(),
    ///         parts.accepts_compressed(),
    ///         parts.send_buffer_size(),
    ///         parts.extensions(),
    ///     );
    ///     Ok(())
    /// })
    /// # }
    /// ```
    #[must_use]
    pub fn on_response(self, interceptor: impl crate::ResponseInterceptor) -> Self {
        let mut hooks: Vec<ResponseHook> = self.response_interceptors.iter().cloned().collect();
        hooks.push(Arc::new(interceptor));
        Self {
            response_interceptors: hooks.into(),
            ..self
        }
    }

    /// Register a lifecycle telemetry observer.
    ///
    /// The observer receives lifecycle events for outbound RPCs:
    /// call start/end, attempt start/end (including transparent retries), queue wait,
    /// bytes sent/received, transport reconnects, rejections, and cancellations.
    /// Paths and targets are raw identity. For metrics, register a
    /// [`crate::telemetry::BoundedMetricObserver`] with a reviewed
    /// [`crate::telemetry::MetricLabelPolicy`].
    ///
    /// Calling this twice stacks observers: the first registered observer runs first.
    #[must_use]
    pub fn observer<O: LifecycleObserver>(self, observer: O) -> Self {
        Self {
            observer: Some(match self.observer {
                None => Arc::new(observer),
                Some(prev) => Arc::new(ObserverChain::new(prev, Arc::new(observer))),
            }),
            ..self
        }
    }

    /// Record RPCs as `grpc.binarylog.v1` entries via `logger`.
    ///
    /// Applies to every call shape. Methods the logger's filter excludes
    /// cost one branch and log nothing. Credential headers are omitted and
    /// sensitive metadata values masked; see
    /// [`BinaryLogger`](crate::binlog::BinaryLogger).
    /// Distinct from [`Self::observer`]: that reports lifecycle events to
    /// telemetry; this records wire-faithful RPC logs to a sink.
    #[must_use]
    pub fn binary_logger(self, logger: crate::binlog::BinaryLogger) -> Self {
        Self {
            binlog: Some(Arc::new(logger)),
            ..self
        }
    }

    /// The `:authority` sent with every request.
    ///
    /// Taken from the [`Target`] used to dial, unless [`Self::origin`]
    /// overrode it on this clone. A [`SocketAddr`] is that
    /// address (`127.0.0.1:port`), not TLS SNI (`ClientTls` verifies a name
    /// such as `localhost` separately). Unix sockets send `localhost`
    /// until [`Self::origin`]. Applies to every call shape.
    /// Distinct from [`Self::origin`], which sets it.
    #[must_use]
    pub fn authority(&self) -> &str {
        self.authority.as_str()
    }

    /// HTTP/2 `:scheme` this clone sends.
    ///
    /// `https` after [`Self::connect_tls`] or [`Self::https_scheme`], otherwise
    /// `http`. Same string as [`crate::Outgoing::scheme`]. Applies to every
    /// call shape.
    #[must_use]
    pub fn scheme(&self) -> &'static str {
        if self.https { "https" } else { "http" }
    }
}
