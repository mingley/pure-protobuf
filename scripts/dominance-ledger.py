#!/usr/bin/env python3
"""Compare paired N/2N endpoint captures and retain losses and missing metrics."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
METRICS = {"instructions": ("instruction_totals", None),
           "allocations": ("allocation_totals", "allocations"),
           "requested_bytes": ("allocation_totals", "requested_bytes")}
REQUIRED_METRICS = [*METRICS, "task_wakeups", "context_switches", "syscalls"]
SIDES = {"client": [("native", "pbrs", "tonic", "prost"), ("native", "prost", "tonic", "prost")],
         "server": [("tonic", "prost", "native", "pbrs"), ("tonic", "prost", "native", "prost")]}
REFERENCE = ("tonic", "prost", "tonic", "prost")


def key(cell):
    return (tuple(cell["pair"]), cell["shape"], cell["payload_bytes"], cell["tls"],
            cell["compression"], cell["connections"], cell["in_flight"], cell["repeat"])


def load(path):
    manifest = json.loads((path / "report.json").read_text())
    if manifest.get("schema") != "pbrs.load-smoke.v2" or not manifest.get("passed") or not manifest.get("binary_unchanged"):
        raise ValueError("capture did not pass all endpoint/accounting checks")
    n = manifest.get("rpc_count")
    if type(n) is not int or n <= 0:
        raise ValueError("a fixed positive RPC count is required")
    rows = {}
    for run in manifest["runs"]:
        row = json.loads((path / run["path"]).read_text())
        if not row.get("passed") or row["metrics"]["successful_rpcs"] != n:
            raise ValueError("failed or incomplete fixed-count endpoint capture")
        identity = key(row["cell"])
        if identity in rows:
            raise ValueError("duplicate workload/repeat")
        rows[identity] = row
    if set(rows) != {key(cell) for cell in manifest["cells"]}:
        raise ValueError("capture rows do not match the declared matrix")
    return manifest, rows


def differential(small, large, side, metric, n):
    section, field = METRICS[metric]
    try:
        before, after = small[section][side], large[section][side]
        if field is not None:
            before, after = before[field], after[field]
    except KeyError:
        return None
    if any(type(value) is not int or value < 0 for value in (before, after)):
        raise ValueError("invalid endpoint counter")
    if after <= before:
        return None  # Retain unresolved setup/noise instead of inventing a win.
    return (after - before) / n


def owner(side, shape, metric):
    if shape == "server_stream" and metric == "instructions":
        return "SV-09"
    if metric in ("allocations", "requested_bytes"):
        return "CL-08"
    return "CL-09" if side == "client" else "SV-09"


def compare(small_path, large_path):
    a, small = load(small_path)
    b, large = load(large_path)
    if b["rpc_count"] != 2 * a["rpc_count"] or set(small) != set(large):
        raise ValueError("matched N/2N counts and workload/repeat coverage are required")
    for field in ["binary_sha256", "head", "dirty", "callgrind", "allocation_counts", "host", "cpu_affinity"]:
        if a.get(field) != b.get(field):
            raise ValueError(f"capture pin/settings mismatch: {field}")
    valid_owners = {t["id"] for path in ["docs/plan/tasks.json", "docs/plan/world-class/tasks.json"]
                    for t in json.loads((ROOT / path).read_text())["tasks"]}
    rows = []
    for identity, first in small.items():
        pair, shape, _, _, _, _, _, repeat = identity
        for side, pairs in SIDES.items():
            if pair not in pairs:
                continue
            reference_key = (REFERENCE, *identity[1:])
            if reference_key not in small:
                raise ValueError("missing same-peer tonic/prost reference")
            for metric in REQUIRED_METRICS:
                owned = owner(side, shape, metric)
                if owned not in valid_owners:
                    raise ValueError(f"loss owner does not exist: {owned}")
                native = reference = None
                if metric in METRICS:
                    native = differential(first, large[identity], side, metric, a["rpc_count"])
                    reference = differential(small[reference_key], large[reference_key], side, metric, a["rpc_count"])
                ratio = native / reference if native is not None and reference is not None else None
                # Exact allocation floors; the existing 2% RPC instruction band.
                band = .02 if metric == "instructions" else 0
                disposition = "not_run" if ratio is None else "loss" if ratio > 1 + band else "win" if ratio < 1 - band else "tie"
                rows.append({"cell": first["cell"], "side": side, "metric": metric,
                             "native_per_rpc": native, "tonic_per_rpc": reference,
                             "ratio": ratio, "disposition": disposition, "owner": owned,
                             "repeat": repeat})
    if not rows:
        raise ValueError("no same-peer client/server comparisons")
    missing = sorted({row["metric"] for row in rows if row["disposition"] == "not_run"})
    return {"schema": "pbrs.dominance-diagnostic.v1", "source": a["head"], "binary_sha256": a["binary_sha256"],
            "capture_paths": [str(small_path), str(large_path)], "rpc_counts": [a["rpc_count"], b["rpc_count"]],
            "qualified": False, "rows": rows, "missing_metrics": missing,
            "remaining": ["complete read-all corpus, saturation and cold/idle coverage",
                          "dedicated x86_64/arm64 statistics and headroom proof",
                          "full original primary/control regression qualification"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--small", type=Path, required=True)
    parser.add_argument("--large", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-partial", action="store_true", help="explicit diagnostic; never closes SB-27")
    args = parser.parse_args()
    try:
        report = compare(args.small.resolve(), args.large.resolve())
        args.output.parent.mkdir(parents=True, exist_ok=True)
        with args.output.open("x") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        print(json.dumps({"rows": len(report["rows"]), "losses": sum(r["disposition"] == "loss" for r in report["rows"]),
                          "missing_metrics": report["missing_metrics"], "qualified": False}))
        return 0 if args.allow_partial else 1
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"dominance-ledger: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
