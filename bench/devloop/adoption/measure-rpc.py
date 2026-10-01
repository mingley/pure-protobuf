#!/usr/bin/env python3
"""Qualify and measure the complete registered SB-26d RPC matrix."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
PROFILES = ("native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost")
SHAPES = ("unary", "server_stream")


def write(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def run(command):
    return subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--qualify-only", action="store_true")
    parser.add_argument("--iters", type=int, default=16)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    if args.iters < 1 or args.repeats < 1:
        parser.error("iterations and repeats must be positive")
    binary = args.binary.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    inventory_path = ROOT / "bench/devloop/adoption/evidence/rpc-inventory.json"
    inventory = json.loads(inventory_path.read_text())
    assert len(inventory) == 64
    qualification = {row["specimen"]: row["qualification"] for row in inventory}
    expected = {
        f"rpc.adoption.{profile}.{specimen}.{shape}"
        for profile in PROFILES for specimen in qualification for shape in SHAPES
    }
    registered = {
        line.split()[0] for line in run([str(binary), "list"]).stdout.splitlines()
        if line.startswith("rpc.adoption.")
    }
    assert registered == expected, "RPC registry must contain all 512 cells"
    sources = [
        "bench/devloop/src/adoption_rpc.rs", "bench/devloop/src/main.rs",
        "bench/devloop/adoption/src/rpc.rs", "bench/devloop/adoption/src/corpus.rs",
        "bench/devloop/adoption/src/sparse_walks.rs", "bench/devloop/adoption/measure-rpc.py",
        "bench/devloop/Cargo.toml", "bench/devloop/Cargo.lock",
    ]
    record = {
        "tier": "instrumented dev-loop diagnostic",
        "source_commit": run(["git", "rev-parse", "HEAD"]).stdout.strip(),
        "source_sha256": {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in sources},
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "tool_versions": {name: run([name, "--version"]).stdout.strip() for name in ("rustc", "protoc", "valgrind")},
        "inventory_sha256": hashlib.sha256(inventory_path.read_bytes()).hexdigest(),
        "iters": args.iters, "repeats": args.repeats, "warmup": 100,
        "runtime_workers": 2,
        "work": "fully read parsed request cloned per RPC; handler reads every field and echoes entire message; client reads every field",
        "caps": "default stack caps; largest qualified specimen fits both defaults",
        "cells": {}, "reports": {},
    }
    # Qualify actual network output and reject byte-mismatched cells before
    # running any collector. A blocked cell must fail its child preflight.
    for profile in PROFILES:
        for shape in SHAPES:
            for specimen, oracle in qualification.items():
                cell = f"rpc.adoption.{profile}.{specimen}.{shape}"
                command = [str(binary), "run-cell", cell, "--iters", "1", "--warmup", "0"]
                if not oracle["equal_wire_work"]:
                    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
                    assert result.returncode != 0 and "RPC encoded bytes must agree" in result.stderr
                    record["cells"][cell] = {
                        "status": "not_run",
                        "reason": "pbrs map entries encode four extra bytes; equal-wire qualification fails",
                        "native_bytes": oracle["native_request_bytes"],
                        "prost_bytes": oracle["request_bytes"],
                        "preflight_exit_code": result.returncode,
                    }
                else:
                    result = run(command)
                    lines = result.stderr.splitlines()
                    network = json.loads(next(line.removeprefix("__QUALIFICATION__ ") for line in lines if line.startswith("__QUALIFICATION__ ")))
                    assert network == {
                        "request_bytes": oracle["request_bytes"],
                        "response_bytes": oracle["response_bytes_per_message"],
                        "checksum": oracle["request_read_checksum"], "stream_replies": 4,
                    }
                    assert sum(line.startswith("__CHILD__ ") for line in lines) == 1
                    record["cells"][cell] = {"status": "qualified", "network": network, "measurement_status": "not_run"}
            write(out / "rpc-measurement.json", record)
            print(f"Qualified {profile} {shape}", flush=True)
    if args.qualify_only:
        return
    reports = []
    for profile in PROFILES:
        for shape in SHAPES:
            ids = [cell for cell, state in record["cells"].items()
                   if cell.startswith(f"rpc.adoption.{profile}.") and cell.endswith(f".{shape}")
                   and state["status"] == "qualified"]
            assert len(ids) == 61
            path = out / f"{profile}-{shape}.json"
            print(f"Measuring {profile} {shape}: {len(ids)} cells", flush=True)
            run([str(binary), "run", "--cells", ",".join(ids), "--iters", str(args.iters),
                 "--repeats", str(args.repeats), "--out", str(path)])
            report = json.loads(path.read_text())
            assert {cell["id"] for cell in report["cells"]} == set(ids)
            for cell in report["cells"]:
                assert cell["iters"] == args.iters and cell["repeats"] == args.repeats
                assert cell["allocs"]["status"] == "measured"
                assert cell["alloc_bytes"]["status"] == "measured"
                record["cells"][cell["id"]]["status"] = "measured"
                record["cells"][cell["id"]]["measurement_status"] = "measured"
            record["reports"][path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
            reports.append(report)
            write(out / "rpc-measurement.json", record)
    combined = {**reports[0], "cells": [cell for report in reports for cell in report["cells"]]}
    assert len(combined["cells"]) == 488
    comparisons = []
    index = {cell["id"]: cell for cell in combined["cells"]}
    for specimen, oracle in qualification.items():
        if not oracle["equal_wire_work"]:
            continue
        for shape in SHAPES:
            base = index[f"rpc.adoption.tonic_prost.{specimen}.{shape}"]
            for profile in PROFILES[:-1]:
                candidate = index[f"rpc.adoption.{profile}.{specimen}.{shape}"]
                ratios = {}
                for metric in ("instructions", "allocs", "alloc_bytes"):
                    a, b = candidate[metric], base[metric]
                    if a["status"] == b["status"] == "measured" and b["data"]["value"] > 0:
                        value = a["data"]["value"] / b["data"]["value"]
                        ratios[metric] = {"ratio": value, "loss": value > 1}
                    else:
                        ratios[metric] = {"status": "not_run", "reason": "ratio requires two available positive metrics"}
                comparisons.append({"specimen": specimen, "shape": shape, "profile": profile, "ratios": ratios})
    record["comparisons"] = comparisons
    record["limits"] = [
        "24 map cells remain blocked by unequal encoded byte counts",
        "copy-counts instrumentation is enabled; counters do not account for all prost copies",
        "one shared cloud host, fixed-order repeats; no claim-grade or native latency claim",
        "TC-29/30 unmodified tonic-codegen transport adapters are not measured",
    ]
    write(out / "rpc-baseline.json", combined)
    write(out / "rpc-measurement.json", record)
    print("Retained 488 measured cells, 24 blocked cells and every paired loss", flush=True)


if __name__ == "__main__":
    main()
