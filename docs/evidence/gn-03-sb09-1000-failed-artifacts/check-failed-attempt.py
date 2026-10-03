#!/usr/bin/env python3
"""Verify the retained GN03 1000 failure without executing compiler output."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parent)
root = parser.parse_args().directory
manifest = json.loads((root / "members-sha256.json").read_text())
with tarfile.open(root / "raw.tar.gz", "r:gz") as archive:
    actual = {item.name: item for item in archive.getmembers()}
    assert actual.keys() == manifest.keys(), "archive/member manifest coverage differs"
    for name, pin in manifest.items():
        assert actual[name].isfile()
        assert not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts
        data = archive.extractfile(actual[name]).read()
        assert len(data) == pin["bytes"] and hashlib.sha256(data).hexdigest() == pin["sha256"], name

    def data(name):
        return archive.extractfile(actual[name]).read()

    def record(name):
        return json.loads(data(name))

    pair = record("pair-1000.json")
    assert pair["status"] == "incomplete" and pair["case"] == "1000"
    assert pair["jobs"] == 1 and pair["repeats"] == 1 and pair["seed"] == 190019
    assert len(pair["runs"]) == 1 and pair["runs"][0]["profile"] == "default"
    assert "metrics" not in pair, "incomplete pair must not carry comparison metrics"
    run = pair["runs"][0]
    assert run["exit_code"] == 1 and "preserved_binary" not in run
    bootstrap = pair["bootstrap"]
    assert bootstrap["bootstrap_excluded_from_all_cell_measurements"] is True
    assert bootstrap["binary_sha256"] == "718aef4131813c1c6fc449e33202588fc6bf48cc330bd28cfd33a96e056f524b"
    assert manifest[bootstrap["binary"]]["sha256"] == bootstrap["binary_sha256"]
    assert manifest["driver/Cargo.lock"]["sha256"] == bootstrap["driver_lock_sha256"]
    assert manifest["coordinator/sb09-paired-screen.py"]["sha256"] == pair["coordinator_sha256"]
    for name, digest in pair["source_sha256"].items():
        assert manifest["source-capsule/" + name]["sha256"] == digest

    prefix = "1000-default/"
    report = record(prefix + "summary.json")
    assert manifest[prefix + "summary.json"]["sha256"] == run["report_sha256"]
    assert report["status"] == "error" and report["qualification"]["qualified"] is False
    assert report["errors"] == ["missing Linux maximum resident set size in time log"]
    assert report["environment"]["repository"]["source_sha256"] == pair["source_sha256"]
    assert report["setup"]["generator_binary_sha256"] == bootstrap["binary_sha256"]
    assert len(report["cells"]) == 1
    cell = report["cells"][0]
    assert cell["case"] == "1000" and cell["generator"] == "pbrs" and cell["repeat"] == 0
    assert cell["shared_descriptor_helper"]["active"] is False
    consumer = prefix + "cases/1000/consumer/"
    release = tomllib.loads(data(consumer + "Cargo.toml").decode())["profile"]["release"]
    assert release["opt-level"] == 3 and release["lto"] == "thin" and release["codegen-units"] == 1
    assert manifest[consumer + "Cargo.lock"]["sha256"] == cell["consumer_lock_sha256"]
    for pin in cell["corpus"]["inputs"]:
        name = prefix + "cases/1000/" + pin["path"]
        assert manifest[name]["sha256"] == pin["sha256"] and manifest[name]["bytes"] == pin["bytes"]
    generated = [name for name in actual if name.startswith(consumer + "generated/") and name.endswith(".rs")]
    assert len(generated) == cell["output"]["rust_file_count"] == 21
    assert sum(manifest[name]["bytes"] for name in generated) == cell["output"]["rust_bytes"] == 35782708
    assert cell["output"]["unchanged_generation_mtimes_preserved"] is True
    assert not any(name.startswith("1000-sharing/") for name in actual)
    assert "release_binary" not in cell
    assert not any(name.startswith(prefix + cell["target_dir"] + "/") for name in actual)
    assert record(prefix + "fingerprint-sha256.json"), "missing retained fingerprint digests"
    phases = cell["phases"]
    assert set(phases) == {"consumer_lock", "generation", "generation_unchanged", "check_clean", "check_incremental"}
    for name in ["consumer_lock", "generation", "generation_unchanged", "check_clean"]:
        phase = phases[name]
        assert phase["status"] == "passed" and phase["exit_code"] == 0
        data(prefix + phase["stdout_log"])
        stderr = data(prefix + phase["stderr_log"])
        assert b"Exit status: 0" in stderr
        peak = int(re.search(rb"Maximum resident set size \(kbytes\): (\d+)", stderr).group(1)) * 1024
        assert peak == phase["direct_command_peak_rss_bytes"] and phase["peak_rss_bytes"] >= peak
    failed = phases["check_incremental"]
    assert failed["status"] == "error" and failed["error"] == report["errors"][0]
    assert "elapsed_ns" not in failed and "peak_rss_bytes" not in failed
    stderr = data(prefix + failed["stderr_log"])
    assert b"Maximum resident set size" not in stderr and b"Exit status:" not in stderr
    telemetry = record(prefix + "resource-telemetry.json")
    assert telemetry["failures"] == run["resource_failures"] and len(telemetry["failures"]) == 2
    first = telemetry["failures"][0]
    assert first["utc"] == "2026-10-03T00:32:55.519467+00:00"
    assert first["owned_cache_bytes"] == 2153570304 > 2 * 1024**3
    assert min(sample["global_free_bytes"] for sample in telemetry["samples"]) == 5077155840 >= 2 * 1024**3
    print(json.dumps({
        "verified_members": len(manifest),
        "status": "incomplete resource-limited attempt; comparison and later phases not_run",
        "default_source_bytes": cell["output"]["rust_bytes"],
        "default_cold_check_seconds": phases["check_clean"]["elapsed_ns"] / 1e9,
        "default_cold_check_peak_rss_bytes": phases["check_clean"]["peak_rss_bytes"],
        "resource_limit_bytes": 2 * 1024**3,
        "observed_cache_bytes": first["owned_cache_bytes"],
        "incremental": "missing GNU-time trailer; no elapsed/RSS measurement",
        "shared_release_smoke_and_binary": "not_run",
    }, indent=2))
