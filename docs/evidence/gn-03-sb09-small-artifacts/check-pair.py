#!/usr/bin/env python3
"""Verify archived GN03 pair inputs/artifacts without executing compiler output."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, default=Path(__file__).resolve().parent)
args = parser.parse_args()
root = args.directory
manifest = json.loads((root / "members-sha256.json").read_text())
with tarfile.open(root / "raw.tar.gz", "r:gz") as archive:
    actual = {item.name: item for item in archive.getmembers()}
    assert actual.keys() == manifest.keys(), "archive/member manifest coverage differs"
    for name, pin in manifest.items():
        assert actual[name].isfile(), f"unexpected non-file member: {name}"
        assert not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts
        data = archive.extractfile(actual[name]).read()
        assert len(data) == pin["bytes"], f"size mismatch: {name}"
        assert hashlib.sha256(data).hexdigest() == pin["sha256"], f"hash mismatch: {name}"

    def data(name):
        return archive.extractfile(actual[name]).read()

    def record(name):
        return json.loads(data(name))

    pair_names = [name for name in actual if re.fullmatch(r"pair-(small|100|1000)\.json", name)]
    assert len(pair_names) == 1, "expected one paired capture"
    pair = record(pair_names[0])
    assert pair["status"] == "complete-local-diagnostic"
    assert pair["jobs"] == 1 and pair["repeats"] == 1 and pair["seed"] == 190019
    assert len(pair["runs"]) == 2 and {run["profile"] for run in pair["runs"]} == {"default", "shared"}
    bootstrap = pair["bootstrap"]
    assert bootstrap["bootstrap_excluded_from_all_cell_measurements"] is True
    assert manifest[bootstrap["binary"]]["sha256"] == bootstrap["binary_sha256"]
    assert manifest["driver/Cargo.lock"]["sha256"] == bootstrap["driver_lock_sha256"]
    assert manifest["coordinator/sb09-paired-screen.py"]["sha256"] == pair["coordinator_sha256"]
    for name, digest in pair["source_sha256"].items():
        assert manifest["source-capsule/" + name]["sha256"] == digest, f"source capsule mismatch: {name}"

    reports = []
    consumer_manifests = []
    results = {}
    for run in pair["runs"]:
        prefix = run["out"] + "/"
        report = record(prefix + "summary.json")
        reports.append(report)
        assert manifest[prefix + "summary.json"]["sha256"] == run["report_sha256"]
        assert run["exit_code"] == 0 and run["resource_failures"] == []
        assert report["status"] == "unqualified" and report["qualification"]["qualified"] is False
        assert "single_run_diagnostic" in report["qualification"]["reasons"]
        assert report["setup"]["generator_binary_sha256"] == bootstrap["binary_sha256"]
        assert report["environment"]["repository"]["source_sha256"] == pair["source_sha256"]
        assert len(report["cells"]) == 1
        cell = report["cells"][0]
        assert cell["case"] == pair["case"] and cell["generator"] == "pbrs" and cell["repeat"] == 0
        consumer = prefix + "cases/" + pair["case"] + "/consumer/"
        consumer_manifests.append(data(consumer + "Cargo.toml"))
        release = tomllib.loads(consumer_manifests[-1].decode())["profile"]["release"]
        assert release["opt-level"] == 3 and release["lto"] == "thin" and release["codegen-units"] == 1
        assert manifest[consumer + "Cargo.lock"]["sha256"] == cell["consumer_lock_sha256"]
        for input_pin in cell["corpus"]["inputs"]:
            name = prefix + "cases/" + pair["case"] + "/" + input_pin["path"]
            assert manifest[name]["sha256"] == input_pin["sha256"] and manifest[name]["bytes"] == input_pin["bytes"]
        generated = [name for name in actual if name.startswith(consumer + "generated/") and name.endswith(".rs")]
        assert len(generated) == cell["output"]["rust_file_count"]
        assert sum(manifest[name]["bytes"] for name in generated) == cell["output"]["rust_bytes"]
        assert cell["output"]["unchanged_generation_mtimes_preserved"] is True
        helper = cell["shared_descriptor_helper"]
        assert helper["active"] is (run["profile"] == "shared")
        if helper["active"]:
            assert manifest[consumer + "generated/" + helper["path"]]["sha256"] == helper["sha256"]
        preserved = run["preserved_binary"]
        assert manifest[preserved["path"]]["sha256"] == preserved["sha256"] == cell["release_binary"]["sha256"]
        assert manifest[preserved["path"]]["bytes"] == preserved["bytes"] == cell["release_binary"]["size_bytes"]
        assert not any(name.startswith(prefix + cell["target_dir"] + "/") for name in actual)
        fingerprints = record(prefix + "fingerprint-sha256.json")
        assert fingerprints, "missing retained compiler fingerprints"
        telemetry = record(prefix + "resource-telemetry.json")
        assert telemetry["samples"] and telemetry["failures"] == []
        assert all(sample["owned_cache_bytes"] <= 2 * 1024**3 and sample["global_free_bytes"] >= 2 * 1024**3 for sample in telemetry["samples"])
        phases = cell["phases"]
        assert set(phases) == {"consumer_lock", "generation", "generation_unchanged", "check_clean", "check_incremental", "build_release", "release_smoke"}
        for name, phase in phases.items():
            assert phase["status"] == "passed" and phase["exit_code"] == 0
            stdout = data(prefix + phase["stdout_log"])
            stderr = data(prefix + phase["stderr_log"])
            if name == "release_smoke":
                assert phase["output_verified"] is True and stdout == b"1\n" and stderr == b""
            else:
                assert b"Exit status: 0" in stderr
                raw_peak = int(re.search(rb"Maximum resident set size \(kbytes\): (\d+)", stderr).group(1)) * 1024
                assert raw_peak == phase["direct_command_peak_rss_bytes"]
                assert phase["peak_rss_bytes"] >= raw_peak
        results[run["profile"]] = {
            "source_bytes": cell["output"]["rust_bytes"],
            "cold_check_seconds": phases["check_clean"]["elapsed_ns"] / 1e9,
            "release_seconds": phases["build_release"]["elapsed_ns"] / 1e9,
            "linked_binary_bytes": preserved["bytes"],
        }
    a, b = [report["cells"][0] for report in reports]
    assert a["corpus"] == b["corpus"] and a["consumer_lock_sha256"] == b["consumer_lock_sha256"]
    assert consumer_manifests[0] == consumer_manifests[1]
    assert reports[0]["environment"]["tools"] == reports[1]["environment"]["tools"]
    print(json.dumps({"verified_members": len(manifest), "case": pair["case"], "status": "unqualified local diagnostic", "metrics": results}, indent=2))
