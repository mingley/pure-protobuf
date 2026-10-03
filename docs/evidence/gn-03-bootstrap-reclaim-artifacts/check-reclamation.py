#!/usr/bin/env python3
"""Verify GN03 bootstrap reclamation inventories and preserved contents."""

import hashlib
import json
from pathlib import Path
import tarfile


root = Path(__file__).resolve().parent
files = json.loads((root / "cache-file-sha256.json").read_text())
record = json.loads((root / "reclamation.json").read_text())
accounting = record["accounting_before"]
assert len(files) == accounting["non_directory_paths"] == 1772
assert sum(row["bytes"] for row in files.values()) == accounting["path_sum_logical_bytes"]
assert sum(row["allocated_bytes"] for row in files.values()) == accounting["path_sum_allocated_bytes"]
unique = {(row["device"], row["inode"]): row for row in files.values()}
assert sum(row["bytes"] for row in unique.values()) == accounting["unique_inode_logical_bytes"]
assert sum(row["allocated_bytes"] for row in unique.values()) == accounting["unique_inode_allocated_bytes"]
assert sum(row["nlink"] > 1 for row in files.values()) == accounting["multiple_link_paths"] == 52
for name in ["process-guard-before.json", "process-guard-immediately-before.json"]:
    guard = json.loads((root / name).read_text())
    assert guard["cache_ownership_matches"] == []
    assert all(row["comm"] in ["dockerd", "containerd"] for row in guard["known_system_daemon_inspection_limits"])
retained = json.loads((root / "retained-content-sha256.json").read_text())
assert len(retained) == record["retained_original_content_files"] == 652
with tarfile.open(root / "fingerprint-and-dependency-contents.tar.gz", "r:gz") as archive:
    members = {item.name: item for item in archive.getmembers()}
    assert members.keys() == retained.keys()
    for name, pin in retained.items():
        assert members[name].isfile() and files[name] == pin
        data = archive.extractfile(members[name]).read()
        assert len(data) == pin["bytes"] and hashlib.sha256(data).hexdigest() == pin["sha256"]
controls = json.loads((root / "unchanged-controls-before.json").read_text())
assert controls["copied_generator"]["sha256"] == "718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b"
assert record["remaining_cache_entries"] == 0 and record["retry"].startswith("not_run")
assert record["global_free_after_bytes"] - record["global_free_before_bytes"] == record["observed_global_free_delta_bytes"] == 813592576
print("Verified recorded inventory/accounting for 1,772 file paths, actual contents of 652 retained files, both process guards, and frozen generator pin.")
