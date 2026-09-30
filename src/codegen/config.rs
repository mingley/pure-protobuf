//! MX-01 split of `super`: config (mechanical move, no behavior change).

use super::*;
use crate::error::ParseError;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;

/// An error that occurred during protobuf code generation.
#[derive(Debug)]
pub enum CodegenError {
    /// The `protoc` executable was not found in PATH or failed to execute.
    MissingProtoc {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Execution of `protoc` failed with a non-zero exit status.
    ProtocExecution {
        status: std::process::ExitStatus,
        stderr: String,
        protos: Vec<PathBuf>,
    },
    /// An imported proto file could not be found.
    MissingImport {
        import: String,
        proto: PathBuf,
        detail: String,
    },
    /// A protobuf descriptor or CodeGeneratorRequest is malformed or invalid.
    MalformedDescriptor {
        detail: String,
        path: Option<PathBuf>,
    },
    /// The output directory or file could not be created or written.
    UnwritableOutput {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The `OUT_DIR` environment variable was not set and no output directory was specified.
    MissingOutDir,
    /// An I/O error occurred on a specific file path.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// An unknown plugin parameter key was encountered.
    UnknownParameter { key: String, detail: String },
    /// An invalid value was supplied for a recognized plugin parameter.
    InvalidParameter { key: String, detail: String },
    /// Multiple proto files share the same stem in an ambiguous single-file request.
    AmbiguousStem {
        stem: String,
        matches: Vec<(String, String)>,
    },
    /// A requested proto file is not present in the descriptor set.
    UnknownFile {
        file: String,
        available: Vec<(String, String)>,
    },
}

impl CodegenError {
    /// The primary path associated with this error, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::MissingProtoc { path, .. }
            | Self::UnwritableOutput { path, .. }
            | Self::Io { path, .. } => Some(path),
            Self::MissingImport { proto, .. } => Some(proto),
            Self::MalformedDescriptor { path, .. } => path.as_deref(),
            Self::ProtocExecution { protos, .. } => protos.first().map(PathBuf::as_path),
            Self::MissingOutDir
            | Self::UnknownParameter { .. }
            | Self::InvalidParameter { .. }
            | Self::AmbiguousStem { .. }
            | Self::UnknownFile { .. } => None,
        }
    }

    /// The protoc stderr output, if this error was caused by a protoc failure.
    pub fn stderr(&self) -> Option<&str> {
        match self {
            Self::ProtocExecution { stderr, .. } => Some(stderr.as_str()),
            Self::MissingImport { detail, .. } => Some(detail.as_str()),
            _ => None,
        }
    }

    /// The parameter key associated with this error, if any.
    pub fn parameter_key(&self) -> Option<&str> {
        match self {
            Self::UnknownParameter { key, .. } | Self::InvalidParameter { key, .. } => {
                Some(key.as_str())
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProtoc { path, source } => {
                write!(
                    f,
                    "protoc executable not found or failed to execute at '{}': {}",
                    path.display(),
                    source
                )
            }
            Self::ProtocExecution {
                status,
                stderr,
                protos,
            } => {
                let proto_list = protos
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                if stderr.is_empty() {
                    write!(
                        f,
                        "protoc failed with {status} while compiling [{proto_list}]"
                    )
                } else {
                    write!(
                        f,
                        "protoc failed with {status} while compiling [{proto_list}]:\n{stderr}"
                    )
                }
            }
            Self::MissingImport {
                import,
                proto,
                detail,
            } => {
                write!(
                    f,
                    "missing import '{}' required by '{}':\n{}",
                    import,
                    proto.display(),
                    detail
                )
            }
            Self::MalformedDescriptor { detail, path } => {
                if let Some(p) = path {
                    write!(
                        f,
                        "malformed protobuf descriptor at '{}': {}",
                        p.display(),
                        detail
                    )
                } else {
                    write!(f, "malformed protobuf descriptor: {}", detail)
                }
            }
            Self::UnwritableOutput { path, source } => {
                write!(
                    f,
                    "failed to write codegen output to '{}': {}",
                    path.display(),
                    source
                )
            }
            Self::MissingOutDir => {
                f.write_str("OUT_DIR environment variable is not set and no out_dir was configured")
            }
            Self::Io { path, source } => {
                write!(f, "IO error at '{}': {}", path.display(), source)
            }
            Self::UnknownParameter { key, detail } => {
                if detail.is_empty() {
                    write!(f, "unknown codegen parameter: {key}")
                } else {
                    write!(f, "unknown codegen parameter '{key}': {detail}")
                }
            }
            Self::InvalidParameter { key, detail } => {
                if detail.is_empty() {
                    write!(f, "invalid codegen parameter: {key}")
                } else {
                    write!(f, "invalid codegen parameter '{key}': {detail}")
                }
            }
            Self::AmbiguousStem { stem, matches } => {
                write!(f, "ambiguous proto stem '{stem}' across multiple files:")?;
                for (path, pkg) in matches {
                    write!(f, "\n  - {path} (package {pkg})")?;
                }
                write!(
                    f,
                    "\nUse the hierarchical path or include the root mod.rs instead."
                )
            }
            Self::UnknownFile { file, available } => {
                write!(
                    f,
                    "unknown proto file '{file}': not present in the descriptor set."
                )?;
                if !available.is_empty() {
                    write!(f, "\nAvailable files:")?;
                    for (path, pkg) in available {
                        if pkg.is_empty() {
                            write!(f, "\n  - {path}")?;
                        } else {
                            write!(f, "\n  - {path} (package {pkg})")?;
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for CodegenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingProtoc { source, .. }
            | Self::UnwritableOutput { source, .. }
            | Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<CodegenError> for ParseError {
    fn from(_: CodegenError) -> Self {
        ParseError
    }
}

thread_local! {
    pub(crate) static IDENTS: RefCell<std::collections::BTreeMap<String, String>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    pub(crate) static FIELD_IDENTS: RefCell<std::collections::BTreeMap<u32, String>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    pub(crate) static FIELD_RAWS: RefCell<std::collections::BTreeMap<u32, String>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    pub(crate) static STUBS: Cell<StubStyle> = const { Cell::new(StubStyle::Kernel) };
    pub(crate) static EMIT_DEPS: Cell<bool> = const { Cell::new(false) };
    pub(crate) static NO_WKT: Cell<bool> = const { Cell::new(false) };
    pub(crate) static SHARED_POOL: Cell<bool> = const { Cell::new(false) };
    pub(crate) static NO_REFLECT: Cell<bool> = const { Cell::new(false) };
    pub(crate) static EMIT_JSON: Cell<bool> = const { Cell::new(true) };
    pub(crate) static EMIT_TEXT: Cell<bool> = const { Cell::new(true) };
    pub(crate) static BUILD_CLIENT: Cell<bool> = const { Cell::new(true) };
    pub(crate) static BUILD_SERVER: Cell<bool> = const { Cell::new(true) };
    pub(crate) static GENERATE_DEFAULT_STUBS: Cell<bool> = const { Cell::new(true) };
    pub(crate) static USE_ARC_SELF: Cell<bool> = const { Cell::new(false) };
    pub(crate) static DISABLE_COMMENTS: Cell<bool> = const { Cell::new(false) };
    pub(crate) static SKIP_DEBUG: Cell<bool> = const { Cell::new(false) };
    pub(crate) static CURRENT_TARGET: RefCell<String> = const { RefCell::new(String::new()) };
    pub(crate) static TYPE_FILES: RefCell<std::collections::BTreeMap<String, (String, String)>> =
        const { RefCell::new(std::collections::BTreeMap::new()) };
    pub(crate) static EXTERN_PATHS: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static RUNTIME_CRATE: RefCell<Option<String>> =
        const { RefCell::new(None) };
    pub(crate) static GRPC_CRATE: RefCell<Option<String>> =
        const { RefCell::new(None) };
    pub(crate) static TONIC_CRATE: RefCell<Option<String>> =
        const { RefCell::new(None) };
    pub(crate) static CODEC_PATH: RefCell<Option<String>> =
        const { RefCell::new(None) };
    pub(crate) static TYPE_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static MESSAGE_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static ENUM_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static FIELD_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static CLIENT_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
    pub(crate) static SERVER_ATTRIBUTES: RefCell<Vec<(String, String)>> =
        const { RefCell::new(Vec::new()) };
}

pub(crate) struct CodegenStateGuard;

impl CodegenStateGuard {
    pub(crate) fn new() -> Self {
        Self::reset();
        Self
    }

    fn reset() {
        IDENTS.with(|c| c.borrow_mut().clear());
        FIELD_IDENTS.with(|c| c.borrow_mut().clear());
        FIELD_RAWS.with(|c| c.borrow_mut().clear());
        STUBS.with(|c| c.set(StubStyle::Kernel));
        EMIT_DEPS.with(|c| c.set(false));
        NO_WKT.with(|c| c.set(false));
        SHARED_POOL.with(|c| c.set(false));
        NO_REFLECT.with(|c| c.set(false));
        EMIT_JSON.with(|c| c.set(true));
        EMIT_TEXT.with(|c| c.set(true));
        BUILD_CLIENT.with(|c| c.set(true));
        BUILD_SERVER.with(|c| c.set(true));
        GENERATE_DEFAULT_STUBS.with(|c| c.set(true));
        USE_ARC_SELF.with(|c| c.set(false));
        DISABLE_COMMENTS.with(|c| c.set(false));
        SKIP_DEBUG.with(|c| c.set(false));
        CURRENT_TARGET.with(|c| c.borrow_mut().clear());
        TYPE_FILES.with(|c| c.borrow_mut().clear());
        EXTERN_PATHS.with(|c| c.borrow_mut().clear());
        RUNTIME_CRATE.with(|c| *c.borrow_mut() = None);
        GRPC_CRATE.with(|c| *c.borrow_mut() = None);
        TONIC_CRATE.with(|c| *c.borrow_mut() = None);
        CODEC_PATH.with(|c| *c.borrow_mut() = None);
        TYPE_ATTRIBUTES.with(|c| c.borrow_mut().clear());
        MESSAGE_ATTRIBUTES.with(|c| c.borrow_mut().clear());
        ENUM_ATTRIBUTES.with(|c| c.borrow_mut().clear());
        FIELD_ATTRIBUTES.with(|c| c.borrow_mut().clear());
        CLIENT_ATTRIBUTES.with(|c| c.borrow_mut().clear());
        SERVER_ATTRIBUTES.with(|c| c.borrow_mut().clear());
    }
}

impl Drop for CodegenStateGuard {
    fn drop(&mut self) {
        Self::reset();
    }
}

/// Which gRPC service stubs to emit alongside generated messages.
///
/// A `.proto` `service` block turns into client and server types. Which flavour
/// you get depends on which gRPC stack you are using; the two are mutually
/// exclusive because they claim the same `FooClient` / `FooServer` names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stubs {
    /// Messages only.
    None,
    /// `tonic` 0.14 stubs over `protobuf_tonic::ProtobufCodec`.
    /// Select with [`Config::emit_tonic_stubs`].
    Tonic,
    /// Native `pbrs-grpc` stubs. The default. Requires the generating crate
    /// to depend on `pbrs-grpc`. `FooClient` dials with `connect` /
    /// `connect_tls` / `connect_unix` / `from_io`; `FooServer` serves with
    /// `serve` / `serve_tls` / `serve_unix`.
    /// The `protoc-gen-pbrs` plugin uses this default; set
    /// `PURE_PROTOBUF_STUBS=tonic` for tonic stubs.
    #[default]
    Kernel,
}

// The public Stubs selector is exhaustive in pbrs 0.2.0. New generator
// surfaces must not add variants or change discriminants on that API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StubStyle {
    None,
    Tonic,
    TonicCompat,
    #[default]
    Kernel,
}

impl From<Stubs> for StubStyle {
    fn from(stubs: Stubs) -> Self {
        match stubs {
            Stubs::None => Self::None,
            Stubs::Tonic => Self::Tonic,
            Stubs::Kernel => Self::Kernel,
        }
    }
}

/// Resolve the stub flavour from the active thread-local config.
#[allow(dead_code, reason = "helper for inspecting current stub flavour")]
pub(crate) fn stubs_setting() -> StubStyle {
    STUBS.with(Cell::get)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExplicitOptions {
    stubs: Option<StubStyle>,
    emit_deps: Option<bool>,
    no_wkt: Option<bool>,
    shared_pool: Option<bool>,
    no_reflect: Option<bool>,
    emit_json: Option<bool>,
    emit_text: Option<bool>,
    build_client: Option<bool>,
    build_server: Option<bool>,
    generate_default_stubs: Option<bool>,
    use_arc_self: Option<bool>,
    disable_comments: Option<bool>,
    skip_debug: Option<bool>,
    include_file: Option<String>,
    pub(crate) extern_paths: Vec<(String, String)>,
    pub(crate) runtime_crate: Option<String>,
    pub(crate) grpc_crate: Option<String>,
    pub(crate) tonic_crate: Option<String>,
    pub(crate) codec_path: Option<String>,
    pub(crate) type_attributes: Vec<(String, String)>,
    pub(crate) message_attributes: Vec<(String, String)>,
    pub(crate) enum_attributes: Vec<(String, String)>,
    pub(crate) field_attributes: Vec<(String, String)>,
    pub(crate) client_attributes: Vec<(String, String)>,
    pub(crate) server_attributes: Vec<(String, String)>,
    include_source_info: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedConfig {
    pub(crate) stubs: StubStyle,
    pub(crate) emit_deps: bool,
    pub(crate) no_wkt: bool,
    pub(crate) shared_pool: bool,
    pub(crate) no_reflect: bool,
    pub(crate) emit_json: bool,
    pub(crate) emit_text: bool,
    pub(crate) build_client: bool,
    pub(crate) build_server: bool,
    pub(crate) generate_default_stubs: bool,
    pub(crate) use_arc_self: bool,
    pub(crate) disable_comments: bool,
    pub(crate) skip_debug: bool,
    pub(crate) include_file: String,
    pub(crate) extern_paths: Vec<(String, String)>,
    pub(crate) runtime_crate: Option<String>,
    pub(crate) grpc_crate: Option<String>,
    pub(crate) tonic_crate: Option<String>,
    pub(crate) codec_path: Option<String>,
    pub(crate) type_attributes: Vec<(String, String)>,
    pub(crate) message_attributes: Vec<(String, String)>,
    pub(crate) enum_attributes: Vec<(String, String)>,
    pub(crate) field_attributes: Vec<(String, String)>,
    pub(crate) client_attributes: Vec<(String, String)>,
    pub(crate) server_attributes: Vec<(String, String)>,
    pub(crate) include_source_info: bool,
}

pub(crate) fn parse_bool_param(key: &str, val: Option<&str>) -> Result<bool, CodegenError> {
    match val {
        None | Some("true") | Some("1") => Ok(true),
        Some("false") | Some("0") => Ok(false),
        Some(other) => Err(CodegenError::InvalidParameter {
            key: key.to_string(),
            detail: format!("expected 'true' or 'false' for '{key}', got '{other}'"),
        }),
    }
}

pub(crate) fn parse_plugin_parameter(parameter: &str) -> Result<ExplicitOptions, CodegenError> {
    let mut explicit = ExplicitOptions::default();
    for item in parameter.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (key, val) = match item.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (item, None),
        };
        match key {
            "stubs" => {
                let s = match val {
                    Some("kernel") => StubStyle::Kernel,
                    Some("tonic") => StubStyle::Tonic,
                    Some("none") => StubStyle::None,
                    Some("compat" | "tonic_compat" | "tonic-compat") => StubStyle::TonicCompat,
                    Some(other) => {
                        return Err(CodegenError::InvalidParameter {
                            key: "stubs".to_string(),
                            detail: format!(
                                "expected 'kernel', 'tonic', 'compat', or 'none' for 'stubs', got '{other}'"
                            ),
                        });
                    }
                    None => {
                        return Err(CodegenError::InvalidParameter {
                            key: "stubs".to_string(),
                            detail: "expected value for 'stubs' parameter ('kernel', 'tonic', 'compat', or 'none')"
                                .to_string(),
                        });
                    }
                };
                explicit.stubs = Some(s);
            }
            "emit_deps" => {
                explicit.emit_deps = Some(parse_bool_param("emit_deps", val)?);
            }
            "no_wkt" => {
                explicit.no_wkt = Some(parse_bool_param("no_wkt", val)?);
            }
            "shared_pool" => {
                explicit.shared_pool = Some(parse_bool_param("shared_pool", val)?);
            }
            "no_reflect" => {
                explicit.no_reflect = Some(parse_bool_param("no_reflect", val)?);
            }
            "emit_reflection" => {
                explicit.no_reflect = Some(!parse_bool_param("emit_reflection", val)?);
            }
            "emit_json" => {
                explicit.emit_json = Some(parse_bool_param("emit_json", val)?);
            }
            "emit_text" => {
                explicit.emit_text = Some(parse_bool_param("emit_text", val)?);
            }
            "build_client" => {
                explicit.build_client = Some(parse_bool_param("build_client", val)?);
            }
            "build_server" => {
                explicit.build_server = Some(parse_bool_param("build_server", val)?);
            }
            "generate_default_stubs" => {
                explicit.generate_default_stubs =
                    Some(parse_bool_param("generate_default_stubs", val)?);
            }
            "use_arc_self" => {
                explicit.use_arc_self = Some(parse_bool_param("use_arc_self", val)?);
            }
            "disable_comments" => {
                explicit.disable_comments = Some(parse_bool_param("disable_comments", val)?);
            }
            "skip_debug" => {
                explicit.skip_debug = Some(parse_bool_param("skip_debug", val)?);
            }
            "compile_well_known_types" => {
                explicit.no_wkt = Some(!parse_bool_param("compile_well_known_types", val)?);
            }
            "include_file" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "include_file".to_string(),
                    detail: "expected value for 'include_file'".to_string(),
                })?;
                if val_str.trim().is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "include_file".to_string(),
                        detail: "empty include_file".to_string(),
                    });
                }
                explicit.include_file = Some(val_str.trim().to_string());
            }
            "extern_path" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "extern_path".to_string(),
                    detail: "expected 'proto_path=rust_path' for 'extern_path'".to_string(),
                })?;
                let (proto_path, rust_path) =
                    val_str
                        .split_once('=')
                        .ok_or_else(|| CodegenError::InvalidParameter {
                            key: "extern_path".to_string(),
                            detail: format!(
                                "expected 'proto_path=rust_path' for 'extern_path', got '{val_str}'"
                            ),
                        })?;
                let proto_path = proto_path.trim();
                let rust_path = rust_path.trim();
                if proto_path.is_empty() || rust_path.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "extern_path".to_string(),
                        detail: format!(
                            "invalid empty proto_path or rust_path in 'extern_path={val_str}'"
                        ),
                    });
                }
                let norm_proto = proto_path.trim_start_matches('.');
                if let Some((_, existing_rust)) = explicit
                    .extern_paths
                    .iter()
                    .find(|(p, _)| p.trim_start_matches('.') == norm_proto)
                {
                    if existing_rust != rust_path {
                        return Err(CodegenError::InvalidParameter {
                            key: "extern_path".to_string(),
                            detail: format!(
                                "conflicting mapping for '{proto_path}': already mapped to '{existing_rust}', cannot remap to '{rust_path}'"
                            ),
                        });
                    }
                } else {
                    explicit
                        .extern_paths
                        .push((proto_path.to_string(), rust_path.to_string()));
                }
            }
            "runtime_crate" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "runtime_crate".to_string(),
                    detail: "expected value for 'runtime_crate'".to_string(),
                })?;
                let val_str = val_str.trim();
                if val_str.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "runtime_crate".to_string(),
                        detail: "empty crate alias for 'runtime_crate'".to_string(),
                    });
                }
                if let Some(existing) = &explicit.runtime_crate {
                    if existing != val_str {
                        return Err(CodegenError::InvalidParameter {
                            key: "runtime_crate".to_string(),
                            detail: format!(
                                "conflicting runtime_crate: already set to '{existing}', cannot reset to '{val_str}'"
                            ),
                        });
                    }
                }
                explicit.runtime_crate = Some(val_str.to_string());
            }
            "grpc_crate" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "grpc_crate".to_string(),
                    detail: "expected value for 'grpc_crate'".to_string(),
                })?;
                let val_str = val_str.trim();
                if val_str.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "grpc_crate".to_string(),
                        detail: "empty crate alias for 'grpc_crate'".to_string(),
                    });
                }
                if let Some(existing) = &explicit.grpc_crate {
                    if existing != val_str {
                        return Err(CodegenError::InvalidParameter {
                            key: "grpc_crate".to_string(),
                            detail: format!(
                                "conflicting grpc_crate: already set to '{existing}', cannot reset to '{val_str}'"
                            ),
                        });
                    }
                }
                explicit.grpc_crate = Some(val_str.to_string());
            }
            "tonic_crate" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "tonic_crate".to_string(),
                    detail: "expected value for 'tonic_crate'".to_string(),
                })?;
                let val_str = val_str.trim();
                if val_str.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "tonic_crate".to_string(),
                        detail: "empty crate alias for 'tonic_crate'".to_string(),
                    });
                }
                if let Some(existing) = &explicit.tonic_crate {
                    if existing != val_str {
                        return Err(CodegenError::InvalidParameter {
                            key: "tonic_crate".to_string(),
                            detail: format!(
                                "conflicting tonic_crate: already set to '{existing}', cannot reset to '{val_str}'"
                            ),
                        });
                    }
                }
                explicit.tonic_crate = Some(val_str.to_string());
            }
            "codec_path" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: "codec_path".to_string(),
                    detail: "expected value for 'codec_path'".to_string(),
                })?;
                let val_str = val_str.trim();
                if val_str.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: "codec_path".to_string(),
                        detail: "empty codec path for 'codec_path'".to_string(),
                    });
                }
                explicit.codec_path = Some(val_str.to_string());
            }
            "type_attribute"
            | "message_attribute"
            | "enum_attribute"
            | "field_attribute"
            | "client_attribute"
            | "server_attribute"
            | "client_mod_attribute"
            | "server_mod_attribute" => {
                let val_str = val.ok_or_else(|| CodegenError::InvalidParameter {
                    key: key.to_string(),
                    detail: format!("expected 'path=attribute' for '{key}'"),
                })?;
                let (path, attr) =
                    val_str
                        .split_once('=')
                        .ok_or_else(|| CodegenError::InvalidParameter {
                            key: key.to_string(),
                            detail: format!(
                                "expected 'path=attribute' for '{key}', got '{val_str}'"
                            ),
                        })?;
                let path = path.trim();
                let attr = attr.trim();
                if path.is_empty() || attr.is_empty() {
                    return Err(CodegenError::InvalidParameter {
                        key: key.to_string(),
                        detail: format!("invalid empty path or attribute in '{key}={val_str}'"),
                    });
                }
                match key {
                    "type_attribute" => explicit
                        .type_attributes
                        .push((path.to_string(), attr.to_string())),
                    "message_attribute" => explicit
                        .message_attributes
                        .push((path.to_string(), attr.to_string())),
                    "enum_attribute" => explicit
                        .enum_attributes
                        .push((path.to_string(), attr.to_string())),
                    "field_attribute" => explicit
                        .field_attributes
                        .push((path.to_string(), attr.to_string())),
                    "client_attribute" | "client_mod_attribute" => explicit
                        .client_attributes
                        .push((path.to_string(), attr.to_string())),
                    "server_attribute" | "server_mod_attribute" => explicit
                        .server_attributes
                        .push((path.to_string(), attr.to_string())),
                    _ => {}
                }
            }
            "include_source_info" | "source_info" | "preserve_comments" => {
                explicit.include_source_info = Some(parse_bool_param(key, val)?);
            }
            other => {
                return Err(CodegenError::UnknownParameter {
                    key: other.to_string(),
                    detail: format!("unrecognized plugin parameter key: '{other}'"),
                });
            }
        }
    }
    Ok(explicit)
}

pub(crate) fn resolve_options(explicit: &ExplicitOptions) -> ResolvedConfig {
    let stubs = if let Some(s) = explicit.stubs {
        s
    } else {
        match std::env::var("PURE_PROTOBUF_STUBS").as_deref() {
            Ok("kernel") => StubStyle::Kernel,
            Ok("tonic") => StubStyle::Tonic,
            Ok("compat" | "tonic_compat" | "tonic-compat") => StubStyle::TonicCompat,
            Ok("none") => StubStyle::None,
            _ => StubStyle::Kernel,
        }
    };
    let emit_deps = if let Some(d) = explicit.emit_deps {
        d
    } else {
        matches!(
            std::env::var("PURE_PROTOBUF_EMIT_DEPS").as_deref(),
            Ok("1") | Ok("true")
        )
    };
    let no_wkt = if let Some(w) = explicit.no_wkt {
        w
    } else {
        match std::env::var("PURE_PROTOBUF_NO_WKT") {
            Ok(v) => !v.is_empty() && v != "0" && v != "false",
            Err(_) => false,
        }
    };
    let shared_pool = if let Some(p) = explicit.shared_pool {
        p
    } else {
        matches!(
            std::env::var("PURE_PROTOBUF_SHARED_POOL").as_deref(),
            Ok("1") | Ok("true")
        )
    };
    let no_reflect = if let Some(r) = explicit.no_reflect {
        r
    } else {
        matches!(
            std::env::var("PURE_PROTOBUF_NO_REFLECT").as_deref(),
            Ok("1") | Ok("true")
        )
    };
    let emit_json = if let Some(j) = explicit.emit_json {
        j
    } else if let Ok(v) = std::env::var("PURE_PROTOBUF_EMIT_JSON") {
        !v.is_empty() && v != "0" && v != "false"
    } else {
        !no_reflect
    };
    let emit_text = if let Some(t) = explicit.emit_text {
        t
    } else if let Ok(v) = std::env::var("PURE_PROTOBUF_EMIT_TEXT") {
        !v.is_empty() && v != "0" && v != "false"
    } else {
        !no_reflect
    };
    let build_client = explicit.build_client.unwrap_or(true);
    let build_server = explicit.build_server.unwrap_or(true);
    let generate_default_stubs = explicit.generate_default_stubs.unwrap_or(true);
    let use_arc_self = explicit.use_arc_self.unwrap_or(false);
    let disable_comments = explicit.disable_comments.unwrap_or(false);
    let skip_debug = explicit.skip_debug.unwrap_or(false);
    let include_file = explicit
        .include_file
        .clone()
        .unwrap_or_else(|| "mod.rs".to_string());
    let runtime_crate = explicit.runtime_crate.clone().or_else(|| {
        std::env::var("PURE_PROTOBUF_RUNTIME_CRATE")
            .ok()
            .filter(|s| !s.is_empty())
    });
    let grpc_crate = explicit.grpc_crate.clone().or_else(|| {
        std::env::var("PURE_PROTOBUF_GRPC_CRATE")
            .ok()
            .filter(|s| !s.is_empty())
    });
    let tonic_crate = explicit.tonic_crate.clone().or_else(|| {
        std::env::var("PURE_PROTOBUF_TONIC_CRATE")
            .ok()
            .filter(|s| !s.is_empty())
    });
    let include_source_info = if let Some(si) = explicit.include_source_info {
        si
    } else {
        matches!(
            std::env::var("PURE_PROTOBUF_INCLUDE_SOURCE_INFO").as_deref(),
            Ok("1") | Ok("true")
        )
    };
    ResolvedConfig {
        stubs,
        emit_deps,
        no_wkt,
        shared_pool,
        no_reflect,
        emit_json,
        emit_text,
        build_client,
        build_server,
        generate_default_stubs,
        use_arc_self,
        disable_comments,
        skip_debug,
        include_file,
        extern_paths: explicit.extern_paths.clone(),
        runtime_crate,
        grpc_crate,
        tonic_crate,
        codec_path: explicit.codec_path.clone(),
        type_attributes: explicit.type_attributes.clone(),
        message_attributes: explicit.message_attributes.clone(),
        enum_attributes: explicit.enum_attributes.clone(),
        field_attributes: explicit.field_attributes.clone(),
        client_attributes: explicit.client_attributes.clone(),
        server_attributes: explicit.server_attributes.clone(),
        include_source_info,
    }
}

pub(crate) fn build_client() -> bool {
    BUILD_CLIENT.with(Cell::get)
}

pub(crate) fn build_server() -> bool {
    BUILD_SERVER.with(Cell::get)
}

pub(crate) fn generate_default_stubs() -> bool {
    GENERATE_DEFAULT_STUBS.with(Cell::get)
}

pub(crate) fn use_arc_self() -> bool {
    USE_ARC_SELF.with(Cell::get)
}

pub(crate) fn comments_disabled() -> bool {
    DISABLE_COMMENTS.with(Cell::get)
}

pub(crate) fn emit_reflection_enabled() -> bool {
    !NO_REFLECT.with(Cell::get)
}

pub(crate) fn emit_json_enabled() -> bool {
    EMIT_JSON.with(Cell::get)
}

pub(crate) fn emit_text_enabled() -> bool {
    EMIT_TEXT.with(Cell::get)
}

pub(crate) fn skip_debug() -> bool {
    SKIP_DEBUG.with(Cell::get)
}

pub(crate) fn tonic_codec_path() -> String {
    CODEC_PATH
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| "ProtobufCodec".to_string())
}

fn attr_path_matches(rule_path: &str, fq_path: &str) -> bool {
    let rule = rule_path.trim_start_matches('.');
    let fq = fq_path.trim_start_matches('.');
    rule == "." || rule == "*" || rule == fq
}

fn normalize_attr(attr: &str) -> String {
    let trimmed = attr.trim();
    if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
        trimmed.to_string()
    } else {
        format!("#[{trimmed}]")
    }
}

pub(crate) fn emit_attr_lines(
    src: &mut String,
    rules: &[(String, String)],
    fq_path: &str,
    indent: &str,
) {
    for (path, attr) in rules {
        if attr_path_matches(path, fq_path) {
            let _ = writeln!(src, "{indent}{}", normalize_attr(attr));
        }
    }
}

pub(crate) fn emit_type_attributes(src: &mut String, fq_path: &str, indent: &str) {
    TYPE_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

pub(crate) fn emit_message_attributes(src: &mut String, fq_path: &str, indent: &str) {
    MESSAGE_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

pub(crate) fn emit_enum_attributes(src: &mut String, fq_path: &str, indent: &str) {
    ENUM_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

pub(crate) fn emit_field_attributes(src: &mut String, fq_path: &str, indent: &str) {
    FIELD_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

pub(crate) fn emit_client_attributes(src: &mut String, fq_path: &str, indent: &str) {
    CLIENT_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

pub(crate) fn emit_server_attributes(src: &mut String, fq_path: &str, indent: &str) {
    SERVER_ATTRIBUTES.with(|rules| emit_attr_lines(src, &rules.borrow(), fq_path, indent));
}

/// Options for [`compile_protos`] and [`Config::compile_descriptor_set`].
///
/// # Configuration precedence
///
/// Every option resolves in the same order, so equivalent [`Config`] and
/// `--pbrs_opt` plugin-parameter inputs select identical options:
///
/// 1. Explicit selection: a [`Config`] builder call or a `--pbrs_opt`
///    `key=value` entry. This always wins and is never silently overridden
///    by ambient environment.
/// 2. `PURE_PROTOBUF_*` environment variable, kept as a legacy-compatibility
///    fallback for existing build scripts (`PURE_PROTOBUF_STUBS`,
///    `PURE_PROTOBUF_EMIT_DEPS`, `PURE_PROTOBUF_NO_WKT`,
///    `PURE_PROTOBUF_SHARED_POOL`, `PURE_PROTOBUF_NO_REFLECT`,
///    `PURE_PROTOBUF_EMIT_JSON`, `PURE_PROTOBUF_EMIT_TEXT`,
///    `PURE_PROTOBUF_RUNTIME_CRATE`, `PURE_PROTOBUF_GRPC_CRATE`,
///    `PURE_PROTOBUF_TONIC_CRATE`, `PURE_PROTOBUF_INCLUDE_SOURCE_INFO`).
///    New code should prefer explicit options.
/// 3. Built-in default (kernel stubs; all other switches off).
///
/// Unknown plugin parameter keys and invalid values are rejected with
/// [`CodegenError::UnknownParameter`] / [`CodegenError::InvalidParameter`]
/// instead of being ignored, and each generation call resolves its own
/// configuration, so sequential or parallel mixed-config calls cannot leak
/// settings into each other.
///
/// Note: `protoc` always attaches source-code info to the descriptors it
/// sends to plugins, while [`Config::compile_protos`] requests it only with
/// [`Config::include_source_info`]; the embedded `FILE_DESCRIPTOR_SET`
/// reflection bytes can therefore differ between entry points even for
/// identical options. All message, enum, and stub output is otherwise
/// byte-identical for equivalent inputs.
#[derive(Clone, Debug, Default)]
pub struct Config {
    protoc_path: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    stubs: Option<StubStyle>,
    emit_deps: Option<bool>,
    no_wkt: Option<bool>,
    shared_pool: Option<bool>,
    no_reflect: Option<bool>,
    emit_json: Option<bool>,
    emit_text: Option<bool>,
    build_client: Option<bool>,
    build_server: Option<bool>,
    generate_default_stubs: Option<bool>,
    use_arc_self: Option<bool>,
    disable_comments: Option<bool>,
    skip_debug: Option<bool>,
    include_file: Option<String>,
    file_descriptor_set_path: Option<PathBuf>,
    emit_rerun_if_changed: Option<bool>,
    protoc_args: Vec<String>,
    pub(crate) extern_paths: Vec<(String, String)>,
    pub(crate) runtime_crate: Option<String>,
    pub(crate) grpc_crate: Option<String>,
    pub(crate) tonic_crate: Option<String>,
    pub(crate) codec_path: Option<String>,
    pub(crate) type_attributes: Vec<(String, String)>,
    pub(crate) message_attributes: Vec<(String, String)>,
    pub(crate) enum_attributes: Vec<(String, String)>,
    pub(crate) field_attributes: Vec<(String, String)>,
    pub(crate) client_attributes: Vec<(String, String)>,
    pub(crate) server_attributes: Vec<(String, String)>,
    include_source_info: Option<bool>,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly configure the path to the `protoc` compiler executable.
    ///
    /// If unset, defaults to the `PROTOC` environment variable, or searches `PATH` for `protoc`.
    pub fn protoc_path(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.protoc_path = Some(path.into());
        self
    }

    /// Returns the explicit `protoc` path configured, if any.
    #[must_use]
    pub fn get_protoc_path(&self) -> Option<&Path> {
        self.protoc_path.as_deref()
    }

    /// Resolve the `protoc` executable to use: either the explicit configured path,
    /// or the path from the `PROTOC` environment variable, or `"protoc"` (searching `PATH`).
    #[must_use]
    pub fn resolve_protoc_path(&self) -> PathBuf {
        self.protoc_path
            .clone()
            .or_else(|| {
                std::env::var_os("PROTOC")
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("protoc"))
    }

    /// Returns the resolved `protoc` executable path.
    #[must_use]
    pub fn selected_protoc_path(&self) -> PathBuf {
        self.resolve_protoc_path()
    }

    /// Query the configured or discovered `protoc` compiler version string (e.g. `libprotoc 29.3`).
    pub fn protoc_version(&self) -> Result<String, CodegenError> {
        let protoc_bin = self.resolve_protoc_path();
        let output = match Command::new(&protoc_bin).arg("--version").output() {
            Ok(o) => o,
            Err(e) => {
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: e,
                });
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.code() == Some(127)
                && (stderr.is_empty() || stderr.contains("not found"))
            {
                let msg = if stderr.is_empty() {
                    "protoc command failed with exit code 127 (not found or executable failed)"
                        .to_string()
                } else {
                    stderr
                };
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, msg),
                });
            }
            if output.status.code() == Some(126) {
                let msg = if stderr.is_empty() {
                    "protoc command failed with exit code 126 (permission denied or not executable)"
                        .to_string()
                } else {
                    stderr
                };
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg),
                });
            }
            return Err(CodegenError::ProtocExecution {
                status: output.status,
                stderr,
                protos: Vec::new(),
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    }

    pub fn out_dir(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.out_dir = Some(path.into());
        self
    }

    /// Choose which gRPC stub flavour to emit. Default [`Stubs::Kernel`].
    pub fn stubs(&mut self, stubs: Stubs) -> &mut Self {
        self.stubs = Some(stubs.into());
        self
    }

    /// Emit tonic `FooClient`/`FooServer` stubs.
    ///
    /// Needed because the default is [`Stubs::Kernel`]. `false` means
    /// messages only. Mutually exclusive with [`Self::emit_kernel_stubs`];
    /// the last call wins.
    pub fn emit_tonic_stubs(&mut self, enable: bool) -> &mut Self {
        self.stubs = Some(if enable {
            StubStyle::Tonic
        } else {
            StubStyle::None
        });
        self
    }

    /// Emit tonic-shaped service stubs over the native `pbrs-grpc` transport.
    pub fn tonic_compat(&mut self, enable: bool) -> &mut Self {
        self.stubs = Some(if enable {
            StubStyle::TonicCompat
        } else {
            StubStyle::Kernel
        });
        self
    }

    /// Emit native `pbrs-grpc` `FooClient`/`FooServer` stubs.
    ///
    /// This is the default for [`Config::new`] / [`compile_protos`]. Pass
    /// `true` to be explicit; `false` is messages only. The generating crate
    /// must depend on `pbrs-grpc`. `FooClient` gets the same dialers as
    /// `Channel` (`connect`, `connect_tls`, `connect_unix`, `from_io`, and
    /// the lazy/`_with` variants) and the same overlays, including
    /// `https_scheme` for already-encrypted `from_io` streams and `origin`
    /// for `:authority`. Read those
    /// overlays with `scheme` / `authority` / `grpc_user_agent` /
    /// `rpc_timeout` / `waits_for_ready` / `compresses_outbound` /
    /// `gzip_level` / `accepts_compressed` / `concurrent_rpc_limit` / `stream_buffer_size` / `send_buffer_size` / `limits` / `config`.
    /// Methods you omit on the generated `Foo` trait answer `UNIMPLEMENTED`.
    /// Mutually exclusive with [`Self::emit_tonic_stubs`]; the last call wins.
    ///
    /// ```no_run
    /// // build.rs
    /// pbrs::codegen::compile_protos(&["proto/hello.proto"], &["proto"])
    ///     .expect("codegen");
    /// ```
    pub fn emit_kernel_stubs(&mut self, enable: bool) -> &mut Self {
        self.stubs = Some(if enable {
            StubStyle::Kernel
        } else {
            StubStyle::None
        });
        self
    }

    /// Emit imported non-WKT messages into the same generated file.
    ///
    /// Needed for `grpc.testing.test.proto`, which imports
    /// `messages.proto` / `empty.proto`. `PURE_PROTOBUF_EMIT_DEPS=1`
    /// remains the plugin/env equivalent.
    pub fn emit_deps(&mut self, enable: bool) -> &mut Self {
        self.emit_deps = Some(enable);
        self
    }

    /// Disable emission of Well-Known Types (WKTs).
    pub fn no_wkt(&mut self, enable: bool) -> &mut Self {
        self.no_wkt = Some(enable);
        self
    }

    /// Use a shared DescriptorPool instead of embedding the FileDescriptorSet.
    pub fn shared_pool(&mut self, enable: bool) -> &mut Self {
        self.shared_pool = Some(enable);
        self
    }

    /// Skip embedding FileDescriptorSet and JSON/text methods for lightweight accessors.
    pub fn no_reflect(&mut self, enable: bool) -> &mut Self {
        self.no_reflect = Some(enable);
        if enable {
            self.emit_json.get_or_insert(false);
            self.emit_text.get_or_insert(false);
        }
        self
    }

    /// Emit reflection descriptor bytes and descriptor-backed helpers.
    ///
    /// This is the positive form of [`Self::no_reflect`]. Disabling
    /// reflection does not remove binary parse/serialize APIs. JSON/text
    /// methods default off with reflection disabled, but can be controlled
    /// independently with [`Self::emit_json`] and [`Self::emit_text`].
    pub fn emit_reflection(&mut self, enable: bool) -> &mut Self {
        self.no_reflect = Some(!enable);
        self
    }

    /// Emit generated JSON helpers. Default `true` unless reflection is disabled.
    pub fn emit_json(&mut self, enable: bool) -> &mut Self {
        self.emit_json = Some(enable);
        self
    }

    /// Emit generated text-format helpers. Default `true` unless reflection is disabled.
    pub fn emit_text(&mut self, enable: bool) -> &mut Self {
        self.emit_text = Some(enable);
        self
    }

    /// Emit generated client stubs. Default `true` when service stubs are enabled.
    pub fn build_client(&mut self, enable: bool) -> &mut Self {
        self.build_client = Some(enable);
        self
    }

    /// Emit generated server traits and server wrappers. Default `true` when service stubs are enabled.
    pub fn build_server(&mut self, enable: bool) -> &mut Self {
        self.build_server = Some(enable);
        self
    }

    /// Generate default service methods returning `UNIMPLEMENTED`.
    ///
    /// This applies to native `pbrs-grpc` stubs. It is enabled by default.
    pub fn generate_default_stubs(&mut self, enable: bool) -> &mut Self {
        self.generate_default_stubs = Some(enable);
        self
    }

    /// Use `Arc<Self>` receivers in generated service traits where supported.
    ///
    /// This option is accepted for API parity. The current native and Tonic
    /// adapter modes keep their default receiver shape unless an opt-in mode
    /// uses this during emission.
    pub fn use_arc_self(&mut self, enable: bool) -> &mut Self {
        self.use_arc_self = Some(enable);
        self
    }

    /// Disable source-comment emission from generated Rust.
    ///
    /// Synthetic documentation that explains generated APIs may still be
    /// emitted; this mirrors `prost-build`'s source-comment suppression.
    pub fn disable_comments(&mut self, enable: bool) -> &mut Self {
        self.disable_comments = Some(enable);
        self
    }

    /// Do not derive `Debug` for generated message storage where possible.
    pub fn skip_debug(&mut self, enable: bool) -> &mut Self {
        self.skip_debug = Some(enable);
        self
    }

    /// Select whether Well-Known Types are generated.
    ///
    /// This is the prost-build shaped inverse of [`Self::no_wkt`].
    pub fn compile_well_known_types(&mut self, enable: bool) -> &mut Self {
        self.no_wkt = Some(!enable);
        self
    }

    /// Write the root module include file under this relative file name.
    ///
    /// Default is `mod.rs`.
    pub fn include_file(&mut self, path: impl Into<String>) -> &mut Self {
        self.include_file = Some(path.into());
        self
    }

    /// Also write the compiled `FileDescriptorSet` bytes to `path`.
    pub fn file_descriptor_set_path(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.file_descriptor_set_path = Some(path.into());
        self
    }

    /// Control Cargo `rerun-if-changed` and `rerun-if-env-changed` emission.
    ///
    /// Defaults to `true`.
    pub fn emit_rerun_if_changed(&mut self, enable: bool) -> &mut Self {
        self.emit_rerun_if_changed = Some(enable);
        self
    }

    /// Add an extra argument passed to `protoc` before input files.
    pub fn protoc_arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.protoc_args.push(arg.into());
        self
    }

    /// Map a protobuf package or message path to an external Rust type or module path.
    pub fn extern_path(
        &mut self,
        proto_path: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> &mut Self {
        self.extern_paths
            .push((proto_path.into(), rust_path.into()));
        self
    }

    /// Override the runtime crate path (default: "pbrs").
    pub fn runtime_crate(&mut self, crate_name: impl Into<String>) -> &mut Self {
        self.runtime_crate = Some(crate_name.into());
        self
    }

    /// Override the native pbrs-grpc crate path (default: "::pbrs_grpc").
    pub fn grpc_crate(&mut self, rust_path: impl Into<String>) -> &mut Self {
        self.grpc_crate = Some(rust_path.into());
        self
    }

    /// Override the protobuf-tonic crate path (default: "protobuf_tonic").
    pub fn tonic_crate(&mut self, rust_path: impl Into<String>) -> &mut Self {
        self.tonic_crate = Some(rust_path.into());
        self
    }

    /// Override the Tonic stub codec type path. Default `protobuf_tonic::ProtobufCodec`.
    pub fn codec_path(&mut self, rust_path: impl Into<String>) -> &mut Self {
        self.codec_path = Some(rust_path.into());
        self
    }

    /// Add a Rust attribute to all generated messages or enums matching `path`.
    pub fn type_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.type_attributes.push((path.into(), attr.into()));
        self
    }

    /// Add a Rust attribute to generated messages matching `path`.
    pub fn message_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.message_attributes.push((path.into(), attr.into()));
        self
    }

    /// Add a Rust attribute to generated enums matching `path`.
    pub fn enum_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.enum_attributes.push((path.into(), attr.into()));
        self
    }

    /// Add a Rust attribute to generated struct fields matching `path`.
    pub fn field_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.field_attributes.push((path.into(), attr.into()));
        self
    }

    /// Add a Rust attribute to generated clients matching service `path`.
    pub fn client_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.client_attributes.push((path.into(), attr.into()));
        self
    }

    /// Alias for [`Self::client_attribute`].
    pub fn client_mod_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.client_attribute(path, attr)
    }

    /// Add a Rust attribute to generated servers matching service `path`.
    pub fn server_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.server_attributes.push((path.into(), attr.into()));
        self
    }

    /// Alias for [`Self::server_attribute`].
    pub fn server_mod_attribute(
        &mut self,
        path: impl Into<String>,
        attr: impl Into<String>,
    ) -> &mut Self {
        self.server_attribute(path, attr)
    }

    /// Include source code info (locations and comments) in generated descriptors and code.
    pub fn include_source_info(&mut self, enable: bool) -> &mut Self {
        self.include_source_info = Some(enable);
        self
    }

    /// Alias for [`Self::include_source_info`].
    pub fn preserve_comments(&mut self, enable: bool) -> &mut Self {
        self.include_source_info(enable)
    }

    fn to_parameter_string(&self) -> String {
        let mut opts = Vec::new();
        if let Some(stubs) = self.stubs {
            match stubs {
                StubStyle::Kernel => opts.push("stubs=kernel".to_string()),
                StubStyle::Tonic => opts.push("stubs=tonic".to_string()),
                StubStyle::TonicCompat => opts.push("stubs=compat".to_string()),
                StubStyle::None => opts.push("stubs=none".to_string()),
            }
        }
        if let Some(emit_deps) = self.emit_deps {
            opts.push(format!("emit_deps={emit_deps}"));
        }
        if let Some(no_wkt) = self.no_wkt {
            opts.push(format!("no_wkt={no_wkt}"));
        }
        if let Some(shared_pool) = self.shared_pool {
            opts.push(format!("shared_pool={shared_pool}"));
        }
        if let Some(no_reflect) = self.no_reflect {
            opts.push(format!("no_reflect={no_reflect}"));
        }
        if let Some(build_client) = self.build_client {
            opts.push(format!("build_client={build_client}"));
        }
        if let Some(build_server) = self.build_server {
            opts.push(format!("build_server={build_server}"));
        }
        if let Some(generate_default_stubs) = self.generate_default_stubs {
            opts.push(format!("generate_default_stubs={generate_default_stubs}"));
        }
        if let Some(use_arc_self) = self.use_arc_self {
            opts.push(format!("use_arc_self={use_arc_self}"));
        }
        if let Some(disable_comments) = self.disable_comments {
            opts.push(format!("disable_comments={disable_comments}"));
        }
        if let Some(skip_debug) = self.skip_debug {
            opts.push(format!("skip_debug={skip_debug}"));
        }
        if let Some(include_file) = &self.include_file {
            opts.push(format!("include_file={include_file}"));
        }
        for (proto, rust) in &self.extern_paths {
            opts.push(format!("extern_path={proto}={rust}"));
        }
        if let Some(rc) = &self.runtime_crate {
            opts.push(format!("runtime_crate={rc}"));
        }
        if let Some(gc) = &self.grpc_crate {
            opts.push(format!("grpc_crate={gc}"));
        }
        if let Some(tc) = &self.tonic_crate {
            opts.push(format!("tonic_crate={tc}"));
        }
        if let Some(codec) = &self.codec_path {
            opts.push(format!("codec_path={codec}"));
        }
        for (path, attr) in &self.type_attributes {
            opts.push(format!("type_attribute={path}={attr}"));
        }
        for (path, attr) in &self.message_attributes {
            opts.push(format!("message_attribute={path}={attr}"));
        }
        for (path, attr) in &self.enum_attributes {
            opts.push(format!("enum_attribute={path}={attr}"));
        }
        for (path, attr) in &self.field_attributes {
            opts.push(format!("field_attribute={path}={attr}"));
        }
        for (path, attr) in &self.client_attributes {
            opts.push(format!("client_attribute={path}={attr}"));
        }
        for (path, attr) in &self.server_attributes {
            opts.push(format!("server_attribute={path}={attr}"));
        }
        if let Some(si) = self.include_source_info {
            opts.push(format!("include_source_info={si}"));
        }
        opts.join(",")
    }

    fn output_dir(&self) -> Result<PathBuf, CodegenError> {
        let out = match &self.out_dir {
            Some(p) => p.clone(),
            None => {
                let var = std::env::var("OUT_DIR").map_err(|_| CodegenError::MissingOutDir)?;
                PathBuf::from(var)
            }
        };
        std::fs::create_dir_all(&out).map_err(|source| CodegenError::UnwritableOutput {
            path: out.clone(),
            source,
        })?;
        Ok(out)
    }

    /// Write Rust output from a precompiled `FileDescriptorSet`, without invoking `protoc`.
    ///
    /// `descriptor_set` is a path to a checked-in or prebuilt descriptor set
    /// containing the requested proto files and their imports (for example,
    /// produced with `protoc --include_imports`). `files_to_generate` accepts
    /// proto names in the set or paths under `includes`, just like
    /// [`Self::compile_protos`]. The descriptor set is always tracked for Cargo
    /// rebuilds; any available source and imported proto files under `includes`
    /// are also tracked, but are not required at generation time.
    ///
    /// Configuration, output layout, and errors match [`Self::compile_protos`].
    /// Source locations and comments must already be present in the descriptor
    /// set; [`Self::include_source_info`] cannot add them after compilation.
    ///
    /// ```no_run
    /// pbrs::codegen::Config::new()
    ///     .emit_kernel_stubs(false)
    ///     .compile_descriptor_set("proto/schema.fds", &["message.proto"], &["proto"])
    ///     .expect("generate from descriptor set");
    /// ```
    pub fn compile_descriptor_set(
        &self,
        descriptor_set: impl AsRef<Path>,
        files_to_generate: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> Result<(), CodegenError> {
        let param = self.to_parameter_string();
        if !param.is_empty() {
            parse_plugin_parameter(&param)?;
        }
        let out = self.output_dir()?;
        let descriptor_set = descriptor_set.as_ref();
        let bytes = std::fs::read(descriptor_set).map_err(|source| CodegenError::Io {
            path: descriptor_set.to_path_buf(),
            source,
        })?;
        if let Some(path) = &self.file_descriptor_set_path {
            write_descriptor_set_bytes(path, &bytes)?;
        }
        let names: Vec<String> = files_to_generate
            .iter()
            .map(|p| resolve_proto_rel_path(p.as_ref(), includes))
            .collect();
        let files = generate_from_file_descriptor_set_with_parameter(
            &bytes,
            &names,
            if param.is_empty() { None } else { Some(&param) },
            true,
        )
        .map_err(|error| match error {
            CodegenError::MalformedDescriptor { detail, .. } => CodegenError::MalformedDescriptor {
                detail,
                path: Some(descriptor_set.to_path_buf()),
            },
            other => other,
        })?;

        if self.emit_rerun_if_changed.unwrap_or(true) {
            emit_codegen_config_rerun_if_env_changed();
            let mut seen_canonical = std::collections::BTreeSet::new();
            emit_rerun_if_changed(descriptor_set, &mut seen_canonical);
            emit_descriptor_source_rerun_if_changed(&bytes, includes, &mut seen_canonical);
        }
        for (name, src) in files {
            write_file_atomic_if_changed(&out.join(name), &src)?;
        }
        Ok(())
    }

    pub fn compile_protos(
        &self,
        protos: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> Result<(), CodegenError> {
        let param = self.to_parameter_string();
        if !param.is_empty() {
            parse_plugin_parameter(&param)?;
        }
        let out = self.output_dir()?;

        if self.emit_rerun_if_changed.unwrap_or(true) {
            println!("cargo:rerun-if-env-changed=PROTOC");
            emit_codegen_config_rerun_if_env_changed();
        }

        let mut seen_canonical = std::collections::BTreeSet::new();
        let mut sorted_protos: Vec<PathBuf> =
            protos.iter().map(|p| p.as_ref().to_path_buf()).collect();
        sorted_protos.sort();
        sorted_protos.dedup();

        if self.emit_rerun_if_changed.unwrap_or(true) {
            for p in &sorted_protos {
                emit_rerun_if_changed(p, &mut seen_canonical);
            }
        }

        let protoc_bin = self.resolve_protoc_path();
        let fds_path = out.join("pbrs.fds");
        let mut cmd = Command::new(&protoc_bin);
        cmd.arg("--include_imports");
        if self.include_source_info.unwrap_or(false)
            || matches!(
                std::env::var("PURE_PROTOBUF_INCLUDE_SOURCE_INFO").as_deref(),
                Ok("1") | Ok("true")
            )
        {
            cmd.arg("--include_source_info");
        }
        let mut fds_arg = std::ffi::OsString::from("--descriptor_set_out=");
        fds_arg.push(fds_path.as_os_str());
        cmd.arg(fds_arg);
        for inc in includes {
            cmd.arg("-I").arg(inc.as_ref());
        }
        for arg in &self.protoc_args {
            cmd.arg(arg);
        }
        for p in &sorted_protos {
            let rel = resolve_proto_rel_path(p, includes);
            if !rel.is_empty() && !Path::new(&rel).is_absolute() {
                cmd.arg(&rel);
            } else {
                cmd.arg(p);
            }
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: e,
                });
            }
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.code() == Some(127)
                && (stderr.is_empty() || stderr.contains("not found"))
            {
                let msg = if stderr.is_empty() {
                    "protoc command failed with exit code 127 (not found or executable failed)"
                        .to_string()
                } else {
                    stderr
                };
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, msg),
                });
            }
            if output.status.code() == Some(126) {
                let msg = if stderr.is_empty() {
                    "protoc command failed with exit code 126 (permission denied or not executable)"
                        .to_string()
                } else {
                    stderr
                };
                return Err(CodegenError::MissingProtoc {
                    path: protoc_bin,
                    source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg),
                });
            }
            if let Some((import_name, proto_file)) = parse_missing_import(&stderr) {
                return Err(CodegenError::MissingImport {
                    import: import_name,
                    proto: missing_import_source(proto_file, protos, includes),
                    detail: stderr,
                });
            }
            return Err(CodegenError::ProtocExecution {
                status: output.status,
                stderr,
                protos: protos.iter().map(|p| p.as_ref().to_path_buf()).collect(),
            });
        }
        let bytes = std::fs::read(&fds_path).map_err(|e| CodegenError::Io {
            path: fds_path.clone(),
            source: e,
        })?;
        if let Some(path) = &self.file_descriptor_set_path {
            write_descriptor_set_bytes(path, &bytes)?;
        }

        if self.emit_rerun_if_changed.unwrap_or(true) {
            emit_descriptor_source_rerun_if_changed(&bytes, includes, &mut seen_canonical);
        }
        let names: Vec<String> = sorted_protos
            .iter()
            .map(|p| resolve_proto_rel_path(p.as_ref(), includes))
            .collect();
        let files = generate_from_file_descriptor_set_with_parameter(
            &bytes,
            &names,
            if param.is_empty() { None } else { Some(&param) },
            false,
        );
        let files = match files {
            Ok(f) => f,
            Err(e) => {
                let _ = std::fs::remove_file(&fds_path);
                return Err(match e {
                    CodegenError::MalformedDescriptor { detail, path: None } => {
                        CodegenError::MalformedDescriptor {
                            detail,
                            path: Some(fds_path.clone()),
                        }
                    }
                    other => other,
                });
            }
        };
        for (name, src) in files {
            let target_path = out.join(name);
            write_file_atomic_if_changed(&target_path, &src)?;
        }
        let _ = std::fs::remove_file(&fds_path);
        Ok(())
    }
}

pub(crate) fn write_descriptor_set_bytes(path: &Path, bytes: &[u8]) -> Result<(), CodegenError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| CodegenError::UnwritableOutput {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, bytes).map_err(|source| CodegenError::UnwritableOutput {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn emit_codegen_config_rerun_if_env_changed() {
    println!("cargo:rerun-if-env-changed=PURE_PROTOBUF_*");
    for var in &[
        "PURE_PROTOBUF_STUBS",
        "PURE_PROTOBUF_EMIT_DEPS",
        "PURE_PROTOBUF_NO_WKT",
        "PURE_PROTOBUF_SHARED_POOL",
        "PURE_PROTOBUF_NO_REFLECT",
        "PURE_PROTOBUF_EMIT_JSON",
        "PURE_PROTOBUF_EMIT_TEXT",
        "PURE_PROTOBUF_RUNTIME_CRATE",
        "PURE_PROTOBUF_GRPC_CRATE",
        "PURE_PROTOBUF_TONIC_CRATE",
        "PURE_PROTOBUF_INCLUDE_SOURCE_INFO",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}

pub(crate) fn emit_rerun_if_changed(
    path: &Path,
    seen_canonical: &mut std::collections::BTreeSet<PathBuf>,
) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen_canonical.insert(canon) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

pub(crate) fn emit_descriptor_source_rerun_if_changed(
    bytes: &[u8],
    includes: &[impl AsRef<Path>],
    seen_canonical: &mut std::collections::BTreeSet<PathBuf>,
) {
    for name in extract_fds_file_names(bytes) {
        let clean_name = name.trim_start_matches('/').trim_start_matches("./");
        let mut resolved = None;
        if Path::new(&name).is_absolute() && Path::new(&name).exists() {
            resolved = Some(PathBuf::from(&name));
        } else {
            for inc in includes {
                let candidate = inc.as_ref().join(clean_name);
                if candidate.exists() {
                    resolved = Some(candidate);
                    break;
                }
            }
            if resolved.is_none() {
                let direct = Path::new(clean_name);
                if direct.exists() {
                    resolved = Some(direct.to_path_buf());
                }
            }
        }
        if let Some(path) = resolved {
            emit_rerun_if_changed(&path, seen_canonical);
        }
    }
}

pub(crate) fn write_file_atomic_if_changed(
    target_path: &Path,
    content: &str,
) -> Result<bool, CodegenError> {
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CodegenError::UnwritableOutput {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    if let Ok(existing) = std::fs::read(target_path) {
        if existing == content.as_bytes() {
            return Ok(false);
        }
    }

    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");

    pub(crate) static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_path = parent.join(format!(".{file_name}.tmp.{pid}_{count}"));

    if let Err(e) = std::fs::write(&tmp_path, content) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(CodegenError::UnwritableOutput {
            path: target_path.to_path_buf(),
            source: e,
        });
    }

    if let Err(e) = std::fs::rename(&tmp_path, target_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(CodegenError::UnwritableOutput {
            path: target_path.to_path_buf(),
            source: e,
        });
    }

    Ok(true)
}
