#!/usr/bin/env python3
"""Check file-memo screen inputs, timing ratios and allocation records."""
import hashlib
import json
import math
from pathlib import Path
import statistics

root = Path("evidence/source")
paths = sorted(root.rglob("results.json"))
assert len(paths) == 2, paths
runs = sorted((json.loads(p.read_text()) for p in paths), key=lambda r: r["parallelism"])
assert [r["parallelism"] for r in runs] == [1, 2]
manifests = list(root.rglob("corpus.json"))
assert len(manifests) == 1
corpus_root = manifests[0].parent
cases = json.loads(manifests[0].read_text())
assert len(cases) == 13
for case in cases:
    data = (corpus_root / Path(case["request"]).name).read_bytes()
    assert hashlib.sha256(data).hexdigest() == case["request_sha256"]
expected = {c["case"] for c in cases}
perf = {c["case"] for c in cases if c["performance"]}
assert len(perf) == 6
variants = ("base", "borrowed", "contains")
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

for result in runs:
    workers = result["parallelism"]
    assert result["source"] == {
        "base": "5fe70efdc798031badefd1a3847d8866dde352f5",
        "borrowed": "9717d306baf1e0391153d8157e5c31990f4a3c92",
        "contains": "5bbdac8f28fea40bd6330067f3b205eaa63db728"}
    assert len(result["cpus"]) == workers
    assert len({tuple(result["topology"][str(c)]) for c in result["cpus"]}) == workers
    assert len(result["validation"]) == len(expected)
    assert {v["case"]["case"] for v in result["validation"]} == expected
    assert len(result["rows"]) == 36 and len(result["summaries"]) == 6
    assert len(result["allocations"]) == 18
    assert {s["case"] for s in result["summaries"]} == perf
    for summary in result["summaries"]:
        case = summary["case"]
        group = [r for r in result["rows"] if r["case"] == case]
        assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
        for index in range(3):
            assert sum(r["order"].index(variants[index]) == j for r in group for j in [0]) == 2
            assert sum(r["order"].index(variants[index]) == j for r in group for j in [1]) == 2
            assert sum(r["order"].index(variants[index]) == j for r in group for j in [2]) == 2
        for row in group:
            assert set(row["order"]) == set(variants)
            base = row["samples"]["base"]
            for variant in variants:
                sample = row["samples"][variant]
                assert sample["parallelism"] == workers and sample["iters"] == row["iterations"]
                assert sample["cpu_ns"] > 0 and sample["alloc_calls"] == sample["requested_bytes"] == 0
                c = row["controls"][variant]
                assert c["a"]["parallelism"] == c["b"]["parallelism"] == workers
                near(c["ratio"], c["b"]["cpu_ns"] / c["a"]["cpu_ns"])
            for variant in ("borrowed", "contains"):
                near(row["ratios"][variant], row["samples"][variant]["cpu_ns"] / base["cpu_ns"])
        for variant in ("borrowed", "contains"):
            vals = [r["ratios"][variant] for r in group]
            record = summary[variant]
            near(record["median"], statistics.median(vals))
            near(record["range"][0], min(vals))
            near(record["range"][1], max(vals))
            assert record["slower"] == sum(v > 1 for v in vals)
        for variant in variants:
            vals = [r["controls"][variant]["ratio"] for r in group]
            near(summary["controls"][variant][0], min(vals))
            near(summary["controls"][variant][1], max(vals))
        allocation = [a for a in result["allocations"] if a["case"] == case]
        assert len(allocation) == 3 and {a["variant"] for a in allocation} == set(variants)
        values = {}
        for a in allocation:
            sample = a["sample"]
            assert sample["parallelism"] == workers and sample["iters"] == 2
            assert sample["alloc_calls"] > 0 and sample["requested_bytes"] > 0
            values[a["variant"]] = {"calls_per_op": sample["alloc_calls"] / 2,
                                    "requested_bytes_per_op": sample["requested_bytes"] / 2}
        print("MEMO_ALLOC_V1 " + json.dumps({"workers": workers, "case": case, "values": values}, sort_keys=True))
assert runs[0]["binary_sha256"] == runs[1]["binary_sha256"]
assert runs[0]["allocation_binary_sha256"] == runs[1]["allocation_binary_sha256"]
by_case = [{v["case"]["case"]: v for v in r["validation"]} for r in runs]
for case in expected:
    a, b = (table[case] for table in by_case)
    assert a["result"] == b["result"]
    if a["result"] == "ok":
        assert a["outputs"] == b["outputs"]
print("MEMO_DATA_VERIFIED workers=2 cases=26 timing_blocks=72 allocation_samples=36")
