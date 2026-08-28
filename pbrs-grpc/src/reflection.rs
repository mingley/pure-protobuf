//! `grpc.reflection.v1.ServerReflection`: the standard reflection service.
//!
//! Register each service's generated `FILE_DESCRIPTOR_SET`, mount the result
//! next to your handlers, and `grpcurl` can list and describe them.
//!
//! ```ignore
//! let reflection = pbrs_grpc::reflection::Builder::new()
//!     .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)?
//!     .build()?;
//! Router::new()
//!     .add_service(reflection)
//!     .add_service(GreeterServer::new(MyGreeter))
//!     .serve(addr)
//!     .await?;
//! ```

#![allow(missing_docs, reason = "messages come from the code generator")]

include!(concat!(env!("OUT_DIR"), "/reflection.rs"));

use crate::request::{Request, Response};
use crate::status::{Code, Status};
use crate::stream::Streaming;
use pbrs::DescriptorPool;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Builds a [`ServerReflection`] service from encoded `FileDescriptorSet`s.
///
/// Call [`Self::register_encoded_file_descriptor_set`] once per generated
/// `FILE_DESCRIPTOR_SET`. Duplicate file names keep the last copy.
#[derive(Clone, Default)]
pub struct Builder {
    sets: Vec<Vec<u8>>,
}

impl Builder {
    /// An empty builder. [`Self::build`] succeeds and lists no services until
    /// you register at least one descriptor set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge `bytes`, a serialized `google.protobuf.FileDescriptorSet`.
    ///
    /// This is what codegen emits as `FILE_DESCRIPTOR_SET`.
    #[must_use]
    pub fn register_encoded_file_descriptor_set(mut self, bytes: impl AsRef<[u8]>) -> Self {
        self.sets.push(bytes.as_ref().to_vec());
        self
    }

    /// Finish. Parses every registered set; a corrupt set is
    /// [`Code::InvalidArgument`].
    pub fn build(self) -> Result<ServerReflectionServer<Reflection>, Status> {
        Ok(ServerReflectionServer::new(Reflection::from_sets(
            &self.sets,
        )?))
    }
}

/// Implementation of [`ServerReflection`] backed by registered descriptor sets.
#[derive(Clone)]
pub struct Reflection {
    inner: Arc<Registry>,
}

struct FileEnt {
    encoded: Vec<u8>,
    deps: Vec<String>,
}

struct Registry {
    files: BTreeMap<String, FileEnt>,
    symbols: BTreeMap<String, String>,
    services: Vec<String>,
}

impl Reflection {
    fn from_sets(sets: &[Vec<u8>]) -> Result<Self, Status> {
        let mut files = BTreeMap::new();
        let mut combined = Vec::new();
        for set in sets {
            combined.extend_from_slice(set);
            for file in split_fds(set)? {
                files.insert(
                    file.name,
                    FileEnt {
                        encoded: file.encoded,
                        deps: file.deps,
                    },
                );
            }
        }
        let pool = DescriptorPool::from_file_descriptor_set(&combined)
            .map_err(|_| Status::invalid_argument("file descriptor set"))?;

        let mut symbols = BTreeMap::new();
        for name in pool.collect_names() {
            if let Some(msg) = pool.get_message(&name) {
                symbols.insert(name, msg.file_name.clone());
            }
        }
        for name in pool.collect_enum_names() {
            if let Some(en) = pool.get_enum(&name) {
                symbols.insert(name, en.file_name.clone());
            }
        }
        let mut services = Vec::new();
        for svc in pool.collect_services() {
            symbols.insert(svc.full_name.clone(), svc.file_name.clone());
            for method in &svc.methods {
                symbols.insert(
                    format!("{}.{}", svc.full_name, method.name),
                    svc.file_name.clone(),
                );
            }
            services.push(svc.full_name.clone());
        }
        services.sort();
        services.dedup();
        Ok(Self {
            inner: Arc::new(Registry {
                files,
                symbols,
                services,
            }),
        })
    }

    fn answer(&self, req: &ServerReflectionRequest) -> ServerReflectionResponse {
        let mut resp = ServerReflectionResponse::new();
        resp.set_valid_host(req.host());
        resp.set_original_request(req.clone());
        if req.has_list_services() {
            resp.set_list_services_response(self.list_services());
        } else if req.has_file_by_filename() {
            match self.file_by_name(req.file_by_filename()) {
                Ok(files) => resp.set_file_descriptor_response(files),
                Err(err) => resp.set_error_response(err),
            }
        } else if req.has_file_containing_symbol() {
            match self.file_for_symbol(req.file_containing_symbol()) {
                Ok(files) => resp.set_file_descriptor_response(files),
                Err(err) => resp.set_error_response(err),
            }
        } else if req.has_file_containing_extension() {
            resp.set_error_response(error(
                Code::NotFound,
                "extension not in any registered descriptor set",
            ));
        } else if req.has_all_extension_numbers_of_type() {
            let mut numbers = ExtensionNumberResponse::new();
            numbers.set_base_type_name(req.all_extension_numbers_of_type());
            resp.set_all_extension_numbers_response(numbers);
        } else {
            resp.set_error_response(error(
                Code::InvalidArgument,
                "reflection request had no message_request",
            ));
        }
        resp
    }

    fn list_services(&self) -> ListServiceResponse {
        let mut list = ListServiceResponse::new();
        for name in &self.inner.services {
            let mut svc = ServiceResponse::new();
            svc.set_name(name.as_str());
            list.service_mut().push(svc);
        }
        list
    }

    fn file_by_name(&self, name: &pbrs::ProtoStr) -> Result<FileDescriptorResponse, ErrorResponse> {
        self.files_for(&proto_str(name))
    }

    fn file_for_symbol(
        &self,
        symbol: &pbrs::ProtoStr,
    ) -> Result<FileDescriptorResponse, ErrorResponse> {
        let symbol = proto_str(symbol);
        let key = symbol.trim_start_matches('.');
        let file = self.inner.symbols.get(key).ok_or_else(|| {
            error(
                Code::NotFound,
                format!("symbol {key:?} is not in any registered descriptor set"),
            )
        })?;
        self.files_for(file)
    }

    fn files_for(&self, name: &str) -> Result<FileDescriptorResponse, ErrorResponse> {
        if self.lookup_file(name).is_none() {
            return Err(error(
                Code::NotFound,
                format!("file {name:?} is not in any registered descriptor set"),
            ));
        }
        let mut out = FileDescriptorResponse::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![name.to_owned()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(file) = self.lookup_file(&current) else {
                continue;
            };
            out.file_descriptor_proto_mut().push(file.encoded.clone());
            stack.extend(file.deps.iter().cloned());
        }
        Ok(out)
    }

    fn lookup_file(&self, name: &str) -> Option<&FileEnt> {
        self.inner.files.get(name).or_else(|| {
            self.inner
                .files
                .iter()
                .find(|(n, _)| {
                    *n == name
                        || n.ends_with(name)
                        || name.ends_with(n.as_str())
                        || n.rsplit('/').next() == Some(name)
                })
                .map(|(_, f)| f)
        })
    }
}

impl ServerReflection for Reflection {
    async fn server_reflection_info(
        &self,
        request: Request<Streaming<ServerReflectionRequest>>,
    ) -> Result<Response<Streaming<ServerReflectionResponse>>, Status> {
        let mut inbound = request.into_inner();
        let (tx, stream) = Streaming::channel(8);
        let inner = self.clone();
        drop(tokio::spawn(async move {
            while let Ok(Some(req)) = inbound.message().await {
                if tx.send(inner.answer(&req)).await.is_err() {
                    break;
                }
            }
        }));
        Ok(Response::new(stream))
    }
}

/// The standard reflection service from `sets`, already wrapped for [`crate::Router`].
pub fn service(
    sets: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> Result<ServerReflectionServer<Reflection>, Status> {
    let mut builder = Builder::new();
    for set in sets {
        builder = builder.register_encoded_file_descriptor_set(set);
    }
    builder.build()
}

fn error(code: Code, message: impl Into<String>) -> ErrorResponse {
    let mut err = ErrorResponse::new();
    err.set_error_code(code.to_i32());
    err.set_error_message(message.into());
    err
}

fn proto_str(value: &pbrs::ProtoStr) -> String {
    String::from_utf8_lossy(value.as_bytes()).into_owned()
}

const WIRE_VARINT: u32 = 0;
const WIRE_I64: u32 = 1;
const WIRE_LEN: u32 = 2;
const WIRE_I32: u32 = 5;

struct ParsedFile {
    name: String,
    encoded: Vec<u8>,
    deps: Vec<String>,
}

fn split_fds(bytes: &[u8]) -> Result<Vec<ParsedFile>, Status> {
    let mut files = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        if n == 1 && w == WIRE_LEN {
            let payload = read_len(bytes, &mut pos)?;
            let (name, deps) = file_name_and_deps(payload)?;
            files.push(ParsedFile {
                name,
                encoded: payload.to_vec(),
                deps,
            });
        } else {
            skip_field(bytes, &mut pos, w)?;
        }
    }
    Ok(files)
}

fn file_name_and_deps(bytes: &[u8]) -> Result<(String, Vec<String>), Status> {
    let mut name = String::new();
    let mut deps = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) = decode_tag(bytes, &mut pos)?;
        match (n, w) {
            (1, WIRE_LEN) => name = read_string(bytes, &mut pos)?,
            (3, WIRE_LEN) => deps.push(read_string(bytes, &mut pos)?),
            _ => skip_field(bytes, &mut pos, w)?,
        }
    }
    Ok((name, deps))
}

fn decode_tag(buf: &[u8], pos: &mut usize) -> Result<(u32, u32), Status> {
    let v = decode_varint(buf, pos)?;
    let wire = u32::try_from(v & 7).unwrap_or(0);
    let number = u32::try_from(v >> 3).map_err(|_| Status::invalid_argument("descriptor tag"))?;
    Ok((number, wire))
}

fn decode_varint(buf: &[u8], pos: &mut usize) -> Result<u64, Status> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for _ in 0..10 {
        let b = *buf
            .get(*pos)
            .ok_or_else(|| Status::invalid_argument("truncated descriptor set"))?;
        *pos += 1;
        let chunk = u64::from(b & 0x7f);
        result |= chunk
            .checked_shl(shift)
            .ok_or_else(|| Status::invalid_argument("descriptor varint"))?;
        if b & 0x80 == 0 {
            return Ok(result);
        }
        shift = shift
            .checked_add(7)
            .ok_or_else(|| Status::invalid_argument("descriptor varint"))?;
    }
    Err(Status::invalid_argument("descriptor varint"))
}

fn read_len<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], Status> {
    let len = decode_varint(buf, pos)?;
    let n = usize::try_from(len).map_err(|_| Status::invalid_argument("descriptor length"))?;
    let start = *pos;
    let end = start
        .checked_add(n)
        .ok_or_else(|| Status::invalid_argument("descriptor length"))?;
    let slice = buf
        .get(start..end)
        .ok_or_else(|| Status::invalid_argument("truncated descriptor set"))?;
    *pos = end;
    Ok(slice)
}

fn read_string(buf: &[u8], pos: &mut usize) -> Result<String, Status> {
    let bytes = read_len(buf, pos)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn skip_field(buf: &[u8], pos: &mut usize, wire: u32) -> Result<(), Status> {
    match wire {
        WIRE_VARINT => {
            decode_varint(buf, pos)?;
            Ok(())
        }
        WIRE_I64 => {
            *pos = pos
                .checked_add(8)
                .filter(|p| *p <= buf.len())
                .ok_or_else(|| Status::invalid_argument("truncated descriptor set"))?;
            Ok(())
        }
        WIRE_LEN => {
            read_len(buf, pos)?;
            Ok(())
        }
        WIRE_I32 => {
            *pos = pos
                .checked_add(4)
                .filter(|p| *p <= buf.len())
                .ok_or_else(|| Status::invalid_argument("truncated descriptor set"))?;
            Ok(())
        }
        _ => Err(Status::invalid_argument("descriptor wire type")),
    }
}
