//! MX-01 split of `super`: native_stubs (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{MethodDescriptor, ServiceDescriptor};

/// Fully qualified path to the kernel crate. Generated code never relies on a
/// `use` the surrounding module might not have.
pub(crate) const G: &str = "::pbrs_grpc";

/// The four gRPC call shapes, as they appear in generated signatures.
pub(crate) struct Shape {
    /// Request envelope contents in the service trait.
    trait_request: String,
    /// Response envelope contents in the service trait.
    trait_response: String,
    /// [`Rpc`] method the server dispatches through.
    dispatch: &'static str,
    /// Client method signature and body.
    client_arg: String,
    client_return: String,
    client_call: &'static str,
    /// Human description for the emitted doc comment.
    pub(crate) description: &'static str,
}

pub(crate) fn shape_of(m: &MethodDescriptor) -> Shape {
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    match (m.client_streaming, m.server_streaming) {
        (false, false) => Shape {
            trait_request: req.clone(),
            trait_response: resp.clone(),
            dispatch: "unary",
            client_arg: format!("{G}::Request<{req}>"),
            client_return: format!("{G}::Call<{G}::Response<{resp}>>"),
            client_call: "unary",
            description: "Unary",
        },
        (true, false) => Shape {
            trait_request: format!("{G}::Streaming<{req}>"),
            trait_response: resp.clone(),
            dispatch: "client_streaming",
            client_arg: format!("{G}::Request<()>"),
            client_return: format!("({G}::StreamSender<{req}>, {G}::Call<{G}::Response<{resp}>>)"),
            client_call: "client_streaming",
            description: "Client-streaming",
        },
        (false, true) => Shape {
            trait_request: req.clone(),
            trait_response: format!("{G}::Streaming<{resp}>"),
            dispatch: "server_streaming",
            client_arg: format!("{G}::Request<{req}>"),
            client_return: format!("{G}::Call<{G}::Response<{G}::Streaming<{resp}>>>"),
            client_call: "server_streaming",
            description: "Server-streaming",
        },
        (true, true) => Shape {
            trait_request: format!("{G}::Streaming<{req}>"),
            trait_response: format!("{G}::Streaming<{resp}>"),
            dispatch: "bidi_streaming",
            client_arg: format!("{G}::Request<()>"),
            client_return: format!(
                "({G}::StreamSender<{req}>, {G}::Call<{G}::Response<{G}::Streaming<{resp}>>>)"
            ),
            client_call: "bidi",
            description: "Bidirectional-streaming",
        },
    }
}

/// Emit `pbrs-grpc` stubs for one service: the handler trait, a server that
/// implements [`Service`](../../pbrs_grpc/trait.Service.html), and a client.
pub(crate) fn emit_kernel_service(src: &mut String, svc: &ServiceDescriptor) {
    let mut svc_clone = (*svc).clone();
    svc_clone.methods.sort_by(|a, b| a.name.cmp(&b.name));
    let svc = &svc_clone;
    let svc_ty = rust_ident(&svc.full_name);
    let trait_name = svc_ty.clone();
    let server = format!("{svc_ty}Server");
    let client = format!("{svc_ty}Client");
    let full_name = &svc.full_name;

    if build_server() {
        emit_kernel_trait(src, &trait_name, svc);
        emit_kernel_server(src, &trait_name, &server, full_name, svc);
    }
    if build_client() {
        emit_kernel_client(src, &trait_name, &client, full_name, svc);
    }
}

pub(crate) fn emit_kernel_trait(src: &mut String, trait_name: &str, svc: &ServiceDescriptor) {
    if !svc.comments.is_empty() {
        emit_doc_comments(src, &svc.comments, "");
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// The `{}` service.", svc.full_name);
    let _ = writeln!(src, "///");
    let _ = writeln!(
        src,
        "/// Implement this, then serve it with [`{trait_name}Server`]."
    );
    let _ = writeln!(src, "///");
    let _ = writeln!(
        src,
        "/// Methods you omit return [`{G}::Status::unimplemented`]."
    );
    if svc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    let _ = writeln!(src, "pub trait {trait_name}: Send + Sync + 'static {{");
    for m in &svc.methods {
        let shape = shape_of(m);
        let fn_name = to_snake(&m.name);
        if !m.comments.is_empty() {
            emit_doc_comments(src, &m.comments, "    ");
            let _ = writeln!(src, "    ///");
        }
        let _ = writeln!(src, "    /// {} call: `{}`.", shape.description, m.name);
        let req = rust_type_path(&m.input_type);
        let resp = rust_type_path(&m.output_type);
        match (m.client_streaming, m.server_streaming) {
            (false, false) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Unary `{req}` -> `{resp}`."
                );
            }
            (true, false) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Client-streaming stream of `{req}` -> `{resp}`."
                );
            }
            (false, true) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Server-streaming `{req}` -> stream of `{resp}`."
                );
            }
            (true, true) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Bidirectional-streaming stream of `{req}` -> stream of `{resp}`."
                );
            }
        }
        let _ = writeln!(src, "    ///");
        let _ = writeln!(
            src,
            "    /// The default answers [`{G}::Status::unimplemented`]."
        );
        let _ = writeln!(
            src,
            "    /// Inbound `grpc-encoding` is [`{G}::Request::encoding`] (`None` for identity)."
        );
        let _ = writeln!(
            src,
            "    /// Spawned work should await [`{G}::Request::cancelled`]; the kernel drops a still-pending handler on client RST."
        );
        let _ = writeln!(
            src,
            "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated handler Err; those trailers reach the client."
        );
        if m.server_streaming {
            let _ = writeln!(
                src,
                "    /// Spawned producers should select on [`{G}::Request::cancelled`] and [`{G}::StreamSender::closed`]; drain aborts on client RST."
            );
            let _ = writeln!(
                src,
                "    /// [`{G}::StreamSender::fail`] after a message ships trailing metadata and `grpc-status-details-bin` the same as a handler `Err`."
            );
            let _ = writeln!(
                src,
                "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated StreamSender fail on a server response producer; those trailers ship after any messages already sent."
            );
        }
        if m.client_streaming && !m.server_streaming {
            let _ = writeln!(
                src,
                "    /// That signal still fires after the client half-closes the request stream."
            );
        }
        if m.deprecated {
            let _ = writeln!(src, "    ///");
            let _ = writeln!(src, "    /// # Deprecated");
            let _ = writeln!(src, "    #[deprecated]");
        }
        let _ = writeln!(src, "    fn {fn_name}(");
        if use_arc_self() {
            let _ = writeln!(src, "        self: ::std::sync::Arc<Self>,");
        } else {
            let _ = writeln!(src, "        &self,");
        }
        let _ = writeln!(
            src,
            "        request: {G}::Request<{}>,",
            shape.trait_request
        );
        if generate_default_stubs() {
            let _ = writeln!(
                src,
                "    ) -> impl ::core::future::Future<Output = ::core::result::Result<{G}::Response<{}>, {G}::Status>> + Send {{",
                shape.trait_response
            );
            let _ = writeln!(src, "        async move {{");
            let _ = writeln!(src, "            drop(request);");
            let _ = writeln!(
                src,
                "            ::core::result::Result::Err({G}::Status::unimplemented(\"method {}/{} not implemented\"))",
                svc.full_name, m.name
            );
            let _ = writeln!(src, "        }}");
            let _ = writeln!(src, "    }}");
        } else {
            let _ = writeln!(
                src,
                "    ) -> impl ::core::future::Future<Output = ::core::result::Result<{G}::Response<{}>, {G}::Status>> + Send;",
                shape.trait_response
            );
        }
    }
    let _ = writeln!(src, "}}");
}

pub(crate) fn emit_kernel_server(
    src: &mut String,
    trait_name: &str,
    server: &str,
    full_name: &str,
    svc: &ServiceDescriptor,
) {
    if !svc.comments.is_empty() {
        emit_doc_comments(src, &svc.comments, "");
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// Serves an implementation of [`{trait_name}`].");
    let _ = writeln!(src, "///");
    let _ = writeln!(
        src,
        "/// [`Self::rpc_timeout`], [`Self::compresses_outbound`], [`Self::gzip_level`], [`Self::accepts_compressed`], [`Self::concurrent_rpc_limit`], [`Self::send_buffer_size`], and [`Self::limits`] read the server overlay without colliding with the setters. Same getters as [`{G}::Server`] / [`{G}::Router`]."
    );
    if svc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    emit_server_attributes(src, full_name, "");
    let _ = writeln!(src, "pub struct {server}<T> {{");
    let _ = writeln!(src, "    inner: ::std::sync::Arc<T>,");
    let _ = writeln!(src, "    config: {G}::ServerConfig,");
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl<T> ::core::clone::Clone for {server}<T> {{");
    let _ = writeln!(src, "    fn clone(&self) -> Self {{");
    let _ = writeln!(src, "        Self {{");
    let _ = writeln!(
        src,
        "            inner: ::std::sync::Arc::clone(&self.inner),"
    );
    let _ = writeln!(src, "            config: self.config,");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl<T> {server}<T> {{");
    let _ = writeln!(
        src,
        "    /// Wrap an existing `Arc` without adding another layer."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn from_arc(inner: ::std::sync::Arc<T>) -> Self {{"
    );
    let _ = writeln!(src, "        Self {{");
    let _ = writeln!(src, "            inner,");
    let _ = writeln!(
        src,
        "            config: <{G}::ServerConfig as ::core::default::Default>::default(),"
    );
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "    /// Take the inner `Arc` back.");
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn into_inner(self) -> ::std::sync::Arc<T> {{ self.inner }}"
    );
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl<T> ::core::fmt::Debug for {server}<T> {{");
    let _ = writeln!(
        src,
        "    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{"
    );
    let _ = writeln!(
        src,
        "        f.debug_struct(\"{server}\").field(\"service\", &\"{full_name}\").field(\"config\", &self.config).finish()"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl<T: {trait_name}> {server}<T> {{");
    let _ = writeln!(src, "    /// Fully qualified proto service name.");
    let _ = writeln!(src, "    pub const NAME: &'static str = \"{full_name}\";");
    let _ = writeln!(
        src,
        "    /// Wrap an implementation with default configuration."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(src, "    pub fn new(inner: T) -> Self {{");
    let _ = writeln!(src, "        Self {{");
    let _ = writeln!(src, "            inner: ::std::sync::Arc::new(inner),");
    let _ = writeln!(
        src,
        "            config: <{G}::ServerConfig as ::core::default::Default>::default(),"
    );
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Replace the transport and limit configuration."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn config(mut self, config: {G}::ServerConfig) -> Self {{ self.config = config; self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn server_config(&self) -> {G}::ServerConfig {{ self.config }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap inbound messages at `limit` bytes. Default 4 MiB. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {{ self.config = self.config.max_decoding_message_size(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap outbound messages at `limit` bytes. Default unlimited. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {{ self.config = self.config.max_encoding_message_size(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Replace both message caps at once. Applies to every call shape. See [`{G}::ServerConfig::message_limits`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn message_limits(mut self, limits: {G}::MessageLimits) -> Self {{ self.config = self.config.message_limits(limits); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn limits(&self) -> {G}::MessageLimits {{ self.config.limits() }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap how many RPCs the process will run at once. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]. See [`{G}::ServerConfig::max_concurrent_rpcs`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {{ self.config = self.config.max_concurrent_rpcs(n); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn concurrent_rpc_limit(&self) -> ::core::option::Option<usize> {{ self.config.concurrent_rpc_limit() }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap how many TCP/Unix connections the accept loop will serve at once, including TLS and mTLS listeners. Applies to every call shape. See [`{G}::ServerConfig::max_concurrent_connections`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_concurrent_connections(mut self, n: usize) -> Self {{ self.config = self.config.max_concurrent_connections(n); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Concurrent RPCs allowed per HTTP/2 connection. Applies to every call shape. See [`{G}::ServerConfig::max_concurrent_streams`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_concurrent_streams(mut self, streams: u32) -> Self {{ self.config = self.config.max_concurrent_streams(streams); self }}"
    );
    let _ = writeln!(
        src,
        "    /// HTTP/2 per-stream receive window. Applies to every call shape. See [`{G}::ServerConfig::initial_stream_window_size`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn initial_stream_window_size(mut self, bytes: u32) -> Self {{ self.config = self.config.initial_stream_window_size(bytes); self }}"
    );
    let _ = writeln!(
        src,
        "    /// HTTP/2 per-connection receive window. Applies to every call shape. See [`{G}::ServerConfig::initial_connection_window_size`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn initial_connection_window_size(mut self, bytes: u32) -> Self {{ self.config = self.config.initial_connection_window_size(bytes); self }}"
    );
    let _ = writeln!(
        src,
        "    /// HTTP/2 `SETTINGS_MAX_FRAME_SIZE`. Applies to every call shape. See [`{G}::ServerConfig::max_frame_size`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_frame_size(mut self, bytes: u32) -> Self {{ self.config = self.config.max_frame_size(bytes); self }}"
    );
    let _ = writeln!(
        src,
        "    /// HTTP/2 `SETTINGS_MAX_HEADER_LIST_SIZE`. Applies to every call shape. See [`{G}::ServerConfig::max_header_list_size`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_header_list_size(mut self, bytes: u32) -> Self {{ self.config = self.config.max_header_list_size(bytes); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn header_table_size(mut self, bytes: u32) -> Self {{ self.config = self.config.header_table_size(bytes); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn data_frame_budget(mut self, bytes: usize) -> Self {{ self.config = self.config.data_frame_budget(bytes); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Per-connection HTTP/2 send buffer. Applies to every call shape. See [`{G}::ServerConfig::max_send_buffer_size`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {{ self.config = self.config.max_send_buffer_size(bytes); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn send_buffer_size(&self) -> usize {{ self.config.send_buffer_size() }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap remotely-reset HTTP/2 streams waiting in the accept queue. Applies to every call shape. See [`{G}::ServerConfig::max_pending_accept_reset_streams`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_pending_accept_reset_streams(mut self, n: usize) -> Self {{ self.config = self.config.max_pending_accept_reset_streams(n); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_local_error_reset_streams(mut self, n: usize) -> Self {{ self.config = self.config.max_local_error_reset_streams(n); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_concurrent_reset_streams(mut self, n: usize) -> Self {{ self.config = self.config.max_concurrent_reset_streams(n); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn reset_stream_duration(mut self, dur: ::std::time::Duration) -> Self {{ self.config = self.config.reset_stream_duration(dur); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap every RPC even when the client omits `grpc-timeout`. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]. See [`{G}::ServerConfig::timeout`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {{ self.config = self.config.timeout(timeout); self }}"
    );
    let _ = writeln!(
        src,
        "    /// gzip responses when the client advertises gzip. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]. See [`{G}::ServerConfig::send_compressed`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn send_compressed(mut self) -> Self {{ self.config = self.config.send_compressed(true); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn gzip_compression_level(mut self, level: u32) -> Self {{ self.config = self.config.gzip_compression_level(level); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn accept_compressed(mut self, accept: bool) -> Self {{ self.config = self.config.accept_compressed(accept); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn rpc_timeout(&self) -> ::core::option::Option<::std::time::Duration> {{ self.config.rpc_timeout() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn compresses_outbound(&self) -> bool {{ self.config.compresses_outbound() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn gzip_level(&self) -> u32 {{ self.config.gzip_level() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn accepts_compressed(&self) -> bool {{ self.config.accepts_compressed() }}"
    );
    let _ = writeln!(
        src,
        "    /// HTTP/2 PING keepalive. Applies to every call shape. See [`{G}::ServerConfig::keep_alive_interval`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn keep_alive_interval(mut self, interval: ::std::time::Duration) -> Self {{ self.config = self.config.keep_alive_interval(interval); self }}"
    );
    let _ = writeln!(
        src,
        "    /// How long to wait for a PING acknowledgement. Applies to every call shape. See [`{G}::ServerConfig::keep_alive_timeout`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn keep_alive_timeout(mut self, timeout: ::std::time::Duration) -> Self {{ self.config = self.config.keep_alive_timeout(timeout); self }}"
    );
    let _ = writeln!(
        src,
        "    /// TCP `SO_KEEPALIVE`. Applies to every call shape. See [`{G}::ServerConfig::tcp_keepalive`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn tcp_keepalive(mut self, time: ::std::time::Duration) -> Self {{ self.config = self.config.tcp_keepalive(time); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn tcp_keepalive_interval(mut self, interval: ::std::time::Duration) -> Self {{ self.config = self.config.tcp_keepalive_interval(interval); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn tcp_keepalive_retries(mut self, retries: u32) -> Self {{ self.config = self.config.tcp_keepalive_retries(retries); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Send GOAWAY this long after accept. The next RPC of every call shape redials, including over TLS, mTLS, and Unix; transparent retry of the same in-flight RPC is unary and server-streaming after request bytes, client-streaming and bidi before HEADERS. See [`{G}::ServerConfig::max_connection_age`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_connection_age(mut self, age: ::std::time::Duration) -> Self {{ self.config = self.config.max_connection_age(age); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Send GOAWAY after this long with no outstanding RPCs. The next RPC of every call shape redials, including over TLS, mTLS, and Unix. See [`{G}::ServerConfig::max_connection_idle`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_connection_idle(mut self, idle: ::std::time::Duration) -> Self {{ self.config = self.config.max_connection_idle(idle); self }}"
    );
    let _ = writeln!(
        src,
        "    /// After age or idle fires, wait this long for in-flight RPCs, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]. Applies to every call shape. See [`{G}::ServerConfig::max_connection_age_grace`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_connection_age_grace(mut self, grace: ::std::time::Duration) -> Self {{ self.config = self.config.max_connection_age_grace(grace); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Drop a client that never finishes TLS or the HTTP/2 preface. Applies to every call shape, including over TLS, mTLS, and Unix. See [`{G}::ServerConfig::handshake_timeout`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn handshake_timeout(mut self, timeout: ::std::time::Duration) -> Self {{ self.config = self.config.handshake_timeout(timeout); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Run `interceptor` before `{trait_name}` methods. It may mutate metadata, cap the deadline (`set_timeout` / `deadline` Instant), inspect path / service / method, `:authority` / `:scheme` / `peer_timeout` / `rpc_timeout` / `effective_timeout` / `local_addr` / `remote_addr` / `peer_identity` / `peer_cred` / message caps / gzip accept and encoding / `compresses_outbound` / `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `send_buffer_size`, attach extensions, or reject. Generated handlers see the same values on [`{G}::Request`]. Calling this twice stacks: the first interceptor runs first. Applies to every call shape; `Err` rejects before the body is read, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]. `Err` may carry [`{G}::Status::with_error_details`]; those trailers reach the client."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated server intercept Err; those trailers reach the client without reading the body."
    );
    let _ = writeln!(
        src,
        "    /// Compiling overlay dumps live on [`{G}::hello`] (`GreeterServer::new(Svc).intercept`)."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn intercept<I>(self, interceptor: I) -> {G}::Server<Self>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(src, "        I: {G}::Interceptor,");
    let _ = writeln!(src, "    {{");
    let _ = writeln!(src, "        self.into_server().intercept(interceptor)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`{G}::ResponseParts::compress_is_set`] is occupancy after a generated server on_response, so a later interceptor can fill compress only when unset."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::ResponseParts::clear_compress`] restores the server gzip overlay after a generated server on_response."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated server on_response Err; a local reject is trailers-only after handler Ok."
    );
    let _ = writeln!(
        src,
        "    /// Compiling overlay dumps live on [`{G}::hello`] (`GreeterServer::new(Svc).on_response`)."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn on_response<I>(self, interceptor: I) -> {G}::Server<Self>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(src, "        I: {G}::ResponseInterceptor,");
    let _ = writeln!(src, "    {{");
    let _ = writeln!(src, "        self.into_server().on_response(interceptor)");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Mount alongside another service. [`Self::max_decoding_message_size`] and [`Self::max_encoding_message_size`] stay in effect on every mounted service, on every call shape of those mounts, including over TLS, mTLS, Unix, and [`{G}::Server::serve_connection`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn add_service<S: {G}::Service>(self, service: S) -> {G}::Router {{ self.into_server().add_service(service) }}"
    );
    let _ = writeln!(
        src,
        "    /// Mount alongside another service using static tuple dispatch instead of dynamic [`{G}::Router`] hashing and boxed service futures."
    );
    let _ = writeln!(
        src,
        "    /// The returned [`{G}::Server`] preserves this server's configuration and dispatches by matching the full `/<service>/<method>` path."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn add_static_service<S: {G}::Service>(self, service: S) -> {G}::Server<(Self, S)> {{"
    );
    let _ = writeln!(src, "        let config = self.config;");
    let _ = writeln!(
        src,
        "        {G}::Server::new((self, service)).config(config)"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn add_optional_service<S: {G}::Service>(self, service: ::core::option::Option<S>) -> {G}::Router {{ self.into_server().add_optional_service(service) }}"
    );
    let _ = writeln!(
        src,
        "    /// Move this service into a [`{G}::Router`], keeping the configuration."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn into_router(self) -> {G}::Router {{ self.into_server().into_router() }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `addr` and serve until the listener fails. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve(self, addr: ::std::net::SocketAddr) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve(addr).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve on an existing listener until it fails. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_listener(self, listener: {G}::codegen_support::TcpListener) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_listener(listener).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve until `shutdown` resolves, then drain. Applies to every call shape. In-flight RPCs finish; new connections are refused. TLS and Unix drain the same way (`serve_tls_with_shutdown`, `serve_unix_with_shutdown`)."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_with_shutdown(self, listener: {G}::codegen_support::TcpListener, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_with_shutdown(listener, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `addr` and serve until `shutdown` resolves, then drain. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_until_shutdown(self, addr: ::std::net::SocketAddr, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_until_shutdown(addr, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `path` and serve h2c over a Unix domain socket. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix(self, path: impl AsRef<::std::path::Path>) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix(path).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `path` after unlinking a crash leftover. A live listener is left alone and this fails with UNAVAILABLE. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix_unlink(self, path: impl AsRef<::std::path::Path>) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix_unlink(path).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve h2c on an existing Unix listener. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix_listener(self, listener: {G}::codegen_support::UnixListener) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix_listener(listener).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve h2c on a Unix listener until `shutdown` resolves, then drain. Applies to every call shape. In-flight RPCs finish; new connections are refused. See [`{G}::Server::serve_with_shutdown`]."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix_with_shutdown(self, listener: {G}::codegen_support::UnixListener, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix_with_shutdown(listener, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `path` and serve h2c until `shutdown` resolves, then drain. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix_until_shutdown(self, path: impl AsRef<::std::path::Path>, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix_until_shutdown(path, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `path` until `shutdown` after unlinking a crash leftover. A live listener is left alone. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn serve_unix_unlink_until_shutdown(self, path: impl AsRef<::std::path::Path>, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_unix_unlink_until_shutdown(path, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `addr` and serve over TLS until the listener fails. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_tls(self, addr: ::std::net::SocketAddr, tls: {G}::ServerTls) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_tls(addr, tls).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve over TLS until `shutdown` resolves, then drain. Applies to every call shape, including mTLS. In-flight RPCs finish; new connections are refused."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_tls_with_shutdown(self, listener: {G}::codegen_support::TcpListener, shutdown: impl ::core::future::Future<Output = ()> + Send, tls: {G}::ServerTls) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_tls_with_shutdown(listener, shutdown, tls).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Bind `addr` and serve over TLS until `shutdown` resolves, then drain. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_tls_until_shutdown(self, addr: ::std::net::SocketAddr, shutdown: impl ::core::future::Future<Output = ()> + Send, tls: {G}::ServerTls) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_tls_until_shutdown(addr, shutdown, tls).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve a single already-accepted byte stream until it closes. Applies to every call shape. Generated handlers see empty peer facts on [`{G}::Request`] / [`{G}::Parts`]; [`{G}::Rpc::scheme`] is the peer's `:scheme`. See [`{G}::Server::serve_connection`]."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_connection<IO>(self, io: IO) -> ::core::result::Result<(), {G}::Status>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(
        src,
        "        IO: {G}::codegen_support::AsyncRead + {G}::codegen_support::AsyncWrite + Unpin + Send + 'static,"
    );
    let _ = writeln!(src, "    {{");
    let _ = writeln!(src, "        self.into_server().serve_connection(io).await");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Serve connections from `incoming` until it is exhausted. Applies to every call shape. Override [`{G}::Incoming::peer`] to stamp connection facts."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_with_incoming<I: {G}::Incoming>(self, incoming: I) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_with_incoming(incoming).await }}"
    );
    let _ = writeln!(
        src,
        "    /// Serve from `incoming` until `shutdown` resolves, then drain. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_with_incoming_shutdown<I: {G}::Incoming>(self, incoming: I, shutdown: impl ::core::future::Future<Output = ()> + Send) -> ::core::result::Result<(), {G}::Status> {{ self.into_server().serve_with_incoming_shutdown(incoming, shutdown).await }}"
    );
    let _ = writeln!(
        src,
        "    fn into_server(self) -> {G}::Server<Self> {{ let config = self.config; {G}::Server::new(self).config(config) }}"
    );
    let _ = writeln!(src, "}}");

    let _ = writeln!(src, "impl<T: {trait_name}> {G}::Service for {server}<T> {{");
    let _ = writeln!(src, "    const NAME: &'static str = \"{full_name}\";");
    if full_name == "grpc.reflection.v1.ServerReflection" {
        let _ = writeln!(
            src,
            "    /// Path alias for older grpcurl that falls back to `grpc.reflection.v1alpha`."
        );
        let _ = writeln!(
            src,
            "    /// Same handler as [`Self::NAME`]. Not a second proto."
        );
        let _ = writeln!(
            src,
            "    const ALIASES: &'static [&'static str] = &[\"grpc.reflection.v1alpha.ServerReflection\"];"
        );
    }
    for boxed in [false, true] {
        if boxed {
            let _ = writeln!(
                src,
                "    fn call_boxed(&self, rpc: {G}::Rpc) -> ::core::pin::Pin<::std::boxed::Box<dyn ::core::future::Future<Output = ()> + Send + '_>> {{"
            );
        } else {
            let _ = writeln!(
                src,
                "    fn call(&self, rpc: {G}::Rpc) -> impl ::core::future::Future<Output = ()> + Send {{"
            );
        }
        let _ = writeln!(
            src,
            "        let inner = ::std::sync::Arc::clone(&self.inner);"
        );
        if !boxed {
            let _ = writeln!(src, "        async move {{");
        }
        let _ = writeln!(src, "            match rpc.path() {{");
        for m in &svc.methods {
            let shape = shape_of(m);
            let fn_name = to_snake(&m.name);
            let mut paths = vec![format!("/{full_name}/{}", m.name)];
            if full_name == "grpc.reflection.v1.ServerReflection" {
                paths.push(format!(
                    "/grpc.reflection.v1alpha.ServerReflection/{}",
                    m.name
                ));
            }
            for path in paths {
                let _ = writeln!(src, "                \"{path}\" => {{");
                let future = format!(
                    "rpc.{}(move |request| async move {{ inner.{fn_name}(request).await }})",
                    shape.dispatch
                );
                if boxed {
                    let _ = writeln!(src, "                    ::std::boxed::Box::pin({future})");
                } else {
                    let _ = writeln!(src, "                    {future}.await;");
                }
                let _ = writeln!(src, "                }}");
            }
        }
        if boxed {
            let _ = writeln!(
                src,
                "                _ => ::std::boxed::Box::pin(async move {{ rpc.unimplemented() }}),"
            );
        } else {
            let _ = writeln!(src, "                _ => rpc.unimplemented(),");
        }
        let _ = writeln!(src, "            }}");
        if !boxed {
            let _ = writeln!(src, "        }}");
        }
        let _ = writeln!(src, "    }}");
    }
    let _ = writeln!(src, "}}");
}

pub(crate) fn emit_kernel_client_dialers(src: &mut String) {
    let _ = writeln!(
        src,
        "    /// Dial `target` (`host:port`, not a tonic `http://` / `https://` URI). See [`{G}::Channel::connect`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn connect(target: impl Into<{G}::Target>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect(target).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn connect_with(target: impl Into<{G}::Target>, config: {G}::ChannelConfig) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_with(target, config).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Open `connections` slots. See [`{G}::Channel::connect_pool`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    /// TLS (including mTLS) pooling is [`Self::connect_tls_with`] plus [`{G}::ChannelConfig::connections`]; Unix is [`Self::connect_unix_with`]. [`{G}::Channel::from_io`] cannot pool."
    );
    let _ = writeln!(
        src,
        "    /// A pool larger than the server's [`{G}::Server::max_concurrent_connections`] fails the dial as `UNAVAILABLE`."
    );
    let _ = writeln!(
        src,
        "    pub async fn connect_pool(target: impl Into<{G}::Target>, connections: usize) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_pool(target, connections).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Dial over TLS. See [`{G}::Channel::connect_tls`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn connect_tls(target: impl Into<{G}::Target>, tls: {G}::ClientTls) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_tls(target, tls).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_tls`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::ChannelConfig::connections`] opens that many TLS sockets (including mTLS); all must succeed. [`{G}::Channel::from_io`] cannot pool."
    );
    let _ = writeln!(
        src,
        "    pub async fn connect_tls_with(target: impl Into<{G}::Target>, config: {G}::ChannelConfig, tls: {G}::ClientTls) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_tls_with(target, config, tls).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Dial on the first RPC. See [`{G}::Channel::connect_lazy`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn connect_lazy(target: impl Into<{G}::Target>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_lazy(target)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_lazy`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn connect_lazy_with(target: impl Into<{G}::Target>, config: {G}::ChannelConfig) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_lazy_with(target, config)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_lazy`] over TLS. See [`{G}::Channel::connect_tls_lazy`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn connect_tls_lazy(target: impl Into<{G}::Target>, tls: {G}::ClientTls) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_tls_lazy(target, tls)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_tls_lazy`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn connect_tls_lazy_with(target: impl Into<{G}::Target>, config: {G}::ChannelConfig, tls: {G}::ClientTls) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_tls_lazy_with(target, config, tls)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Dial a Unix domain socket. See [`{G}::Channel::connect_unix`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn connect_unix(path: impl AsRef<::std::path::Path>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_unix(path).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_unix`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::ChannelConfig::connections`] opens that many Unix sockets; all must succeed. [`{G}::Channel::from_io`] cannot pool."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub async fn connect_unix_with(path: impl AsRef<::std::path::Path>, config: {G}::ChannelConfig) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_unix_with(path, config).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_unix`] that dials on the first RPC. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub fn connect_unix_lazy(path: impl AsRef<::std::path::Path>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_unix_lazy(path)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::connect_unix_lazy`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[cfg(unix)]");
    let _ = writeln!(
        src,
        "    pub fn connect_unix_lazy_with(path: impl AsRef<::std::path::Path>, config: {G}::ChannelConfig) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::connect_unix_lazy_with(path, config)?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Speak gRPC over an already-connected byte stream. See [`{G}::Channel::from_io`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub async fn from_io<IO>(io: IO, authority: impl Into<{G}::Target>) -> ::core::result::Result<Self, {G}::Status>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(
        src,
        "        IO: {G}::codegen_support::AsyncRead + {G}::codegen_support::AsyncWrite + Unpin + Send + 'static,"
    );
    let _ = writeln!(src, "    {{");
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::from_io(io, authority).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`Self::from_io`] with [`{G}::ChannelConfig`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::ChannelConfig::connections`] is forced to 1: one duplex is one HTTP/2 connection."
    );
    let _ = writeln!(
        src,
        "    pub async fn from_io_with<IO>(io: IO, authority: impl Into<{G}::Target>, config: {G}::ChannelConfig) -> ::core::result::Result<Self, {G}::Status>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(
        src,
        "        IO: {G}::codegen_support::AsyncRead + {G}::codegen_support::AsyncWrite + Unpin + Send + 'static,"
    );
    let _ = writeln!(src, "    {{");
    let _ = writeln!(
        src,
        "        Ok(Self::new({G}::Channel::from_io_with(io, authority, config).await?))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Send `:scheme https` from a [`Self::from_io`] channel. See [`{G}::Channel::https_scheme`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn https_scheme(self) -> Self {{ Self {{ channel: self.channel.https_scheme() }} }}"
    );
    let _ = writeln!(
        src,
        "    /// Override `:authority` on this clone without changing the dial. See [`{G}::Channel::origin`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn origin(self, authority: impl Into<{G}::Target>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self {{ channel: self.channel.origin(authority)? }})"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// HTTP/2 `:scheme` this client sends. See [`{G}::Channel::scheme`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn scheme(&self) -> &'static str {{ self.channel.scheme() }}"
    );
    let _ = writeln!(
        src,
        "    /// The HTTP/2 `:authority` this client sends. See [`{G}::Channel::authority`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn authority(&self) -> &str {{ self.channel.authority() }}"
    );
    let _ = writeln!(
        src,
        "    /// The `user-agent` this client sends. See [`{G}::Channel::grpc_user_agent`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn grpc_user_agent(&self) -> &str {{ self.channel.grpc_user_agent() }}"
    );
    let _ = writeln!(
        src,
        "    /// The configuration in effect. See [`{G}::Channel::config`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn config(&self) -> {G}::ChannelConfig {{ self.channel.config() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn connected(&self) -> bool {{ self.channel.connected() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn rpc_timeout(&self) -> ::core::option::Option<::std::time::Duration> {{ self.channel.rpc_timeout() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn waits_for_ready(&self) -> bool {{ self.channel.waits_for_ready() }}"
    );
    let _ = writeln!(
        src,
        "    /// Whether this client gzips outbound payloads. See [`{G}::Channel::compresses_outbound`]. Applies to every call shape."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn compresses_outbound(&self) -> bool {{ self.channel.compresses_outbound() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn gzip_level(&self) -> u32 {{ self.channel.gzip_level() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn accepts_compressed(&self) -> bool {{ self.channel.accepts_compressed() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn concurrent_rpc_limit(&self) -> ::core::option::Option<usize> {{ self.channel.concurrent_rpc_limit() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn stream_buffer_size(&self) -> usize {{ self.channel.stream_buffer_size() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn send_buffer_size(&self) -> usize {{ self.channel.send_buffer_size() }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn limits(&self) -> {G}::MessageLimits {{ self.channel.limits() }}"
    );
}

pub(crate) fn emit_kernel_client(
    src: &mut String,
    trait_name: &str,
    client: &str,
    full_name: &str,
    svc: &ServiceDescriptor,
) {
    if !svc.comments.is_empty() {
        emit_doc_comments(src, &svc.comments, "");
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// Client for `{full_name}`.");
    let _ = writeln!(src, "///");
    let _ = writeln!(
        src,
        "/// Wraps a [`{G}::Channel`]; cloning is cheap and shares connections."
    );
    let _ = writeln!(
        src,
        "/// Dial with [`Self::connect`], [`Self::connect_tls`], or [`Self::from_io`]."
    );
    let _ = writeln!(
        src,
        "/// On Unix, `connect_unix` takes a filesystem path. Wrap an existing channel with [`Self::new`]."
    );
    let _ = writeln!(
        src,
        "/// [`Self::authority`], [`Self::scheme`], and [`Self::grpc_user_agent`] read the same values interceptors see on [`{G}::Outgoing`]. [`Self::config`] is the channel overlay those values come from. [`Self::rpc_timeout`], [`Self::waits_for_ready`], [`Self::compresses_outbound`], [`Self::gzip_level`], [`Self::accepts_compressed`], [`Self::concurrent_rpc_limit`], [`Self::stream_buffer_size`], [`Self::send_buffer_size`], and [`Self::limits`] read that overlay without colliding with the setters."
    );
    if svc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    emit_client_attributes(src, full_name, "");
    let _ = writeln!(src, "#[derive(::core::clone::Clone)]");
    let _ = writeln!(src, "pub struct {client} {{");
    let _ = writeln!(src, "    channel: {G}::Channel,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl ::core::fmt::Debug for {client} {{");
    let _ = writeln!(
        src,
        "    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {{"
    );
    let _ = writeln!(
        src,
        "        f.debug_struct(\"{client}\").field(\"channel\", &self.channel).finish()"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl {client} {{");
    let _ = writeln!(src, "    /// Fully qualified proto service name.");
    let _ = writeln!(src, "    pub const NAME: &'static str = \"{full_name}\";");
    let _ = writeln!(src, "    /// Wrap a channel.");
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn new(channel: {G}::Channel) -> Self {{ Self {{ channel }} }}"
    );
    let _ = writeln!(src, "    /// Take the channel back.");
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn into_inner(self) -> {G}::Channel {{ self.channel }}"
    );
    emit_kernel_client_dialers(src);
    let _ = writeln!(
        src,
        "    /// Cap inbound messages at `limit` bytes. Default 4 MiB. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {{ self.channel = self.channel.max_decoding_message_size(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Cap outbound messages at `limit` bytes. Default unlimited. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {{ self.channel = self.channel.max_encoding_message_size(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Replace both message caps at once. Applies to every call shape. See [`{G}::Channel::message_limits`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn message_limits(mut self, limits: {G}::MessageLimits) -> Self {{ self.channel = self.channel.message_limits(limits); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Run `interceptor` on every outbound RPC before the stream opens. Invoked when the generated method is called, not on first poll of the [`{G}::Call`]. `Err` fails that Call on poll, including [`{G}::Status::with_error_details`]; nothing is sent. A local [`{G}::Status::with_error_details`] is [`{G}::Status::rpc`] / [`{G}::Status::error_details`] on that Call for every call shape. [`{G}::Outgoing::set_timeout`] is that Call's deadline on every call shape. `clear_timeout` opts out of the channel timeout on every call shape. `clear_compress` then `set_compress(compresses_outbound())` reapplies channel gzip on every call shape. `set_compress` stamps [`{G}::StreamSender::compress`] on client-streaming and bidi request streams. Outgoing getters apply to every call shape. `clear_user_agent` restores the channel user-agent after a generated intercept prefix."
    );
    let _ = writeln!(
        src,
        "    /// The interceptor sees a [`{G}::Outgoing`]: path, service, method, `:authority`, `:scheme`,"
    );
    let _ = writeln!(
        src,
        "    /// Compiling overlay dumps live on [`{G}::hello`] (`GreeterClient::new(channel).intercept`)."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Outgoing::user_agent_is_set`] is occupancy on this generated intercept path, so a later interceptor can prefix only when unset."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Outgoing::wait_for_ready_is_set`] is occupancy on this generated intercept path, so a later interceptor can fill wait-for-ready only when unset."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Outgoing::compress_is_set`] is occupancy on this generated intercept path, so a later interceptor can fill compress only when unset."
    );
    let _ = writeln!(
        src,
        "    /// `clear_wait_for_ready` restores the channel wait-for-ready overlay after a generated intercept choice."
    );
    let _ = writeln!(
        src,
        "    /// `clear_timeout` opts out of the channel timeout after a generated intercept choice."
    );
    let _ = writeln!(
        src,
        "    /// `clear_compress` then `set_compress` from `compresses_outbound` reapplies channel gzip after a generated intercept choice."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated intercept Err; a local reject never opens a stream."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(src, "    pub fn intercept<I>(self, interceptor: I) -> Self");
    let _ = writeln!(src, "    where");
    let _ = writeln!(src, "        I: {G}::ClientInterceptor,");
    let _ = writeln!(src, "    {{");
    let _ = writeln!(
        src,
        "        Self {{ channel: self.channel.intercept(interceptor) }}"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// [`{G}::ResponseParts::compress_is_set`] is occupancy after a generated client on_response, so a later interceptor can fill compress only when unset."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::ResponseParts::clear_compress`] drops a compress choice after a generated client on_response; a received reply has no server gzip overlay to restore."
    );
    let _ = writeln!(
        src,
        "    /// [`{G}::Status::from_error_details`] is the typed bag after a generated client on_response Err; a local reject fails the Call after a successful receive."
    );
    let _ = writeln!(
        src,
        "    /// Compiling overlay dumps live on [`{G}::hello`] (`GreeterClient::new(channel).on_response`)."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn on_response<I>(self, interceptor: I) -> Self"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(src, "        I: {G}::ResponseInterceptor,");
    let _ = writeln!(src, "    {{");
    let _ = writeln!(
        src,
        "        Self {{ channel: self.channel.on_response(interceptor) }}"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// Prefix the kernel `user-agent`. See [`{G}::Channel::user_agent`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    pub fn user_agent(self, prefix: impl AsRef<str>) -> ::core::result::Result<Self, {G}::Status> {{"
    );
    let _ = writeln!(
        src,
        "        Ok(Self {{ channel: self.channel.user_agent(prefix)? }})"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    /// gzip unary and server-streaming request payloads and [`{G}::StreamSender::send`]. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]. See [`{G}::Channel::send_compressed`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn send_compressed(mut self) -> Self {{ self.channel = self.channel.send_compressed(); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn gzip_compression_level(mut self, level: u32) -> Self {{ self.channel = self.channel.gzip_compression_level(level); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn accept_compressed(mut self, accept: bool) -> Self {{ self.channel = self.channel.accept_compressed(accept); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Default per-RPC deadline when the request omits one. See [`{G}::Channel::timeout`]. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]. Interceptors run after this fill and can still set or [`{G}::Outgoing::clear_timeout`]."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {{ self.channel = self.channel.timeout(timeout); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Wait for a connection instead of failing fast. See [`{G}::Channel::wait_for_ready`]. Applies to every call shape."
    );
    let _ = writeln!(
        src,
        "    /// A request that already called [`{G}::Request::set_wait_for_ready`] is left alone. Interceptors run after this fill and can still set or clear it."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn wait_for_ready(mut self) -> Self {{ self.channel = self.channel.wait_for_ready(); self }}"
    );
    let _ = writeln!(
        src,
        "    /// How many messages sit between a client-streaming caller and the wire. See [`{G}::Channel::stream_buffer`]. Applies to client-streaming and bidi request streams."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn stream_buffer(mut self, messages: usize) -> Self {{ self.channel = self.channel.stream_buffer(messages); self }}"
    );
    let _ = writeln!(
        src,
        "    /// Write-time HTTP/2 send buffer threshold for outbound DATA. See [`{G}::Channel::max_send_buffer_size`]. Applies to every call shape, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]. Overlay: does not change how a dead slot is redialed."
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_send_buffer_size(mut self, bytes: usize) -> Self {{ self.channel = self.channel.max_send_buffer_size(bytes); self }}"
    );
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn max_concurrent_rpcs(mut self, n: usize) -> Self {{ self.channel = self.channel.max_concurrent_rpcs(n); self }}"
    );
    let _ = writeln!(src, "    /// The channel this client sends on.");
    let _ = writeln!(src, "    #[must_use]");
    let _ = writeln!(
        src,
        "    pub fn channel(&self) -> &{G}::Channel {{ &self.channel }}"
    );
    for m in &svc.methods {
        let shape = shape_of(m);
        let fn_name = to_snake(&m.name);
        let path = format!("/{full_name}/{}", m.name);
        let _ = writeln!(src, "    /// {} `{}`.", shape.description, m.name);
        let _ = writeln!(
            src,
            "    /// Interceptors run when this method is called, not on first poll of the [`{G}::Call`]."
        );
        let _ = writeln!(
            src,
            "    /// The reply's [`{G}::Response::encoding`] is the peer's `grpc-encoding` (`None` for identity). The [`{G}::Call`] is fused after it resolves."
        );
        if !m.client_streaming && !m.server_streaming {
            let _ = writeln!(
                src,
                "    /// A [`{G}::CallHandle`] taken before await cancels the RPC."
            );
            let _ = writeln!(
                src,
                "    /// OK-path custom trailers land on [`{G}::Response::trailers`]; a `-bin` trailer must not appear as a header, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
            );
        }
        if m.server_streaming {
            let _ = writeln!(
                src,
                "    /// Dropping the received [`{G}::Streaming`] before the end resets the RPC."
            );
            let _ = writeln!(
                src,
                "    /// After end-of-stream or error that stream is fused (`{G}::FusedStream`)."
            );
            let _ = writeln!(
                src,
                "    /// A [`{G}::CallHandle`] taken before await still cancels while waiting for headers, and still cancels that live stream after headers."
            );
            let _ = writeln!(
                src,
                "    /// Letting the deadline fire RSTs the send half before headers and after headers the same way."
            );
            let _ = writeln!(
                src,
                "    /// [`{G}::Streaming::trailers`] waits for end-of-stream, including when called before draining messages. A non-OK trailing `grpc-status` is `Err`. A `-bin` trailer must not appear as a header, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
            );
        }
        if m.client_streaming && !m.server_streaming {
            let _ = writeln!(
                src,
                "    /// A [`{G}::CallHandle`] taken before await still cancels after the sender is closed, while the unary response is pending."
            );
            let _ = writeln!(
                src,
                "    /// Cancelling before any request message (`cancel_after_begin`) is [`{G}::Code::Cancelled`], not OK from a half-close: hold the [`{G}::StreamSender`] until the [`{G}::Call`] settles, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
            );
            let _ = writeln!(
                src,
                "    /// Dropping the [`{G}::Call`] or letting its deadline fire after that half-close resets the same way."
            );
            let _ = writeln!(
                src,
                "    /// OK-path custom trailers land on [`{G}::Response::trailers`]; a `-bin` trailer must not appear as a header, including over TLS, mTLS, Unix, and [`{G}::Channel::from_io`]."
            );
            let _ = writeln!(
                src,
                "    /// [`{G}::StreamSender::fail`] resolves the [`{G}::Call`] with that status (no request-side `grpc-status`; the stream is reset with CANCEL)."
            );
        }
        if m.client_streaming && m.server_streaming {
            let _ = writeln!(
                src,
                "    /// [`{G}::StreamSender::fail`] before headers resolves the [`{G}::Call`] with that status, not `UNAVAILABLE` from the reset; after headers the received [`{G}::Streaming`] sees [`{G}::Code::Cancelled`], not that status."
            );
        }
        if shape.client_return.starts_with('(') {
            let _ = writeln!(
                src,
                "    /// Dropping the pair without awaiting the [`{G}::Call`] resets the stream."
            );
            let _ = writeln!(
                src,
                "    #[must_use = \"dropping a streaming Call resets the stream\"]"
            );
        }
        if !m.comments.is_empty() {
            emit_doc_comments(src, &m.comments, "    ");
            let _ = writeln!(src, "    ///");
        }
        let _ = writeln!(src, "    /// {} call: `{}`.", shape.description, m.name);
        let req = rust_type_path(&m.input_type);
        let resp = rust_type_path(&m.output_type);
        match (m.client_streaming, m.server_streaming) {
            (false, false) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Unary `{req}` -> `{resp}`."
                );
            }
            (true, false) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Client-streaming stream of `{req}` -> `{resp}`."
                );
            }
            (false, true) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Server-streaming `{req}` -> stream of `{resp}`."
                );
            }
            (true, true) => {
                let _ = writeln!(
                    src,
                    "    /// Streaming signature: Bidirectional-streaming stream of `{req}` -> stream of `{resp}`."
                );
            }
        }
        if m.deprecated {
            let _ = writeln!(src, "    ///");
            let _ = writeln!(src, "    /// # Deprecated");
            let _ = writeln!(src, "    #[deprecated]");
        }
        let _ = writeln!(
            src,
            "    pub fn {fn_name}(&self, request: {}) -> {} {{",
            shape.client_arg, shape.client_return
        );
        let _ = writeln!(
            src,
            "        self.channel.{}(\"{path}\", request)",
            shape.client_call
        );
        let _ = writeln!(src, "    }}");
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(
        src,
        "impl ::core::convert::From<{G}::Channel> for {client} {{"
    );
    let _ = writeln!(
        src,
        "    fn from(channel: {G}::Channel) -> Self {{ Self::new(channel) }}"
    );
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "// `{trait_name}` is the server-side counterpart.");
}
