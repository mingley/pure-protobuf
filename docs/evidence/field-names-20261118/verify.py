#!/usr/bin/env python3
"""Verify the retained output trees and extract all raw timing samples."""
import hashlib
import json
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
case_rows = json.loads((root / "corpus/cases.json").read_text())
assert len(case_rows) == 7 and sum(c["timed"] for c in case_rows) == 6
raw = dict()
assert (root / "revisions.txt").read_text().splitlines()[0].split()[0] == "f20b8f49d6f3889e4afbae3427eae061ad081195"
assert (root / "revisions.txt").read_text().splitlines()[1].split()[0] == "9061b8ee0a75d32acb19ebf7090e4a95660012db"

def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def sample(x):
    assert set(x) == {"iters", "cpu_ns", "wall_ns", "alloc_calls",
                      "requested_bytes", "parallelism", "rss_kib"}
    return [x[key] for key in ("cpu_ns", "wall_ns", "rss_kib")]

for case in case_rows:
    name = case["name"]
    request_dir = root / "corpus" / name
    assert digest(request_dir / "descriptor.bin") == case["descriptor_sha256"]
    for filename, expected in {**case["protos"], **case["requests"]}.items():
        assert digest(request_dir / filename) == expected
    for n in (1, 2):
        data = json.loads((root / f"workers-{n}" / (name + ".json")).read_text())
        assert data["case"] == case and data["workers"] == n
        assert len(data["cpu_ids"]) == n and len(set(map(tuple, data["topology"].values()))) == n
        manifests = data["outputs"]
        nominal = manifests["request"]["base"]
        assert all(entry == nominal for versions in manifests.values() for entry in versions.values())
        assert len(nominal) >= len(case["protos"]) + 1
        manifest_sha = hashlib.sha256(json.dumps(nominal, sort_keys=True).encode()).hexdigest()
        if n == 1:
            raw[name] = {"input": case, "output_files": len(nominal),
                         "output_bytes": sum(f["bytes"] for f in nominal),
                         "output_manifest_sha256": manifest_sha, "workers": {}}
        else:
            assert raw[name]["output_manifest_sha256"] == manifest_sha
        if n == 1 and case["consumer"]:
            generated = root / "consumers" / name / "generated"
            actual = {str(p.relative_to(generated)) for p in generated.rglob("*.rs")}
            assert actual == {f["name"] for f in nominal}
            for f in nominal:
                p = generated / f["name"]
                assert p.stat().st_size == f["bytes"] and digest(p) == f["sha256"], p
        if not case["timed"]:
            assert "timing" not in data
            continue
        assert len(data["timing"]) == 8
        rows = []
        for index, row in enumerate(data["timing"]):
            assert row["block"] == index
            order = row["order"]
            assert order == list(reversed(order)) and set(order) == {"base", "candidate"}
            if index:
                assert order != data["timing"][index - 1]["order"]
            samples = row["samples"]
            iters = {x["iters"] for v in samples.values() for x in v}
            assert len(iters) == 1 and min(iters) > 0
            assert all(len(s) == 2 and all(x["parallelism"] == n for x in s)
                       for s in samples.values())
            for field, recorded in (("cpu_ns", "cpu_ratio"), ("wall_ns", "wall_ratio")):
                ratio = sum(x[field] for x in samples["candidate"]) / sum(x[field] for x in samples["base"])
                assert ratio == row[recorded]
            for v, control in row["controls"].items():
                assert samples[v][1]["cpu_ns"] / samples[v][0]["cpu_ns"] == control
            rows.append([order[0], iters.pop(),
                         [sample(s) for s in samples["base"]],
                         [sample(s) for s in samples["candidate"]]])
        allocations = data["allocation"]
        assert all(len(v) == 3 and all(s["iters"] == 1 and s["parallelism"] == n for s in v)
                   for v in allocations.values())
        summary = data["summary"]
        for field, key in (("cpu_median", "cpu_ratio"), ("wall_median", "wall_ratio")):
            assert summary[field] == statistics.median(r[key] for r in data["timing"])
        for v, captures in allocations.items():
            for key in ("alloc_calls", "requested_bytes"):
                assert summary[key][v] == statistics.median(s[key] for s in captures)
        raw[name]["workers"][str(n)] = {
            "cpu_ids": data["cpu_ids"], "topology": data["topology"], "timing": rows,
            "allocation": {v: [[a[key] for key in ("alloc_calls", "requested_bytes", "rss_kib")]
                               for a in captures] for v, captures in allocations.items()},
            "summary": summary,
        }

output = Path(sys.argv[2])
output.write_text(json.dumps(raw, indent=2, sort_keys=True) + "\n")
for name, row in raw.items():
    print("FIELD_NAME_RAW_INPUT " + json.dumps({"case": name, **{k: v for k, v in row.items()
                                                                if k != "workers"}}, separators=(",", ":")))
    for n, capture in row["workers"].items():
        print("FIELD_NAME_RAW_CAPTURE " + json.dumps(
            {"case": name, "cores": n, "record": capture}, separators=(",", ":")))
print("FIELD_NAME_VERIFY 7 exact output cases, 96 timing blocks, 72 separate allocation samples")
