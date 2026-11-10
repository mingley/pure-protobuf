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
    assert len(summaries) == len(validations) == 23 and len(rows) == 138 and len(allocations) == 46
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
    for summary in summaries:
        label = summary["case"]
        group = [r for r in rows if r["case"] == label]
        assert len(group) == 6 and sorted(r["block"] for r in group) == list(range(6))
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
marker = b'pub const FILE_DESCRIPTOR_SET: &[u8] = b"'
def canonical(source):
    position = 0
    pieces, literals = [], []
    while True:
        start = source.find(marker, position)
        if start < 0:
            pieces.append(source[position:])
            break
        body = start + len(marker)
        pieces.extend((source[position:body], b"<descriptor-bytes>"))
        value, p = bytearray(), body
        while True:
            assert p < len(source)
            c = source[p]
            p += 1
            if c == ord('"'):
                break
            if c == ord("\\"):
                assert p < len(source)
                escape = source[p]
                p += 1
                if escape == ord("x"):
                    assert p + 2 <= len(source)
                    value.append(int(source[p:p+2], 16))
                    p += 2
                else:
                    assert escape in (ord('"'), ord("\\"))
                    value.append(escape)
            else:
                assert 32 <= c <= 126
                value.append(c)
        literals.append(bytes(value))
        position = p - 1
    return b"".join(pieces), tuple(literals)
def tree(path):
    manifest, canonical_files = {}, {}
    for file in sorted(path.rglob("*")):
        if file.is_file():
            relative = str(file.relative_to(path))
            payload = file.read_bytes()
            manifest[relative] = {"length": len(payload), "sha256": sha(payload)}
            canonical_files[relative] = canonical(payload)
    assert manifest
    return manifest, canonical_files
retained = {}
for label in ("single-default", "nested5-default", "balanced20-default", "balanced20-lean", "balanced20-shared"):
    found = [p for p in root.rglob(label) if p.is_dir() and (p / "base").is_dir() and (p / "candidate").is_dir()]
    assert len(found) == 1, (label, found)
    base, base_decoded = tree(found[0] / "base")
    candidate, candidate_decoded = tree(found[0] / "candidate")
    assert base_decoded == candidate_decoded
    digest = lambda v: sha(json.dumps(v, sort_keys=True).encode())
    for run in runs:
        item = next(v for v in run["validation"] if v["case"]["case"] + "-" + v["case"]["profile"] == label)
        assert digest(base) == item["baseline_tree_sha256"]
        assert digest(candidate) == item["candidate_tree_sha256"]
        assert sum(x["length"] for x in base.values()) == item["capacity"]["base"]["bytes"]
        assert sum(x["length"] for x in candidate.values()) == item["capacity"]["candidate"]["bytes"]
    retained[label] = {"baseline_tree_sha256": digest(base), "candidate_tree_sha256": digest(candidate),
                       "files": len(base), "exact_decoded_and_other_source_equal": True}
runs.sort(key=lambda x: x["parallelism"])
out = {"schema": "pbrs-compact-literal-screen-v1",
       "baseline": "5fe70efdc798031badefd1a3847d8866dde352f5",
       "candidate": "f2cca72100959a3dcce4f68ca003f7f573066726",
       "artifact_sha256": "e3c83a77d7049c2444e4c526a99aedba924c4e1ae393fa56e1d7d99fd14e84d4",
       "runs": runs, "retained_outputs": retained}
(root / "verified-literal.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print("LITERAL_PERF_VERIFIED cases=46 pairs=276 allocation_rows=92 retained_output_pairs=5")
for run in runs:
    for label in ("balanced20-default", "single-default", "nested5-lean", "nested5-shared_pool"):
        s = next(x for x in run["summaries"] if x["case"] == label)
        a = [x for x in run["allocations"] if x["case"] == label]
        print("LITERAL_VERIFIED_RESULT", json.dumps({"cores": run["parallelism"], "case": label,
              "summary": s, "allocations": a}, sort_keys=True))
