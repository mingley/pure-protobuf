#!/usr/bin/env python3
"""Build overlay for the pinned grpc-go benchmark client; transport stays upstream."""

import argparse
import hashlib
import json
import shutil
from pathlib import Path


PIN = "dd51b1c90aaf9b7ee0b07b1d14fa8e3a89132bef"
CLIENT_SHA256 = "73ae33ebad3f1bf2f5e0196bb28b06d96618594dfe650b2de0ef6e410be5f464"
MODULE_TREE_SHA256 = "743826376a0fbdca8e0550dd587c83281b597a7b637a187b7c5b7e3c5e556266"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_digest(root):
    digest = hashlib.sha256()
    for path in sorted(root.rglob("*")):
        if path.is_file():
            digest.update(path.relative_to(root).as_posix().encode() + b"\0")
            digest.update(path.read_bytes())
    return digest.hexdigest()


def prepare(module: Path, output: Path):
    source = module / "benchmark/worker/benchmark_client.go"
    if digest(source) != CLIENT_SHA256:
        raise ValueError("pinned grpc-go benchmark_client.go source hash drift")
    if tree_digest(module) != MODULE_TREE_SHA256:
        raise ValueError("pinned grpc-go module source tree hash drift")
    output.mkdir(parents=True, exist_ok=True)
    local_module = output / "grpc-go-source"
    if local_module.exists():
        if tree_digest(local_module) != tree_digest(module):
            raise ValueError("pinned grpc-go local module copy drift")
    else:
        shutil.copytree(module, local_module)
    text = source.read_text()
    replacements = (
        ("type benchmarkClient struct {", "type benchmarkClient struct {\n\tqpsOracle *qpsAccounting"),
        ("\tswitch config.RpcType {", "\tif poissonLambda != nil {\n"
         "\t\tbc.qpsOracle = newQpsAccounting(bc.histogramOptions, config)\n"
         "\t\treturn bc.qpsOracle.start(ctx, conns, rpcCountPerConn, config, payloadReqSize, payloadRespSize)\n\t}\n\tswitch config.RpcType {"),
        ("func (bc *benchmarkClient) getStats(reset bool) *testpb.ClientStats {",
         "func (bc *benchmarkClient) getStats(reset bool) *testpb.ClientStats {\n"
         "\tif bc.qpsOracle != nil { return bc.qpsOracle.snapshot(reset) }"),
    )
    for original, replacement in replacements:
        if text.count(original) != 1:
            raise ValueError("pinned grpc-go benchmark client overlay anchor drift")
        text = text.replace(original, replacement)
    patched = output / "benchmark_client.go"
    patched.write_text(text)
    helper = Path(__file__).with_name("qps-go-accounting.go").resolve()
    overlay = {"Replace": {str(local_module.resolve() / "benchmark/worker/benchmark_client.go"): str(patched.resolve()),
                           str(local_module.resolve() / "benchmark/worker/qps_accounting.go"): str(helper)}}
    overlay_path = output / "overlay.json"
    overlay_path.write_text(json.dumps(overlay, indent=2) + "\n")
    manifest = {"schema_version": 1, "qualification": "benchmark harness overlay",
                "peer": "overlaid pinned grpc-go benchmark client", "upstream_pin": PIN,
                "upstream_client_sha256": CLIENT_SHA256, "patched_client_sha256": digest(patched),
                "accounting_source_sha256": digest(helper), "overlay_sha256": digest(overlay_path),
                "upstream_module_tree_sha256": tree_digest(local_module),
                "arrival_schedule": "aggregate SplitMix64 Poisson; seed 0x5eed20260918",
                "slot_limit": "client_channels * outstanding_rpcs_per_channel",
                "closed_loop": "original upstream paths", "claim_eligible": False}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def verify(output: Path, binary: Path):
    manifest = json.loads((output / "manifest.json").read_text())
    helper = Path(__file__).with_name("qps-go-accounting.go")
    expected = {"upstream_pin": PIN, "upstream_client_sha256": CLIENT_SHA256,
                "accounting_source_sha256": digest(helper),
                "upstream_module_tree_sha256": MODULE_TREE_SHA256,
                "dependencies_mod_sha256": digest(output / "worker.mod"),
                "dependencies_sum_sha256": digest(output / "worker.sum"),
                "patched_client_sha256": digest(output / "benchmark_client.go"),
                "overlay_sha256": digest(output / "overlay.json"), "binary_sha256": digest(binary)}
    if (tree_digest(output / "grpc-go-source") != MODULE_TREE_SHA256
            or any(manifest.get(key) != value for key, value in expected.items())):
        raise ValueError("Go benchmark overlay source/binary provenance drift; rebuild required")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("module", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--record-binary", type=Path)
    parser.add_argument("--verify-binary", type=Path)
    args = parser.parse_args()
    try:
        if args.verify_binary:
            verify(args.output, args.verify_binary)
        elif args.record_binary:
            manifest_path = args.output / "manifest.json"
            manifest = json.loads(manifest_path.read_text())
            manifest["binary_sha256"] = digest(args.record_binary)
            manifest["dependencies_mod_sha256"] = digest(args.output / "worker.mod")
            manifest["dependencies_sum_sha256"] = digest(args.output / "worker.sum")
            manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
            verify(args.output, args.record_binary)
        else:
            prepare(args.module, args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"FAIL: {error}\n")


if __name__ == "__main__":
    main()
