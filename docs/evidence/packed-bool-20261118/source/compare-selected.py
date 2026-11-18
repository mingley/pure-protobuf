#!/usr/bin/env python3
"""Matched packed bool CPU screen and independent allocation counts."""

import json
import math
import os
import pathlib
import statistics
import subprocess
import sys

BIN_DIR = pathlib.Path(sys.argv[1])
OUTPUT = pathlib.Path(sys.argv[2])
VARIANTS = ("a", "b", "c")  # Baseline, candidate, independent baseline build.
CASES = {}
for shape in ("boolc", "boolo"):
    for count in (8, 4096):
        for mode in ("wirecold", "wirewarmcopy", "wirecoldget", "wirewarmget"):
            initial = 1_000_000 if "warm" in mode else 30_000 if count == 8 else 1_000
            CASES[f"{shape}.{count}.{mode}"] = (initial, 200 if count == 8 else 64)

CASES.update({
    "boolc.8.cold": (30_000, 200), "boolc.4096.cold": (1_000, 64),
    "boolc.8.wirewarm": (1_000_000, 200), "boolo.4096.wirewarm": (1_000_000, 64),
    "genc.8.cold": (30_000, 200), "genc.8.warm": (1_000_000, 200),
    "geno.4096.cold": (1_000, 64), "geno.4096.warm": (100_000, 64),
    "fx32.3.cold": (30_000, 200), "fx32.1024.warm": (1_000_000, 64),
    "varint.3.cold": (30_000, 200), "varint.1024.warm": (1_000_000, 64),
})
CASES = {k: CASES[k] for k in ('boolc.4096.wirecoldget', 'boolo.4096.wirecoldget', 'boolc.8.wirecoldget', 'boolo.8.wirecoldget', 'boolc.4096.wirewarmget', 'boolo.4096.wirewarmget', 'boolc.4096.wirecold', 'fx32.1024.warm')}
allowed = sorted(os.sched_getaffinity(0))
os.sched_setaffinity(0, {allowed[0]})
identities = {}
results = {
    "affinity_cpu": allowed[0],
    "allowed_cpus": allowed,
    "target_baseline_cpu_ns": 180_000_000,
    "cases": {},
    "wire_identity": identities,
}

def run(variant, case, iterations, count=False):
    binary = BIN_DIR / (variant + ("-count" if count else ""))
    proc = subprocess.run([str(binary), case, str(iterations)], text=True,
                          capture_output=True, timeout=180)
    if proc.returncode:
        raise RuntimeError((str(binary), case, proc.returncode, proc.stdout, proc.stderr))
    row = json.loads(proc.stdout)
    assert row["case"] == case and row["n"] == iterations, row
    if count:
        assert row["cpu_ns"] == 0 and row["wall_ns"] == 0, row
    else:
        assert row["cpu_ns"] > 0 and row["wall_ns"] > 0, row
        assert row["allocs"] == row["reallocs"] == row["bytes"] == 0, row
    per_op, remainder = divmod(row["checksum"], iterations)
    assert remainder == 0, row
    identity = [row["wire_len"], per_op]
    assert identities.setdefault(case, identity) == identity, row
    return row

def stats(values):
    return {"median": statistics.median(values), "range": [min(values), max(values)],
            "slower": sum(x > 1 for x in values)}

def rotation(block):
    start = block % len(VARIANTS)
    return VARIANTS[start:] + VARIANTS[:start]

def save():
    OUTPUT.write_text(json.dumps(results, indent=2) + "\n")

for case, (initial, count_n) in CASES.items():
    calibration = run("a", case, initial)
    calibrated = math.ceil(initial * results["target_baseline_cpu_ns"] / calibration["cpu_ns"])
    iterations = min(1_000_000_000, max(initial, calibrated))
    blocks = []
    for block in range(6):
        order = rotation(block)
        order += order[::-1]
        rows = [run(v, case, iterations) for v in order]
        by_variant = {v: [row["cpu_ns"] for name, row in zip(order, rows) if name == v]
                      for v in VARIANTS}
        blocks.append({
            "order": "".join(order), "rows": rows,
            "ba": sum(by_variant["b"]) / sum(by_variant["a"]),
            "ca": sum(by_variant["c"]) / sum(by_variant["a"]),
            "repeated": {v: samples[1] / samples[0] for v, samples in by_variant.items()},
        })
    report = {
        "iterations": iterations, "calibration": calibration, "blocks": blocks,
        "ba": stats([block["ba"] for block in blocks]),
        "ca": stats([block["ca"] for block in blocks]),
        "same_binary": {v: stats([block["repeated"][v] for block in blocks]) for v in VARIANTS},
        "min_process_cpu_ns": min(row["cpu_ns"] for block in blocks for row in block["rows"]),
    }
    results["cases"][case] = report
    save()
    print("CPU", case, "B/A", report["ba"], "C/A", report["ca"],
          "min_cpu_ms", round(report["min_process_cpu_ns"] / 1e6, 3), flush=True)

for case, (_, count_n) in CASES.items():
    records = []
    for block in range(3):
        order = rotation(block)
        rows = {v + "_n": run(v, case, count_n, count=True) for v in order}
        rows.update({v + "_2n": run(v, case, 2 * count_n, count=True) for v in order[::-1]})
        differences = {}
        for v in VARIANTS:
            differences[v] = {}
            for field in ("allocs", "reallocs", "bytes"):
                diff = rows[v + "_2n"][field] - rows[v + "_n"][field]
                amount, rem = divmod(diff, count_n)
                assert amount >= 0 and rem == 0, (v, case, field, diff)
                differences[v][field] = amount
        records.append({"order": "".join(order + order[::-1]), "rows": rows, "per_op": differences})
    results["cases"][case]["allocation_n"] = count_n
    results["cases"][case]["allocation_pairs"] = records
    save()
    assert all(r["per_op"] == records[0]["per_op"] for r in records), (case, records)
    assert records[0]["per_op"]["a"] == records[0]["per_op"]["c"], (case, records)
    diff = records[0]["per_op"]
    print("ALLOC", case, "A", diff["a"], "B", diff["b"], "C", diff["c"], flush=True)
