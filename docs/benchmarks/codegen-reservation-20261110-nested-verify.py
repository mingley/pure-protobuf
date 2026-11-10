#!/usr/bin/env python3
"""Independently check the nested codegen reservation screen."""
import json
import math
from pathlib import Path
import statistics

root = Path("evidence")
found = list(root.rglob("results.json"))
assert len(found) == 2, found
runs = [json.loads(path.read_text()) for path in found]
assert {x["parallelism"] for x in runs} == {1, 2}
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)
expected = {
    "balanced20-default", "balanced20-lean", "balanced20-shared",
    "single-default", "single-lean", "nested5-lean", "nested5-shared",
    "nested20-lean", "nested20-shared", "nested128-lean", "nested512-lean",
    "alias128-lean", "alias512-lean", "normalized-lean", "deps-lean",
    "deps-lean_deps", "collision-lean",
}
for result in runs:
    workers = result["parallelism"]
    assert len(result["cpus"]) == workers
    assert len({tuple(result["topology"][str(cpu)]) for cpu in result["cpus"]}) == workers
    rows, summaries = result["rows"], result["summaries"]
    validations, allocations = result["validation"], result["allocations"]
    assert len(rows) == 102 and len(summaries) == len(validations) == 17
    assert len(allocations) == 34
    assert {s["case"] for s in summaries} == expected
    by_case = {}
    for item in validations:
        case = item["case"]
        label = case["case"] + "-" + case["profile"]
        assert label in expected and label not in by_case
        a, b = item["capacity"]["base"], item["capacity"]["candidate"]
        assert a["bytes"] == b["bytes"] and a["files"] == b["files"] == item["file_count"]
        assert a["parallelism"] == b["parallelism"] == workers
        assert len(item["output_tree_sha256"]) == len(case["request_sha256"]) == 64
        by_case[label] = item
    for summary in summaries:
        case = summary["case"]
        group = [row for row in rows if row["case"] == case]
        assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
        ratios = []
        for row in group:
            assert row["order"] == (["base", "candidate"] if row["block"] % 2 == 0
                                   else ["candidate", "base"])
            a, b = row["samples"]["base"], row["samples"]["candidate"]
            assert a["iters"] == b["iters"] == row["iterations"]
            assert a["parallelism"] == b["parallelism"] == workers
            near(row["ratio"], b["cpu_ns"] / a["cpu_ns"])
            ratios.append(row["ratio"])
            for variant in ("base", "candidate"):
                ctl = row["controls"][variant]
                assert ctl["a"]["parallelism"] == ctl["b"]["parallelism"] == workers
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
        assert summary["capacity"] == by_case[case]["capacity"]
        pair = [a for a in allocations if a["case"] == case]
        assert len(pair) == 2 and {a["variant"] for a in pair} == {"base", "candidate"}
        for item in pair:
            a = item["sample"]
            assert a["parallelism"] == workers and a["iters"] == 2
            assert a["alloc_calls"] > 0 and a["requested_bytes"] > 0
            assert a["peak_extra"] == max(0, a["peak_live"] - a["initial_live"])
            assert a["held_extra"] == max(0, a["held_live"] - a["initial_live"])
            assert a["peak_live"] >= a["held_live"]
assert runs[0]["binary_sha256"] == runs[1]["binary_sha256"]
assert runs[0]["allocation_binary_sha256"] == runs[1]["allocation_binary_sha256"]
runs.sort(key=lambda x: x["parallelism"])
combined = {
    "schema": "pbrs-codegen-reserve-nested-v1",
    "baseline_source": "0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be",
    "candidate_source": "79cddac2b759487134924dda28d800d7203a698d",
    "artifact_sha256": "2a04aa2a25bc6e693cb08ac885cf29532a1d74d0dfa7e70e40caad6ae2f8ad73",
    "runs": runs,
    "limitations": [
        "Per-file paths, lengths and SHA-256 manifests matched during collection; only a digest of each manifest was retained.",
        "String capacity is virtual allocation capacity, not RSS or physical memory.",
        "Requested-live accounting omits allocator overhead and physical realloc peaks; parallel atomics may affect timing.",
        "Single shared host; CPU measurements use process CPU and are not an end-to-end Prost comparison."
    ]
}
(root / "codegen-reserve-nested-20261110.json").write_text(
    json.dumps(combined, indent=2, sort_keys=True) + "\n")
print("CODEGEN_NESTED_VERIFIED cases=34 cpu_pairs=204 allocation_rows=68")
