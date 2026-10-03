"""Audit the frozen TC-30b2b archive and replay its source-specific proof checks."""
import hashlib
import json
import pathlib
import subprocess
import tarfile
import tempfile

package = pathlib.Path(__file__).resolve().parent
root = pathlib.Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
for name, digest in json.loads((package / "artifact-sha256.json").read_text()).items():
    assert pathlib.PurePosixPath(name).name == name, name
    assert hashlib.sha256((package / name).read_bytes()).hexdigest() == digest, name
members = json.loads((package / "members-sha256.json").read_text())
assert len({item["path"] for item in members}) == len(members)
expected = {item["path"]: item for item in members}
(root / "work").mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="tc30b2b-audit-", dir=root / "work") as tmp:
    raw = pathlib.Path(tmp)
    with tarfile.open(package / "raw.tar.gz", "r:gz") as archive:
        actual = archive.getmembers()
        assert {item.name for item in actual} == set(expected)
        assert len(actual) == len(expected)
        for item in actual:
            assert item.isfile() and pathlib.PurePosixPath(item.name).parent == pathlib.PurePosixPath("raw"), item.name
            payload = archive.extractfile(item).read()
            pin = expected[item.name]
            assert len(payload) == pin["bytes"]
            assert hashlib.sha256(payload).hexdigest() == pin["sha256"], item.name
            (raw / pathlib.PurePosixPath(item.name).name).write_bytes(payload)
    output = subprocess.check_output(["python3", str(raw / "verify-proof.py"), "--raw-dir", str(raw), "--complete", "--combined"], cwd=root, text=True)
    proof = json.loads(output)
    assert proof == json.loads((package / "proof.json").read_text())
print(json.dumps({"members": len(members), "previous_source_executions": proof["previous_total"], "optout_only_executions": proof["final_total"], "combined_executions": proof["combined_source_replay"]["total"], "combined_source": proof["combined_source_replay"]["source"], "shipping_correctness_proof_complete": proof["shipping_correctness_proof_complete"], "performance_qualified": False}, indent=2))
