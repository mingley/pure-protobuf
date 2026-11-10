#!/usr/bin/env python3
"""Recompute the generated-consumer compile comparisons."""
import json
import math
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
found = list(root.rglob("results.json"))
assert len(found) == 1, found
data = json.loads(found[0].read_text())
assert data["source"] == "5fe70efdc798031badefd1a3847d8866dde352f5"
assert data["candidate"] == "f2cca72100959a3dcce4f68ca003f7f573066726"
expected = {
    ("single-default", "check"): 4, ("nested5-default", "check"): 4,
    ("balanced20-default", "check"): 3, ("balanced20-lean", "check"): 3,
    ("balanced20-shared", "check"): 3, ("single-default", "release"): 4,
    ("nested5-default", "release"): 3, ("nested5-default", "incremental"): 3,
    ("balanced20-default", "incremental"): 3,
}
assert len(data["results"]) == len(expected)
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)
for case in data["results"]:
    key = (case["case"], case["mode"])
    count = expected.pop(key)
    blocks = case["blocks"]
    assert len(blocks) == count and sorted(b["block"] for b in blocks) == list(range(count))
    if case["case"].endswith("-lean"):
        assert case["source"]["base"] == case["source"]["candidate"]
    ratios = []
    for block in blocks:
        assert block["order"] == (["base", "candidate"] if block["block"] % 2 == 0 else ["candidate", "base"])
        a, b = block["samples"]["base"], block["samples"]["candidate"]
        near(block["ratio"], b["cpu_ns"] / a["cpu_ns"])
        ratios.append(block["ratio"])
        for variant in ("base", "candidate"):
            ctl = block["controls"][variant]
            near(ctl["ratio"], ctl["b"]["cpu_ns"] / ctl["a"]["cpu_ns"])
            for sample in (block["samples"][variant], ctl["a"], ctl["b"]):
                assert sample["cpu_ns"] > 0 and sample["wall_ns"] > 0 and sample["max_child_rss_kib"] > 0
                assert sample["consumer_rebuilt"] and sample["dependencies_fresh"]
                if case["mode"] == "release":
                    assert sample["executable_bytes"] > 0 and len(sample["executable_sha256"]) == 64
    summary = case["summary"]
    near(summary["median"], statistics.median(ratios))
    near(summary["range"][0], min(ratios))
    near(summary["range"][1], max(ratios))
    assert summary["slower"] == sum(x > 1 for x in ratios)
    for v in ("base", "candidate"):
        vals = [b["controls"][v]["ratio"] for b in blocks]
        near(summary["controls"][v][0], min(vals))
        near(summary["controls"][v][1], max(vals))
assert not expected
(root / "verified-consumer.json").write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
print("LITERAL_CONSUMER_VERIFIED cases=9 paired_blocks=30")
for case in data["results"]:
    print("LITERAL_CONSUMER_RESULT", json.dumps({"case": case["case"], "mode": case["mode"],
          "source": case["source"], "summary": case["summary"]}, sort_keys=True))
