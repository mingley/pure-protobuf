#!/usr/bin/env python3
"""Verify retained scalar-map samples; does not run a benchmark."""
import copy
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys
import tarfile

ROOT = Path(__file__).resolve().parent

def require(condition, message):
    if not condition:
        raise ValueError(message)

def verify(records, schedule, outcomes, summary):
    require(len(records) == len(schedule) == len(outcomes) == 272, "sample count")
    require(len(summary["rows"]) == 34, "cell count")
    grouped = {}
    identities = {}
    for ordinal, (item, result, record) in enumerate(zip(schedule, outcomes, records)):
        require(item["ordinal"] == result["ordinal"] == ordinal, "ordinal")
        for field in ("side", "cell", "repeat"):
            require(item[field] == result[field], "schedule/outcome identity")
        require(result["returncode"] == 0, "sample exit status")
        data = record["sample"]
        cell = tuple(item["cell"])
        require(data["schema"] == "owned-scalar-map/1", "sample schema")
        require(tuple(data[f] for f in ("case", "implementation", "input", "operation")) == cell, "cell identity")
        require(data["warmup"] == 100 and data["allocation_iterations"] == 100, "warmup/allocation iterations")
        require(type(data["wall_ns"]) is int and data["wall_ns"] > 0, "elapsed wall time")
        require(type(data["allocations"]) is int and data["allocations"] >= 0, "allocation events")
        require(len(bytes.fromhex(data["wire_hex"])) == data["wire_bytes"], "wire length")
        identity = tuple(data[f] for f in ("wire_hex", "wire_bytes", "expected_checksum", "iterations", "allocation_iterations"))
        require(identities.setdefault(cell, identity) == identity, "cross-side input/checksum identity")
        grouped.setdefault((cell, item["side"]), []).append((item, data))
    for row in summary["rows"]:
        cell = tuple(row["cell"])
        sides = {side: grouped[(cell, side)] for side in ("base", "candidate")}
        for side, samples in sides.items():
            require({item["repeat"] for item, _ in samples} == {0, 1, 2, 3}, "repeat coverage")
            values = {data["allocations"] for _, data in samples}
            require(len(values) == 1, "deterministic allocation events")
            require(next(iter(values)) / 100 == row[side + "_allocations_per_parse"], "allocation summary")
            ns = statistics.median(data["wall_ns"] / data["iterations"] for _, data in samples)
            require(math.isclose(ns, row[side + "_ns"], rel_tol=1e-12, abs_tol=1e-9), "elapsed summary")
        require(math.isclose(row["ratio"], row["candidate_ns"] / row["base_ns"], rel_tol=1e-12), "ratio")
        delta = 1 if cell[0].startswith("scalar") and cell[1] == "owned" and cell[2] == "borrowed" else 0
        require(row["base_allocations_per_parse"] - row["candidate_allocations_per_parse"] == delta, "allocation delta")
        first_sides = []
        for repeat in range(4):
            pair = sorted((item for side in sides.values() for item, _ in side if item["repeat"] == repeat), key=lambda item: item["ordinal"])
            require(len(pair) == 2 and pair[1]["ordinal"] == pair[0]["ordinal"] + 1, "adjacent side pair")
            require({item["side"] for item in pair} == {"base", "candidate"}, "paired sides")
            first_sides.append(pair[0]["side"])
        require(first_sides.count("base") == first_sides.count("candidate") == 2, "side order balance")

def main():
    manifest = json.loads((ROOT / "sha256.json").read_text())
    for name, expected in manifest.items():
        require(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == expected, "retained file hash: " + name)
    schedule = json.loads((ROOT / "schedule.json").read_text())
    outcomes = json.loads((ROOT / "outcomes.json").read_text())
    summary = json.loads((ROOT / "summary.json").read_text())
    with tarfile.open(ROOT / "raw-json.tar.gz", "r:gz") as archive:
        files = {member.name: archive.extractfile(member).read() for member in archive.getmembers() if member.isfile()}
    records = []
    for item in schedule:
        stem = str(item["ordinal"]).zfill(3) + "-" + "-".join(item["cell"]) + "-" + item["side"]
        require(files["raw/" + stem + ".stderr"] == b"", "sample stderr")
        records.append({"sample": json.loads(files["raw/" + stem + ".json"])})
    require(len(files) == 544, "complete raw stdout/stderr archive")
    verify(records, schedule, outcomes, summary)
    negative = (ROOT / "baseline-negative-control.log").read_text()
    require("scalar map values must allocate only the map container" in negative, "baseline negative-control diagnostic")
    require("left: 3" in negative and "right: 2" in negative, "baseline negative-control allocation counts")
    require("default/lazy proof+strict outputs are byte-identical" in (ROOT / "default-profile-parity.txt").read_text(), "default profile comparison")
    for name in ("candidate-proof.log", "candidate-instrumented-proof.log", "msrv-proof.log"):
        log = (ROOT / name).read_text()
        require("PASS scalar map messages" in log and "PASS scalar required map values" in log, "consumer proof: " + name)
    if "--negative-controls" in sys.argv:
        rejected = 0
        for field, value in (("allocations", records[0]["sample"]["allocations"] + 1), ("expected_checksum", -1), ("wire_bytes", -1)):
            changed = copy.deepcopy(records)
            changed[0]["sample"][field] = value
            try:
                verify(changed, schedule, outcomes, summary)
            except ValueError:
                rejected += 1
        require(rejected == 3, "verifier negative controls")
    print("verified 34 cells, 272 samples, allocation deltas, input identity, counterbalanced order and retained proofs")

if __name__ == "__main__":
    main()
