#!/usr/bin/env python3
"""Read-only audit of SB-26d raw RPC reports, coverage, source and ratios."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
PROFILES = ("native_pbrs", "native_prost", "tonic_pbrs", "tonic_prost")
SHAPES = ("unary", "server_stream")


def raw_report(directory, name):
    # Collector output is flat; checked-in artifacts keep the eight parts in
    # rpc-raw. Accept either layout, but reject ambiguous duplicate reports.
    paths = [p for p in (directory / name, directory / "rpc-raw" / name) if p.is_file()]
    assert len(paths) == 1, f"expected one raw report: {name}"
    return paths[0].read_bytes()


def audit(directory):
    record = json.loads((directory / "rpc-measurement.json").read_text())
    inventory_path = ROOT / "bench/devloop/adoption/evidence/rpc-inventory.json"
    assert hashlib.sha256(inventory_path.read_bytes()).hexdigest() == record["inventory_sha256"]
    inventory = {row["specimen"]: row["qualification"] for row in json.loads(inventory_path.read_text())}
    assert len(inventory) == 64
    for path, checksum in record["source_sha256"].items():
        source = subprocess.run(["git", "show", f"{record['source_commit']}:{path}"],
                                cwd=ROOT, check=True, capture_output=True).stdout
        assert hashlib.sha256(source).hexdigest() == checksum, f"source binding: {path}"
    expected = {f"rpc.adoption.{p}.{s}.{k}" for p in PROFILES for s in inventory for k in SHAPES}
    assert set(record["cells"]) == expected and len(expected) == 512
    blocked = {f"rpc.adoption.{p}.{s}.{k}" for p in PROFILES for s, q in inventory.items()
               if not q["equal_wire_work"] for k in SHAPES}
    assert len(blocked) == 24
    for cell in blocked:
        state = record["cells"][cell]
        assert state["status"] == "not_run" and state["preflight_exit_code"] != 0
        assert state["native_bytes"] == state["prost_bytes"] + 4
    combined = json.loads((directory / "rpc-baseline.json").read_text())
    rows = {row["id"]: row for row in combined["cells"]}
    assert len(rows) == len(combined["cells"]) == 488
    assert set(rows) == expected - blocked
    seen = set()
    expected_reports = {f"{p}-{k}.json" for p in PROFILES for k in SHAPES}
    assert set(record["reports"]) == expected_reports
    for name, checksum in record["reports"].items():
        raw = raw_report(directory, name)
        assert hashlib.sha256(raw).hexdigest() == checksum, f"raw report: {name}"
        report = json.loads(raw)
        assert report["schema"] == combined["schema"] == "devloop/1"
        assert report["host"] == combined["host"]
        if name == "native_pbrs-unary.json":
            assert {k: v for k, v in report.items() if k != "cells"} == {
                k: v for k, v in combined.items() if k != "cells"
            }
        full_commit = subprocess.run(["git", "rev-parse", report["devloop_commit"]],
                                     cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()
        # Evidence-only checkpoints can advance main while the fixed binary
        # collects later batches. Verify unchanged source, not equal git IDs.
        for path, source_hash in record["source_sha256"].items():
            content = subprocess.run(["git", "show", f"{full_commit}:{path}"],
                                     cwd=ROOT, check=True, capture_output=True).stdout
            assert hashlib.sha256(content).hexdigest() == source_hash
        changed = subprocess.run(["git", "diff", "--name-only", record["source_commit"], full_commit,
                                  "--", "src", "pbrs-grpc", "protobuf-tonic", "Cargo.toml", "Cargo.lock",
                                  "build.rs", "bench/devloop/src", "bench/devloop/build.rs",
                                  "bench/devloop/Cargo.toml", "bench/devloop/Cargo.lock",
                                  "bench/devloop/adoption/src", "bench/devloop/adoption/proto",
                                  "bench/devloop/adoption/build.rs", "bench/devloop/adoption/Cargo.toml",
                                  "bench/devloop/adoption/Cargo.lock"], cwd=ROOT, check=True,
                                 capture_output=True, text=True).stdout.strip()
        assert not changed, f"runtime source changed during collection: {changed}"
        assert len(report["cells"]) == 61
        profile, shape = name.removesuffix(".json").split("-")
        for row in report["cells"]:
            cell = row["id"]
            assert cell not in seen and rows[cell] == row
            assert cell.startswith(f"rpc.adoption.{profile}.") and cell.endswith(f".{shape}")
            assert row["kind"] == "rpc" and row["codec"] == profile
            seen.add(cell)
            assert row["iters"] == record["iters"] and row["repeats"] == record["repeats"]
            assert record["cells"][cell]["status"] == "measured"
            assert record["cells"][cell]["measurement_status"] == "measured"
            specimen = cell.removeprefix(f"rpc.adoption.{profile}.").removesuffix(f".{shape}")
            oracle = inventory[specimen]
            assert record["cells"][cell]["network"] == {
                "request_bytes": oracle["request_bytes"],
                "response_bytes": oracle["response_bytes_per_message"],
                "checksum": oracle["request_read_checksum"], "stream_replies": 4,
            }
            for metric in ("allocs", "alloc_bytes", "instructions", "syscalls", "locks"):
                assert row[metric]["status"] in ("measured", "not_run")
            assert row["allocs"]["status"] == row["alloc_bytes"]["status"] == "measured"
            if row["instructions"]["status"] == "measured":
                assert row["instruction_method"] in (
                    "differential_callgrind_2n_minus_n", "differential_perf_2n_minus_n"
                )
    assert seen == set(rows)
    assert record["warmup"] == 100 and record["runtime_workers"] == 2
    assert 1 <= record["report_parallelism"] <= 4
    assert record["socket_defaults"] == {
        "tcp_nodelay": True, "tcp_keepalive": "unset",
        "tonic_server_incoming": "TcpIncoming.with_nodelay(Some(true))",
    }
    assert record["allocation_summary"] == "median of exact per-run counts per RPC"
    expected_pairs = {(s, k, p) for s, q in inventory.items() if q["equal_wire_work"]
                      for k in SHAPES for p in PROFILES[:-1]}
    pairs = {(c["specimen"], c["shape"], c["profile"]): c for c in record["comparisons"]}
    assert len(pairs) == len(record["comparisons"]) == 366 and set(pairs) == expected_pairs
    for (specimen, shape, profile), comparison in pairs.items():
        candidate = rows[f"rpc.adoption.{profile}.{specimen}.{shape}"]
        baseline = rows[f"rpc.adoption.tonic_prost.{specimen}.{shape}"]
        for metric in ("instructions", "allocs", "alloc_bytes"):
            a, b = candidate[metric], baseline[metric]
            ratio = comparison["ratios"][metric]
            if a["status"] == b["status"] == "measured" and b["data"]["value"] > 0:
                expected_ratio = a["data"]["value"] / b["data"]["value"]
                assert ratio == {"ratio": expected_ratio, "loss": expected_ratio > 1}
            else:
                assert ratio["status"] == "not_run"
    return len(rows), len(blocked), len(pairs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    measured, blocked, pairs = audit(args.directory)
    print(f"RPC evidence OK: {measured} measured, {blocked} blocked, {pairs} comparisons; hashes/source/ratios verified")


if __name__ == "__main__":
    main()
