#!/usr/bin/env python3
"""Balanced TLS config construction CPU and separate allocation controls."""
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys

base, candidate, alloc_base, alloc_candidate, fixture, out = map(Path, sys.argv[1:])
normal = {"base": base, "candidate": candidate}
counting = {"base": alloc_base, "candidate": alloc_candidate}
available = sorted(os.sched_getaffinity(0))
assert available
cpu = available[0]
os.sched_setaffinity(0, {cpu})
topology_dir = Path("/sys/devices/system/cpu") / f"cpu{cpu}" / "topology"
topology = {key: int((topology_dir / key).read_text())
            for key in ("physical_package_id", "core_id")}
out.mkdir(parents=True, exist_ok=False)
cases = ("client-ca",)

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def run(binary, case, iterations):
    p = subprocess.run([str(binary), case, str(iterations), str(fixture)],
                       capture_output=True, text=True, timeout=180)
    assert p.returncode == 0, (binary.name, case, p.returncode, p.stdout[-1000:], p.stderr[-2000:])
    assert p.stdout.startswith("TLS_CONSTRUCT_V1 "), p.stdout
    values = dict(re.findall(r"(\w[\w-]*)=([^\s]+)", p.stdout))
    assert values["case"] == case
    for name in ("iters", "cpu_ns", "wall_ns", "alloc_calls", "requested_bytes", "parallelism"):
        values[name] = int(values[name])
    assert values["iters"] == iterations and values["parallelism"] == 1
    assert values["cpu_ns"] > 0
    return values

rows, summaries, allocations = [], [], []
for case in cases:
    iterations = 65536
    for repeat in range(3):
        for variant in ("base", "candidate"):
            sample = run(counting[variant], case, 16)
            assert sample["alloc_calls"] > 0 and sample["requested_bytes"] > 0
            allocations.append({"case": case, "variant": variant, "repeat": repeat,
                                "sample": sample})
    group = []
    for block in range(12):
        order = ("base", "candidate") if block % 2 == 0 else ("candidate", "base")
        samples = {variant: run(normal[variant], case, iterations) for variant in order}
        controls = {}
        for variant in order:
            a = run(normal[variant], case, iterations)
            b = run(normal[variant], case, iterations)
            controls[variant] = {"a": a, "b": b, "ratio": b["cpu_ns"] / a["cpu_ns"]}
        row = {"case": case, "block": block, "order": order, "iterations": iterations,
               "samples": samples, "controls": controls,
               "ratio": samples["candidate"]["cpu_ns"] / samples["base"]["cpu_ns"]}
        rows.append(row)
        group.append(row)
        print("TLS_CONSTRUCT_PAIR_V1 " + json.dumps(row, sort_keys=True), flush=True)
    ratios = [r["ratio"] for r in group]
    summary = {"case": case, "iterations": iterations,
               "median": statistics.median(ratios),
               "range": [min(ratios), max(ratios)], "slower": sum(x > 1 for x in ratios),
               "controls": {v: [min(r["controls"][v]["ratio"] for r in group),
                                  max(r["controls"][v]["ratio"] for r in group)]
                            for v in ("base", "candidate")},
               "cpu_ns_per_op": {v: [min(r["samples"][v]["cpu_ns"] / iterations for r in group),
                                      max(r["samples"][v]["cpu_ns"] / iterations for r in group)]
                                 for v in ("base", "candidate")}}
    summaries.append(summary)
    print("TLS_CONSTRUCT_SUMMARY_V1 " + json.dumps(summary, sort_keys=True), flush=True)

result = {"scope": "fresh configuration and drop only; test PEM files read and identities parsed before measurement; CA parsing remains inside constructors; no handshake or RPC; process CPU; one pinned guest CPU; separate counting allocator; shared host",
          "source": {"base": "f73b85d067ea861fe36f23536153e4c5add3137f",
                     "candidate": "171388257f15d261a1667799f40a38edc3bbeffe"},
          "cpu": cpu, "topology": topology,
          "fixtures": {name: sha(fixture / name) for name in
                       ("ca.crt", "server.crt", "server.key", "client.crt", "client.key")},
          "binary_sha256": {v: sha(p) for v, p in normal.items()},
          "allocation_binary_sha256": {v: sha(p) for v, p in counting.items()},
          "rows": rows, "summaries": summaries, "allocations": allocations}
(out / "results.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
print("TLS_CONSTRUCT_COMPLETE_V1 cases=1 blocks=12 allocation_samples=6", flush=True)
