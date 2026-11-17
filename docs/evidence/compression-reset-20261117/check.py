#!/usr/bin/env python3
"""Check retained samples and print the measured candidate/baseline CPU ratios."""
import hashlib
import json
import math
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parent
BASE = "e0d6d09230c4e53934be6aa8aa928612bb40733d"
CANDIDATE = "2f27343e23770571c3ab40a15038c6dcb2ed5217"
KINDS = ("empty", "1k-noise", "64k-noise", "1m-zero",
         "roundtrip-1k-noise", "encode-control-1k-noise")
CASES = {f"{codec}-{kind}" for codec in ("gzip", "deflate") for kind in KINDS}

def positive(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise AssertionError("CPU sample must be a number")
    assert math.isfinite(value) and value > 0
    return value

def same(left, right):
    assert math.isclose(left, right, rel_tol=1e-12, abs_tol=1e-12)

def main():
    manifest = json.loads((ROOT / "sha256.json").read_text())
    for name, digest in manifest.items():
        assert Path(name).name == name
        assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest, name
    data = json.loads((ROOT / "results.json").read_text())
    assert data["base"] == BASE and data["candidate"] == CANDIDATE
    assert (ROOT / "baseline.sha").read_text().strip() == BASE
    assert (ROOT / "candidate.sha").read_text().strip() == CANDIDATE
    assert data["scope"] == "shared-host whole-process component CPU; not RPC or qualification"
    assert len(data["affinity"]) == 1 and data["affinity"][0] in data["available_cpus"]
    assert data["blocks"] == 4 and set(data["runs"]) == CASES
    for case, row in data["runs"].items():
        n = row["iterations"]
        assert isinstance(n, int) and not isinstance(n, bool) and n > 0
        expected = row["calibration"]["output"]
        positive(row["calibration"]["cpu"])
        assert expected["iterations"] == n
        length = 0 if "empty" in case else 65536 if "64k" in case else 1048576 if "1m" in case else 1024
        assert expected["payload_len"] == length and expected["encoded_len"] > 0
        observed_length = expected["encoded_len"] if "encode-control" in case else length
        assert expected["sum"] == (n * observed_length) % (1 << 64)
        assert len(row["blocks"]) == 4
        for index, block in enumerate(row["blocks"]):
            order = ["base", "candidate", "candidate", "base"] if index % 2 == 0 else ["candidate", "base", "base", "candidate"]
            assert block["order"] == order and set(block["samples"]) == {"base", "candidate"}
            cpus = {}
            for label, samples in block["samples"].items():
                assert len(samples) == 2
                cpus[label] = [positive(sample["cpu"]) for sample in samples]
                assert all(sample["output"] == expected for sample in samples)
            same(block["ratio"], sum(cpus["candidate"]) / sum(cpus["base"]))
            same(block["base_control"], cpus["base"][1] / cpus["base"][0])
            same(block["candidate_control"], cpus["candidate"][1] / cpus["candidate"][0])
        ratios = [block["ratio"] for block in row["blocks"]]
        print(f"{case}: median {statistics.median(ratios):.4f}, range {min(ratios):.4f}..{max(ratios):.4f}")
    print("Retained source IDs, file hashes, workloads, samples and ratios agree.")

if __name__ == "__main__":
    main()
