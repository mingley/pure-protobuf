#!/usr/bin/env python3
"""Recompute the retained filename-memo timing summaries."""
import json
import math
from pathlib import Path
import statistics
import sys

path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("codegen-file-memo-20261110.json")
data = json.loads(path.read_text())
assert len(data["corpus"]) == 13
perf = {case["case"] for case in data["corpus"] if case["performance"]}
assert len(perf) == 6

def near(a, b):
    assert math.isclose(a, b, rel_tol=1e-12, abs_tol=1e-12), (a, b)

for name, blocks, variants in (("initial", 6, ("base", "borrowed", "contains")),
                                ("repeat", 12, ("base", "borrowed"))):
    run = data[name]
    assert len(run["rows"]) == 2 * len(perf) * blocks
    assert len(run["summaries"]) == len(run["allocations"]) == 2 * len(perf)
    assert {(x["workers"], x["case"]) for x in run["summaries"]} == {
        (w, case) for w in (1, 2) for case in perf}
    assert {(x["workers"], x["case"]) for x in run["allocations"]} == {
        (w, case) for w in (1, 2) for case in perf}
    for s in run["summaries"]:
        workers, case = s["workers"], s["case"]
        rows = [r for r in run["rows"] if r["case"] == case
                and r["samples"]["base"]["parallelism"] == workers]
        assert sorted(r["block"] for r in rows) == list(range(blocks))
        for row in rows:
            if name == "repeat":
                assert row["order"] == (["base", "borrowed"] if row["block"] % 2 == 0
                                        else ["borrowed", "base"])
            else:
                assert set(row["order"]) == set(variants)
            base = row["samples"]["base"]["cpu_ns"]
            assert base > 0
            for variant in variants:
                sample = row["samples"][variant]
                assert sample["parallelism"] == workers
                assert sample["iters"] == row["iterations"] and sample["cpu_ns"] > 0
                control = row["controls"][variant]
                a, b = control["a"], control["b"]
                assert a["iters"] == b["iters"] == row["iterations"]
                assert a["parallelism"] == b["parallelism"] == workers
                assert a["cpu_ns"] > 0 and b["cpu_ns"] > 0
                near(control["ratio"], b["cpu_ns"] / a["cpu_ns"])
                if variant != "base":
                    ratio = row["ratios"][variant] if name == "initial" else row["ratio"]
                    near(ratio, sample["cpu_ns"] / base)
        for variant in variants:
            controls = [r["controls"][variant]["ratio"] for r in rows]
            near(s["controls"][variant][0], min(controls))
            near(s["controls"][variant][1], max(controls))
            if variant == "base":
                continue
            ratios = [r["ratios"][variant] if name == "initial" else r["ratio"] for r in rows]
            summary = s[variant] if name == "initial" else s
            near(summary["median"], statistics.median(ratios))
            near(summary["range"][0], min(ratios))
            near(summary["range"][1], max(ratios))
            assert summary["slower"] == sum(x > 1 for x in ratios)
        if name == "repeat":
            for variant in variants:
                times = [r["samples"][variant]["cpu_ns"] for r in rows]
                assert s["sample_cpu_ns"][variant] == [min(times), max(times)]
    for a in run["allocations"]:
        if name == "initial":
            assert set(a["values"]) == set(variants)
            assert all(v["calls_per_op"] > 0 and v["requested_bytes_per_op"] > 0
                       for v in a["values"].values())
        else:
            assert set(a["allocations"]) == set(variants)
            for v in a["allocations"].values():
                assert len(v["calls_per_op"]) == len(v["requested_bytes_per_op"]) == 3
                assert min(v["calls_per_op"]) > 0 and min(v["requested_bytes_per_op"]) > 0
    print(f"{name}: {len(run['rows'])} timing blocks, {len(run['summaries'])} summaries, "
          f"{len(run['allocations'])} allocation groups checked")
print("File-memo extracted data verified")
