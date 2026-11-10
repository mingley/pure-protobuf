#!/usr/bin/env python3
"""Recompute the retained lazy-cell tables and controls."""
import json
import math
from pathlib import Path
import statistics

root = Path(__file__).resolve().parent
data = json.loads((root / "lazy-cell-seeding-20261110.json").read_text())
note = (root / "lazy-cell-seeding-20261110.md").read_text()
assert data["schema"] == "pbrs.seeded-lazy-query.v1"
assert data["base"] == "e521462b360892232d8e0c8cac72760617fa561d"
expected = {
    "seeded-1.99-first": ("eac5ffd54507595a55e4eb5c50a514fee20bdd4c", 42775074195),
    "seeded-1.99-repeat": ("eac5ffd54507595a55e4eb5c50a514fee20bdd4c", 42775883171),
    "seeded-tested-1.99": ("05e42378fcd29ac372b214e38570e0c7ecca2bdf", 42776002159),
    "seeded-1.85-first": ("eac5ffd54507595a55e4eb5c50a514fee20bdd4c", 42775599055),
    "seeded-1.85-repeat": ("eac5ffd54507595a55e4eb5c50a514fee20bdd4c", 42776474950),
    "inline-1.85": ("4bb89cb7864be35374d3c1f5633c3b60434081ea", 42776552744),
    "inline-1.99": ("4bb89cb7864be35374d3c1f5633c3b60434081ea", 42776796175),
}
assert set(data["runs"]) == set(expected)

def close(a, b):
    assert math.isclose(a, b, rel_tol=1e-12, abs_tol=1e-12), (a, b)

for name, (source, run) in expected.items():
    entry = data["runs"][name]
    assert (entry["source"], entry["run"]) == (source, run)
    assert len(entry["zip_sha256"]) == len(entry["generated_sha256"]) == 64
    result = entry["result"]
    assert result["wire_identity"][0] == 293 and len(result["cases"]) == 17
    for key, case in result["cases"].items():
        assert len(case["blocks"]) == 10 and len(case["allocations"]) == 3
        ratios = []
        controls = {"aa": [], "bb": []}
        for block in case["blocks"]:
            assert block["order"] in ("abba", "baab")
            samples = {
                kind: [row for tag, row in zip(block["order"], block["rows"]) if tag == kind]
                for kind in "ab"
            }
            assert all(len(values) == 2 for values in samples.values())
            assert all(row["case"] == key and row["n"] == case["n"] and row["cpu_ns"] > 0
                       for values in samples.values() for row in values)
            a, b = ([row["cpu_ns"] for row in samples[kind]] for kind in "ab")
            ratio = sum(b) / sum(a)
            close(ratio, block["ba"])
            close(a[1] / a[0], block["aa"])
            close(b[1] / b[0], block["bb"])
            ratios.append(ratio)
            controls["aa"].append(block["aa"])
            controls["bb"].append(block["bb"])
        close(statistics.median(ratios), case["ba"]["median"])
        close(min(ratios), case["ba"]["range"][0])
        close(max(ratios), case["ba"]["range"][1])
        assert sum(value > 1 for value in ratios) == case["ba"]["slower"]
        for control in controls:
            close(statistics.median(controls[control]), case["controls"][control]["median"])
            close(min(controls[control]), case["controls"][control]["range"][0])
            close(max(controls[control]), case["controls"][control]["range"][1])
        expected_alloc = case["allocations"][0]["per_op"]
        for sample in case["allocations"]:
            assert sample["per_op"] == expected_alloc
            for kind in "ab":
                for metric in ("allocs", "reallocs", "bytes"):
                    value = (sample["rows"][kind + "_2n"][metric] -
                             sample["rows"][kind + "_n"][metric]) / case["allocation_n"]
                    close(value, sample["per_op"][kind][metric])

def cell(run, case):
    row = data["runs"][run]["result"]["cases"][case]["ba"]
    return (f'{row["median"]:.4f} ({row["range"][0]:.4f}–{row["range"][1]:.4f}), '
            f'{row["slower"]}/10')

cases = [
    ("d4 construct", "query4_construct"),
    ("d4 fresh encode", "query4_fresh"),
    ("d4 decode", "query4_decode"),
    ("Empty Query", "query_empty"),
    ("Operator only", "query_operator_only"),
    ("Ten Queries, one populated", "query_mixed_10"),
    ("Eight u64s, unchanged", "repeat_eight_collect"),
    ("Prost fresh, unchanged", "query4_prost_fresh"),
]
tables = [
    ["seeded-1.99-first", "seeded-1.99-repeat", "seeded-tested-1.99", "inline-1.99"],
    ["seeded-1.85-first", "seeded-1.85-repeat", "inline-1.85"],
]
for runs in tables:
    for label, case in cases:
        row = "| " + label + " | " + " | ".join(cell(run, case) for run in runs) + " |"
        assert note.count(row) == 1, row
index = (root.parent / "benchmarks.md").read_text()
assert index.count("benchmarks/lazy-cell-seeding-20261110.md") == 1
print("verified seven runs, 119 cases, 1,190 blocks, controls, allocations and table rows")
