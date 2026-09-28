//! MX-01 split of `super`: descriptors (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, DescriptorPool, FieldType};
use crate::wire::{self, WIRE_LEN, decode_tag, encode_len_field, read_len_bytes};
use std::path::{Path, PathBuf};

pub fn generate_from_code_generator_request(
    bytes: &[u8],
) -> Result<Vec<(String, String)>, CodegenError> {
    let _guard = CodegenStateGuard::new();
    let mut files_to_generate = Vec::new();
    let mut proto_files = Vec::new();
    let mut parameter = None;
    let mut pos = 0;
    while pos < bytes.len() {
        let (n, w) =
            decode_tag(bytes, &mut pos).map_err(|_| CodegenError::MalformedDescriptor {
                detail: "failed to decode CodeGeneratorRequest wire tag".to_string(),
                path: None,
            })?;
        match (n, w) {
            (1, WIRE_LEN) => {
                let s = read_len_bytes(bytes, &mut pos).map_err(|_| {
                    CodegenError::MalformedDescriptor {
                        detail: "failed to read file_to_generate in CodeGeneratorRequest"
                            .to_string(),
                        path: None,
                    }
                })?;
                files_to_generate.push(String::from_utf8_lossy(s).into_owned());
            }
            (2, WIRE_LEN) => {
                let s = read_len_bytes(bytes, &mut pos).map_err(|_| {
                    CodegenError::MalformedDescriptor {
                        detail: "failed to read parameter in CodeGeneratorRequest".to_string(),
                        path: None,
                    }
                })?;
                parameter = Some(String::from_utf8_lossy(s).into_owned());
            }
            (15, WIRE_LEN) => {
                let blob = read_len_bytes(bytes, &mut pos)
                    .map_err(|_| CodegenError::MalformedDescriptor {
                        detail: "failed to read proto_file in CodeGeneratorRequest".to_string(),
                        path: None,
                    })?
                    .to_vec();
                proto_files.push(blob);
            }
            _ => wire::skip_field(bytes, &mut pos, w).map_err(|_| {
                CodegenError::MalformedDescriptor {
                    detail: "failed to skip unrecognized field in CodeGeneratorRequest".to_string(),
                    path: None,
                }
            })?,
        }
    }
    let explicit = match parameter.as_deref() {
        Some(p) => parse_plugin_parameter(p)?,
        None => ExplicitOptions::default(),
    };
    let resolved = resolve_options(&explicit);
    STUBS.with(|c| c.set(resolved.stubs));
    EMIT_DEPS.with(|c| c.set(resolved.emit_deps));
    NO_WKT.with(|c| c.set(resolved.no_wkt));
    SHARED_POOL.with(|c| c.set(resolved.shared_pool));
    NO_REFLECT.with(|c| c.set(resolved.no_reflect));
    BUILD_CLIENT.with(|c| c.set(resolved.build_client));
    BUILD_SERVER.with(|c| c.set(resolved.build_server));
    GENERATE_DEFAULT_STUBS.with(|c| c.set(resolved.generate_default_stubs));
    USE_ARC_SELF.with(|c| c.set(resolved.use_arc_self));
    DISABLE_COMMENTS.with(|c| c.set(resolved.disable_comments));
    SKIP_DEBUG.with(|c| c.set(resolved.skip_debug));
    EXTERN_PATHS.with(|c| *c.borrow_mut() = resolved.extern_paths.clone());
    RUNTIME_CRATE.with(|c| *c.borrow_mut() = resolved.runtime_crate.clone());
    GRPC_CRATE.with(|c| *c.borrow_mut() = resolved.grpc_crate.clone());
    TONIC_CRATE.with(|c| *c.borrow_mut() = resolved.tonic_crate.clone());
    CODEC_PATH.with(|c| *c.borrow_mut() = resolved.codec_path.clone());
    TYPE_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.type_attributes.clone());
    MESSAGE_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.message_attributes.clone());
    ENUM_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.enum_attributes.clone());
    FIELD_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.field_attributes.clone());
    CLIENT_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.client_attributes.clone());
    SERVER_ATTRIBUTES.with(|c| *c.borrow_mut() = resolved.server_attributes.clone());

    let mut fds = Vec::new();
    // Stabilize proto_files ordering by file name so fds is byte-identical across input order permutations
    proto_files.sort_by_cached_key(|blob| extract_proto_file_name_from_blob(blob));
    for blob in &proto_files {
        encode_len_field(&mut fds, 1, blob);
    }
    let pool = DescriptorPool::from_file_descriptor_set(&fds).map_err(|_| {
        CodegenError::MalformedDescriptor {
            detail: "failed to parse FileDescriptorSet in CodeGeneratorRequest".to_string(),
            path: files_to_generate.first().map(PathBuf::from),
        }
    })?;
    let names: Vec<String> = pool.collect_names();
    let mut file_packages: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for blob in &proto_files {
        let mut pos = 0;
        let mut name = String::new();
        let mut package = String::new();
        while pos < blob.len() {
            if let Ok((n, w)) = decode_tag(blob, &mut pos) {
                if n == 1 && w == WIRE_LEN {
                    if let Ok(b) = read_len_bytes(blob, &mut pos) {
                        name = String::from_utf8_lossy(b).into_owned();
                    }
                } else if n == 2 && w == WIRE_LEN {
                    if let Ok(b) = read_len_bytes(blob, &mut pos) {
                        package = String::from_utf8_lossy(b).into_owned();
                    }
                } else {
                    let _ = wire::skip_field(blob, &mut pos, w);
                }
            } else {
                break;
            }
        }
        if !name.is_empty() {
            file_packages.insert(name, package);
        }
    }

    let mut targets: Vec<String> = if files_to_generate.is_empty() {
        vec!["generated.proto".into()]
    } else {
        files_to_generate
            .iter()
            .map(|t| clean_proto_target_name(t))
            .collect()
    };
    targets.sort();
    targets.dedup();

    for target in &targets {
        let norm_target = normalize_proto_path_str(target);
        if norm_target != "generated.proto"
            && norm_target != "generated"
            && !norm_target.contains('/')
        {
            let stem = Path::new(&norm_target)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&norm_target);
            let exact = file_packages.contains_key(&norm_target)
                || file_packages.contains_key(&format!("{norm_target}.proto"))
                || file_packages.contains_key(&format!("{stem}.proto"));
            if !exact {
                let matches: Vec<(String, String)> = file_packages
                    .iter()
                    .filter(|(file_name, _)| {
                        let f_stem = Path::new(file_name)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        f_stem == stem
                    })
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if matches.len() > 1 {
                    return Err(CodegenError::AmbiguousStem {
                        stem: stem.to_string(),
                        matches,
                    });
                }
            }
        }
    }

    for target in &targets {
        let norm_target = normalize_proto_path_str(target);
        if norm_target == "generated.proto" || norm_target == "generated" {
            continue;
        }
        let wanted: std::collections::BTreeSet<String> = std::iter::once(target.clone()).collect();
        let known = file_packages
            .keys()
            .any(|file_name| file_matches(&wanted, file_name));
        if !known {
            let available: Vec<(String, String)> = file_packages
                .iter()
                .map(|(path, pkg)| (path.clone(), pkg.clone()))
                .collect();
            return Err(CodegenError::UnknownFile {
                file: target.clone(),
                available,
            });
        }
    }

    let mut stem_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for target in &targets {
        let norm = normalize_proto_path_str(target);
        let stem = Path::new(&norm)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("generated");
        *stem_counts.entry(stem.to_string()).or_default() += 1;
    }

    let mut type_files = std::collections::BTreeMap::new();
    for name in pool.collect_names() {
        if let Some(desc) = pool.get_message(&name) {
            let pkg = file_packages
                .get(&desc.file_name)
                .cloned()
                .unwrap_or_else(|| {
                    pool.get_file(&desc.file_name)
                        .map(|f| f.package.clone())
                        .unwrap_or_default()
                });
            type_files.insert(name, (desc.file_name.clone(), pkg));
        }
    }
    for name in pool.collect_enum_names() {
        if let Some(ed) = pool.get_enum(&name) {
            let pkg = file_packages
                .get(&ed.file_name)
                .cloned()
                .unwrap_or_else(|| {
                    pool.get_file(&ed.file_name)
                        .map(|f| f.package.clone())
                        .unwrap_or_default()
                });
            type_files.insert(name, (ed.file_name.clone(), pkg));
        }
    }
    TYPE_FILES.with(|c| *c.borrow_mut() = type_files);

    // Every emitted file embeds the same descriptor bytes; render the hex
    // block once and share it rather than re-formatting per target.
    let fds_block = (!resolved.no_reflect && !resolved.shared_pool).then(|| fds_hex_block(&fds));
    let mut out_files = Vec::new();
    for target in &targets {
        let norm_target = normalize_proto_path_str(target);
        CURRENT_TARGET.with(|c| *c.borrow_mut() = norm_target.clone());
        let safe_target = norm_target.replace(['/', '.', '-'], "_");
        let gen_mod = format!("__gen_{safe_target}");
        let wanted: std::collections::BTreeSet<String> = std::iter::once(target.clone()).collect();
        let target_is_wkt = wanted.iter().any(|w| {
            let s = w.replace('\\', "/");
            s.contains("google/protobuf/") && !s.contains("test_messages")
        });
        let mut src = format!(
            "// @generated by protoc-gen-pbrs\n\
#[allow(unused, reason = \"generated protobuf code may not exercise all fields, methods, or imports\")]\n\
#[allow(non_snake_case, reason = \"protobuf field and method names follow schema conventions\")]\n\
#[allow(non_camel_case_types, reason = \"protobuf message and enum names follow schema conventions\")]\n\
#[allow(non_upper_case_globals, reason = \"protobuf enum value names follow schema conventions\")]\n\
#[allow(unreachable_pub, reason = \"generated items are re-exported at module level\")]\n\
#[allow(clippy::all, reason = \"generated protobuf code does not adhere to hand-written clippy style\")]\n\
#[allow(clippy::pedantic, reason = \"generated protobuf code does not adhere to clippy pedantic rules\")]\n\
#[allow(clippy::nursery, reason = \"generated protobuf code does not adhere to clippy nursery rules\")]\n\
#[allow(clippy::indexing_slicing, reason = \"bounds-checked wire decoding and reflection tables\")]\n\
#[allow(clippy::cast_possible_truncation, reason = \"protobuf wire format varint and field number conversions\")]\n\
#[allow(clippy::cast_possible_wrap, reason = \"protobuf wire format varint and field number conversions\")]\n\
#[allow(clippy::cast_sign_loss, reason = \"protobuf wire format varint and field number conversions\")]\n\
#[allow(clippy::let_underscore_must_use, reason = \"ignoring results in generated wire helpers\")]\n\
mod {gen_mod} {{\n\
#![allow(unused, reason = \"generated protobuf code may not exercise all fields, methods, or imports\")]\n\
#![allow(non_snake_case, reason = \"protobuf field and method names follow schema conventions\")]\n\
#![allow(non_camel_case_types, reason = \"protobuf message and enum names follow schema conventions\")]\n\
#![allow(non_upper_case_globals, reason = \"protobuf enum value names follow schema conventions\")]\n\
#![allow(unreachable_pub, reason = \"generated items are re-exported at module level\")]\n\
#![allow(unsafe_code, reason = \"generated deserialization and transmutes use unsafe for performance\")]\n\
#![allow(clippy::all, reason = \"generated protobuf code does not adhere to hand-written clippy style\")]\n\
#![allow(clippy::pedantic, reason = \"generated protobuf code does not adhere to clippy pedantic rules\")]\n\
#![allow(clippy::nursery, reason = \"generated protobuf code does not adhere to clippy nursery rules\")]\n\
#![allow(clippy::indexing_slicing, reason = \"bounds-checked wire decoding and reflection tables\")]\n\
#![allow(clippy::cast_possible_truncation, reason = \"protobuf wire format varint and field number conversions\")]\n\
#![allow(clippy::cast_possible_wrap, reason = \"protobuf wire format varint and field number conversions\")]\n\
#![allow(clippy::cast_sign_loss, reason = \"protobuf wire format varint and field number conversions\")]\n\
#![allow(clippy::let_underscore_must_use, reason = \"ignoring results in generated wire helpers\")]\n\
use pbrs::prelude::*;\n\
use pbrs::{{Enum, Map, MapMut, MapView, ParseError, ProtoBytes, ProtoString, Repeated, RepeatedMut, RepeatedView, SerializeError, UnknownEnumValue}};\n\
use pbrs::UnknownFields;\n\n"
        );
        // Generated text dwarfs the descriptor bytes; pre-size the buffer
        // so per-message emission does not repeatedly reallocate + memcpy.
        src.reserve(fds.len() * 8);
        if resolved.no_reflect {
            // Accessor/binary tests: skip FileDescriptorSet hex and JSON/text.
        } else if resolved.shared_pool {
            src.push_str(
                "fn generated_pool() -> std::sync::Arc<pbrs::DescriptorPool> {\n    pbrs::gencode::conformance_pool()\n}\n\n",
            );
        } else {
            src.push_str(
                fds_block
                    .as_deref()
                    .expect("FDS block prebuilt when reflection is on"),
            );
            src.push_str(
                "#[allow(clippy::expect_used, reason = \"embedded descriptor bytes were validated during code generation\")]\nfn generated_pool() -> std::sync::Arc<pbrs::DescriptorPool> {\n    static P: std::sync::OnceLock<std::sync::Arc<pbrs::DescriptorPool>> = std::sync::OnceLock::new();\n    P.get_or_init(|| {\n        std::sync::Arc::new(pbrs::DescriptorPool::from_file_descriptor_set(FILE_DESCRIPTOR_SET).expect(\"fds\"))\n    }).clone()\n}\n\n",
            );
        }
        let direct_pub_files = pool.public_import_files(std::slice::from_ref(target));
        let transitive_pub_files = transitive_public_imports(&pool, target);
        let mut emit_names = Vec::new();
        for name in &names {
            let Some(desc) = pool.get_message(name) else {
                continue;
            };
            if desc.is_map_entry {
                continue;
            }
            if is_extern_type(&desc.full_name) {
                continue;
            }
            let wkt = desc.full_name.starts_with("google.protobuf.");
            let emit_wkt = wkt && !target_is_wkt && !resolved.no_wkt;
            let emit_deps = resolved.emit_deps;
            let is_target_file = file_matches(&wanted, &desc.file_name);
            let is_pub_import = transitive_pub_files
                .iter()
                .any(|p| file_matches(&std::iter::once(p.clone()).collect(), &desc.file_name));
            let is_same_stem_non_target = {
                let f_stem = std::path::Path::new(&desc.file_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let w_stem = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                f_stem == w_stem
                    && !targets.iter().any(|t| {
                        file_matches(&std::iter::once(t.clone()).collect(), &desc.file_name)
                    })
            };
            if !is_target_file
                && !emit_wkt
                && !(emit_deps && !wkt)
                && !is_pub_import
                && !is_same_stem_non_target
            {
                continue;
            }
            emit_names.push(name.clone());
        }
        if !resolved.emit_deps {
            emit_names.retain(|n| {
                let Some(d) = pool.get_message(n) else {
                    return true;
                };
                let wkt = d.full_name.starts_with("google.protobuf.");
                if wkt && !target_is_wkt && !resolved.no_wkt {
                    return true;
                }
                let is_pub = transitive_pub_files
                    .iter()
                    .any(|p| file_matches(&std::iter::once(p.clone()).collect(), &d.file_name));
                let pub_file_in_targets = targets
                    .iter()
                    .any(|t| file_matches(&std::iter::once(t.clone()).collect(), &d.file_name));
                !(is_pub && pub_file_in_targets)
            });
        }
        emit_names.sort();
        emit_names.dedup();
        let mut emit_enums = Vec::new();
        for name in pool.collect_enum_names() {
            let Some(ed) = pool.get_enum(&name) else {
                continue;
            };
            if is_extern_type(&ed.full_name) {
                continue;
            }
            let wkt = ed.full_name.starts_with("google.protobuf.");
            let emit_wkt = wkt && !target_is_wkt && !resolved.no_wkt;
            let emit_deps = resolved.emit_deps;
            let is_target_file = file_matches(&wanted, &ed.file_name);
            let is_pub_import = transitive_pub_files
                .iter()
                .any(|p| file_matches(&std::iter::once(p.clone()).collect(), &ed.file_name));
            let is_same_stem_non_target = {
                let f_stem = std::path::Path::new(&ed.file_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let w_stem = std::path::Path::new(target)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                f_stem == w_stem
                    && !targets
                        .iter()
                        .any(|t| file_matches(&std::iter::once(t.clone()).collect(), &ed.file_name))
            };
            if !is_target_file
                && !emit_wkt
                && !(emit_deps && !wkt)
                && !is_pub_import
                && !is_same_stem_non_target
            {
                continue;
            }
            emit_enums.push(name);
        }
        if !resolved.emit_deps {
            emit_enums.retain(|n| {
                let Some(ed) = pool.get_enum(n) else {
                    return true;
                };
                let wkt = ed.full_name.starts_with("google.protobuf.");
                if wkt && !target_is_wkt && !resolved.no_wkt {
                    return true;
                }
                let is_pub = transitive_pub_files
                    .iter()
                    .any(|p| file_matches(&std::iter::once(p.clone()).collect(), &ed.file_name));
                let pub_file_in_targets = targets
                    .iter()
                    .any(|t| file_matches(&std::iter::once(t.clone()).collect(), &ed.file_name));
                !(is_pub && pub_file_in_targets)
            });
        }
        emit_enums.sort();
        emit_enums.dedup();
        let mut ident_names = emit_names.clone();
        ident_names.extend(emit_enums.iter().cloned());
        let msg_set: std::collections::BTreeSet<String> =
            pool.collect_names().into_iter().collect();
        IDENTS.with(|c| *c.borrow_mut() = unique_idents(&ident_names, &msg_set));
        if !resolved.emit_deps {
            emit_public_uses(&mut src, &pool, &direct_pub_files, &targets);
        }
        for name in &emit_enums {
            let ed = pool.get_enum(name).expect("emit enum");
            emit_enum(&mut src, &ed);
        }
        for name in &emit_names {
            let desc = pool.get_message(name).expect("emit");
            let edition2024 = pool.file_edition(&desc.file_name) == Some(1001);
            let emitted = edition2024.then(|| {
                let mut message = desc.as_ref().clone();
                // CG-14b will add typed extension access; for now keep them in unknown wire fields.
                message
                    .fields
                    .retain(|_, field| field.extension_name.is_none());
                message
                    .fields_by_name
                    .retain(|_, number| message.fields.contains_key(number));
                message
                    .fields_by_json_name
                    .retain(|_, number| message.fields.contains_key(number));
                message
            });
            let desc = emitted.as_ref().unwrap_or(desc.as_ref());
            if edition2024 {
                for field in desc.fields.values() {
                    let repeated_closed = field.cardinality == Cardinality::Repeated
                        && field.enum_ty.as_ref().is_some_and(|en| en.closed);
                    let map_closed = field.is_map
                        && field
                            .message
                            .as_ref()
                            .and_then(|entry| entry.field(2))
                            .filter(|value| value.field_type == FieldType::Enum)
                            .and_then(|value| value.type_name.as_deref())
                            .and_then(|name| pool.get_enum(name))
                            .is_some_and(|en| en.closed);
                    if repeated_closed || map_closed {
                        return Err(CodegenError::MalformedDescriptor {
                            detail: format!(
                                "Edition 2024 closed enum in repeated/map field {}.{} is not supported",
                                desc.full_name, field.name
                            ),
                            path: Some(PathBuf::from(&desc.file_name)),
                        });
                    }
                }
            }
            emit_message(&mut src, desc, edition2024);
            emit_map_decoders(&mut src, desc, edition2024)?;
        }
        emit_nested_mods(&mut src, &emit_names, &emit_enums);
        let mut services: Vec<_> = pool
            .collect_services()
            .into_iter()
            .filter(|s| {
                if is_extern_type(&s.full_name) {
                    return false;
                }
                file_matches(&wanted, &s.file_name)
                    || transitive_pub_files
                        .iter()
                        .any(|p| file_matches(&std::iter::once(p.clone()).collect(), &s.file_name))
                    || {
                        let f_stem = std::path::Path::new(&s.file_name)
                            .file_stem()
                            .and_then(|st| st.to_str())
                            .unwrap_or("");
                        let w_stem = std::path::Path::new(target)
                            .file_stem()
                            .and_then(|st| st.to_str())
                            .unwrap_or("");
                        f_stem == w_stem
                            && !targets.iter().any(|t| {
                                file_matches(&std::iter::once(t.clone()).collect(), &s.file_name)
                            })
                    }
            })
            .collect();
        services.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        match resolved.stubs {
            Stubs::None => {}
            Stubs::Tonic
                if !services.is_empty() && (resolved.build_client || resolved.build_server) =>
            {
                src.push_str("\n// --- gRPC stubs (protobuf-tonic, not tonic-prost) ---\n");
                if resolved.codec_path.is_none() {
                    src.push_str("use protobuf_tonic::ProtobufCodec;\n");
                }
                src.push_str("use std::convert::Infallible;\n");
                src.push_str("use std::future::Future;\n");
                src.push_str("use std::pin::Pin;\n");
                src.push_str("use std::sync::Arc;\n");
                src.push_str("use std::task::{Context, Poll};\n");
                for svc in &services {
                    emit_service(&mut src, svc);
                }
            }
            Stubs::Kernel
                if !services.is_empty() && (resolved.build_client || resolved.build_server) =>
            {
                src.push_str("\n// --- gRPC stubs (pbrs-grpc kernel) ---\n");
                for svc in &services {
                    emit_kernel_service(&mut src, svc);
                }
            }
            Stubs::TonicCompat
                if !services.is_empty() && (resolved.build_client || resolved.build_server) =>
            {
                src.push_str("\n// --- gRPC stubs (tonic-shaped pbrs-grpc compat) ---\n");
                for svc in &services {
                    emit_compat_service(&mut src, svc);
                }
            }
            Stubs::Tonic | Stubs::Kernel | Stubs::TonicCompat => {}
        }
        src.push_str(&format!(
            "}}\n#[allow(unused_imports, reason = \"generated re-exports may not all be used\")]\npub use {gen_mod}::*;\n"
        ));
        if let Some(gc) = &resolved.grpc_crate {
            if gc != "::pbrs_grpc" {
                src = src.replace("::pbrs_grpc", gc);
            }
        }
        if let Some(tc) = &resolved.tonic_crate {
            if tc != "protobuf_tonic" {
                src = src.replace("protobuf_tonic", tc);
            }
        }
        if let Some(rc) = &resolved.runtime_crate {
            if rc != "pbrs" {
                src = src.replace("pbrs::", &format!("{rc}::"));
            }
        }
        let rel_rs = if let Some(stripped) = norm_target.strip_suffix(".proto") {
            format!("{stripped}.rs")
        } else {
            format!("{norm_target}.rs")
        };
        out_files.push((rel_rs.clone(), src.clone()));

        let stem = std::path::Path::new(&norm_target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("generated");
        let root_rs = format!("{stem}.rs");
        if stem_counts.get(stem) == Some(&1) && root_rs != rel_rs {
            out_files.push((root_rs, src));
        }
    }
    let mod_rs = emit_root_mod_rs(&targets, &file_packages, &pool);
    out_files.push((resolved.include_file.clone(), mod_rs));
    out_files.sort_by(|a, b| a.0.cmp(&b.0));
    out_files.dedup_by(|a, b| a.0 == b.0);
    Ok(out_files)
}

/// Generate from a FileDescriptorSet plus the proto paths to emit.
pub fn generate_from_file_descriptor_set(
    fds: &[u8],
    files_to_generate: &[String],
) -> Result<Vec<(String, String)>, CodegenError> {
    generate_from_file_descriptor_set_with_parameter(fds, files_to_generate, None, false)
}

pub(crate) fn generate_from_file_descriptor_set_with_parameter(
    fds: &[u8],
    files_to_generate: &[String],
    parameter: Option<&str>,
    require_files: bool,
) -> Result<Vec<(String, String)>, CodegenError> {
    let mut req = Vec::new();
    for f in files_to_generate {
        encode_string_field(&mut req, 1, f);
    }
    if let Some(param) = parameter {
        if !param.is_empty() {
            encode_string_field(&mut req, 2, param);
        }
    }
    let mut pos = 0;
    let mut has_file = false;
    while pos < fds.len() {
        let (n, w) = decode_tag(fds, &mut pos).map_err(|_| CodegenError::MalformedDescriptor {
            detail: "failed to decode FileDescriptorSet wire tag".to_string(),
            path: None,
        })?;
        if n == 1 && w == WIRE_LEN {
            let blob =
                read_len_bytes(fds, &mut pos).map_err(|_| CodegenError::MalformedDescriptor {
                    detail: "failed to read FileDescriptorProto in FileDescriptorSet".to_string(),
                    path: None,
                })?;
            if require_files && extract_proto_file_name_from_blob(blob).is_empty() {
                return Err(CodegenError::MalformedDescriptor {
                    detail: "FileDescriptorProto is malformed or has no file name".to_string(),
                    path: None,
                });
            }
            has_file = true;
            encode_len_field(&mut req, 15, blob);
        } else {
            wire::skip_field(fds, &mut pos, w).map_err(|_| CodegenError::MalformedDescriptor {
                detail: "failed to skip field in FileDescriptorSet".to_string(),
                path: None,
            })?;
        }
    }
    if require_files && !has_file {
        return Err(CodegenError::MalformedDescriptor {
            detail: "FileDescriptorSet contains no file descriptors".to_string(),
            path: None,
        });
    }
    generate_from_code_generator_request(&req)
}

/// prost-build-shaped compile: `protoc` FileDescriptorSet, then gencode into
/// `OUT_DIR` (or [`Config::out_dir`]).
pub fn compile_protos(
    protos: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
) -> Result<(), CodegenError> {
    Config::new().compile_protos(protos, includes)
}

pub(crate) fn parse_missing_import(stderr: &str) -> Option<(String, PathBuf)> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        if let Some(import_idx) = trimmed.find("Import \"") {
            let rest = &trimmed[import_idx + 8..];
            if let Some(end_quote) = rest.find('"') {
                let import_name = &rest[..end_quote];
                let proto_file = if let Some(colon_idx) = trimmed.find(':') {
                    trimmed[..colon_idx].trim()
                } else {
                    ""
                };
                return Some((import_name.to_string(), PathBuf::from(proto_file)));
            }
        }
    }
    for line in stderr.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_suffix(": File not found.") {
            let missing_file = rest.trim();
            if missing_file.ends_with(".proto") {
                return Some((missing_file.to_string(), PathBuf::new()));
            }
        }
    }
    None
}

pub(crate) fn missing_import_source(
    reported: PathBuf,
    protos: &[impl AsRef<Path>],
    includes: &[impl AsRef<Path>],
) -> PathBuf {
    if reported.as_os_str().is_empty() {
        return protos
            .first()
            .map(|proto| proto.as_ref().to_path_buf())
            .unwrap_or_default();
    }
    if reported.is_absolute() {
        return reported;
    }
    let name = normalize_proto_path_str(&reported.to_string_lossy());
    let mut requested = protos
        .iter()
        .filter(|proto| resolve_proto_rel_path(proto.as_ref(), includes) == name);
    match (requested.next(), requested.next()) {
        (Some(proto), None) => return proto.as_ref().to_path_buf(),
        (Some(_), Some(_)) => return reported,
        _ => {}
    }
    let imported = {
        let mut candidates = includes
            .iter()
            .map(|include| include.as_ref().join(&reported))
            .filter(|candidate| candidate.is_file());
        match (candidates.next(), candidates.next()) {
            (Some(proto), None) => Some(proto),
            _ => None,
        }
    };
    imported.unwrap_or(reported)
}

pub fn encode_code_generator_response(files: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    // PROTO3_OPTIONAL | SUPPORTS_EDITIONS
    encode_varint_field(&mut out, 2, 3);
    encode_varint_field(&mut out, 3, 998); // EDITION_PROTO2
    encode_varint_field(&mut out, 4, 1000); // EDITION_2023
    for (name, content) in files {
        let mut file = Vec::new();
        encode_string_field(&mut file, 1, name);
        encode_string_field(&mut file, 15, content);
        encode_len_field(&mut out, 15, &file);
    }
    out
}

pub fn encode_code_generator_response_error(error: &str) -> Vec<u8> {
    let mut out = Vec::new();
    encode_string_field(&mut out, 1, error);
    // PROTO3_OPTIONAL | SUPPORTS_EDITIONS
    encode_varint_field(&mut out, 2, 3);
    encode_varint_field(&mut out, 3, 998); // EDITION_PROTO2
    encode_varint_field(&mut out, 4, 1000); // EDITION_2023
    out
}

pub(crate) fn normalize_proto_path_str(s: &str) -> String {
    if !s.contains('\\') && !s.contains("//") && !s.starts_with("./") && !s.starts_with('/') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let replaced = s.replace('\\', "/");
    let mut prev_slash = false;
    for c in replaced.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
                prev_slash = true;
            }
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    let mut s = out.as_str();
    while let Some(stripped) = s.strip_prefix("./") {
        s = stripped;
    }
    s = s.trim_start_matches('/');
    s.to_string()
}

pub(crate) fn clean_proto_target_name(target: &str) -> String {
    let norm = normalize_proto_path_str(target);
    if target.starts_with('/') || (target.len() > 2 && target.chars().nth(1) == Some(':')) {
        if let Some(file_name) = Path::new(target).file_name().and_then(|n| n.to_str()) {
            return file_name.to_string();
        }
    }
    norm
}

pub(crate) fn extract_proto_file_name_from_blob(blob: &[u8]) -> String {
    let mut pos = 0;
    while pos < blob.len() {
        if let Ok((n, w)) = decode_tag(blob, &mut pos) {
            if n == 1 && w == WIRE_LEN {
                if let Ok(b) = read_len_bytes(blob, &mut pos) {
                    return String::from_utf8_lossy(b).into_owned();
                }
            } else {
                let _ = wire::skip_field(blob, &mut pos, w);
            }
        } else {
            break;
        }
    }
    String::new()
}

pub(crate) fn file_matches(wanted: &std::collections::BTreeSet<String>, file_name: &str) -> bool {
    if file_name.is_empty() {
        return true;
    }
    let file_norm = normalize_proto_path_str(file_name);
    let file_norm_no_proto = file_norm.strip_suffix(".proto").unwrap_or(&file_norm);
    let suff = format!("/{file_norm}");
    let suff_no_proto = format!("/{file_norm_no_proto}");
    wanted.iter().any(|w| {
        let w_norm = normalize_proto_path_str(w);
        if w_norm == "generated.proto" || w_norm == "generated" {
            return true;
        }
        let w_norm_no_proto = w_norm.strip_suffix(".proto").unwrap_or(&w_norm);
        if w_norm == file_norm || w_norm_no_proto == file_norm_no_proto {
            return true;
        }
        if w_norm.ends_with(&suff) || w_norm_no_proto.ends_with(&suff_no_proto) {
            return true;
        }
        let f_suff = format!("/{w_norm}");
        let f_suff_no_proto = format!("/{w_norm_no_proto}");
        if file_norm.ends_with(&f_suff) || file_norm_no_proto.ends_with(&f_suff_no_proto) {
            return true;
        }
        false
    })
}

pub(crate) fn transitive_public_imports(pool: &DescriptorPool, target: &str) -> Vec<String> {
    let mut all = Vec::new();
    let mut frontier = pool.public_import_files(&[target.to_string()]);
    for f in &frontier {
        if !all.contains(f) {
            all.push(f.clone());
        }
    }
    while !frontier.is_empty() {
        let next = pool.public_import_files(&frontier);
        frontier.clear();
        for f in next {
            if !all.contains(&f) {
                all.push(f.clone());
                frontier.push(f);
            }
        }
    }
    all
}

pub(crate) fn resolve_proto_rel_path(proto: &Path, includes: &[impl AsRef<Path>]) -> String {
    let norm_proto = normalize_proto_path_str(&proto.to_string_lossy());
    for inc in includes {
        let norm_inc = normalize_proto_path_str(&inc.as_ref().to_string_lossy());
        if norm_inc == "." || norm_inc.is_empty() {
            continue;
        }
        let prefix = format!("{norm_inc}/");
        if let Some(stripped) = norm_proto.strip_prefix(&prefix) {
            return stripped.to_string();
        }
    }
    for inc in includes {
        if let Ok(rel) = proto.strip_prefix(inc.as_ref()) {
            let s = normalize_proto_path_str(&rel.to_string_lossy());
            if !s.is_empty() && s != "." {
                return s;
            }
        }
        if let (Ok(p_canon), Ok(inc_canon)) = (proto.canonicalize(), inc.as_ref().canonicalize()) {
            if let Ok(rel) = p_canon.strip_prefix(&inc_canon) {
                let s = normalize_proto_path_str(&rel.to_string_lossy());
                if !s.is_empty() && s != "." {
                    return s;
                }
            }
        }
    }
    if proto.is_absolute() {
        if let Some(file_name) = proto.file_name().and_then(|n| n.to_str()) {
            return file_name.to_string();
        }
    }
    norm_proto
}

pub(crate) fn emit_root_mod_rs(
    targets: &[String],
    file_packages: &std::collections::BTreeMap<String, String>,
    pool: &DescriptorPool,
) -> String {
    #[derive(Default)]
    struct ModNode {
        includes: Vec<String>,
        submodules: std::collections::BTreeMap<String, ModNode>,
    }
    let mut root = ModNode::default();
    let mut sorted_targets = targets.to_vec();
    sorted_targets.sort();
    sorted_targets.dedup();
    for target in &sorted_targets {
        let norm_target = normalize_proto_path_str(target);
        let rel_rs = if let Some(stripped) = norm_target.strip_suffix(".proto") {
            format!("{stripped}.rs")
        } else {
            format!("{norm_target}.rs")
        };
        let pkg = file_packages.get(&norm_target).cloned().unwrap_or_else(|| {
            pool.get_file(&norm_target)
                .map(|f| f.package.clone())
                .unwrap_or_default()
        });
        if pkg.is_empty() {
            if !root.includes.contains(&rel_rs) {
                root.includes.push(rel_rs);
            }
        } else {
            let parts: Vec<&str> = pkg.split('.').collect();
            let mut cur = &mut root;
            for part in parts {
                cur = cur.submodules.entry(mod_ident(part)).or_default();
            }
            if !cur.includes.contains(&rel_rs) {
                cur.includes.push(rel_rs);
            }
        }
    }
    fn emit_node(src: &mut String, node: &ModNode, indent: usize) {
        let ind = "    ".repeat(indent);
        let mut sorted_includes = node.includes.clone();
        sorted_includes.sort();
        sorted_includes.dedup();
        for inc in &sorted_includes {
            let _ = writeln!(src, "{ind}include!(\"{inc}\");");
        }
        for (name, sub) in &node.submodules {
            let _ = writeln!(src, "{ind}pub mod {name} {{");
            emit_node(src, sub, indent + 1);
            let _ = writeln!(src, "{ind}}}");
        }
    }
    let mut src = String::from("// @generated by protoc-gen-pbrs\n");
    emit_node(&mut src, &root, 0);
    src
}
