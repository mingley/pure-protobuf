#!/usr/bin/env python3
"""Recompute the retained target-cache timing and allocation summaries."""
import json
import math
from pathlib import Path
import statistics
import sys

path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("codegen-target-cache-20261110.json")
data = json.loads(path.read_text())
assert data["source"] == {"baseline": "5fe70efdc798031badefd1a3847d8866dde352f5",
                          "candidate": "540e12e1a71d6ebdad86bb55b36ab89711451455"}
assert len(data["corpus"]) == 18 and len(data["rows"]) == 84
perf = {x["case"] for x in data["corpus"] if x["performance"]}
assert len(perf) == 7
assert len(data["summaries"]) == len(data["allocations"]) == 14
assert {(s["workers"], s["case"]) for s in data["summaries"]} == {(w, c) for w in (1, 2) for c in perf}
assert {(s["workers"], s["case"]) for s in data["allocations"]} == {(w, c) for w in (1, 2) for c in perf}

def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

for summary in data["summaries"]:
    workers, case = summary["workers"], summary["case"]
    assert workers in (1, 2) and case in perf
    group = [r for r in data["rows"] if r["case"] == case
             and r["samples"]["base"]["parallelism"] == workers]
    assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
    for row in group:
        assert row["order"] == (["base", "candidate"] if row["block"] % 2 == 0 else ["candidate", "base"])
        a, b = row["samples"]["base"], row["samples"]["candidate"]
        assert a["parallelism"] == b["parallelism"] == workers
        assert a["iters"] == b["iters"] == row["iterations"] and a["cpu_ns"] > 0
        near(row["ratio"], b["cpu_ns"] / a["cpu_ns"])
        for variant in ("base", "candidate"):
            c = row["controls"][variant]
            assert c["a"]["parallelism"] == c["b"]["parallelism"] == workers
            near(c["ratio"], c["b"]["cpu_ns"] / c["a"]["cpu_ns"])
    ratios = [r["ratio"] for r in group]
    near(summary["median"], statistics.median(ratios))
    assert summary["range"] == [min(ratios), max(ratios)]
    assert summary["slower"] == sum(x > 1 for x in ratios)
    for variant in ("base", "candidate"):
        controls = [r["controls"][variant]["ratio"] for r in group]
        assert summary["controls"][variant] == [min(controls), max(controls)]
        times = [r["samples"][variant]["cpu_ns"] for r in group]
        assert summary["sample_cpu_ns"][variant] == [min(times), max(times)]
    matches = [a for a in data["allocations"] if a["workers"] == workers and a["case"] == case]
    assert len(matches) == 1 and len(matches[0]["samples"]) == 6
    by_variant = {}
    for variant in ("base", "candidate"):
        samples = [s for s in matches[0]["samples"] if s["variant"] == variant]
        assert sorted(s["repeat"] for s in samples) == [0, 1, 2]
        assert all(s["sample"]["iters"] == 2 and s["sample"]["parallelism"] == workers for s in samples)
        by_variant[variant] = [(s["sample"]["alloc_calls"], s["sample"]["requested_bytes"])
                               for s in samples]
    assert by_variant["base"] == by_variant["candidate"]
    assert len(set(by_variant["base"])) == 1
assert data["witness"] == {"grouped": {"base": 20000, "candidate": 400},
                           "interleaved": {"base": 20000, "candidate": 20000},
                           "invalidation_mutant_changed_files": 20}
print("Target-cache data verified: 84 CPU blocks, 84 allocation samples")
