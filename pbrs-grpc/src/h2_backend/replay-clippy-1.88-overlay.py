#!/usr/bin/env python3
"""Read-only replay of the one-lint overlay from the qualified base bytes."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
record = json.loads((HERE / "clippy-1.88-overlay.json").read_text())
sha = lambda raw: hashlib.sha256(raw).hexdigest()
if len(sys.argv) > 1:
    original = Path(sys.argv[1]).read_bytes()
else:
    original = subprocess.check_output(["git", "show", record["base_topology_source"] + ":" + record["file"]], cwd=ROOT)
assert sha(original) == record["old_sha256"]
operation = record["operation"]
needle = operation["needle"].encode()
insertion = operation["insert_after"].encode()
assert original.count(needle) == operation["occurrences"] == 1
replayed = original.replace(needle, needle + insertion)
assert sha(replayed) == record["new_sha256"]
assert (HERE / "mod.rs").read_bytes() == replayed
assert sha((HERE / record["exact_patch_file"]).read_bytes()) == record["exact_patch_sha256"]
assert sha((HERE / "provenance.json").read_bytes()) == record["original_topology_provenance_sha256"]
print(json.dumps({"status": "pass", "base": record["base_topology_source"],
    "old_mod_sha256": record["old_sha256"], "new_mod_sha256": record["new_sha256"],
    "original_mapping_preserved": True, "new_compilation_run": False}, indent=2))
