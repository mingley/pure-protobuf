#!/usr/bin/env python3
"""Read-only consistency audit of the retained SB-26c codec evidence."""

import hashlib
import json
from pathlib import Path

EVIDENCE = Path(__file__).resolve().parent / "evidence"
record = json.loads((EVIDENCE / "codec-measurement.json").read_text())
inventory = json.loads((EVIDENCE / "codec-inventory.json").read_text())["cells"]
baseline_path = EVIDENCE / record["baseline"]["path"]
assert hashlib.sha256(baseline_path.read_bytes()).hexdigest() == record["baseline"]["sha256"]
baseline = json.loads(baseline_path.read_text())
expected = {c["id"] for c in inventory}
rows = {c["id"]: c for c in baseline["cells"]}
assert len(rows) == len(baseline["cells"]) == len(expected) == 512
assert set(rows) == expected
assert baseline["devloop_commit"] == record["runtime_source"]["commit"]
raw_rows = {}
for part in record["completed_parts"]:
    path = EVIDENCE / part["path"]
    assert hashlib.sha256(path.read_bytes()).hexdigest() == part["sha256"]
    raw = json.loads(path.read_text())
    assert raw["host"] == baseline["host"]
    assert raw["devloop_commit"] == baseline["devloop_commit"]
    assert len(raw["cells"]) == part["cells"] == 64
    for cell in raw["cells"]:
        assert cell["id"] not in raw_rows
        assert cell == rows[cell["id"]]
        assert cell["iters"] == 16 and cell["repeats"] == 3
        assert cell["instruction_method"] == "differential_callgrind_2n_minus_n"
        for metric in ("instructions", "allocs", "alloc_bytes"):
            assert cell[metric]["status"] == "measured"
            assert cell[metric]["data"]["value"] >= 0
        raw_rows[cell["id"]] = cell
assert len(record["completed_parts"]) == 8 and raw_rows == rows
assert len(record["pairs"]) == 256
assert {p["pbrs_id"] for p in record["pairs"]} == {c for c in rows if c.startswith("codec.adoption.pbrs.")}
for pair in record["pairs"]:
    p, r = rows[pair["pbrs_id"]], rows[pair["prost_id"]]
    assert pair["prost_id"] == pair["pbrs_id"].replace(".pbrs.", ".prost.", 1)
    ratios = pair["pbrs_over_prost"]
    for metric in ("instructions", "allocs", "alloc_bytes"):
        assert ratios[metric] == p[metric]["data"]["value"] / r[metric]["data"]["value"]
    assert pair["observed_p1_instruction_floor"] == (ratios["instructions"] <= 1)
    assert pair["observed_p1_allocation_floor"] == (ratios["allocs"] <= 1)
    instruction_target = ratios["instructions"] <= 0.8 if pair["operation"] in ("fresh_encode", "read_all") else None
    allocation_target = ratios["allocs"] <= 0.5 if pair["operation"] == "read_all" else None
    assert pair["observed_p2_instruction_target"] == instruction_target
    assert pair["observed_p2_read_all_allocation_target"] == allocation_target
for summary in record["corpus_operation_summary"]:
    group = [p for p in record["pairs"] if p["corpus"] == summary["corpus"] and p["operation"] == summary["operation"]]
    assert len(group) == summary["pairs"]
    for metric, flag, prefix in (("instructions", "observed_p1_instruction_floor", "instruction"), ("allocs", "observed_p1_allocation_floor", "allocation")):
        assert summary[prefix + "_losses"] == sum(not p[flag] for p in group)
        assert summary[prefix + "_ratio_min"] == min(p["pbrs_over_prost"][metric] for p in group)
        assert summary[prefix + "_ratio_max"] == max(p["pbrs_over_prost"][metric] for p in group)
targets = record["observed_targets"]
assert targets["P1_codec"]["instruction_losses"] == sum(not p["observed_p1_instruction_floor"] for p in record["pairs"])
assert targets["P1_codec"]["allocation_losses"] == sum(not p["observed_p1_allocation_floor"] for p in record["pairs"])
for operation, key in (("fresh_encode", "P2_fresh_encode"), ("read_all", "P2_read_all")):
    group = [p for p in record["pairs"] if p["operation"] == operation]
    assert targets[key]["pairs"] == len(group) == 64
    assert targets[key]["instruction_target_met"] == sum(p["observed_p2_instruction_target"] is True for p in group)
    if operation == "read_all":
        assert targets[key]["allocation_target_met"] == sum(p["observed_p2_read_all_allocation_target"] is True for p in group)
print("Evidence consistent: 512 cells, eight raw reports, 256 paired ratios and all recorded target counts.")
