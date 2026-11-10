#!/usr/bin/env python3
"""Replay a retained x86 TLS configuration-construction screen."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys
import tomllib

root = Path(sys.argv[1])
data = json.loads((root / "results/results.json").read_text())
assert data == json.loads(Path(sys.argv[3]).read_text())
expected_source = {"base": "f73b85d067ea861fe36f23536153e4c5add3137f",
                   "candidate": "171388257f15d261a1667799f40a38edc3bbeffe"}
assert data["source"] == expected_source
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def git_blob(path):
    raw = path.read_bytes()
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
def near(a, b):
    assert math.isclose(float(a), float(b), rel_tol=1e-12, abs_tol=1e-12), (a, b)

expected_blobs = {
    "sources/base/Cargo.toml": "4d56688060370664e4868df336c6b41c26311fd2",
    "sources/base/tls.rs": "7dc0c26a49df12d8699b99e2adeddfb86c761f21",
    "sources/candidate/Cargo.toml": "dd47d99141529eac9aa51b319105c46d19356c4c",
    "sources/candidate/tls.rs": "218b3bbfd6545c77df5e2005c9d9f6bdc1fc4a5b",
    "sources/probe.rs": "f73c01807949606984f9720ea3cf050cce62c852",
    "sources/compare.py": "f32a75b368e5959281e381719e5ebfbf241bad2b",
    "workspace-Cargo.lock": "6e082422103b0a488f13794475a91b5c71627f10",
}
for name, digest in expected_blobs.items():
    assert git_blob(root / name) == digest, name
features = (root / "features-base.txt").read_bytes()
assert features == (root / "features-candidate.txt").read_bytes()
assert b"rustls-graviola" in features and b"rustls" in features

def packages(path):
    with path.open("rb") as f:
        contents = tomllib.load(f)
    return {(p["name"], p["version"], p["source"], p.get("checksum"))
            for p in contents["package"] if "source" in p}
assert not packages(root / "probe-Cargo.lock") - packages(root / "workspace-Cargo.lock")
fixture = Path(sys.argv[2])
expected_files = ("ca.crt", "server.crt", "server.key", "client.crt", "client.key")
assert set(data["fixtures"]) == set(expected_files)
for name in expected_files:
    assert sha(fixture / name) == data["fixtures"][name]
assert data["topology"].keys() == {"physical_package_id", "core_id"}
binary_lines = (root / "binaries.sha256").read_text().splitlines()
binary_digests = {Path(line.split(maxsplit=1)[1]).name: line.split()[0] for line in binary_lines}
assert data["binary_sha256"] == {v: binary_digests[v] for v in ("base", "candidate")}
assert data["allocation_binary_sha256"] == {v: binary_digests[v + "-alloc"]
                                             for v in ("base", "candidate")}
for variant in ("base", "candidate"):
    rows = [json.loads(line) for line in (root / ("build-" + variant + ".jsonl")).read_text().splitlines()
            if line.startswith("{")]
    matches = [r for r in rows if r.get("reason") == "compiler-artifact"
               and r.get("target", {}).get("name") == "pbrs_grpc" and "lib" in r["target"]["kind"]]
    assert matches and any(not r["fresh"] for r in matches)

cases = {"client-ca", "server", "client-mtls", "server-mtls"}
rows, summaries, allocs = data["rows"], data["summaries"], data["allocations"]
assert len(rows) == 32 and len(summaries) == 4 and len(allocs) == 24
assert {s["case"] for s in summaries} == cases
for summary in summaries:
    case = summary["case"]
    group = [r for r in rows if r["case"] == case]
    assert len(group) == 8 and sorted(r["block"] for r in group) == list(range(8))
    for row in group:
        assert row["order"] == (["base", "candidate"] if row["block"] % 2 == 0 else ["candidate", "base"])
        a, b = row["samples"]["base"], row["samples"]["candidate"]
        assert a["case"] == b["case"] == case and a["parallelism"] == b["parallelism"] == 1
        assert a["iters"] == b["iters"] == row["iterations"] and a["cpu_ns"] > 0 and b["cpu_ns"] > 0
        near(row["ratio"], b["cpu_ns"] / a["cpu_ns"])
        for variant in ("base", "candidate"):
            c = row["controls"][variant]
            assert c["a"]["case"] == c["b"]["case"] == case
            assert c["a"]["parallelism"] == c["b"]["parallelism"] == 1
            assert c["a"]["iters"] == c["b"]["iters"] == row["iterations"]
            assert c["a"]["cpu_ns"] > 0 and c["b"]["cpu_ns"] > 0
            near(c["ratio"], c["b"]["cpu_ns"] / c["a"]["cpu_ns"])
    ratios = [r["ratio"] for r in group]
    near(summary["median"], statistics.median(ratios))
    assert summary["range"] == [min(ratios), max(ratios)]
    assert summary["slower"] == sum(x > 1 for x in ratios)
    for variant in ("base", "candidate"):
        controls = [r["controls"][variant]["ratio"] for r in group]
        assert summary["controls"][variant] == [min(controls), max(controls)]
        per_op = [r["samples"][variant]["cpu_ns"] / r["iterations"] for r in group]
        near(summary["cpu_ns_per_op"][variant][0], min(per_op))
        near(summary["cpu_ns_per_op"][variant][1], max(per_op))
    allocation = [a for a in allocs if a["case"] == case]
    assert len(allocation) == 6
    values = {}
    for variant in ("base", "candidate"):
        samples = [a for a in allocation if a["variant"] == variant]
        assert sorted(x["repeat"] for x in samples) == [0, 1, 2]
        assert all(x["sample"]["case"] == case and x["sample"]["parallelism"] == 1
                   and x["sample"]["iters"] == 16 and x["sample"]["alloc_calls"] > 0
                   and x["sample"]["requested_bytes"] > 0 for x in samples)
        values[variant] = [{"calls_per_op": x["sample"]["alloc_calls"] / 16,
                            "requested_bytes_per_op": x["sample"]["requested_bytes"] / 16}
                           for x in samples]
    print("TLS_CONSTRUCT_VERIFIED_V1 " + json.dumps({"summary": summary, "allocations": values}, sort_keys=True))
print("TLS_CONSTRUCT_DATA_VERIFIED cases=4 cpu_blocks=32 allocation_samples=24")
