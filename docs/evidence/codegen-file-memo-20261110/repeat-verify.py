#!/usr/bin/env python3
"""Verify the longer borrowed file-memo screen and its output manifests."""
import hashlib
import json
import math
from pathlib import Path
import statistics

root = Path("evidence/repeat")
paths = sorted(root.rglob("results.json"))
assert len(paths) == 2, paths
runs = sorted((json.loads(p.read_text()) for p in paths), key=lambda r: r["parallelism"])
assert [r["parallelism"] for r in runs] == [1, 2]
def single(path, name):
    matches = list(path.rglob(name))
    assert len(matches) == 1, matches
    return matches[0]
def requests(root):
    manifest = single(root, "corpus.json")
    entries = json.loads(manifest.read_text())
    data = {}
    for case in entries:
        value = (manifest.parent / Path(case["request"]).name).read_bytes()
        digest = hashlib.sha256(value).hexdigest()
        assert digest == case["request_sha256"]
        data[case["case"]] = (digest, case["targets"], case["descriptor_files"],
                               case["performance"], case["expected_error"])
    return data
corpus = requests(root)
assert len(corpus) == 13 and corpus == requests(Path("evidence/initial"))
expected = set(corpus)
perf = {case for case, value in corpus.items() if value[3]}
assert len(perf) == 6
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

for result in runs:
    workers = result["parallelism"]
    assert result["source"] == {"base": "5fe70efdc798031badefd1a3847d8866dde352f5",
                                "borrowed": "9717d306baf1e0391153d8157e5c31990f4a3c92"}
    assert len(result["cpus"]) == workers
    assert len({tuple(result["topology"][str(c)]) for c in result["cpus"]}) == workers
    assert len(result["validation"]) == len(expected)
    assert {v["case"]["case"] for v in result["validation"]} == expected
    for item in result["validation"]:
        case = item["case"]["case"]
        a, b = item["result"]["base"], item["result"]["borrowed"]
        assert a == b
        if corpus[case][4]:
            assert a.startswith("RESULT_ERR_V1 ")
        elif case != "duplicate-descriptor":
            assert a == "RESULT_OK_V1"
        if a == "RESULT_OK_V1":
            manifests = item["variant_outputs"]
            assert manifests["base"] == manifests["borrowed"]
            assert manifests["base"] and all(len(x["sha256"]) == 64 for x in manifests["base"])
    rows, summaries, allocations = result["rows"], result["summaries"], result["allocations"]
    assert len(rows) == 72 and len(summaries) == 6 and len(allocations) == 36
    assert {s["case"] for s in summaries} == perf
    for summary in summaries:
        case = summary["case"]
        group = [r for r in rows if r["case"] == case]
        assert len(group) == 12 and sorted(r["block"] for r in group) == list(range(12))
        for row in group:
            assert row["order"] == (["base", "borrowed"] if row["block"] % 2 == 0 else ["borrowed", "base"])
            a, b = row["samples"]["base"], row["samples"]["borrowed"]
            assert a["parallelism"] == b["parallelism"] == workers
            assert a["iters"] == b["iters"] == row["iterations"]
            assert a["cpu_ns"] > 0 and b["cpu_ns"] > 0
            near(row["ratio"], b["cpu_ns"] / a["cpu_ns"])
            for variant in ("base", "borrowed"):
                c = row["controls"][variant]
                assert c["a"]["parallelism"] == c["b"]["parallelism"] == workers
                near(c["ratio"], c["b"]["cpu_ns"] / c["a"]["cpu_ns"])
        ratios = [r["ratio"] for r in group]
        near(summary["median"], statistics.median(ratios))
        near(summary["range"][0], min(ratios))
        near(summary["range"][1], max(ratios))
        assert summary["slower"] == sum(x > 1 for x in ratios)
        for variant in ("base", "borrowed"):
            controls = [r["controls"][variant]["ratio"] for r in group]
            near(summary["controls"][variant][0], min(controls))
            near(summary["controls"][variant][1], max(controls))
            cpu = [r["samples"][variant]["cpu_ns"] for r in group]
            assert summary["sample_cpu_ns"][variant] == [min(cpu), max(cpu)]
        samples = [a for a in allocations if a["case"] == case]
        assert len(samples) == 6
        values = {}
        for variant in ("base", "borrowed"):
            group_alloc = [a for a in samples if a["variant"] == variant]
            assert sorted(a["repeat"] for a in group_alloc) == [0, 1, 2]
            assert all(a["sample"]["iters"] == 2 and a["sample"]["parallelism"] == workers
                       for a in group_alloc)
            values[variant] = {
                "calls_per_op": [a["sample"]["alloc_calls"] / 2 for a in group_alloc],
                "requested_bytes_per_op": [a["sample"]["requested_bytes"] / 2 for a in group_alloc]}
        print("MEMO_REPEAT_VERIFIED_V1 " + json.dumps({"workers": workers, "summary": summary, "allocations": values}, sort_keys=True))
assert runs[0]["binary_sha256"] == runs[1]["binary_sha256"]
assert runs[0]["allocation_binary_sha256"] == runs[1]["allocation_binary_sha256"]
validation = [{v["case"]["case"]: v for v in r["validation"]} for r in runs]
for case in expected:
    a, b = validation[0][case], validation[1][case]
    assert a["result"] == b["result"]
    if a["result"]["base"] == "RESULT_OK_V1":
        assert a["variant_outputs"] == b["variant_outputs"]
print("MEMO_REPEAT_DATA_VERIFIED workers=2 cases=26 timing_blocks=144 allocation_samples=72")
