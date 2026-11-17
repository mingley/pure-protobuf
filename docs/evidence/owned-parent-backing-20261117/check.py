#!/usr/bin/env python3
"""Verify retained allocation controls without running generators or timings."""
import copy
import hashlib
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent

def require(condition, message):
    if not condition:
        raise ValueError(message)

def verify(rows):
    require(set(rows) == {"previous", "original", "fix"}, "variant set")
    require(all(len(cases) == 4 for cases in rows.values()), "four mixed cases")
    seen = set()
    for previous, original, fixed in zip(rows["previous"], rows["original"], rows["fix"]):
        for field in ("schema", "children", "payload_bytes", "map_first", "shared_allocations", "expected_borrowed"):
            require(previous[field] == original[field] == fixed[field], "shared case identity: " + field)
        require(fixed["schema"] == "mixed-parent-wire/1" and fixed["children"] == 32, "control identity")
        seen.add((fixed["payload_bytes"], fixed["map_first"]))
        require(original == fixed, "original backing restored")
        require(fixed["borrowed_allocations"] == fixed["expected_borrowed"], "fixed allocation expectation")
        delta = 31 if fixed["map_first"] else -1
        require(previous["borrowed_allocations"] - fixed["borrowed_allocations"] == delta, "prior regression detected")
    require(seen == {(32, True), (32, False), (8192, True), (8192, False)}, "payload/order coverage")

def main():
    for name, expected in json.loads((ROOT / "sha256.json").read_text()).items():
        require(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, "file hash: " + name)
    rows = json.loads((ROOT / "allocation-controls.json").read_text())
    verify(rows)
    for side in rows:
        raw = [json.loads(line) for line in (ROOT / (side + "-report.jsonl")).read_text().splitlines()]
        require(raw == rows[side], "raw control report: " + side)
    identity = json.loads((ROOT / "generated-identity.json").read_text())
    require(set(identity) == {"default/strict.rs", "default/proof.rs", "lazy/strict.rs", "lazy/proof.rs",
                             "owned/strict-excluding-mixed-helper", "owned/mixed-helper",
                             "owned/proof.rs", "api-codec/codec_cases.rs"}, "generated identity coverage")
    require(all(re.fullmatch("[0-9a-f]{64}", value) for value in identity.values()), "generated SHA256 values")
    negative = (ROOT / "previous-negative.stderr").read_text()
    require("mixed scalar-map parents must retain backing for eager child payloads" in negative, "negative-control message")
    require("left: 37" in negative and "right: 6" in negative, "negative-control counts")
    for name in ("fix-full-proof.log", "msrv-proof.log"):
        data = (ROOT / name).read_text()
        require("PASS scalar map messages" in data and "PASS scalar required map values" in data, "pure/required proof: " + name)
    if "--negative-controls" in sys.argv:
        rejected = 0
        changed = copy.deepcopy(rows)
        changed["previous"][0]["borrowed_allocations"] -= 1
        changed_shared = copy.deepcopy(rows)
        changed_shared["fix"][0]["shared_allocations"] += 1
        for bad in (changed, changed_shared):
            try:
                verify(bad)
            except ValueError:
                rejected += 1
        require(rejected == 2, "verifier negative controls")
    print("verified original/previous/fixed controls, both orders/sizes, generated identity and retained proofs")

if __name__ == "__main__":
    main()
