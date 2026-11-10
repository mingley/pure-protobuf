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
patch = (root / "docs/evidence/metadata-lookup-20261110/candidate.patch").read_bytes()
patch_blob = hashlib.sha1(
    b"blob " + str(len(patch)).encode() + b"\0" + patch
).hexdigest()
assert patch_blob == "433b6c95eabb1e394b44e1b6e72978d86b0106b2", patch_blob
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

def load_blob(name, expected):
    path = root / "docs/benchmarks" / name
    content = path.read_bytes()
    digest = hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()
    assert digest == expected, (name, digest)
    return json.loads(content)


def close(value, expected):
    assert math.isfinite(value) and math.isfinite(expected) and expected > 0
    assert math.isclose(value, expected, rel_tol=1e-12), (value, expected)


component = load_blob(
    "metadata-lookup-20261110-uninstrumented.json",
    "be541b144c6b8c8102df2b4216d5ed8eb2de7383",
)
rpc = load_blob(
    "metadata-lookup-20261110-rpc.json",
    "0d4a88cd81e097a7aae0c9ea81b033d0f59d24a3",
)
assert component["timed_iterations"] == 2000000
assert (rpc["warmup"], rpc["iterations"]) == (2000, 20000)
assert set(component["runs"]) == set(data["runs"])
assert set(rpc["runs"]) == {"0", "1", "8"}
for dataset in (component, rpc):
    assert dataset["blocks"] == 8 and len(dataset["affinity"]) == 1
    assert dataset["available_cpu_count"] >= 1


def check_order(block, index):
    order = (
        ["base", "candidate", "candidate", "base"]
        if index % 2 == 0
        else ["candidate", "base", "base", "candidate"]
    )
    assert block["order"] == order


# Reject an all-ABBA series: odd blocks must use the distinct BAAB order.
try:
    check_order({"order": ["base", "candidate", "candidate", "base"]}, 1)
except AssertionError:
    pass
else:
    raise AssertionError("accepted ABBA in an odd block")


component_medians = {}
control_ranges = {"base_control": [], "candidate_control": []}
for name, blocks in component["runs"].items():
    assert len(blocks) == 8
    for index, block in enumerate(blocks):
        check_order(block, index)
        before = block["samples"]["base"]
        after = block["samples"]["candidate"]
        assert len(before) == len(after) == 2 and min(before + after) > 0
        close(block["ratio"], sum(after) / sum(before))
        close(block["base_control"], before[1] / before[0])
        close(block["candidate_control"], after[1] / after[0])
        for key in control_ranges:
            control_ranges[key].append(block[key])
    component_medians[name] = statistics.median(block["ratio"] for block in blocks)
for names in groups.values():
    values = [component_medians[name] for name in names]
    rendered = f"{min(values):.4f}–{max(values):.4f}"
    assert rendered in text, rendered
for key, values in control_ranges.items():
    for value in (min(values), max(values)):
        rendered = f"{value:.4f}"
        assert rendered in text, (key, rendered)
for name in controls:
    vals = [block["ratio"] for block in component["runs"][name]]
    for value in (statistics.median(vals), min(vals), max(vals)):
        rendered = f"{value:.4f}"
        assert rendered in text, (name, rendered)

for name, blocks in rpc["runs"].items():
    assert len(blocks) == 8
    for index, block in enumerate(blocks):
        check_order(block, index)
        before = block["samples"]["base"]
        after = block["samples"]["candidate"]
        assert len(before) == len(after) == 2
        for label, field in (("cpu", "cpu_s"), ("wall", "measured_wall_ns")):
            close(
                block[label + "_ratio"],
                (after[0][field] + after[1][field]) / (before[0][field] + before[1][field]),
            )
            close(block[label + "_base_control"], before[1][field] / before[0][field])
            close(block[label + "_candidate_control"], after[1][field] / after[0][field])
    for key in ("cpu_ratio", "cpu_base_control", "cpu_candidate_control"):
        vals = [block[key] for block in blocks]
        needed = (statistics.median(vals), min(vals), max(vals)) if key == "cpu_ratio" else (min(vals), max(vals))
        for value in needed:
            rendered = f"{value:.4f}"
            assert rendered in text, (name, key, rendered)
print("verified allocation, uninstrumented component and loopback RPC samples")
