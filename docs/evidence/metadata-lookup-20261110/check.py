#!/usr/bin/env python3
"""Check the retained metadata lookup screen and the published summary."""

import hashlib
import json
import math
import pathlib
import statistics

root = pathlib.Path(__file__).resolve().parents[3]
raw = (root / "docs/benchmarks/metadata-lookup-20261110.json").read_bytes()
git_blob = hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
assert git_blob == "38f39deabc9b1462941936292cc5ebfc419eed13", git_blob
data = json.loads(raw)
assert (data["count_iterations"], data["timed_iterations"], data["blocks"]) == (
    10000,
    2000000,
    10,
)
groups = {
    "short ASCII": ["lower-hit", "upper-hit", "lower-miss", "upper-miss"],
    "long ASCII": [
        "long-lower-hit",
        "long-upper-hit",
        "long-lower-miss",
        "long-upper-miss",
    ],
    "short binary": ["binary-hit", "binary-upper-hit"],
    "long binary": ["binary-long-hit", "binary-long-upper-hit"],
}
controls = ["all-control", "reserved-control"]
assert set(data["runs"]) == {name for names in groups.values() for name in names} | set(
    controls
)
medians = {}
for name, result in data["runs"].items():
    before = result["cpu_seconds"]["base"]
    after = result["cpu_seconds"]["candidate"]
    ratios = result["ratios"]
    assert len(before) == len(after) == len(ratios) == 10
    assert all(math.isfinite(value) and value > 0 for value in before + after + ratios)
    assert all(
        math.isclose(ratio, candidate / baseline, rel_tol=1e-12)
        for ratio, candidate, baseline in zip(ratios, after, before)
    )
    count = result["allocations"]
    pair = count["base"]["allocations"], count["candidate"]["allocations"]
    if name in controls:
        assert pair == (0, 0), (name, pair)
    elif name.startswith("binary"):
        assert pair == (20000, 10000), (name, pair)
    else:
        assert pair == (10000, 0), (name, pair)
    medians[name] = statistics.median(ratios)
text = (root / "docs/benchmarks/metadata-lookup-20261110.md").read_text()
for names in groups.values():
    values = [medians[name] for name in names]
    rendered = f"{min(values):.4f}–{max(values):.4f}"
    assert rendered in text, rendered
for name in controls:
    vals = data["runs"][name]["ratios"]
    for value in (medians[name], min(vals), max(vals)):
        rendered = f"{value:.4f}"
        assert rendered in text, (name, rendered)
print(f"verified {len(data['runs'])} cases and their 10 paired samples")
