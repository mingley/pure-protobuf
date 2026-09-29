//! MX-01 split of `super`: descriptors (mechanical move, no behavior change).

use super::*;
use crate::dynamic::{Cardinality, DescriptorPool, FieldType, ServiceDescriptor};
use crate::wire::{self, WIRE_LEN, decode_tag, encode_len_field, read_len_bytes};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    apply_resolved_config(&resolved);

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
    // Collected once: the per-target loops below used to re-clone the full
    // name lists (and rebuild the message-name set) for every target.
    let msg_names: Vec<String> = pool.collect_names();
    let enum_names: Vec<String> = pool.collect_enum_names();
    let msg_set: std::collections::BTreeSet<String> = msg_names.iter().cloned().collect();
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
    // Per-type facts are derived here, once per request, instead of once
    // per (target, type) in the emission loops below.
    let mut msg_facts = Vec::with_capacity(msg_names.len());
    for name in &msg_names {
        if let Some(desc) = pool.get_message(name) {
            let pkg = file_packages
                .get(&desc.file_name)
                .cloned()
                .unwrap_or_else(|| {
                    pool.get_file(&desc.file_name)
                        .map(|f| f.package.clone())
                        .unwrap_or_default()
                });
            type_files.insert(name.clone(), (desc.file_name.clone(), pkg));
            msg_facts.push(TypeFacts {
                name: name.clone(),
                file: desc.file_name.clone(),
                is_map_entry: desc.is_map_entry,
                is_extern: is_extern_type(&desc.full_name),
                is_wkt: desc.full_name.starts_with("google.protobuf."),
            });
        }
    }
    let mut enum_facts = Vec::with_capacity(enum_names.len());
    for name in &enum_names {
        if let Some(ed) = pool.get_enum(name) {
            let pkg = file_packages
                .get(&ed.file_name)
                .cloned()
                .unwrap_or_else(|| {
                    pool.get_file(&ed.file_name)
                        .map(|f| f.package.clone())
                        .unwrap_or_default()
                });
            type_files.insert(name.clone(), (ed.file_name.clone(), pkg));
            enum_facts.push(TypeFacts {
                name: name.clone(),
                file: ed.file_name.clone(),
                is_map_entry: false,
                is_extern: is_extern_type(&ed.full_name),
                is_wkt: ed.full_name.starts_with("google.protobuf."),
            });
        }
    }
    TYPE_FILES.with(|c| *c.borrow_mut() = type_files);

    // Every emitted file embeds the same descriptor bytes; render the hex
    // block once and share it rather than re-formatting per target.
    let fds_block = (!resolved.no_reflect && !resolved.shared_pool).then(|| fds_hex_block(&fds));
    // File-level facts shared by every target: each unique file's stem and
    // whether it matches any target. The emission loops below used to
    // re-derive these per (target, type) with fresh normalizations.
    let services_all: Vec<Arc<ServiceDescriptor>> = pool.collect_services();
    let all_matcher = FileMatcher::for_slice(&targets);
    let mut file_memo: std::collections::BTreeMap<String, FileFacts> =
        std::collections::BTreeMap::new();
    for file in msg_facts
        .iter()
        .map(|facts| facts.file.as_str())
        .chain(enum_facts.iter().map(|facts| facts.file.as_str()))
        .chain(services_all.iter().map(|svc| svc.file_name.as_str()))
    {
        file_memo
            .entry(file.to_string())
            .or_insert_with_key(|file| FileFacts::for_file(file, &all_matcher));
    }
    // One target's emission as a pure closure over shared-immutable inputs
    // (plus thread-local codegen state): this lets multi-target requests
    // emit files on worker threads while staying byte-identical.
    let emit_one = |target: &String| -> Result<Vec<(String, String)>, CodegenError> {
        let mut partial: Vec<(String, String)> = Vec::new();
        let norm_target = normalize_proto_path_str(target);
        CURRENT_TARGET.with(|c| *c.borrow_mut() = norm_target.clone());
        let safe_target = norm_target.replace(['/', '.', '-'], "_");
        let gen_mod = format!("__gen_{safe_target}");
        // `wanted` was a singleton set holding `target`; test it directly.
        let target_slashes = target.replace('\\', "/");
        let target_is_wkt = target_slashes.contains("google/protobuf/")
            && !target_slashes.contains("test_messages");
        // Pre-normalized matchers: every per-type `file_matches` below used
        // to re-normalize and rebuild `format!` suffixes from scratch.
        let target_matcher = FileMatcher::single(target);
        let target_stem = std::path::Path::new(target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
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
        let pub_matcher = FileMatcher::for_slice(&transitive_pub_files);
        let mut emit_names = Vec::new();
        {
            let mut scratch = None;
            for facts in &msg_facts {
                if facts.is_map_entry {
                    continue;
                }
                if facts.is_extern {
                    continue;
                }
                let emit_wkt = facts.is_wkt && !target_is_wkt && !resolved.no_wkt;
                let emit_deps = resolved.emit_deps;
                let ff = file_facts(&file_memo, &facts.file, &all_matcher, &mut scratch);
                let is_target_file = target_matcher.matches(&facts.file);
                let is_pub_import = !pub_matcher.is_empty() && pub_matcher.matches(&facts.file);
                let is_same_stem_non_target = ff.stem == target_stem && !ff.in_any_target;
                if !is_target_file
                    && !emit_wkt
                    && !(emit_deps && !facts.is_wkt)
                    && !is_pub_import
                    && !is_same_stem_non_target
                {
                    continue;
                }
                // Folded from the post-pass `retain` below it: drop a pub-import
                // type whose file is itself generated, unless WKT emission
                // keeps it. Same inputs, same outcome, one pass instead of two.
                if !emit_deps && is_pub_import && ff.in_any_target && !emit_wkt {
                    continue;
                }
                emit_names.push(facts.name.clone());
            }
        }
        emit_names.sort();
        emit_names.dedup();
        let mut emit_enums = Vec::new();
        {
            let mut scratch = None;
            for facts in &enum_facts {
                if facts.is_extern {
                    continue;
                }
                let emit_wkt = facts.is_wkt && !target_is_wkt && !resolved.no_wkt;
                let emit_deps = resolved.emit_deps;
                let ff = file_facts(&file_memo, &facts.file, &all_matcher, &mut scratch);
                let is_target_file = target_matcher.matches(&facts.file);
                let is_pub_import = !pub_matcher.is_empty() && pub_matcher.matches(&facts.file);
                let is_same_stem_non_target = ff.stem == target_stem && !ff.in_any_target;
                if !is_target_file
                    && !emit_wkt
                    && !(emit_deps && !facts.is_wkt)
                    && !is_pub_import
                    && !is_same_stem_non_target
                {
                    continue;
                }
                // Folded post-pass `retain`, mirroring the message loop.
                if !emit_deps && is_pub_import && ff.in_any_target && !emit_wkt {
                    continue;
                }
                emit_enums.push(facts.name.clone());
            }
        }
        emit_enums.sort();
        emit_enums.dedup();
        let mut ident_names = emit_names.clone();
        ident_names.extend(emit_enums.iter().cloned());
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
        let mut services: Vec<Arc<ServiceDescriptor>> = Vec::new();
        {
            let mut scratch = None;
            for svc in &services_all {
                if is_extern_type(&svc.full_name) {
                    continue;
                }
                let ff = file_facts(&file_memo, &svc.file_name, &all_matcher, &mut scratch);
                let same_stem = ff.stem == target_stem && !ff.in_any_target;
                if target_matcher.matches(&svc.file_name)
                    || (!pub_matcher.is_empty() && pub_matcher.matches(&svc.file_name))
                    || same_stem
                {
                    services.push(Arc::clone(svc));
                }
            }
        }
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
        partial.push((rel_rs.clone(), src.clone()));

        let stem = std::path::Path::new(&norm_target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("generated");
        let root_rs = format!("{stem}.rs");
        if stem_counts.get(stem) == Some(&1) && root_rs != rel_rs {
            partial.push((root_rs, src));
        }
        Ok(partial)
    };
    // Targets are independent given the shared-immutable inputs above plus
    // per-thread codegen state, so multi-target requests emit in parallel.
    // Order is restored below (`out_files` is sorted after), and the first
    // error in target order wins, exactly like the sequential loop.
    let mut out_files: Vec<(String, String)> = Vec::new();
    let worker_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(targets.len())
        .max(1);
    if worker_count <= 1 {
        for target in &targets {
            out_files.extend(emit_one(target)?);
        }
    } else {
        // Move the shared map out of this thread's TLS; each worker takes a
        // clone for its own TLS. Nothing after this point reads it here.
        let type_files_snapshot = TYPE_FILES.with(|c| c.take());
        let chunk_size = targets.len().div_ceil(worker_count);
        // Shared references for the workers (bound outside so the `move`
        // closures copy references instead of moving owned values).
        let emit_ref = &emit_one;
        let resolved_ref = &resolved;
        let snapshot_ref = &type_files_snapshot;
        let ordered: Vec<TargetEmission> = std::thread::scope(|s| {
            let mut handles = Vec::new();
            for (chunk_idx, group) in targets.chunks(chunk_size).enumerate() {
                let base = chunk_idx * chunk_size;
                handles.push(s.spawn(move || {
                    let _guard = CodegenStateGuard::new();
                    apply_resolved_config(resolved_ref);
                    TYPE_FILES.with(|c| *c.borrow_mut() = snapshot_ref.clone());
                    let mut part = Vec::with_capacity(group.len());
                    for (offset, target) in group.iter().enumerate() {
                        part.push((base + offset, emit_ref(target)));
                    }
                    part
                }));
            }
            let mut ordered = Vec::new();
            for handle in handles {
                match handle.join() {
                    Ok(part) => ordered.extend(part),
                    Err(payload) => std::panic::resume_unwind(payload),
                }
            }
            ordered
        });
        for (_, result) in ordered {
            out_files.extend(result?);
        }
    }
    let mod_rs = emit_root_mod_rs(&targets, &file_packages, &pool);
    out_files.push((resolved.include_file.clone(), mod_rs));
    out_files.sort_by(|a, b| a.0.cmp(&b.0));
    out_files.dedup_by(|a, b| a.0 == b.0);
    Ok(out_files)
}

/// Publish a resolved plugin/request configuration to this thread's
/// codegen state. The request entry point calls this for the calling
/// thread; parallel-emission workers call it for theirs.
pub(crate) fn apply_resolved_config(resolved: &ResolvedConfig) {
    STUBS.with(|c| c.set(resolved.stubs));
    EMIT_DEPS.with(|c| c.set(resolved.emit_deps));
    NO_WKT.with(|c| c.set(resolved.no_wkt));
    SHARED_POOL.with(|c| c.set(resolved.shared_pool));
    NO_REFLECT.with(|c| c.set(resolved.no_reflect));
    EMIT_JSON.with(|c| c.set(resolved.emit_json));
    EMIT_TEXT.with(|c| c.set(resolved.emit_text));
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
}

/// Per-message/per-enum emission inputs that do not vary by target.
///
/// Derived once per request while the type→file map is built, instead of
/// re-derived from the pool for every (target, type) pair.
struct TypeFacts {
    name: String,
    file: String,
    is_map_entry: bool,
    is_extern: bool,
    is_wkt: bool,
}

/// Per-file emission inputs shared by every target of a request.
struct FileFacts {
    /// `Path::file_stem` of the proto path, computed once per unique file.
    stem: String,
    /// Whether the file matches any request target.
    in_any_target: bool,
}

impl FileFacts {
    fn for_file(file: &str, all_targets: &FileMatcher) -> Self {
        Self {
            stem: Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string(),
            in_any_target: all_targets.matches(file),
        }
    }
}

/// One target's emission result paired with its index in `targets`, so
/// parallel workers can be re-sequenced deterministically.
type TargetEmission = (usize, Result<Vec<(String, String)>, CodegenError>);

/// Look up a file's [`FileFacts`], computing them into `scratch` when the
/// file was not enumerated up front (defensive; every pool file is).
fn file_facts<'memo, 'out>(
    memo: &'memo std::collections::BTreeMap<String, FileFacts>,
    file: &str,
    all_targets: &FileMatcher,
    scratch: &'out mut Option<FileFacts>,
) -> &'out FileFacts
where
    'memo: 'out,
{
    if let Some(facts) = memo.get(file) {
        return facts;
    }
    scratch.insert(FileFacts::for_file(file, all_targets))
}

/// Pre-normalized `wanted` set for repeated file-membership tests.
///
/// [`file_matches`] used to normalize every `wanted` entry and rebuild
/// `format!` suffixes on each call; the emission loops test every
/// (target, type) pair, which made path matching ~40% of generation time.
/// A matcher normalizes once and then tests allocation-free.
pub(crate) struct FileMatcher {
    /// `(normalized, without ".proto" suffix)` per wanted entry.
    entries: Vec<(String, String)>,
    /// A `generated[.proto]` entry matches every file.
    matches_all: bool,
}

impl FileMatcher {
    fn from_iter<'a>(wanted: impl IntoIterator<Item = &'a str>) -> Self {
        let mut entries = Vec::new();
        let mut matches_all = false;
        for w in wanted {
            let norm = normalize_cow(w).into_owned();
            if norm == "generated.proto" || norm == "generated" {
                matches_all = true;
            } else {
                let no_proto = norm.strip_suffix(".proto").unwrap_or(&norm).to_string();
                entries.push((norm, no_proto));
            }
        }
        Self {
            entries,
            matches_all,
        }
    }

    pub(crate) fn for_slice(wanted: &[String]) -> Self {
        Self::from_iter(wanted.iter().map(String::as_str))
    }

    pub(crate) fn single(wanted: &str) -> Self {
        Self::from_iter(std::iter::once(wanted))
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.matches_all && self.entries.is_empty()
    }

    pub(crate) fn matches(&self, file_name: &str) -> bool {
        if file_name.is_empty() || self.matches_all {
            return true;
        }
        let file_norm = normalize_cow(file_name);
        let file_no_proto = file_norm.strip_suffix(".proto").unwrap_or(&file_norm);
        self.entries.iter().any(|(w, w_no_proto)| {
            wanted_entry_matches_norm(w, w_no_proto, &file_norm, file_no_proto)
        })
    }
}

/// `hay` ends with `/needle` on a path-segment boundary, without
/// allocating the joined `/{needle}` suffix.
fn ends_with_slash_suffix(hay: &str, needle: &str) -> bool {
    hay.strip_suffix(needle)
        .is_some_and(|prefix| prefix.ends_with('/'))
}

/// One pre-normalized `wanted` entry against one pre-normalized file:
/// exactly the [`file_matches`] comparisons, without allocation.
fn wanted_entry_matches_norm(
    w_norm: &str,
    w_no_proto: &str,
    file_norm: &str,
    file_no_proto: &str,
) -> bool {
    if w_norm == file_norm || w_no_proto == file_no_proto {
        return true;
    }
    if ends_with_slash_suffix(w_norm, file_norm)
        || ends_with_slash_suffix(w_no_proto, file_no_proto)
    {
        return true;
    }
    ends_with_slash_suffix(file_norm, w_norm) || ends_with_slash_suffix(file_no_proto, w_no_proto)
}

fn wanted_entry_matches(wanted: &str, file_norm: &str, file_no_proto: &str) -> bool {
    let w_norm = normalize_cow(wanted);
    if w_norm.as_ref() == "generated.proto" || w_norm.as_ref() == "generated" {
        return true;
    }
    let w_no_proto = w_norm.strip_suffix(".proto").unwrap_or(&w_norm);
    wanted_entry_matches_norm(&w_norm, w_no_proto, file_norm, file_no_proto)
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
    encode_varint_field(&mut out, 4, 1001); // EDITION_2024 (CG-14 qualified subset)
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
    encode_varint_field(&mut out, 4, 1001); // EDITION_2024 (CG-14 qualified subset)
    out
}

fn normalize_slow(s: &str) -> String {
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

/// Borrowed fast path for already-normal paths (the common case); the
/// slow path is byte-identical to the previous implementation.
fn normalize_cow(s: &str) -> Cow<'_, str> {
    if !s.contains('\\') && !s.contains("//") && !s.starts_with("./") && !s.starts_with('/') {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(normalize_slow(s))
    }
}

pub(crate) fn normalize_proto_path_str(s: &str) -> String {
    normalize_cow(s).into_owned()
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
    let file_norm = normalize_cow(file_name);
    let file_no_proto = file_norm.strip_suffix(".proto").unwrap_or(&file_norm);
    wanted
        .iter()
        .any(|w| wanted_entry_matches(w, &file_norm, file_no_proto))
}

/// [`file_matches`] for a single `wanted` entry, without building a set.
pub(crate) fn file_matches_single(wanted: &str, file_name: &str) -> bool {
    if file_name.is_empty() {
        return true;
    }
    let file_norm = normalize_cow(file_name);
    let file_no_proto = file_norm.strip_suffix(".proto").unwrap_or(&file_norm);
    wanted_entry_matches(wanted, &file_norm, file_no_proto)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pre-GN-04 `file_matches`, kept as the equivalence oracle.
    fn reference_file_matches(
        wanted: &std::collections::BTreeSet<String>,
        file_name: &str,
    ) -> bool {
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

    fn corpus_paths() -> Vec<&'static str> {
        vec![
            "",
            "/",
            "///",
            ".",
            "./",
            "generated",
            "generated.proto",
            "./generated",
            "./generated.proto",
            "part_00.proto",
            "part_00",
            "a/part_00.proto",
            "x/a/part_00.proto",
            "/abs/part_00.proto",
            "./rel/part_00.proto",
            "a//b//part_00.proto",
            "a\\b\\part_00.proto",
            "part_00.proto.proto",
            ".proto",
            "foo.",
            ".foo",
            "google/protobuf/timestamp.proto",
            "google/protobuf/",
            "envoy/config/core/v3/address.proto",
            "test_messages_proto3.proto",
            "a/b",
            "a/b/",
            "C:\\win\\path.proto",
            "trailing.proto/",
            ".hidden/file.proto",
            "UPPER.PROTO",
        ]
    }

    #[test]
    fn file_matcher_matches_reference() {
        let paths = corpus_paths();
        for file in &paths {
            // Singleton sets.
            for wanted in &paths {
                let set = std::iter::once(wanted.to_string()).collect();
                assert_eq!(
                    file_matches(&set, file),
                    reference_file_matches(&set, file),
                    "file_matches({wanted:?}, {file:?})"
                );
                assert_eq!(
                    FileMatcher::single(wanted).matches(file),
                    reference_file_matches(&set, file),
                    "matcher.single({wanted:?}).matches({file:?})"
                );
                assert_eq!(
                    file_matches_single(wanted, file),
                    reference_file_matches(&set, file),
                    "file_matches_single({wanted:?}, {file:?})"
                );
            }
            // Multi-entry sets incl. duplicates and generated entries.
            let multi: std::collections::BTreeSet<String> = paths
                .iter()
                .step_by(3)
                .map(|s| s.to_string())
                .chain(["generated".to_string()])
                .collect();
            assert_eq!(
                file_matches(&multi, file),
                reference_file_matches(&multi, file),
                "file_matches(multi, {file:?})"
            );
            let slice: Vec<String> = multi.iter().cloned().collect();
            assert_eq!(
                FileMatcher::for_slice(&slice).matches(file),
                reference_file_matches(&multi, file),
                "matcher.for_slice(multi).matches({file:?})"
            );
            let empty: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            assert_eq!(
                file_matches(&empty, file),
                reference_file_matches(&empty, file),
                "file_matches(empty, {file:?})"
            );
            assert!(
                FileMatcher::for_slice(&[]).is_empty(),
                "empty matcher reports empty"
            );
        }
    }

    #[test]
    fn normalize_cow_matches_owned() {
        for path in corpus_paths() {
            let owned = normalize_proto_path_str(path);
            assert_eq!(normalize_cow(path).into_owned(), owned, "{path:?}");
            if !path.contains('\\')
                && !path.contains("//")
                && !path.starts_with("./")
                && !path.starts_with('/')
            {
                assert!(
                    matches!(normalize_cow(path), Cow::Borrowed(_)),
                    "fast path borrows for {path:?}"
                );
            }
        }
    }

    /// Hand-encoded `FileDescriptorSet` with two files so multi-target
    /// emission (and its parallel path) is covered without `protoc`.
    fn two_file_fds() -> Vec<u8> {
        fn file_proto(name: &str, package: &str, message: &str) -> Vec<u8> {
            // FileDescriptorProto: 1=name, 2=package, 4=message_type.
            // DescriptorProto: 1=name.
            let mut msg = Vec::new();
            encode_string_field(&mut msg, 1, message);
            let mut file = Vec::new();
            encode_string_field(&mut file, 1, name);
            encode_string_field(&mut file, 2, package);
            encode_len_field(&mut file, 4, &msg);
            file
        }
        let mut fds = Vec::new();
        encode_len_field(&mut fds, 1, &file_proto("a.proto", "pkg", "MsgA"));
        encode_len_field(&mut fds, 1, &file_proto("dir/b.proto", "pkg", "MsgB"));
        fds
    }

    #[test]
    fn multi_target_emission_is_deterministic() {
        let fds = two_file_fds();
        let targets = ["a.proto".to_string(), "dir/b.proto".to_string()];
        let first = generate_from_file_descriptor_set(&fds, &targets).expect("generate");
        assert!(first.len() >= 3, "two targets plus mod.rs: {}", first.len());
        for _ in 0..25 {
            let again = generate_from_file_descriptor_set(&fds, &targets).expect("generate");
            assert_eq!(again, first, "parallel emission must be deterministic");
        }
    }
}
