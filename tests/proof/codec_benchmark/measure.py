#!/usr/bin/env python3
"""Collect diagnostic paired codec CPU timings and separate allocation counts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess

CASES = {
    "empty": 1_000_000,
    "scalars": 200_000,
    "blob_4kib": 30_000,
    "packed_256": 50_000,
    "nested_8": 50_000,
    "nested_64": 10_000,
    "rpc_sparse": 200_000,
    "rpc_mixed": 30_000,
}
OPS = ("parse_owned", "read_all")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_optional(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shipping", required=True, type=Path)
    parser.add_argument("--allocations", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--pairs", type=int, default=5)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--seed", type=int, default=271027)
    args = parser.parse_args()
    if args.pairs < 5 or args.samples < 1:
        parser.error("at least five pairs and one sample are required")
    args.output.mkdir(parents=True, exist_ok=False)
    shipping = args.shipping.resolve()
    allocations = args.allocations.resolve()
    allowed = sorted(os.sched_getaffinity(0))
    core = allowed[0]
    os.sched_setaffinity(0, {core})
    manifest = {
        "schema": "codec-proof-campaign/1",
        "qualification": "shared-host diagnostic, not performance leadership",
        "seed": args.seed,
        "pairs": args.pairs,
        "samples_per_process": args.samples,
        "cases": CASES,
        "operations": list(OPS),
        "cpu": core,
        "allowed_cpus_before_pin": allowed,
        "uname": platform.uname()._asdict(),
        "cpuinfo": read_optional("/proc/cpuinfo"),
        "cgroup_cpu_max": read_optional("/sys/fs/cgroup/cpu.max"),
        "cgroup_cpuset": read_optional("/sys/fs/cgroup/cpuset.cpus.effective"),
        "shipping_sha256": digest(shipping),
        "allocation_sha256": digest(allocations),
        "shipping_binary": str(shipping),
        "allocation_binary": str(allocations),
        "allocation_scope": "Rust alloc/alloc_zeroed/realloc requests during parse+drop; requested bytes, not RSS or retained memory",
        "cpu_scope": "CLOCK_PROCESS_CPUTIME_ID around parse+drop; allocation counters absent from shipping binary",
        "fixture_scope": "owned results from same borrowed input; parse_owned may defer nested materialization; read_all visits every populated field and every string/bytes payload byte",
        "dedicated_host": False,
    }
    rng = random.Random(args.seed)
    plan = []
    for pair in range(args.pairs):
        cells = [(case, op) for case in CASES for op in OPS]
        rng.shuffle(cells)
        for case, op in cells:
            order = ["pbrs", "prost"]
            rng.shuffle(order)
            for backend in order:
                plan.append({"pair": pair, "case": case, "operation": op, "backend": backend})
    manifest["plan"] = plan
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    fixtures = {}
    records = []
    with (args.output / "raw.jsonl").open("w") as raw:
        for index, entry in enumerate(plan):
            command = [str(shipping), entry["case"], entry["backend"], entry["operation"], str(CASES[entry["case"]]), str(args.samples)]
            proc = subprocess.run(command, text=True, capture_output=True)
            (args.output / f"{index:03d}.stderr").write_text(proc.stderr)
            record = {**entry, "command": command, "returncode": proc.returncode}
            if proc.returncode != 0:
                record["stdout"] = proc.stdout
                raw.write(json.dumps(record) + "\n")
                raw.flush()
                raise RuntimeError(f"probe failed: {command}: {proc.stderr}")
            value = json.loads(proc.stdout)
            assert value["semantic_prechecks"] and not value["allocation_instrumentation"]
            fingerprint = (value["wire_bytes"], value["wire_fnv1a64"], value["checksum"])
            assert fixtures.setdefault(entry["case"], fingerprint) == fingerprint
            record["result"] = value
            records.append(record)
            raw.write(json.dumps(record) + "\n")
            raw.flush()
            print(f"{index+1}/{len(plan)} {entry}", flush=True)
    allocation_records = []
    for case in CASES:
        for op in OPS:
            for backend in ("pbrs", "prost"):
                command = [str(allocations), case, backend, op, "1000", "1"]
                proc = subprocess.run(command, text=True, capture_output=True)
                assert proc.returncode == 0, (command, proc.stderr)
                result = json.loads(proc.stdout)
                assert result["semantic_prechecks"] and result["allocation_instrumentation"]
                assert fixtures[case] == (result["wire_bytes"], result["wire_fnv1a64"], result["checksum"])
                allocation_records.append({"case": case, "operation": op, "backend": backend, "command": command, "returncode": proc.returncode, "result": result})
    (args.output / "allocations.json").write_text(json.dumps(allocation_records, indent=2) + "\n")
    summary = []
    for case in CASES:
        for op in OPS:
            measurements = {}
            cpu = {backend: [] for backend in ("pbrs", "prost")}
            for record in records:
                if record["case"] == case and record["operation"] == op:
                    avg = statistics.mean(s["cpu_ns"] / s["iterations"] for s in record["result"]["samples"])
                    measurements[(record["pair"], record["backend"])] = avg
                    cpu[record["backend"]].append(avg)
            ratios = [measurements[(pair, "pbrs")] / measurements[(pair, "prost")] for pair in range(args.pairs)]
            bootstrap = sorted(statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(20_000))
            alloc = {}
            for backend in ("pbrs", "prost"):
                item = next(r for r in allocation_records if r["case"] == case and r["operation"] == op and r["backend"] == backend)
                sample = item["result"]["samples"][0]
                alloc[backend] = {name: sample[name] / sample["iterations"] for name in ("allocation_calls", "requested_bytes")}
            summary.append({"case": case, "operation": op, "cpu_ns_per_op_process_means": cpu,
                "cpu_ns_per_op_medians": {backend: statistics.median(values) for backend, values in cpu.items()},
                "paired_cpu_ratios_pbrs_over_prost": ratios,
                "median_paired_ratio": statistics.median(ratios),
                "diagnostic_percentile_bootstrap_95_interval": [bootstrap[499], bootstrap[19499]],
                "interval_warning": "five pairs on one shared host; not an independent-host or leadership confidence interval",
                "allocation_per_op": alloc,
                "wire_bytes": fixtures[case][0], "wire_fnv1a64": fixtures[case][1], "checksum": fixtures[case][2]})
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    (args.output / "complete.json").write_text(json.dumps({"complete": True, "processes": len(records), "allocation_processes": len(allocation_records), "semantic_prechecks": True}, indent=2) + "\n")


if __name__ == "__main__":
    main()
