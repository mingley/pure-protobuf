use pbrs::codegen::{Config, Stubs};
use std::{fmt::Write as _, fs, path::Path};

fn emit(
    root: &Path,
    fds: &Path,
    label: &str,
    mode: &str,
    grpc: Option<&str>,
    adapter: Option<&str>,
    codec: Option<&str>,
) -> Result<String, pbrs::codegen::CodegenError> {
    let directory = root.join(label);
    let mut config = Config::new();
    config
        .out_dir(&directory)
        .emit_reflection(false)
        .emit_json(false)
        .emit_text(false)
        .client_attribute(
            ".proof.rpcbridges.protobuf_tonic",
            "#[doc = \"Literal ::pbrs_grpc protobuf_tonic ::pbrs_grpc_extra protobuf_tonic_extra\"]",
        );
    if mode == "tonic" {
        config.stubs(Stubs::Tonic);
        config.extern_path(
            ".proof.rpcbridges.Input",
            "crate::foreign::protobuf_tonic_extra::Input",
        );
        config.extern_path(
            ".proof.rpcbridges.Output",
            "crate::foreign::protobuf_tonic::Output",
        );
    } else {
        config.stubs(Stubs::Kernel).tonic_compat(mode == "compat");
        config.extern_path(
            ".proof.rpcbridges.Input",
            "crate::foreign::pbrs_grpc_extra::Input",
        );
        config.extern_path(
            ".proof.rpcbridges.Output",
            "crate::foreign::pbrs_grpc::Output",
        );
    }
    if let Some(path) = grpc {
        config
            .grpc_crate("unused_old_native_alias")
            .grpc_crate(path);
    }
    if let Some(path) = adapter {
        config
            .tonic_crate("unused_old_adapter_alias")
            .tonic_crate(path);
    }
    if let Some(path) = codec {
        config.codec_path(path);
    }
    config.compile_descriptor_set(fds, &["bridge.proto", "worker.proto"], &[] as &[&str])?;
    let mut rows = String::new();
    for file in ["bridge.rs", "worker.rs"] {
        writeln!(
            rows,
            "{label}\t{mode}\t{}\t{}\t{}\t{file}",
            grpc.unwrap_or(""),
            adapter.unwrap_or(""),
            codec.unwrap_or("")
        )
        .unwrap();
    }
    Ok(rows)
}

pub fn generate(root: &Path, fds: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root)?;
    let mut rows = String::from("label\tmode\tgrpc\tadapter\tcodec\tfile\n");
    for mode in ["kernel", "compat", "tonic"] {
        rows.push_str(&emit(
            root,
            fds,
            &format!("{mode}-before"),
            mode,
            None,
            None,
            None,
        )?);
        for (label, grpc, adapter) in [
            ("bare", "grpc_bridge_alias", "adapter_bridge_alias"),
            ("absolute", "::grpc_bridge_alias", "::adapter_bridge_alias"),
            ("crate", "crate::grpc_module", "crate::adapter_module"),
            (
                "self",
                "self::super::grpc_module",
                "self::super::adapter_module",
            ),
            ("super", "super::grpc_module", "super::adapter_module"),
        ] {
            rows.push_str(&emit(
                root,
                fds,
                &format!("{mode}-{label}"),
                mode,
                Some(grpc),
                Some(adapter),
                None,
            )?);
        }
        rows.push_str(&emit(
            root,
            fds,
            &format!("{mode}-after"),
            mode,
            None,
            None,
            None,
        )?);
    }
    for (label, codec) in [
        ("bare", "explicit_codec_alias::ProtobufCodec"),
        ("absolute", "::adapter_bridge_alias::ProtobufCodec"),
        ("crate", "crate::foreign::protobuf_tonic::Codec"),
    ] {
        rows.push_str(&emit(
            root,
            fds,
            &format!("codec-{label}"),
            "tonic",
            None,
            Some("unused_adapter_must_not_be_resolved"),
            Some(codec),
        )?);
    }
    let parallel: Result<Vec<String>, pbrs::codegen::CodegenError> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (label, grpc, adapter) in [
            ("a", "grpc_bridge_alias", "adapter_bridge_alias"),
            ("b", "crate::grpc_module", "crate::adapter_module"),
        ] {
            handles.push(scope.spawn(move || {
                let mut result = String::new();
                for mode in ["kernel", "compat", "tonic"] {
                    result.push_str(&emit(
                        root,
                        fds,
                        &format!("thread-{label}-{mode}"),
                        mode,
                        Some(grpc),
                        Some(adapter),
                        None,
                    )?);
                }
                Ok(result)
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for result in parallel? {
        rows.push_str(&result);
    }
    let prost = root.join("prost-after");
    pbrs::codegen::prost_stubs::compile_descriptor_set(fds, &["bridge.proto"], &prost)?;
    rows.push_str("prost-after\tprost\t\t\t\tbridge.pbrs_grpc.rs\n");
    fs::write(root.join("cases.tsv"), rows)?;
    Ok(())
}
