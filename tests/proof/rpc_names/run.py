#!/usr/bin/env python3
"""Compile generated RPC namespaces against real native and Tonic runtimes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time
import sys
sys.dont_write_bytecode = True
import bridge_aliases

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PACKAGES = ("pbrs", "pbrs_grpc", "protobuf_tonic", "tonic", "tokio_stream", "http", "prost", "tonic_prost_build")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--only-bridge-aliases", action="store_true", help="Local framework-alias consumer subset")
    parser.add_argument("--artifacts", type=Path, help="Cargo JSON artifact output from a workspace all-feature build")
    parser.add_argument("--prebuilt", type=Path, help="Explicit plugin/dependency receipt for a local metadata-only run")
    parser.add_argument("--baseline-plugin", type=Path, help="Optional historical generator; its failures are retained separately")
    parser.add_argument("--skip-prost", action="store_true", help="Explicit local subset when the prebuilt native runtime lacks its prost feature")
    args = parser.parse_args()
    if args.artifacts and args.prebuilt:
        parser.error("choose --artifacts or --prebuilt")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()
    env["LD_LIBRARY_PATH"] = str(Path(sysroot) / "lib") + os.pathsep + env.get("LD_LIBRARY_PATH", "")
    records = []
    peak = 0
    minimum = shutil.disk_usage(out).free

    def run(label, command, folder=out):
        nonlocal peak, minimum
        stdout = folder / (label + ".stdout")
        stderr = folder / (label + ".stderr")
        with stdout.open("w") as output, stderr.open("w") as error:
            process = subprocess.Popen([str(value) for value in command], cwd=ROOT, env=env,
                                       stdout=output, stderr=error, start_new_session=True)
            stop = None
            while process.poll() is None:
                size = sum(path.stat().st_size for path in out.rglob("*") if path.is_file())
                free = shutil.disk_usage(out).free
                peak, minimum = max(peak, size), min(minimum, free)
                if size > 200 * 1024 ** 2 or free < 1024 ** 3:
                    stop = "200 MiB private-output cap or 1 GiB free-space floor"
                    os.killpg(process.pid, signal.SIGTERM)
                    break
                time.sleep(0.1)
            code = process.wait()
        row = {"label": label, "command": [str(value) for value in command], "exit_code": code,
               "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr), "resource_stop": stop}
        records.append(row)
        if stop:
            raise RuntimeError(stop)
        return code, stdout, stderr

    if args.prebuilt:
        receipt = json.loads(args.prebuilt.read_text())
    else:
        artifact_file = args.artifacts
        if artifact_file is None:
            code, artifact_file, error = run("workspace-build", ["cargo", "build", "--locked", "--offline",
                "--workspace", "--all-features", "--message-format=json", "-j1"])
            if code:
                raise RuntimeError("workspace dependency build failed; see " + str(error))
        artifacts = []
        for line in artifact_file.read_text().splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if item.get("reason") == "compiler-artifact":
                artifacts.append(item)
        dependencies = {}
        package_ids = {}
        for name in PACKAGES:
            choices = [item for item in artifacts if item["target"]["name"] == name
                       and any(path.endswith(".rlib") for path in item["filenames"])]
            if not choices:
                raise RuntimeError("Cargo receipt lacks " + name)
            selected = max(choices, key=lambda item: len(item.get("features", [])))
            dependencies[name] = next(path for path in selected["filenames"] if path.endswith(".rlib"))
            package_ids[name] = selected["package_id"]
        if not package_ids["tonic_prost_build"].endswith("@0.14.6"):
            raise RuntimeError("reference generator must be tonic-prost-build 0.14.6")
        plugin = next(item["executable"] for item in artifacts
                      if item["target"]["name"] == "protoc-gen-pbrs" and item.get("executable"))
        artifact_dirs = {
            str(Path(path).parent)
            for item in artifacts
            for path in item.get("filenames", [])
            if path.endswith((".rlib", ".rmeta", ".so", ".dylib", ".dll"))
        }
        receipt = {"plugin": plugin, "dependencies": dependencies, "package_ids": package_ids,
                   "dependency_dirs": sorted({*(str(Path(path).parent) for path in dependencies.values()), *artifact_dirs})}
    (out / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    deps = {name: Path(path).resolve() for name, path in receipt["dependencies"].items()}
    plugin = Path(receipt["plugin"]).resolve()
    generator_library = Path(receipt.get("generator_library", deps["pbrs"])).resolve()
    dependency_args = [value for path in receipt["dependency_dirs"] for value in ["-L", "dependency=" + path]]
    inputs = {str(path): digest(path) for path in {plugin, generator_library, *deps.values(),
              *HERE.glob("*.py"), *HERE.glob("*.rs"), *HERE.glob("*.proto"), HERE / "services.txt"}}
    if args.baseline_plugin:
        inputs[str(args.baseline_plugin.resolve())] = digest(args.baseline_plugin)
    helper = out / "generator"
    code, _, error = run("reference-builder", ["rustc", "--edition=2024", "-C", "debuginfo=0",
        HERE / "generator.rs", "--extern", "pbrs=" + str(generator_library), "--extern",
        "tonic_prost_build=" + str(deps["tonic_prost_build"]), *dependency_args, "-o", helper])
    if code:
        raise RuntimeError("reference builder failed; see " + str(error))
    protoc = env.get("PROTOC", "protoc")
    symbols = [line.strip() for line in (HERE / "services.txt").read_text().splitlines() if line.strip()]
    template = (HERE / "case.proto").read_text()
    rows = []

    def compile_case(symbol, mode, variant="candidate", parameters=(), runtime="pbrs", codec=None, strict=False):
        label = variant + "-" + mode + "-" + symbol
        folder = out / label
        folder.mkdir()
        proto = folder / "case.proto"
        proto.write_text(template.replace("ServiceName", symbol))
        native = mode in ("kernel", "compat")
        request = "::pbrs_grpc::pb::Any" if native else "::protobuf_tonic::hello::HelloRequest"
        response = "::pbrs_grpc::pb::Any" if native else "::protobuf_tonic::hello::HelloReply"
        extras = ""
        if parameters:
            request, response = "crate::foreign::pbrs::Input", "crate::foreign::pbrs::Output"
            original_request = "::pbrs_grpc::pb::Any" if native else "::protobuf_tonic::hello::HelloRequest"
            original_response = "::pbrs_grpc::pb::Any" if native else "::protobuf_tonic::hello::HelloReply"
            codec_module = "pub mod protobuf_tonic { pub type Codec<E,D> = ::protobuf_tonic::ProtobufCodec<E,D>; }" if codec and codec.startswith("crate::") else ""
            extras = "mod foreign { pub mod pbrs { pub type Input = " + original_request + "; pub type Output = " + original_response + "; } " + codec_module + " }\n"
        options = ["stubs=" + mode, "no_reflect=true", "extern_path=.proof.rpcnames.Input=" + request,
                   "extern_path=.proof.rpcnames.Output=" + response, "runtime_crate=" + runtime]
        if strict:
            options.append("safe_message_defaults=true")
        options.extend(parameters)
        if codec:
            options.append("codec_path=" + codec)
        chosen = args.baseline_plugin if variant == "baseline" else plugin
        if mode == "prost":
            code, _, error = run("descriptor", [protoc, "-I" + str(folder), "--descriptor_set_out=" + str(folder / "case.fds"), proto], folder)
            if code:
                raise RuntimeError(error.read_text())
            code, _, error = run("generate", [helper, "prost", folder, folder / "case.fds"], folder)
            generated = folder / "case.pbrs_grpc.rs"
            extras = "#[derive(::prost::Message)] pub struct Input {}\n#[derive(::prost::Message)] pub struct Output {}\n"
        else:
            code, _, error = run("generate", [protoc, "-I" + str(folder), "--plugin=protoc-gen-pbrs=" + str(chosen),
                "--pbrs_out=" + str(folder), "--pbrs_opt=" + ",".join(options), proto], folder)
            generated = folder / "case.rs"
        if code:
            raise RuntimeError("schema generation failed: " + error.read_text())
        source = generated.read_text()
        if "Preserve user text: pbrs:: tonic:: protobuf_tonic:: std:: and core::." not in source and mode != "prost":
            raise RuntimeError("user documentation literal changed")
        if parameters and literal not in source:
            raise RuntimeError("user attribute literal changed")
        if parameters and (request not in source or response not in source):
            raise RuntimeError("explicit user type path changed")
        if codec and codec + "::<" not in source:
            raise RuntimeError("explicit codec path changed")
        if runtime.startswith("crate::"):
            extras += "#[allow(unused_imports, reason=\"runtime module reexports the public API\")] mod wire_runtime { pub use ::pbrs::*; }\n"
        prefix = "#![deny(warnings)]\n#![forbid(unsafe_code)]\n" if strict else ""
        main_rs = folder / "main.rs"
        main_rs.write_text(prefix + extras + 'include!("' + generated.name + '");\nfn main() {}\n')
        command = ["rustc", "--edition=2024", "--emit=metadata", main_rs, *dependency_args, "-o", folder / "consumer.rmeta"]
        if not strict:
            command += ["--cap-lints=allow"]
        for name, library in deps.items():
            if name != "tonic_prost_build":
                command += ["--extern", name + "=" + str(library)]
        command += ["--extern", "wire_alias=" + str(deps["pbrs"]), "--extern", "protobuf_wire_alias=" + str(deps["protobuf_tonic"])]
        code, _, error = run("compile", command, folder)
        row = {"symbol": symbol, "mode": mode, "variant": variant, "strict": strict,
               "runtime": runtime, "codec": codec, "exit_code": code, "schema_sha256": digest(proto),
               "generated_sha256": digest(generated), "main_sha256": digest(main_rs),
               "trait_names": re.findall(r"pub trait (\w+)", source),
               "diagnostics": [line for line in error.read_text().splitlines() if line.startswith("error")]}
        rows.append(row)
        return code

    reference_code = None
    modes = ("kernel", "tonic", "compat") + (() if args.skip_prost else ("prost",))
    if not args.only_bridge_aliases:
        if args.baseline_plugin:
            for symbol in symbols:
                for mode in ("kernel", "tonic", "compat"):
                    compile_case(symbol, mode, "baseline")
        for symbol in symbols:
            for mode in modes:
                compile_case(symbol, mode)
        literal = '#[doc = "pbrs:: tonic:: std:: user literal"]'
        attribute = "client_attribute=.proof.rpcnames.Echo=" + literal
        for number, runtime in enumerate(("pbrs", "wire_alias", "::wire_alias", "crate::wire_runtime")):
            for mode in ("kernel", "tonic", "compat"):
                codec = ("protobuf_wire_alias::ProtobufCodec", "::protobuf_wire_alias::ProtobufCodec",
                         "crate::foreign::protobuf_tonic::Codec", None)[number] if mode == "tonic" else None
                compile_case("Echo", mode, "aliases" + str(number), (attribute,), runtime, codec, True)
        reference = out / "reference-tonic"
        reference.mkdir()
        proto = reference / "case.proto"
        proto.write_text(template.replace("ServiceName", "Echo"))
        code, _, error = run("generate", [helper, "reference", reference, proto], reference)
        if code:
            raise RuntimeError("Tonic reference generation failed: " + error.read_text())
        main_rs = reference / "main.rs"
        main_rs.write_text('include!("proof.rpcnames.rs");\nfn main() {}\n')
        command = ["rustc", "--edition=2024", "--emit=metadata", "--cap-lints=allow", main_rs,
                   *dependency_args, "-o", reference / "consumer.rmeta"]
        for name, library in deps.items():
            if name != "tonic_prost_build":
                command += ["--extern", name + "=" + str(library)]
        reference_code, _, _ = run("compile", command, reference)
    bridge_rows = bridge_aliases.check(HERE, out, helper, run, deps, dependency_args, protoc)
    unchanged = all(digest(path) == expected for path, expected in inputs.items())
    failures = [row for row in rows if row["variant"] != "baseline" and row["exit_code"]]
    for row in rows:
        if row["variant"] == "baseline" and row["exit_code"] == 0:
            matching = next(candidate for candidate in rows if candidate["variant"] == "candidate"
                            and candidate["symbol"] == row["symbol"] and candidate["mode"] == row["mode"])
            if row["trait_names"] != matching["trait_names"]:
                raise RuntimeError("previously valid service identifier changed")
    report = {"symbols": symbols, "modes": modes, "rows": rows, "bridge_alias_rows": bridge_rows, "commands": records,
              "inputs_sha256": inputs, "inputs_unchanged": unchanged, "reference_exit_code": reference_code,
              "peak_private_bytes": peak, "minimum_free_bytes": minimum,
              "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
              "scope": "Generated-source metadata consumers, real runtime DTOs, all four method shapes. No network-call or performance result."}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"symbols": len(symbols), "matrix_modes": modes, "candidate_consumers": sum(row["variant"] == "candidate" for row in rows),
                      "alias_consumers": 0 if args.only_bridge_aliases else 12, "bridge_alias_consumers": len(bridge_rows), "bridge_alias_compile_failures": sum(row["exit_code"] != 0 for row in bridge_rows), "bridge_alias_preservation_failures": sum(bool(row["preservation_errors"]) for row in bridge_rows), "baseline_consumers": sum(row["variant"] == "baseline" for row in rows),
                      "failures": len(failures), "reference_exit_code": reference_code, "inputs_unchanged": unchanged}))
    if failures or any(row["exit_code"] or row["preservation_errors"] for row in bridge_rows) or reference_code or not unchanged:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
