"""Audit retained real-API coverage and unchanged runtime corpus oracles."""
import hashlib
import json
from pathlib import Path

here = Path(__file__).resolve().parent
root = next(p for p in here.parents if (p / "bench/devloop/adoption").is_dir())
inventory = here / "bridge-inventory.json"
rows = json.loads(inventory.read_text())["cells"]
specimens = [f"query.d{d}.v{v}" for d in range(3, 9) for v in range(8)]
specimens += [f"entities.n{n}" for n in (10, 100, 1000)]
specimens += [f"sparse.v{v}" for v in range(10)]
specimens += [f"maps.n{n}" for n in (8, 64, 512)]
specimens += [f"options.part{i:02}" for i in range(20)]
expected = {f"codec.adoption.bridge.{direction}.{specimen}.{mode}"
            for direction in ("prost_to_pbrs", "pbrs_to_prost")
            for specimen in specimens for mode in ("api_only", "read_all")}
assert len(rows) == 336 and {row["id"] for row in rows} == expected
old = json.loads((root / "bench/devloop/adoption/evidence/codec-inventory.json").read_text())["cells"]
oracles = {row["specimen"]: (row["read_checksum"], row["wire_bytes"]) for row in old}
blocked = 0
runtime_rows = 0
for row in rows:
    assert row["api"] == f"protobuf_tonic::{row['direction']}"
    assert all(row[key] == "passed" for key in ("full_equality", "complete_reads", "both_api_roundtrips"))
    assert row["numeric_cost"] == row["instructions"] == "not_run"
    assert row["wire_fingerprint"].endswith(row["actual_source_wire_fingerprint"])
    if ".maps." in row["id"]:
        assert row["timing_qualification"] == "blocked" and row["timing_blocked_reason"]
        assert not row["source_output_equal_wire"]
        assert abs(row["actual_source_wire_bytes"] - row["target_reencoded_wire_bytes"]) == 4
        blocked += 1
    else:
        assert row["timing_qualification"] == "passed" and row["source_output_equal_wire"]
        assert row["actual_source_wire_bytes"] == row["target_reencoded_wire_bytes"] == row["common_wire_bytes"]
    if row["specimen"] in oracles:
        assert (row["read_checksum"], row["common_wire_bytes"]) == oracles[row["specimen"]]
        runtime_rows += 1
assert blocked == 12 and runtime_rows == 256
assert hashlib.sha256(inventory.read_bytes()).hexdigest() == json.loads((here / "inventory-audit.json").read_text())["inventory_sha256"]
print("336 actual API rows / 84 specimens; 324 wire guards; 12 map blocks; 256 retained runtime oracles match; all costs not_run")
