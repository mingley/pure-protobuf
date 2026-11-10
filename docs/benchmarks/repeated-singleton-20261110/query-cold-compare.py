#!/usr/bin/env python3
import json, math, os, pathlib, statistics, subprocess, sys

a_cpu, b_cpu, a_count, b_count, dest = sys.argv[1:]
cases = {
    "query_empty": (200000, 200),
    "query_operator_only": (100000, 200),
    "query_cold_empty_proxy": (50000, 200),
    "query_one_predicate": (10000, 200),
    "query_mixed_10": (10000, 100),
    "query3_fresh": (2000, 100),
    "query4_construct": (2000, 100),
    "query4_fresh": (2000, 100),
    "query8_fresh": (1000, 100),
    "query4_mutate": (2000, 100),
    "query4_warm": (10000, 100),
    "query4_clone_encode": (2000, 100),
    "query4_decode": (2000, 100),
    "query4_decode_read": (2000, 100),
    "query4_prost_fresh": (2000, 100),
    "repeat_eight_collect": (100000, 200),
    "repeat_large_many": (10000, 100),
}
allowed = sorted(os.sched_getaffinity(0))
os.sched_setaffinity(0, {allowed[0]})
identity = None

def run(binary, case, n):
    global identity
    result = subprocess.run([binary, case, str(n)], check=True, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180)
    row = json.loads(result.stdout.strip().splitlines()[-1])
    assert row["case"] == case and row["n"] == n and row["cpu_ns"] > 0
    found = (row["wire_len"], row["wire_fnv"])
    if identity is None:
        identity = found
    assert identity == found
    return row

def summary(values):
    return {"median": statistics.median(values), "range": [min(values), max(values)],
            "slower": sum(value > 1 for value in values)}

result = {"affinity_cpu": allowed[0], "wire_identity": None, "cases": {}}
for case, (initial, alloc_n) in cases.items():
    calibration = run(a_cpu, case, initial)
    n = min(20_000_000, max(initial, math.ceil(initial * 120_000_000 / calibration["cpu_ns"])))
    blocks = []
    for i in range(10):
        order = ["a", "b", "b", "a"] if i % 2 == 0 else ["b", "a", "a", "b"]
        rows = [(kind, run(a_cpu if kind == "a" else b_cpu, case, n)) for kind in order]
        counts = {kind: [row["cpu_ns"] for k, row in rows if k == kind] for kind in "ab"}
        blocks.append({"order": "".join(order), "rows": [row for _, row in rows],
                       "ba": sum(counts["b"]) / sum(counts["a"]),
                       "aa": counts["a"][1] / counts["a"][0],
                       "bb": counts["b"][1] / counts["b"][0]})
    allocations = []
    for i in range(3):
        names = ["a", "b"] if i % 2 == 0 else ["b", "a"]
        specs = [(name + "_n", a_count if name == "a" else b_count, alloc_n) for name in names]
        specs += [(name + "_2n", a_count if name == "a" else b_count, 2 * alloc_n) for name in reversed(names)]
        rows = {label: run(binary, case, count) for label, binary, count in specs}
        delta = {name: {field: (rows[name + "_2n"][field] - rows[name + "_n"][field]) / alloc_n
                        for field in ("allocs", "reallocs", "bytes")} for name in "ab"}
        allocations.append({"rows": rows, "per_op": delta})
    cpus = [row["cpu_ns"] for block in blocks for row in block["rows"]]
    item = {"n": n, "allocation_n": alloc_n, "calibration": calibration, "blocks": blocks,
            "allocations": allocations, "ba": summary([block["ba"] for block in blocks]),
            "controls": {key: summary([block[key] for block in blocks]) for key in ("aa", "bb")},
            "min_process_cpu_ns": min(cpus)}
    result["cases"][case] = item
    print(case, "n", n, "b/a", item["ba"], "controls", item["controls"],
          "alloc", allocations[0]["per_op"], "min_cpu_ms", round(min(cpus) / 1e6, 3), flush=True)
result["wire_identity"] = identity
pathlib.Path(dest).write_text(json.dumps(result, indent=2) + "\n")
