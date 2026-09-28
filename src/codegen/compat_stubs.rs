//! Tonic-shaped native-transport stubs.

use super::*;
use crate::dynamic::{MethodDescriptor, ServiceDescriptor};

const C: &str = "::pbrs_grpc::compat";
const G: &str = "::pbrs_grpc";

pub(crate) fn emit_compat_service(src: &mut String, svc: &ServiceDescriptor) {
    let mut svc_clone = (*svc).clone();
    svc_clone.methods.sort_by(|a, b| a.name.cmp(&b.name));
    let svc = &svc_clone;
    let svc_ty = rust_ident(&svc.full_name);
    let client = format!("{svc_ty}Client");
    let server = format!("{svc_ty}Server");
    let full_name = &svc.full_name;
    let path_prefix = format!("/{full_name}");

    if build_server() {
        emit_compat_trait(src, &svc_ty, svc);
        emit_compat_server(src, &svc_ty, &server, full_name, svc);
    }
    if build_client() {
        emit_compat_client(src, &client, full_name, svc, &path_prefix);
    }
}

fn stream_assoc(m: &MethodDescriptor) -> String {
    format!("{}Stream", m.name)
}

fn emit_compat_trait(src: &mut String, trait_name: &str, svc: &ServiceDescriptor) {
    emit_doc_comments(src, &svc.comments, "");
    if !svc.comments.is_empty() {
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(
        src,
        "/// Tonic-shaped `{}` service over the native pbrs-grpc transport.",
        svc.full_name
    );
    if svc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    let _ = writeln!(src, "pub trait {trait_name}: Send + Sync + 'static {{");
    for m in &svc.methods {
        emit_doc_comments(src, &m.comments, "    ");
        if !m.comments.is_empty() {
            let _ = writeln!(src, "    ///");
        }
        if m.deprecated {
            let _ = writeln!(src, "    ///");
            let _ = writeln!(src, "    /// # Deprecated");
            let _ = writeln!(src, "    #[deprecated]");
        }
        let fn_name = to_snake(&m.name);
        let req = rust_type_path(&m.input_type);
        let resp = rust_type_path(&m.output_type);
        match (m.client_streaming, m.server_streaming) {
            (false, false) => emit_compat_unary_trait(src, &fn_name, &req, &resp, svc, m),
            (true, false) => {
                emit_compat_client_streaming_trait(src, &fn_name, &req, &resp, svc, m);
            }
            (false, true) => {
                let assoc = stream_assoc(m);
                let _ = writeln!(
                    src,
                    "    type {assoc}: {C}::ResponseStream<Item = ::core::result::Result<{resp}, {C}::Status>> + Send + 'static;"
                );
                emit_compat_server_streaming_trait(src, &fn_name, &req, &assoc, svc, m);
            }
            (true, true) => {
                let assoc = stream_assoc(m);
                let _ = writeln!(
                    src,
                    "    type {assoc}: {C}::ResponseStream<Item = ::core::result::Result<{resp}, {C}::Status>> + Send + 'static;"
                );
                emit_compat_bidi_trait(src, &fn_name, &req, &assoc, svc, m);
            }
        }
    }
    let _ = writeln!(src, "}}");
}

fn emit_compat_unary_trait(
    src: &mut String,
    fn_name: &str,
    req: &str,
    resp: &str,
    svc: &ServiceDescriptor,
    m: &MethodDescriptor,
) {
    let _ = writeln!(
        src,
        "    fn {fn_name}(&self, request: {C}::Request<{req}>) -> impl ::core::future::Future<Output = ::core::result::Result<{C}::Response<{resp}>, {C}::Status>> + Send{}",
        if generate_default_stubs() { " {" } else { ";" }
    );
    if generate_default_stubs() {
        let _ = writeln!(src, "        async move {{");
        let _ = writeln!(src, "            drop(request);");
        let _ = writeln!(
            src,
            "            ::core::result::Result::Err({C}::Status::unimplemented(\"method {}/{} not implemented\"))",
            svc.full_name, m.name
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "    }}");
    }
}

fn emit_compat_client_streaming_trait(
    src: &mut String,
    fn_name: &str,
    req: &str,
    resp: &str,
    svc: &ServiceDescriptor,
    m: &MethodDescriptor,
) {
    let _ = writeln!(
        src,
        "    fn {fn_name}(&self, request: {C}::Request<{C}::Streaming<{req}>>) -> impl ::core::future::Future<Output = ::core::result::Result<{C}::Response<{resp}>, {C}::Status>> + Send{}",
        if generate_default_stubs() { " {" } else { ";" }
    );
    if generate_default_stubs() {
        let _ = writeln!(src, "        async move {{");
        let _ = writeln!(src, "            drop(request);");
        let _ = writeln!(
            src,
            "            ::core::result::Result::Err({C}::Status::unimplemented(\"method {}/{} not implemented\"))",
            svc.full_name, m.name
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "    }}");
    }
}

fn emit_compat_server_streaming_trait(
    src: &mut String,
    fn_name: &str,
    req: &str,
    assoc: &str,
    svc: &ServiceDescriptor,
    m: &MethodDescriptor,
) {
    let _ = writeln!(
        src,
        "    fn {fn_name}(&self, request: {C}::Request<{req}>) -> impl ::core::future::Future<Output = ::core::result::Result<{C}::Response<Self::{assoc}>, {C}::Status>> + Send{}",
        if generate_default_stubs() { " {" } else { ";" }
    );
    if generate_default_stubs() {
        let _ = writeln!(src, "        async move {{");
        let _ = writeln!(src, "            drop(request);");
        let _ = writeln!(
            src,
            "            ::core::result::Result::Err({C}::Status::unimplemented(\"method {}/{} not implemented\"))",
            svc.full_name, m.name
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "    }}");
    }
}

fn emit_compat_bidi_trait(
    src: &mut String,
    fn_name: &str,
    req: &str,
    assoc: &str,
    svc: &ServiceDescriptor,
    m: &MethodDescriptor,
) {
    let _ = writeln!(
        src,
        "    fn {fn_name}(&self, request: {C}::Request<{C}::Streaming<{req}>>) -> impl ::core::future::Future<Output = ::core::result::Result<{C}::Response<Self::{assoc}>, {C}::Status>> + Send{}",
        if generate_default_stubs() { " {" } else { ";" }
    );
    if generate_default_stubs() {
        let _ = writeln!(src, "        async move {{");
        let _ = writeln!(src, "            drop(request);");
        let _ = writeln!(
            src,
            "            ::core::result::Result::Err({C}::Status::unimplemented(\"method {}/{} not implemented\"))",
            svc.full_name, m.name
        );
        let _ = writeln!(src, "        }}");
        let _ = writeln!(src, "    }}");
    }
}

fn emit_compat_server(
    src: &mut String,
    trait_name: &str,
    server: &str,
    full_name: &str,
    svc: &ServiceDescriptor,
) {
    emit_server_attributes(src, full_name, "");
    let _ = writeln!(src, "pub struct {server}<T> {{");
    let _ = writeln!(src, "    inner: ::std::sync::Arc<T>,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T> ::core::clone::Clone for {server}<T> {{");
    let _ = writeln!(
        src,
        "    fn clone(&self) -> Self {{ Self {{ inner: ::std::sync::Arc::clone(&self.inner) }} }}"
    );
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T: {trait_name}> {server}<T> {{");
    let _ = writeln!(src, "    pub const NAME: &'static str = \"{full_name}\";");
    let _ = writeln!(
        src,
        "    #[must_use] pub fn new(inner: T) -> Self {{ Self {{ inner: ::std::sync::Arc::new(inner) }} }}"
    );
    let _ = writeln!(
        src,
        "    pub async fn serve(self, addr: ::std::net::SocketAddr) -> ::core::result::Result<(), {C}::Status> {{ {G}::Server::new(self).serve(addr).await }}"
    );
    let _ = writeln!(
        src,
        "    pub async fn serve_listener(self, listener: {G}::codegen_support::TcpListener) -> ::core::result::Result<(), {C}::Status> {{ {G}::Server::new(self).serve_listener(listener).await }}"
    );
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T: {trait_name}> {G}::Service for {server}<T> {{");
    let _ = writeln!(src, "    const NAME: &'static str = \"{full_name}\";");
    let _ = writeln!(
        src,
        "    fn call(&self, rpc: {G}::Rpc) -> impl ::core::future::Future<Output = ()> + Send {{"
    );
    let _ = writeln!(
        src,
        "        let inner = ::std::sync::Arc::clone(&self.inner);"
    );
    let _ = writeln!(src, "        async move {{");
    let _ = writeln!(src, "            match rpc.method() {{");
    for m in &svc.methods {
        emit_compat_route(src, m);
    }
    let _ = writeln!(src, "                _ => rpc.unimplemented(),");
    let _ = writeln!(src, "            }}");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
}

fn emit_compat_route(src: &mut String, m: &MethodDescriptor) {
    let fn_name = to_snake(&m.name);
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    let _ = writeln!(src, "                \"{}\" => {{", m.name);
    match (m.client_streaming, m.server_streaming) {
        (false, false) => {
            let _ = writeln!(
                src,
                "                    rpc.unary(move |request: {C}::Request<{req}>| async move {{ inner.{fn_name}(request).await }}).await;"
            );
        }
        (true, false) => {
            let _ = writeln!(
                src,
                "                    rpc.client_streaming(move |request: {C}::Request<{C}::Streaming<{req}>>| async move {{ inner.{fn_name}(request).await }}).await;"
            );
        }
        (false, true) => {
            let _ = writeln!(
                src,
                "                    rpc.server_streaming(move |request: {C}::Request<{req}>| async move {{ inner.{fn_name}(request).await.map({C}::response_stream::<{resp}, _>) }}).await;"
            );
        }
        (true, true) => {
            let _ = writeln!(
                src,
                "                    rpc.bidi_streaming(move |request: {C}::Request<{C}::Streaming<{req}>>| async move {{ inner.{fn_name}(request).await.map({C}::response_stream::<{resp}, _>) }}).await;"
            );
        }
    }
    let _ = writeln!(src, "                }}");
}

fn emit_compat_client(
    src: &mut String,
    client: &str,
    full_name: &str,
    svc: &ServiceDescriptor,
    path_prefix: &str,
) {
    emit_client_attributes(src, full_name, "");
    let _ = writeln!(src, "#[derive(::core::clone::Clone)]");
    let _ = writeln!(src, "pub struct {client} {{");
    let _ = writeln!(src, "    channel: {G}::Channel,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl {client} {{");
    let _ = writeln!(src, "    pub const NAME: &'static str = \"{full_name}\";");
    let _ = writeln!(
        src,
        "    #[must_use] pub fn new(channel: {G}::Channel) -> Self {{ Self {{ channel }} }}"
    );
    let _ = writeln!(
        src,
        "    pub async fn connect(target: impl Into<{G}::Target>) -> ::core::result::Result<Self, {C}::Status> {{ Ok(Self::new({G}::Channel::connect(target).await?)) }}"
    );
    for m in &svc.methods {
        emit_compat_client_method(src, m, path_prefix);
    }
    let _ = writeln!(src, "}}");
}

fn emit_compat_client_method(src: &mut String, m: &MethodDescriptor, path_prefix: &str) {
    let fn_name = to_snake(&m.name);
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    let path = format!("{path_prefix}/{}", m.name);
    match (m.client_streaming, m.server_streaming) {
        (false, false) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}(&mut self, request: impl {C}::IntoRequest<{req}>) -> ::core::result::Result<{C}::Response<{resp}>, {C}::Status> {{"
            );
            let _ = writeln!(
                src,
                "        self.channel.unary(\"{path}\", request.into_request()).await"
            );
            let _ = writeln!(src, "    }}");
        }
        (true, false) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}<S>(&mut self, request: S) -> ::core::result::Result<{C}::Response<{resp}>, {C}::Status>"
            );
            let _ = writeln!(src, "    where S: {C}::IntoStreamingRequest<{req}> {{");
            let _ = writeln!(
                src,
                "        let request = request.into_streaming_request();"
            );
            let _ = writeln!(
                src,
                "        let (stream, parts) = request.into_message_and_parts();"
            );
            let _ = writeln!(
                src,
                "        let request = {C}::Request::<()>::from_message_and_parts((), parts);"
            );
            let _ = writeln!(
                src,
                "        let (sender, call) = self.channel.client_streaming(\"{path}\", request);"
            );
            let _ = writeln!(src, "        {C}::spawn_request_stream(stream, sender);");
            let _ = writeln!(src, "        call.await");
            let _ = writeln!(src, "    }}");
        }
        (false, true) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}(&mut self, request: impl {C}::IntoRequest<{req}>) -> ::core::result::Result<{C}::Response<{C}::Streaming<{resp}>>, {C}::Status> {{"
            );
            let _ = writeln!(
                src,
                "        self.channel.server_streaming(\"{path}\", request.into_request()).await"
            );
            let _ = writeln!(src, "    }}");
        }
        (true, true) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}<S>(&mut self, request: S) -> ::core::result::Result<{C}::Response<{C}::Streaming<{resp}>>, {C}::Status>"
            );
            let _ = writeln!(src, "    where S: {C}::IntoStreamingRequest<{req}> {{");
            let _ = writeln!(
                src,
                "        let request = request.into_streaming_request();"
            );
            let _ = writeln!(
                src,
                "        let (stream, parts) = request.into_message_and_parts();"
            );
            let _ = writeln!(
                src,
                "        let request = {C}::Request::<()>::from_message_and_parts((), parts);"
            );
            let _ = writeln!(
                src,
                "        let (sender, call) = self.channel.bidi(\"{path}\", request);"
            );
            let _ = writeln!(src, "        {C}::spawn_request_stream(stream, sender);");
            let _ = writeln!(src, "        call.await");
            let _ = writeln!(src, "    }}");
        }
    }
}
