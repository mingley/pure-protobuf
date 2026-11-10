#!/usr/bin/env python3
"""Balanced warmed plaintext/TLS unary RPC CPU and allocation control."""
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys

base, candidate, base_alloc, candidate_alloc, base_witness, candidate_witness, output = map(Path, sys.argv[1:])
available = sorted(os.sched_getaffinity(0))
def topology(cpu):
    root = Path("/sys/devices/system/cpu") / ("cpu" + str(cpu)) / "topology"
    return (int((root / "physical_package_id").read_text()),
            int((root / "core_id").read_text()))
cores = {}
for cpu in available:
    cores.setdefault(topology(cpu), []).append(cpu)
assert len(cores) >= 2, cores
selected = [x[0] for x in list(cores.values())[:2]]
os.sched_setaffinity(0, set(selected))
binaries = {"base": base, "candidate": candidate}
alloc_binaries = {"base": base_alloc, "candidate": candidate_alloc}
witness_binaries = {"base": base_witness, "candidate": candidate_witness}
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def run(binary, mode, count, width, witness=False, allocation=False):
    p = subprocess.run([str(binary), mode, str(count), str(width)],
                       capture_output=True, text=True, timeout=180)
    if p.returncode:
        raise RuntimeError((str(binary), mode, count, width, p.returncode, p.stderr[-4000:]))
    lines = [line for line in p.stdout.splitlines() if line.startswith("{")]
    assert len(lines) == 1, lines
    result = json.loads(lines[0])
    assert result["schema"] == "tls-rpc-control/1"
    assert (result["mode"], result["requests"], result["width"]) == (mode, count, width)
    assert result["cpu_ns"] > 0 and result["wall_ns"] > 0
    assert result["warmup"] == 256 and result["response_validated"] is True
    assert result["tls_verified"] == (mode == "tls")
    assert result["scheme_witness"] is witness
    assert result["allocation_instrumented"] is allocation
    if allocation:
        assert result["allocations"] > 0 and result["requested_bytes"] > 0
    else:
        assert result["allocations"] == result["requested_bytes"] == 0
    return result
output.mkdir(parents=True)
witnesses = []
for mode in ("plain", "tls"):
    for variant, binary in witness_binaries.items():
        r = run(binary, mode, 64, 8, witness=True)
        witnesses.append({"variant": variant, "result": r})
        print("TLS_RPC_WITNESS", json.dumps(witnesses[-1], sort_keys=True), flush=True)
rows, allocations, summaries = [], [], []
for mode in ("plain", "tls"):
    for width in (1, 32):
        pilot = run(base, mode, 512, width)
        target = math.ceil(750_000_000 * 512 / pilot["cpu_ns"])
        count = min(50000, max(2048, math.ceil(target / width) * width))
        for variant, binary in alloc_binaries.items():
            lo = run(binary, mode, 1024, width, allocation=True)
            hi = run(binary, mode, 2048, width, allocation=True)
            delta_calls = (hi["allocations"] - lo["allocations"]) / 1024
            delta_bytes = (hi["requested_bytes"] - lo["requested_bytes"]) / 1024
            assert delta_calls > 0 and delta_bytes > 0
            allocations.append({"mode": mode, "width": width, "variant": variant,
                                "low": lo, "high": hi,
                                "delta_calls_per_rpc": delta_calls,
                                "delta_requested_bytes_per_rpc": delta_bytes})
        group = []
        for block in range(8):
            order = ("base", "candidate") if block % 2 == 0 else ("candidate", "base")
            samples = {v: run(binaries[v], mode, count, width) for v in order}
            controls = {}
            for v in order:
                a, b = run(binaries[v], mode, count, width), run(binaries[v], mode, count, width)
                controls[v] = {"a": a, "b": b, "ratio": b["cpu_ns"] / a["cpu_ns"]}
            row = {"mode": mode, "width": width, "block": block,
                   "order": order, "count": count, "pilot": pilot,
                   "samples": samples, "controls": controls,
                   "ratio": samples["candidate"]["cpu_ns"] / samples["base"]["cpu_ns"]}
            rows.append(row)
            group.append(row)
            (output / "partial.json").write_text(json.dumps({
                "rows": rows, "allocations": allocations, "witnesses": witnesses,
                "summaries": summaries}, indent=2, sort_keys=True) + "\n")
            print("TLS_RPC_PAIR", json.dumps(row, sort_keys=True), flush=True)
        ratios = [r["ratio"] for r in group]
        summary = {"mode": mode, "width": width, "requests": count,
                   "median": statistics.median(ratios), "range": [min(ratios), max(ratios)],
                   "slower": sum(r > 1 for r in ratios),
                   "control_ranges": {v: [min(r["controls"][v]["ratio"] for r in group),
                                          max(r["controls"][v]["ratio"] for r in group)]
                                      for v in ("base", "candidate")}}
        summaries.append(summary)
        print("TLS_RPC_SUMMARY", json.dumps(summary, sort_keys=True), flush=True)
out = {
    "schema": "tls-portability-rpc-screen/1",
    "baseline": "f73b85d067ea861fe36f23536153e4c5add3137f",
    "candidate": "171388257f15d261a1667799f40a38edc3bbeffe",
    "scope": "Combined native client and server process CPU; one warmed loopback connection; TLS handshake and configuration outside timed region; two Tokio workers pinned to two guest-visible cores. Allocation samples instrument process-wide requests and are not RSS.",
    "available_cpus": available, "cpus": selected,
    "topology": {str(c): topology(c) for c in selected},
    "binaries": {v: sha(p) for v, p in binaries.items()},
    "allocation_binaries": {v: sha(p) for v, p in alloc_binaries.items()},
    "witness_binaries": {v: sha(p) for v, p in witness_binaries.items()},
    "witnesses": witnesses, "rows": rows, "allocations": allocations, "summaries": summaries,
}
(output / "results.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
