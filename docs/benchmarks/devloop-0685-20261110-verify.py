#!/usr/bin/env python3
"""Audit the 0685 advisory dev-loop comparison from original samples."""
import collections
import hashlib
import json
import math
from pathlib import Path
import re
import sys
from zipfile import ZipFile

archive = Path(sys.argv[1])
assert hashlib.sha256(archive.read_bytes()).hexdigest() == "96902b4f39bc2be1a8d03743eb63d800ceeee9a810df96de1e9e775cc41865a7"
with ZipFile(archive) as z:
    names = z.namelist()
    def read(name):
        found = [x for x in names if x == name or x.endswith("/" + name)]
        assert len(found) == 1, (name, found)
        return z.read(found[0])
    base = json.loads(read("base.json"))
    head = json.loads(read("head.json"))
    validation = json.loads(read("validation.json"))
    compare = read("compare.txt").decode()
    summary = read("summary.md").decode()
assert base["schema"] == head["schema"]
assert base["devloop_commit"] == "c8dd4bd7738074a29dee8727814832a4e754198f", base["devloop_commit"]
assert head["devloop_commit"] == "0685d7f6cbc4e0ac9cdfb020bd085a8b7eb5b2be", head["devloop_commit"]
assert "Comparison exit: 1 (advisory; SB-20 sets gates)." in summary
assert "Common cells: 1634; new: 0; removed: 0." in summary
left = {c["id"]: c for c in base["cells"]}
right = {c["id"]: c for c in head["cells"]}
assert len(left) == len(base["cells"]) == len(right) == len(head["cells"]) == 1634
assert set(left) == set(right)
metrics = ("instructions", "allocs", "alloc_bytes", "syscalls", "locks")
pattern = re.compile(r"^(\S+) (instructions|allocs|alloc_bytes|syscalls|locks): (REGRESSED|improved|BUDGET EXCEEDED) (.*)$")
reported = {}
for line in compare.splitlines():
    match = pattern.match(line)
    if not match:
        continue
    cell, metric, kind, detail = match.groups()
    key = (cell, metric, kind)
    assert key not in reported, key
    reported[key] = {"cell": cell, "metric": metric, "kind": kind,
                     "detail": detail, "line": line}
assert "INCOMPARABLE" not in compare
expected = set()
for ident in sorted(left):
    a, b = left[ident], right[ident]
    for key in ("kind", "codec", "iters", "repeats", "input_wire_fingerprint", "input_wire_bytes", "fresh_encode_profile"):
        assert a.get(key) == b.get(key), (ident, key, a.get(key), b.get(key))
    for metric in metrics:
        old, new = a[metric], b[metric]
        if old["status"] != "measured" or new["status"] != "measured":
            continue
        x, y = old["data"]["value"], new["data"]["value"]
        assert math.isfinite(x) and math.isfinite(y) and x >= 0 and y >= 0
        assert old["data"]["unit"] == new["data"]["unit"] and old["data"]["unit"]
        if x == 0:
            if y > 0:
                expected.add((ident, metric, "REGRESSED"))
        else:
            delta = (y - x) / x
            limit = 0.02 if b["kind"] == "rpc" else 0.01
            if delta > limit:
                expected.add((ident, metric, "REGRESSED"))
            elif delta < 0 and abs(delta) >= 0.02:
                expected.add((ident, metric, "improved"))
actual = {k for k in reported if k[2] != "BUDGET EXCEEDED"}
assert expected == actual, {"missing": sorted(expected - actual)[:10], "extra": sorted(actual - expected)[:10]}
budget = json.loads(Path(__file__).with_name("devloop-0685-20261110-picker.json").read_text())["cells"]
expected_budget = set()
for ident, limits in budget.items():
    assert ident in right
    for metric, maximum in limits.items():
        m = right[ident][metric]
        if m["status"] == "measured" and m["data"]["value"] > maximum:
            expected_budget.add((ident, metric, "BUDGET EXCEEDED"))
actual_budget = {k for k in reported if k[2] == "BUDGET EXCEEDED"}
assert expected_budget == actual_budget, (expected_budget, actual_budget)
counts = collections.Counter(f"{k[2]}.{k[1]}" for k in reported)
assert all(c["instructions"]["status"] == "not_run" for c in base["cells"] + head["cells"])
rows = []
for key in sorted(reported):
    row = dict(reported[key])
    cell, metric, _ = key
    for label, report in (("base", left), ("head", right)):
        item = report[cell][metric]
        row[label] = item["data"]["value"] if item["status"] == "measured" else None
        row[label + "_unit"] = item["data"].get("unit")
    rows.append(row)
out = {"schema": "devloop-advisory-audit-0685/1",
       "base": base["devloop_commit"], "head": head["devloop_commit"],
       "artifact_id": 13744891059,
       "artifact_sha256": "96902b4f39bc2be1a8d03743eb63d800ceeee9a810df96de1e9e775cc41865a7",
       "common_cells": 1634, "instructions": "not_run", "comparison_exit": 1,
       "counts": dict(counts), "host_base": base["host"], "host_head": head["host"],
       "validation": validation, "all_rows": rows}
Path(sys.argv[2]).write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
print("DEVLOOP_0685_AUDITED", json.dumps({"counts": dict(counts),
      "rows": len(rows), "validation_errors": validation["errors"],
      "alloc_bytes_rows": [r for r in rows if r["metric"] == "alloc_bytes"]}, sort_keys=True))
