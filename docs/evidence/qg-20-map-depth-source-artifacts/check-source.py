#!/usr/bin/env python3
"""Verify portable source evidence. Does not compile, generate, or extract files."""
import hashlib
import json
import re
import tarfile
from pathlib import Path

here = Path(__file__).resolve().parent
digest = lambda data: hashlib.sha256(data).hexdigest()
artifacts = json.loads((here / "artifact-sha256.json").read_text())
for name, record in artifacts.items():
    data = (here / name).read_bytes()
    assert len(data) == record["bytes"] and digest(data) == record["sha256"], name

members = json.loads((here / "raw-member-sha256.json").read_text())
payloads = {}
with tarfile.open(here / "raw-source-proof.tar.gz", "r:gz") as archive:
    for member in archive:
        assert member.isfile() and member.name in members, member.name
        data = archive.extractfile(member).read()
        record = members[member.name]
        assert len(data) == record["bytes"] and digest(data) == record["sha256"], member.name
        payloads[member.name] = data
assert set(payloads) == set(members)
inventory = json.loads(payloads["work/qg20/map-migration.json"])
identity = json.loads((here / "source-identity.json").read_text())
assert inventory["baseline"] == identity["baseline"] == "ad04010b166325ec09bb9b403afd47338a38ab1f"
assert inventory["applied"] and len(inventory["files"]) == 7 and len(inventory["operations"]) == 327
counts = inventory["counts"]
assert counts["private-map-decoder"] == counts["map-entry-before-wire"] == counts["map-entry-validation"] == 109
assert counts["message-value-decoder"] == counts["message-value-validator"] == 17
assert counts["scalar-value-decoder"] == counts["scalar-value-validator"] == 92
literals = re.compile(r'"(?:\\.|[^"\\])*"')


def normalized(text):
    text = re.sub(r"\s+", "", text)
    text = re.sub(r",(?=\))", "", text)
    return text.replace("},_=>", "}_=>")


for row in inventory["files"]:
    original = payloads["source-baseline/" + row["path"]]
    assert digest(original) == row["original_sha256"]
    original_text = original.decode()
    expected = original_text
    operations = sorted((op for op in inventory["operations"] if op["path"] == row["path"]),
                        key=lambda op: op["start"])
    assert all(a["end"] <= b["start"] for a, b in zip(operations, operations[1:]))
    for operation in reversed(operations):
        assert original_text[operation["start"]:operation["end"]] == operation["before"]
        expected = expected[:operation["start"]] + operation["after"] + expected[operation["end"]:]
    actual = payloads["source-candidate/" + row["path"]].decode()
    assert literals.findall(expected) == literals.findall(actual)
    assert normalized(expected) == normalized(actual), row["path"]

for path, record in identity["source"].items():
    before = payloads["source-baseline/" + path]
    after = payloads["source-candidate/" + path]
    assert digest(before) == record["baseline_sha256"] and digest(after) == record["candidate_sha256"]
    assert (before == after) == record["unchanged"]
assert sum(not row["unchanged"] for row in identity["source"].values()) == 8
assert len(identity["source"]) == 15  # Thirteen registered outputs, registry, generator parse.rs.
driver = payloads["matched-fixtures/tests/generated_map_value_depth.rs"]
consumer = payloads["matched-fixtures/tests/fixtures/generated_map_value_depth_consumer.rs"]
assert digest(driver) == identity["matched_driver_sha256"]
assert digest(consumer) == identity["matched_consumer_sha256"]
assert len(re.findall(rb"#\[test\]\s*fn\s+\w+", consumer)) == 15
assert b"15 passed; 0 failed" in driver
assert identity["runtime_status"] == "NOT_RUN"
print(json.dumps({"status": "passed", "scope": "source textual proof only",
                  "raw_members": len(payloads), "shipping_files_changed": 8,
                  "migrated_generated_files": 7, "operations": 327,
                  "map_entries": 109, "message_value_maps": 17,
                  "consumer_oracles_unchanged_not_executed": 15,
                  "compiler_generator_regeneration_runtime": "NOT_RUN"}, sort_keys=True))
