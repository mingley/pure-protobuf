#!/usr/bin/env python3
"""Check the trace-context summary against its retained raw samples."""
import hashlib
import json
import statistics
from pathlib import Path

path = Path(__file__).with_name("traceparent-screen-20261110.json")
report = json.loads(path.read_text())
expected = {
    ("direct", "valid"): (0.894, 10),
    ("metadata", "valid"): (0.928, 10),
    ("direct", "missing"): (1.004, 3),
    ("metadata", "missing"): (0.999, 5),
    ("direct", "invalid"): (0.719, 10),
    ("metadata", "invalid"): (0.880, 10),
    ("direct", "future"): (0.713, 10),
    ("metadata", "state"): (0.974, 9),
}
assert report["blocks"] == 10 and report["iterations"] == 1_000_000
assert report["allocation_iterations"] == 20_000
for item in report["results"]:
    key = (item["carrier"], item["scenario"])
    baseline = item["cpu_seconds"]["base"]
    candidate = item["cpu_seconds"]["candidate"]
    assert len(baseline) == len(candidate) == 10
    # Array indices identify blocks even when execution order was reversed.
    # The driver's separate-median ratio is not this within-block median.
    ratios = [c / b for b, c in zip(baseline, candidate, strict=True)]
    expected_ratio, expected_wins = expected.pop(key)
    assert abs(statistics.median(ratios) - expected_ratio) < 0.0005
    assert sum(ratio < 1 for ratio in ratios) == expected_wins
    want_checksum = 20_000 if item["scenario"] in ("valid", "future", "state") else 0
    assert item["counts"]["base"]["checksum"] == want_checksum
    assert item["counts"]["candidate"]["checksum"] == want_checksum
    if key == ("direct", "valid"):
        assert item["counts"]["base"]["allocations"] == 40_001
        assert item["counts"]["candidate"]["allocations"] == 20_001
    if key == ("metadata", "valid"):
        assert item["counts"]["base"]["allocations"] == 80_001
        assert item["counts"]["candidate"]["allocations"] == 60_001
assert not expected
fixture_dir = path.with_name("traceparent-screen-20261110")
fixture_hashes = {
    "candidate.patch": "eff692bd3a565fa811bf5f482bec3e255d90f750154cdc5e32b850a83061be89",
    "Cargo.toml": "5f43c8db0372bac856a74099f486165be00f5cf1b6784805bdfddf025b9029b7",
    "main.rs": "cad2af1d0f64845a71975c1c9494be8be058be6326939f0f008fd3e4c2a485d7",
    "compare.py": "b59fef67b3b3266111ba2e37f3ae92fd26d7f6a2589b05c60f4b78daff862f02",
}
for name, expected_hash in fixture_hashes.items():
    digest = hashlib.sha256((fixture_dir / name).read_bytes()).hexdigest()
    assert digest == expected_hash, f"{name}: SHA-256 mismatch"
print("trace-context screen summary and fixture hashes match")
