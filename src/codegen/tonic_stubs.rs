//! MX-01 split of `super`: tonic_stubs (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{MethodDescriptor, ServiceDescriptor};

pub(crate) fn emit_service(src: &mut String, svc: &ServiceDescriptor) {
    let mut svc_clone = (*svc).clone();
    svc_clone.methods.sort_by(|a, b| a.name.cmp(&b.name));
    let svc = &svc_clone;
    let svc_ty = rust_ident(&svc.full_name);
    let client = format!("{svc_ty}Client");
    let server = format!("{svc_ty}Server");
    let path_prefix = format!("/{}", svc.full_name);
    emit_doc_comments(src, &svc.comments, "");
    if !svc.comments.is_empty() {
        let _ = writeln!(src, "///");
    }
    let _ = writeln!(src, "/// The `{}` service.", svc.full_name);
    if svc.deprecated {
        let _ = writeln!(src, "///");
        let _ = writeln!(src, "/// # Deprecated");
        let _ = writeln!(src, "#[deprecated]");
    }
    let _ = writeln!(src, "pub trait {svc_ty}: Send + Sync + 'static {{");
    for m in &svc.methods {
        emit_service_trait_method(src, m);
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "#[derive(Clone, Debug)]");
    let _ = writeln!(src, "pub struct {client}<T> {{");
    let _ = writeln!(src, "    inner: tonic::client::Grpc<T>,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T> {client}<T>");
    let _ = writeln!(src, "where");
    let _ = writeln!(src, "    T: tonic::client::GrpcService<tonic::body::Body>,");
    let _ = writeln!(src, "    T::Error: Into<tonic::codegen::StdError>,");
    let _ = writeln!(
        src,
        "    T::ResponseBody: tonic::codegen::Body<Data = tonic::codegen::Bytes> + Send + 'static,"
    );
    let _ = writeln!(
        src,
        "    <T::ResponseBody as tonic::codegen::Body>::Error: Into<tonic::codegen::StdError> + Send,"
    );
    let _ = writeln!(src, "{{");
    let _ = writeln!(
        src,
        "    pub fn new(inner: T) -> Self {{ Self {{ inner: tonic::client::Grpc::new(inner) }} }}"
    );
    let _ = writeln!(
        src,
        "    pub fn with_interceptor<F>(inner: T, interceptor: F) -> {client}<tonic::service::interceptor::InterceptedService<T, F>>"
    );
    let _ = writeln!(src, "    where");
    let _ = writeln!(src, "        F: tonic::service::Interceptor,");
    let _ = writeln!(src, "        T::ResponseBody: Default,");
    let _ = writeln!(
        src,
        "        T: tonic::codegen::Service<http::Request<tonic::body::Body>, Response = http::Response<<T as tonic::client::GrpcService<tonic::body::Body>>::ResponseBody>>,"
    );
    let _ = writeln!(
        src,
        "        <T as tonic::codegen::Service<http::Request<tonic::body::Body>>>::Error: Into<tonic::codegen::StdError> + Send + Sync,"
    );
    let _ = writeln!(src, "    {{");
    let _ = writeln!(
        src,
        "        {client}::new(tonic::service::interceptor::InterceptedService::new(inner, interceptor))"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn send_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {{ self.inner = self.inner.send_compressed(encoding); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn accept_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {{ self.inner = self.inner.accept_compressed(encoding); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {{ self.inner = self.inner.max_decoding_message_size(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {{ self.inner = self.inner.max_encoding_message_size(limit); self }}"
    );
    for m in &svc.methods {
        emit_client_method(src, m, &path_prefix);
    }
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "pub struct {server}<T> {{");
    let _ = writeln!(src, "    inner: Arc<T>,");
    let _ = writeln!(
        src,
        "    accept_compression_encodings: tonic::codec::EnabledCompressionEncodings,"
    );
    let _ = writeln!(
        src,
        "    send_compression_encodings: tonic::codec::EnabledCompressionEncodings,"
    );
    let _ = writeln!(src, "    max_decoding_message_size: Option<usize>,");
    let _ = writeln!(src, "    max_encoding_message_size: Option<usize>,");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T> Clone for {server}<T> {{");
    let _ = writeln!(src, "    fn clone(&self) -> Self {{");
    let _ = writeln!(src, "        Self {{");
    let _ = writeln!(src, "            inner: self.inner.clone(),");
    let _ = writeln!(
        src,
        "            accept_compression_encodings: self.accept_compression_encodings,"
    );
    let _ = writeln!(
        src,
        "            send_compression_encodings: self.send_compression_encodings,"
    );
    let _ = writeln!(
        src,
        "            max_decoding_message_size: self.max_decoding_message_size,"
    );
    let _ = writeln!(
        src,
        "            max_encoding_message_size: self.max_encoding_message_size,"
    );
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src, "impl<T: {svc_ty}> {server}<T> {{");
    let _ = writeln!(src, "    pub fn new(inner: T) -> Self {{");
    let _ = writeln!(src, "        Self {{");
    let _ = writeln!(src, "            inner: Arc::new(inner),");
    let _ = writeln!(
        src,
        "            accept_compression_encodings: Default::default(),"
    );
    let _ = writeln!(
        src,
        "            send_compression_encodings: Default::default(),"
    );
    let _ = writeln!(src, "            max_decoding_message_size: None,");
    let _ = writeln!(src, "            max_encoding_message_size: None,");
    let _ = writeln!(src, "        }}");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn with_interceptor<F>(inner: T, interceptor: F) -> tonic::service::interceptor::InterceptedService<Self, F>"
    );
    let _ = writeln!(src, "    where F: tonic::service::Interceptor {{");
    let _ = writeln!(
        src,
        "        tonic::service::interceptor::InterceptedService::new(Self::new(inner), interceptor)"
    );
    let _ = writeln!(src, "    }}");
    let _ = writeln!(
        src,
        "    pub fn accept_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {{ self.accept_compression_encodings.enable(encoding); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn send_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {{ self.send_compression_encodings.enable(encoding); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn max_decoding_message_size(mut self, limit: usize) -> Self {{ self.max_decoding_message_size = Some(limit); self }}"
    );
    let _ = writeln!(
        src,
        "    pub fn max_encoding_message_size(mut self, limit: usize) -> Self {{ self.max_encoding_message_size = Some(limit); self }}"
    );
    let _ = writeln!(src, "}}");
    let _ = writeln!(
        src,
        "impl<T: {svc_ty}> tonic::server::NamedService for {server}<T> {{"
    );
    let _ = writeln!(src, "    const NAME: &'static str = \"{}\";", svc.full_name);
    let _ = writeln!(src, "}}");
    let _ = writeln!(
        src,
        "impl<T, B> tonic::codegen::Service<http::Request<B>> for {server}<T>"
    );
    let _ = writeln!(src, "where");
    let _ = writeln!(src, "    T: {svc_ty},");
    let _ = writeln!(src, "    B: tonic::codegen::Body + Send + 'static,");
    let _ = writeln!(
        src,
        "    B::Error: Into<tonic::codegen::StdError> + Send + 'static,"
    );
    let _ = writeln!(src, "{{");
    let _ = writeln!(
        src,
        "    type Response = http::Response<tonic::body::Body>;"
    );
    let _ = writeln!(src, "    type Error = Infallible;");
    let _ = writeln!(
        src,
        "    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;"
    );
    let _ = writeln!(
        src,
        "    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {{ Poll::Ready(Ok(())) }}"
    );
    let _ = writeln!(
        src,
        "    fn call(&mut self, req: http::Request<B>) -> Self::Future {{"
    );
    let _ = writeln!(src, "        let inner = self.inner.clone();");
    let _ = writeln!(
        src,
        "        let accept = self.accept_compression_encodings;"
    );
    let _ = writeln!(src, "        let send = self.send_compression_encodings;");
    let _ = writeln!(src, "        let max_dec = self.max_decoding_message_size;");
    let _ = writeln!(src, "        let max_enc = self.max_encoding_message_size;");
    let _ = writeln!(src, "        Box::pin(async move {{");
    let _ = writeln!(src, "            match req.uri().path() {{");
    for m in &svc.methods {
        emit_server_route(src, m, &path_prefix, &svc_ty);
    }
    let _ = writeln!(src, "                _ => {{");
    let _ = writeln!(
        src,
        "                    let mut response = http::Response::new(tonic::body::Body::default());"
    );
    let _ = writeln!(
        src,
        "                    let headers = response.headers_mut();"
    );
    let _ = writeln!(
        src,
        "                    headers.insert(tonic::Status::GRPC_STATUS, (tonic::Code::Unimplemented as i32).into());"
    );
    let _ = writeln!(
        src,
        "                    headers.insert(http::header::CONTENT_TYPE, tonic::metadata::GRPC_CONTENT_TYPE);"
    );
    let _ = writeln!(src, "                    Ok(response)");
    let _ = writeln!(src, "                }}");
    let _ = writeln!(src, "            }}");
    let _ = writeln!(src, "        }})");
    let _ = writeln!(src, "    }}");
    let _ = writeln!(src, "}}");
}

pub(crate) fn emit_service_trait_method(src: &mut String, m: &MethodDescriptor) {
    let fn_name = to_snake(&m.name);
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    let assoc = format!("{}Stream", m.name);
    emit_doc_comments(src, &m.comments, "    ");
    if !m.comments.is_empty() {
        let _ = writeln!(src, "    ///");
    }
    let shape = shape_of(m);
    let _ = writeln!(src, "    /// {} call: `{}`.", shape.description, m.name);
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
    match (m.client_streaming, m.server_streaming) {
        (false, false) => {
            let _ = writeln!(
                src,
                "    fn {fn_name}(&self, request: tonic::Request<{req}>) -> impl Future<Output = Result<tonic::Response<{resp}>, tonic::Status>> + Send;"
            );
        }
        (true, true) => {
            let _ = writeln!(
                src,
                "    type {assoc}: tokio_stream::Stream<Item = Result<{resp}, tonic::Status>> + Send + 'static;"
            );
            let _ = writeln!(
                src,
                "    fn {fn_name}(&self, request: tonic::Request<tonic::Streaming<{req}>>) -> impl Future<Output = Result<tonic::Response<Self::{assoc}>, tonic::Status>> + Send;"
            );
        }
        (true, false) => {
            let _ = writeln!(
                src,
                "    fn {fn_name}(&self, request: tonic::Request<tonic::Streaming<{req}>>) -> impl Future<Output = Result<tonic::Response<{resp}>, tonic::Status>> + Send;"
            );
        }
        (false, true) => {
            let _ = writeln!(
                src,
                "    type {assoc}: tokio_stream::Stream<Item = Result<{resp}, tonic::Status>> + Send + 'static;"
            );
            let _ = writeln!(
                src,
                "    fn {fn_name}(&self, request: tonic::Request<{req}>) -> impl Future<Output = Result<tonic::Response<Self::{assoc}>, tonic::Status>> + Send;"
            );
        }
    }
}

pub(crate) fn emit_client_method(src: &mut String, m: &MethodDescriptor, prefix: &str) {
    let fn_name = to_snake(&m.name);
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    let path = format!("{prefix}/{}", m.name);
    emit_doc_comments(src, &m.comments, "    ");
    if !m.comments.is_empty() {
        let _ = writeln!(src, "    ///");
    }
    let shape = shape_of(m);
    let _ = writeln!(src, "    /// {} call: `{}`.", shape.description, m.name);
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
    match (m.client_streaming, m.server_streaming) {
        (false, false) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}(&mut self, request: tonic::Request<{req}>) -> Result<tonic::Response<{resp}>, tonic::Status> {{"
            );
            let _ = writeln!(
                src,
                "        self.inner.ready().await.map_err(|e| tonic::Status::unknown(e.into().to_string()))?;"
            );
            let _ = writeln!(
                src,
                "        self.inner.unary(request, http::uri::PathAndQuery::from_static(\"{path}\"), ProtobufCodec::<{req}, {resp}>::default()).await"
            );
            let _ = writeln!(src, "    }}");
        }
        (true, true) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}<S>(&mut self, request: tonic::Request<S>) -> Result<tonic::Response<tonic::Streaming<{resp}>>, tonic::Status>"
            );
            let _ = writeln!(
                src,
                "    where S: tokio_stream::Stream<Item = {req}> + Send + 'static {{"
            );
            let _ = writeln!(
                src,
                "        self.inner.ready().await.map_err(|e| tonic::Status::unknown(e.into().to_string()))?;"
            );
            let _ = writeln!(
                src,
                "        self.inner.streaming(request, http::uri::PathAndQuery::from_static(\"{path}\"), ProtobufCodec::<{req}, {resp}>::default()).await"
            );
            let _ = writeln!(src, "    }}");
        }
        (true, false) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}<S>(&mut self, request: tonic::Request<S>) -> Result<tonic::Response<{resp}>, tonic::Status>"
            );
            let _ = writeln!(
                src,
                "    where S: tokio_stream::Stream<Item = {req}> + Send + 'static {{"
            );
            let _ = writeln!(
                src,
                "        self.inner.ready().await.map_err(|e| tonic::Status::unknown(e.into().to_string()))?;"
            );
            let _ = writeln!(
                src,
                "        self.inner.client_streaming(request, http::uri::PathAndQuery::from_static(\"{path}\"), ProtobufCodec::<{req}, {resp}>::default()).await"
            );
            let _ = writeln!(src, "    }}");
        }
        (false, true) => {
            let _ = writeln!(
                src,
                "    pub async fn {fn_name}(&mut self, request: tonic::Request<{req}>) -> Result<tonic::Response<tonic::Streaming<{resp}>>, tonic::Status> {{"
            );
            let _ = writeln!(
                src,
                "        self.inner.ready().await.map_err(|e| tonic::Status::unknown(e.into().to_string()))?;"
            );
            let _ = writeln!(
                src,
                "        self.inner.server_streaming(request, http::uri::PathAndQuery::from_static(\"{path}\"), ProtobufCodec::<{req}, {resp}>::default()).await"
            );
            let _ = writeln!(src, "    }}");
        }
    }
}

pub(crate) fn emit_server_route(
    src: &mut String,
    m: &MethodDescriptor,
    prefix: &str,
    svc_ty: &str,
) {
    let fn_name = to_snake(&m.name);
    let req = rust_type_path(&m.input_type);
    let resp = rust_type_path(&m.output_type);
    let path = format!("{prefix}/{}", m.name);
    match (m.client_streaming, m.server_streaming) {
        (false, false) => {
            let _ = writeln!(src, "                \"{path}\" => {{");
            let _ = writeln!(src, "                    struct Svc<T>(Arc<T>);");
            let _ = writeln!(
                src,
                "                    impl<T: {svc_ty}> tonic::server::UnaryService<{req}> for Svc<T> {{"
            );
            let _ = writeln!(src, "                        type Response = {resp};");
            let _ = writeln!(
                src,
                "                        type Future = Pin<Box<dyn Future<Output = Result<tonic::Response<{resp}>, tonic::Status>> + Send>>;"
            );
            let _ = writeln!(
                src,
                "                        fn call(&mut self, request: tonic::Request<{req}>) -> Self::Future {{"
            );
            let _ = writeln!(
                src,
                "                            let inner = self.0.clone();"
            );
            let _ = writeln!(
                src,
                "                            Box::pin(async move {{ inner.{fn_name}(request).await }})"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }}");
            let _ = writeln!(
                src,
                "                    let mut grpc = tonic::server::Grpc::new(ProtobufCodec::<{resp}, {req}>::default()).apply_compression_config(accept, send).apply_max_message_size_config(max_dec, max_enc);"
            );
            let _ = writeln!(
                src,
                "                    Ok(grpc.unary(Svc(inner), req).await)"
            );
            let _ = writeln!(src, "                }}");
        }
        (true, true) => {
            let assoc = format!("{}Stream", m.name);
            let _ = writeln!(src, "                \"{path}\" => {{");
            let _ = writeln!(src, "                    struct Svc<T>(Arc<T>);");
            let _ = writeln!(
                src,
                "                    impl<T: {svc_ty}> tonic::server::StreamingService<{req}> for Svc<T> {{"
            );
            let _ = writeln!(src, "                        type Response = {resp};");
            let _ = writeln!(
                src,
                "                        type ResponseStream = T::{assoc};"
            );
            let _ = writeln!(
                src,
                "                        type Future = Pin<Box<dyn Future<Output = Result<tonic::Response<Self::ResponseStream>, tonic::Status>> + Send>>;"
            );
            let _ = writeln!(
                src,
                "                        fn call(&mut self, request: tonic::Request<tonic::Streaming<{req}>>) -> Self::Future {{"
            );
            let _ = writeln!(
                src,
                "                            let inner = self.0.clone();"
            );
            let _ = writeln!(
                src,
                "                            Box::pin(async move {{ inner.{fn_name}(request).await }})"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }}");
            let _ = writeln!(
                src,
                "                    let mut grpc = tonic::server::Grpc::new(ProtobufCodec::<{resp}, {req}>::default()).apply_compression_config(accept, send).apply_max_message_size_config(max_dec, max_enc);"
            );
            let _ = writeln!(
                src,
                "                    Ok(grpc.streaming(Svc(inner), req).await)"
            );
            let _ = writeln!(src, "                }}");
        }
        (true, false) => {
            let _ = writeln!(src, "                \"{path}\" => {{");
            let _ = writeln!(src, "                    struct Svc<T>(Arc<T>);");
            let _ = writeln!(
                src,
                "                    impl<T: {svc_ty}> tonic::server::ClientStreamingService<{req}> for Svc<T> {{"
            );
            let _ = writeln!(src, "                        type Response = {resp};");
            let _ = writeln!(
                src,
                "                        type Future = Pin<Box<dyn Future<Output = Result<tonic::Response<{resp}>, tonic::Status>> + Send>>;"
            );
            let _ = writeln!(
                src,
                "                        fn call(&mut self, request: tonic::Request<tonic::Streaming<{req}>>) -> Self::Future {{"
            );
            let _ = writeln!(
                src,
                "                            let inner = self.0.clone();"
            );
            let _ = writeln!(
                src,
                "                            Box::pin(async move {{ inner.{fn_name}(request).await }})"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }}");
            let _ = writeln!(
                src,
                "                    let mut grpc = tonic::server::Grpc::new(ProtobufCodec::<{resp}, {req}>::default()).apply_compression_config(accept, send).apply_max_message_size_config(max_dec, max_enc);"
            );
            let _ = writeln!(
                src,
                "                    Ok(grpc.client_streaming(Svc(inner), req).await)"
            );
            let _ = writeln!(src, "                }}");
        }
        (false, true) => {
            let assoc = format!("{}Stream", m.name);
            let _ = writeln!(src, "                \"{path}\" => {{");
            let _ = writeln!(src, "                    struct Svc<T>(Arc<T>);");
            let _ = writeln!(
                src,
                "                    impl<T: {svc_ty}> tonic::server::ServerStreamingService<{req}> for Svc<T> {{"
            );
            let _ = writeln!(src, "                        type Response = {resp};");
            let _ = writeln!(
                src,
                "                        type ResponseStream = T::{assoc};"
            );
            let _ = writeln!(
                src,
                "                        type Future = Pin<Box<dyn Future<Output = Result<tonic::Response<Self::ResponseStream>, tonic::Status>> + Send>>;"
            );
            let _ = writeln!(
                src,
                "                        fn call(&mut self, request: tonic::Request<{req}>) -> Self::Future {{"
            );
            let _ = writeln!(
                src,
                "                            let inner = self.0.clone();"
            );
            let _ = writeln!(
                src,
                "                            Box::pin(async move {{ inner.{fn_name}(request).await }})"
            );
            let _ = writeln!(src, "                        }}");
            let _ = writeln!(src, "                    }}");
            let _ = writeln!(
                src,
                "                    let mut grpc = tonic::server::Grpc::new(ProtobufCodec::<{resp}, {req}>::default()).apply_compression_config(accept, send).apply_max_message_size_config(max_dec, max_enc);"
            );
            let _ = writeln!(
                src,
                "                    Ok(grpc.server_streaming(Svc(inner), req).await)"
            );
            let _ = writeln!(src, "                }}");
        }
    }
}
