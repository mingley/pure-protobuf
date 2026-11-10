#!/usr/bin/env python3
"""Check the retained codegen comparison and rejected reservation screen."""
import json
import math
from pathlib import Path
import statistics

ROOT = Path("evidence")
def unique(name):
    found = list(ROOT.rglob(name))
    assert len(found) == 1, (name, found)
    return found[0]
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

profiles = {}
for profile in ("default", "lean", "shared"):
    matches = list((ROOT / "baseline").glob(f"compare-0685-{profile}/summary.json"))
    assert len(matches) == 1, matches
    report = json.loads(matches[0].read_text())
    assert report["errors"] == [] and report["repeats"] == 5
    matrix = report["matrix"]
    for generator in ("pbrs", "prost"):
        cell = matrix["cells"][f"1000/{generator}"]
        assert cell["generator"] == generator and cell["case"] == "1000"
        for metric in ("generation.elapsed_ns", "generation.peak_rss_bytes", "output.rust_bytes"):
            sample = cell["metrics"][metric]
            assert sample["n"] == 5 and sample["median"] > 0
    profiles[profile] = {
        "profile": report["pbrs_profiles"]["pbrs"],
        "matrix": matrix,
        "setup": {k: v for k, v in report["setup"].items()
                  if k.endswith("_sha256")},
        "qualification": report["qualification"],
    }

result = json.loads(unique("results.json").read_text())
rows = result["rows"]
summaries = result["summaries"]
validations = result["validation"]
allocations = result["allocations"]
assert len(rows) == 90 and len(summaries) == len(validations) == 15
assert len(allocations) == 30
by_case = {}
for item in validations:
    case = item["case"]
    label = case["case"] + "-" + case["profile"]
    assert label not in by_case
    caps = item["capacity"]
    assert caps["base"]["bytes"] == caps["candidate"]["bytes"]
    assert caps["base"]["files"] == caps["candidate"]["files"] == item["file_count"]
    assert len(item["output_tree_sha256"]) == 64
    assert len(case["request_sha256"]) == 64
    by_case[label] = item
for summary in summaries:
    case = summary["case"]
    assert case in by_case
    group = [row for row in rows if row["case"] == case]
    assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
    ratios = []
    for row in group:
        assert row["order"] == (["base","candidate"] if row["block"] % 2 == 0
                               else ["candidate","base"])
        a = row["samples"]["base"]
        b = row["samples"]["candidate"]
        assert a["iters"] == b["iters"] == row["iterations"] and a["cpu_ns"] > 0
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
    assert summary["capacity"] == by_case[case]["capacity"]
for case in by_case:
    a = [x for x in allocations if x["case"] == case]
    assert {x["variant"] for x in a} == {"base", "candidate"} and len(a) == 2
    for item in a:
        sample = item["sample"]
        assert sample["iters"] == 2 and sample["alloc_calls"] > 0 and sample["requested_bytes"] > 0

combined = {
    "schema": "pbrs-codegen-reserve-rejected-v1",
    "baseline_source": "0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be",
    "candidate_source": "79cddac2b759487134924dda28d800d7203a698d",
    "baseline_artifact_sha256": "69bcb203fc49f2e4eea2493ca929bc8a668d6cffef4aec0515f503baa7097a09",
    "candidate_artifact_sha256": "8ee951f6bade6d23ce86636fd1561a7a5124b9d986f592ea04d46bf99aa1eed4",
    "comparison_profiles": profiles,
    "screen": result,
    "limits": {
        "source_outputs": "During collection, per-file path, length, and SHA-256 manifests matched; only a digest of each manifest was retained, so this artifact cannot independently re-run that comparison.",
        "reference": "Prost 0.14.4 output has a different API and feature surface.",
        "memory": "RSS is process-level; retained String capacity is not peak memory; requested allocation bytes are not RSS.",
        "environment": "Shared runner, one pinned CPU in candidate screen; no controlled-host ranking.",
    },
}
(ROOT / "codegen-reserve-20261110.json").write_text(json.dumps(combined, indent=2, sort_keys=True) + "\n")
print("CODEGEN_EVIDENCE_VERIFIED profiles=3 cases=15 cpu_pairs=90 allocation_rows=30")
