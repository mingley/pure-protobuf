#!/usr/bin/env python3
"""Read-only verification of every sealed archive member and qualified gate."""

import hashlib
import json
from pathlib import Path
import tarfile

ROOT = Path(__file__).resolve().parent


def digest_bytes(data):
    return hashlib.sha256(data).hexdigest()


proof = json.loads((ROOT / "proof.json").read_text())
archive = ROOT / proof["archive"]["file"]
assert archive.stat().st_size == proof["archive"]["bytes"]
assert digest_bytes(archive.read_bytes()) == proof["archive"]["sha256"]
expected = json.loads((ROOT / "members-sha256.json").read_text())
observed = {}
content = {}
with tarfile.open(archive, "r:gz") as entries:
    for entry in entries:
        assert entry.name.startswith("embedded-topology-604f4c2e/") or entry.isdir()
        assert not entry.issym() and not entry.islnk(), entry.name
        if entry.isfile():
            raw = entries.extractfile(entry).read()
            observed[entry.name] = {"bytes": len(raw), "sha256": digest_bytes(raw)}
            if entry.name.endswith(".json"):
                content[entry.name] = raw
assert observed == expected
assert len(observed) == proof["archive"]["regular_members"]
prefix = "embedded-topology-604f4c2e/"
for label, gate in proof["gates"].items():
    raw = content[prefix + "gate-" + label + ".json"]
    assert digest_bytes(raw) == gate["record_sha256"], label
    record = json.loads(raw)
    assert record["exit"] == 0
    assert record["source"]["source_commit"] == proof["source_commit"]
    snapshot = prefix + Path(record["source"]["path"]).name
    assert digest_bytes(content[snapshot]) == record["source"]["sha256"]
    source = json.loads(content[snapshot])
    assert source["clean_source"]
    for relative, sha in source["source_sha256"].items():
        assert observed[prefix + "qualified-source/" + relative]["sha256"] == sha, (label, relative)
print(json.dumps({"status": "pass", "source_commit": proof["source_commit"],
    "regular_members": len(observed), "qualified_gates": len(proof["gates"]),
    "archive_sha256": proof["archive"]["sha256"]}, indent=2))
