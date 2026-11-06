"""Actual consumers for framework aliases and preserved user fragments."""
import csv
from pathlib import Path

LITERAL = "Literal ::pbrs_grpc protobuf_tonic ::pbrs_grpc_extra protobuf_tonic_extra"


def check(here, out, helper, run, deps, dependency_args, protoc):
    root = out / "bridge-aliases"
    root.mkdir()
    fds = root / "bridges.fds"
    code, _, error = run("descriptor", [protoc, "-I" + str(here), "--include_imports", "--include_source_info",
        "--descriptor_set_out=" + str(fds), here / "bridge.proto", here / "worker.proto"], root)
    if code:
        raise RuntimeError(error.read_text())
    code, _, error = run("generate", [helper, "bridges", root, fds], root)
    if code:
        raise RuntimeError(error.read_text())
    rows = []
    for case in csv.DictReader((root / "cases.tsv").open(), delimiter="\t"):
        folder = root / case["label"]
        source = folder / case["file"]
        text = source.read_text()
        prost = case["mode"] == "prost"
        tonic = case["mode"] == "tonic"
        expected = "Worker" if case["file"] == "worker.rs" else "protobuf_tonic"
        preservation_errors = []
        if not prost:
            if LITERAL not in text:
                preservation_errors.append("user documentation literal changed")
            kind = "protobuf_tonic" if tonic else "pbrs_grpc"
            for path in [f"crate::foreign::{kind}_extra::Input", f"crate::foreign::{kind}::Output"]:
                if path not in text:
                    preservation_errors.append("explicit user path changed: " + path)
            if case["codec"] and case["codec"] + "::<" not in text:
                preservation_errors.append("explicit codec path changed: " + case["codec"])
        request = "::protobuf_tonic::hello::HelloRequest" if tonic else "::pbrs_grpc::pb::Any"
        response = "::protobuf_tonic::hello::HelloReply" if tonic else "::pbrs_grpc::pb::Any"
        modules = f'''#[allow(unused_imports, reason="framework alias shim")] mod grpc_module {{ pub use ::pbrs_grpc::*; }}
#[allow(unused_imports, reason="adapter alias shim")] mod adapter_module {{ pub use ::protobuf_tonic::*; }}
#[allow(dead_code, reason="independent path preservation controls")] mod foreign {{
    pub mod pbrs_grpc {{ pub type Input = {request}; pub type Output = {response}; }}
    pub mod pbrs_grpc_extra {{ pub type Input = {request}; }}
    pub mod protobuf_tonic {{ pub type Input = {request}; pub type Output = {response}; pub type Codec<E,D> = ::protobuf_tonic::ProtobufCodec<E,D>; }}
    pub mod protobuf_tonic_extra {{ pub type Input = {request}; }}
}}
'''
        if prost:
            modules += "#[derive(::prost::Message)] pub struct Input {}\n#[derive(::prost::Message)] pub struct Output {}\n"
        main_rs = folder / (case["file"] + ".main.rs")
        main_rs.write_text(modules + 'include!("' + case["file"] + '");\n' +
                           'fn expected_public_trait<T: self::' + expected + '>() {}\nfn main() {}\n')
        command = ["rustc", "--edition=2024", "--crate-name", "bridge_consumer", "--emit=metadata", "--cap-lints=allow",
                   main_rs, *dependency_args, "-o", folder / (case["file"] + ".rmeta")]
        for name, library in deps.items():
            if name != "tonic_prost_build":
                command += ["--extern", name + "=" + str(library)]
        command += ["--extern", "grpc_bridge_alias=" + str(deps["pbrs_grpc"]),
                    "--extern", "adapter_bridge_alias=" + str(deps["protobuf_tonic"]),
                    "--extern", "explicit_codec_alias=" + str(deps["protobuf_tonic"])]
        code, _, error = run(case["file"] + "-compile", command, folder)
        rows.append({**case, "exit_code": code, "preservation_errors": preservation_errors,
                     "diagnostics": [line for line in error.read_text().splitlines() if line.startswith("error")]})
    for mode in ["kernel", "compat", "tonic"]:
        for file in ["bridge.rs", "worker.rs"]:
            if (root / (mode + "-before") / file).read_bytes() != (root / (mode + "-after") / file).read_bytes():
                raise RuntimeError("same-thread alias state leaked into later defaults")
    return rows
