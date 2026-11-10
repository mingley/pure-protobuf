#!/usr/bin/env python3
"""Recompute target-cache screen summaries and check retained output bytes."""
import hashlib
import json
import math
from pathlib import Path
import statistics

root = Path("evidence/target")
def sha(data):
    return hashlib.sha256(data).hexdigest()
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)
assert sha((root / "original.zip").read_bytes()) == "3ec488759acd5e27f234c222c6b64f6aa97ae162ea6ff3d3a39cba3ee7c01a7e"
manifest = json.loads((root / "corpus/corpus.json").read_text())
assert len(manifest) == 18
cases = {r["case"]: r for r in manifest}
assert len(cases) == 18
for item in manifest:
    path = root / "corpus" / Path(item["request"]).name
    assert sha(path.read_bytes()) == item["request_sha256"]
perf = {case for case, item in cases.items() if item["performance"]}
assert perf == {"balanced20-default", "balanced20-lean", "many128-lean",
                "many512-lean", "alias128-lean", "services-lean", "interleaved-lean"}

def outputs(directory):
    names, sources = sorted(directory.glob("*.name")), sorted(directory.glob("*.rs"))
    assert names and len(names) == len(sources)
    out = []
    for i, (name, source) in enumerate(zip(names, sources)):
        assert name.stem == source.stem == f"{i:06}"
        data = source.read_bytes()
        out.append({"name": name.read_text(), "bytes": len(data), "sha256": sha(data)})
    return out

results = []
for workers in (1, 2):
    result = json.loads((root / f"cores-{workers}/results.json").read_text())
    assert result["source"] == {"base": "5fe70efdc798031badefd1a3847d8866dde352f5",
                                "candidate": "540e12e1a71d6ebdad86bb55b36ab89711451455"}
    assert result["parallelism"] == workers and len(result["cpus"]) == workers
    assert len({tuple(result["topology"][str(c)]) for c in result["cpus"]}) == workers
    assert len(result["validation"]) == 18
    assert {v["case"]["case"] for v in result["validation"]} == set(cases)
    for item in result["validation"]:
        name = item["case"]["case"]
        a, b = item["result"]["base"], item["result"]["candidate"]
        assert a == b
        if cases[name]["expected_error"]:
            assert a.startswith("RESULT_ERR_V1 ")
        elif name != "duplicate-descriptor":
            assert a == "RESULT_OK_V1"
        if a == "RESULT_OK_V1":
            assert item["variant_outputs"]["base"] == item["variant_outputs"]["candidate"]
            if workers == 1 and name in {"balanced20-lean", "interleaved-lean", "services-lean",
                                         "public-transitive", "public-emit-deps", "public-owned", "wkt-shared"}:
                x = outputs(root / f"cores-{workers}/dump-{name}-base")
                y = outputs(root / f"cores-{workers}/dump-{name}-candidate")
                assert x == y == item["variant_outputs"]["base"]
    rows, summaries, allocs = result["rows"], result["summaries"], result["allocations"]
    assert len(rows) == 42 and len(summaries) == 7 and len(allocs) == 42
    assert {v["case"] for v in summaries} == perf
    for summary in summaries:
        name = summary["case"]
        group = [r for r in rows if r["case"] == name]
        assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
        for row in group:
            assert row["order"] == (["base", "candidate"] if row["block"] % 2 == 0 else ["candidate", "base"])
            a, b = row["samples"]["base"], row["samples"]["candidate"]
            assert a["parallelism"] == b["parallelism"] == workers
            assert a["iters"] == b["iters"] == row["iterations"]
            assert a["cpu_ns"] > 0 and b["cpu_ns"] > 0
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
            c = [r["controls"][variant]["ratio"] for r in group]
            assert summary["controls"][variant] == [min(c), max(c)]
            times = [r["samples"][variant]["cpu_ns"] for r in group]
            assert summary["sample_cpu_ns"][variant] == [min(times), max(times)]
        allocation = [a for a in allocs if a["case"] == name]
        assert len(allocation) == 6
        for variant in ("base", "candidate"):
            v = [a for a in allocation if a["variant"] == variant]
            assert sorted(a["repeat"] for a in v) == [0, 1, 2]
            assert all(a["sample"]["iters"] == 2 and a["sample"]["parallelism"] == workers for a in v)
        print("TARGET_CACHE_VERIFIED_V1 " + json.dumps({"workers": workers, "summary": summary,
              "allocations": allocation}, sort_keys=True))
    results.append(result)
assert results[0]["binary_sha256"] == results[1]["binary_sha256"]
assert results[0]["allocation_binary_sha256"] == results[1]["allocation_binary_sha256"]
witness = json.loads((root / "witness/witness.json").read_text())
assert (witness["balanced20-lean"]["base_match_calls"],
        witness["balanced20-lean"]["candidate_match_calls"]) == (20000, 400)
assert (witness["interleaved-lean"]["base_match_calls"],
        witness["interleaved-lean"]["candidate_match_calls"]) == (20000, 20000)
for case in ("balanced20-lean", "interleaved-lean"):
    variants = [outputs(root / f"witness/{case}-{v}") for v in
                ("base", "candidate", "trace-base", "trace-candidate")]
    assert all(a == variants[0] for a in variants)
    lookup = {x["name"]: {"bytes": x["bytes"], "sha256": x["sha256"]} for x in variants[0]}
    assert lookup == witness[case]["outputs"]
bad = outputs(root / "witness/interleaved-lean-mutant")
good = witness["interleaved-lean"]["outputs"]
bad_by_name = {x["name"]: {"bytes": x["bytes"], "sha256": x["sha256"]} for x in bad}
changed = sorted(name for name in set(good) | set(bad_by_name)
                 if good.get(name) != bad_by_name.get(name))
assert changed == witness["mutant_changed_names"] and changed
print("TARGET_CACHE_DATA_VERIFIED workers=2 requests=36 cpu_blocks=84 allocation_samples=84")
