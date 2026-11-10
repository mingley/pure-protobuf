#!/usr/bin/env python3
"""Recompute literal benchmark rows and verify retained output bytes."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
def sha(data):
    return hashlib.sha256(data).hexdigest()
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)
paths = list(root.rglob("results.json"))
assert len(paths) == 2, paths
runs = [json.loads(p.read_text()) for p in paths]
assert {x["parallelism"] for x in runs} == {1, 2}
for run in runs:
    cores = run["parallelism"]
    rows, summaries, validations, allocations = (run["rows"], run["summaries"],
                                                   run["validation"], run["allocations"])
    assert len(summaries) == len(validations) == 5 and len(rows) == 120 and len(allocations) == 10
    by_case = {}
    for v in validations:
        label = v["case"]["case"] + "-" + v["case"]["profile"]
        assert label not in by_case
        assert v["decoded_descriptors_and_other_source_equal"] is True
        assert v["capacity"]["base"]["files"] == v["capacity"]["candidate"]["files"] == v["file_count"]
        if v["case"]["profile"] in ("lean", "lean_deps", "shared_pool"):
            assert v["changed_files"] == 0
            assert v["capacity"]["base"]["bytes"] == v["capacity"]["candidate"]["bytes"]
        by_case[label] = v
    expected_labels = {"nested5-default", "nested5-lean", "nested5-shared_pool",
                       "balanced20-lean", "balanced20-shared_pool"}
    assert set(by_case) == expected_labels
    assert {s["case"] for s in summaries} == expected_labels
    for summary in summaries:
        label = summary["case"]
        group = [r for r in rows if r["case"] == label]
        assert len(group) == 24 and sorted(r["block"] for r in group) == list(range(24))
        ratios = []
        for row in group:
            assert row["order"] == (["base", "candidate"] if row["block"] % 2 == 0 else ["candidate", "base"])
            a, b = row["samples"]["base"], row["samples"]["candidate"]
            assert a["iters"] == b["iters"] == row["iterations"]
            assert a["parallelism"] == b["parallelism"] == cores
            near(row["ratio"], b["cpu_ns"] / a["cpu_ns"])
            ratios.append(row["ratio"])
            for variant in ("base", "candidate"):
                ctl = row["controls"][variant]
                near(ctl["ratio"], ctl["b"]["cpu_ns"] / ctl["a"]["cpu_ns"])
        near(summary["median"], statistics.median(ratios))
        near(summary["range"][0], min(ratios))
        near(summary["range"][1], max(ratios))
        assert summary["slower"] == sum(x > 1 for x in ratios)
        for variant in ("base", "candidate"):
            ctl = [r["controls"][variant]["ratio"] for r in group]
            near(summary["control_ranges"][variant][0], min(ctl))
            near(summary["control_ranges"][variant][1], max(ctl))
            rss = [r["samples"][variant]["peak_rss_kib"] for r in group]
            assert summary["peak_rss_kib_range"][variant] == [min(rss), max(rss)]
        assert summary["capacity"] == by_case[label]["capacity"]
        pair = [x for x in allocations if x["case"] == label]
        assert len(pair) == 2 and {x["variant"] for x in pair} == {"base", "candidate"}
        for item in pair:
            sample = item["sample"]
            assert sample["parallelism"] == cores and sample["iters"] == 2
            assert sample["alloc_calls"] > 0 and sample["requested_bytes"] > 0
            assert sample["peak_extra"] == max(0, sample["peak_live"] - sample["initial_live"])
            assert sample["held_extra"] == max(0, sample["held_live"] - sample["initial_live"])
assert runs[0]["binary_sha256"] == runs[1]["binary_sha256"]
assert runs[0]["allocation_binary_sha256"] == runs[1]["allocation_binary_sha256"]
runs.sort(key=lambda x: x["parallelism"])
out = {"schema": "pbrs-compact-literal-lean-repeat-v1",
       "baseline": "5fe70efdc798031badefd1a3847d8866dde352f5",
       "candidate": "f2cca72100959a3dcce4f68ca003f7f573066726",
       "runs": runs,
       "limits": "Original generated file trees were compared in-memory and not retained in this artifact."}
(root / "verified-holdout.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print("LITERAL_LEAN_REPEAT_VERIFIED cases=10 pairs=240 allocation_rows=20")
for run in runs:
    for label in ("nested5-default", "nested5-lean", "nested5-shared_pool", "balanced20-lean", "balanced20-shared_pool"):
        summary = next(x for x in run["summaries"] if x["case"] == label)
        print("LITERAL_LEAN_REPEAT_RESULT", json.dumps({"cores": run["parallelism"], "summary": summary}, sort_keys=True))
