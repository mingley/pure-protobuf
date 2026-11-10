#!/usr/bin/env python3
"""Verify the aligned codegen diagnostic against its retained archive."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys
from zipfile import ZipFile

archive = Path(sys.argv[1])
retained = json.loads(Path(sys.argv[2]).read_text())
expected_archive = "725f3ec5f990aa61a4ced8bde68356b4e71d3e13890d16bb67beb46ba3f296b0"
def sha(data):
    return hashlib.sha256(data).hexdigest()
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)
assert sha(archive.read_bytes()) == expected_archive
with ZipFile(archive) as bundle:
    names = bundle.namelist()
    def read(name):
        found = [x for x in names if x == name or x.endswith("/" + name)]
        assert len(found) == 1, (name, found)
        return bundle.read(found[0])
    source = json.loads(read("results.json"))
    assert source["source"] == retained["source"] == "5fe70efdc798031badefd1a3847d8866dde352f5"
    assert retained["artifact_sha256"] == expected_archive
    assert retained["cases"] == source["results"]
    assert retained["descriptor_byte_identity"] is True
    variants = ("pbrs-default", "pbrs-lean", "prost-default", "prost-aligned")
    assert {x["case"]["label"] for x in retained["cases"]} == {"small", "medium", "large"}
    for case in retained["cases"]:
        label = case["case"]["label"]
        fds = {}
        for variant in variants:
            content = read(f"witness-{label}-{variant}.fds")
            witness = case["witnesses"][variant]
            assert sha(content) == witness["fds_sha256"] and len(content) == witness["fds_bytes"]
            fds[variant] = content
        assert fds["pbrs-default"] == fds["pbrs-lean"] == fds["prost-aligned"]
        assert fds["prost-default"] != fds["prost-aligned"]
        blocks = case["blocks"]
        assert len(blocks) == 8
        positions = {v: [0, 0, 0, 0] for v in variants}
        for block in blocks:
            assert set(block["order"]) == set(variants)
            for position, v in enumerate(block["order"]):
                positions[v][position] += 1
                control = block["controls"][v]
                for sample in (block["samples"][v], control["a"], control["b"]):
                    assert sample["iterations"] == case["iterations"] == len(sample["runs"])
                    assert sample["cpu_ns"] == sum(x["cpu_ns"] for x in sample["runs"])
                    assert sample["wall_ns"] == sum(x["wall_ns"] for x in sample["runs"])
                    assert sample["max_child_rss_kib"] == max(x["max_child_rss_kib"] for x in sample["runs"])
                near(control["ratio"], control["b"]["cpu_ns"] / control["a"]["cpu_ns"])
            cpu = {v: block["samples"][v]["cpu_ns"] for v in variants}
            near(block["ratios"]["default"], cpu["pbrs-default"] / cpu["prost-default"])
            near(block["ratios"]["aligned-lean"], cpu["pbrs-lean"] / cpu["prost-aligned"])
            near(block["ratios"]["prost-aligned/default"], cpu["prost-aligned"] / cpu["prost-default"])
        assert all(x == [2, 2, 2, 2] for x in positions.values())
        for key, summary in case["summary"].items():
            values = [b["ratios"][key] for b in blocks]
            near(summary["median"], statistics.median(values))
            near(summary["range"][0], min(values))
            near(summary["range"][1], max(values))
            assert summary["above_one"] == sum(x > 1 for x in values)
        for v in variants:
            values = [b["controls"][v]["ratio"] for b in blocks]
            near(case["control_ranges"][v][0], min(values))
            near(case["control_ranges"][v][1], max(values))
    large = next(c for c in retained["cases"] if c["case"]["label"] == "large")
    for variant, literals in (("pbrs-default", 20), ("pbrs-lean", 0)):
        witness = retained["generated_output"][variant]
        assert witness["literal_count"] == literals and witness["total_files"] == 21
        assert witness["distinct_message_structs"] == 1000
        assert len(witness["per_file"]) == 21
        assert witness["per_file"]["mod.rs"]["message_structs"] == 0
        assert all(witness["per_file"][f"part_{i:02}.rs"]["message_structs"] == 50
                   for i in range(20))
        assert sum(x["bytes"] for x in witness["per_file"].values()) == witness["total_bytes"]
        assert witness["total_bytes"] == large["witnesses"][variant]["output"]["bytes"]
        assert witness["output_manifest_sha256"] == large["witnesses"][variant]["output"]["sha256"]
print("ALIGNED_DATA_VERIFIED descriptors=3 timing_blocks=24 output_partition=recorded")
